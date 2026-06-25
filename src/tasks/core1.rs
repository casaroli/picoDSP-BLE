use crate::data::presets::Preset;
use alloc::boxed::Box;
use alloc::sync::Arc;
use core::sync::atomic::Ordering;
use embassy_time::Instant;
use infinitedsp_core::FrameProcessor;
use infinitedsp_core::core::audio_param::AudioParam;
use infinitedsp_core::core::channels::{DualMono, Mono, Stereo};
use infinitedsp_core::core::dsp_chain::DspChain;
use infinitedsp_core::core::parallel_mixer::ParallelMixer;
use infinitedsp_core::effects::time::delay::Delay;
use infinitedsp_core::effects::time::reverb::Reverb;
use infinitedsp_core::effects::utility::bypass::Bypass;
use infinitedsp_core::effects::utility::gain::Gain;
use infinitedsp_core::effects::utility::stereo_widener::StereoWidener;

use crate::HEAP;
use crate::common::shared::{
    AUDIO_CHANNEL, AUDIO_QUEUE_MIN, AUDIO_UNDERRUNS, AudioData, BLOCK_SIZE, CORE1_RUNNING,
    CORE1_STACK_SIZE, PRESET_CHANNEL, PSRAM_GATE_ACK, PSRAM_GATE_REQ, SAMPLE_RATE,
    disable_denormals,
};
use crate::control::midi::MidiControl;
use crate::dsp::moog::new_moog_voice;
use crate::dsp::psram_delay::PsramDelay;
use crate::usb::logger::{LOG_CHANNEL, LogData, SYSTEM_STATUS_CHANNEL};

/// Prototype toggle: back the two delay ring buffers with PSRAM (`true`) or the
/// SRAM heap via the stock `Delay` (`false`). Flip to A/B the peak per-buffer
/// DSP time and isolate the cost of routing delay traffic over the QMI bus.
const USE_PSRAM_DELAY: bool = true;

macro_rules! log_status {
    ($($arg:tt)*) => {
        {
            let mut msg = heapless::String::<64>::new();
            if core::fmt::write(&mut msg, format_args!($($arg)*)).is_ok() {
                let _ = SYSTEM_STATUS_CHANNEL.send(msg).await;
            }
        }
    };
}

fn build_synth(
    midi_control: Arc<MidiControl>,
    preset: Preset,
) -> impl FrameProcessor<Stereo> + Send {
    let voice = new_moog_voice(SAMPLE_RATE, midi_control, preset);

    // Rewind the PSRAM bump allocator before (re)building so preset switches
    // reuse the same region instead of leaking it. Safe because the previous
    // synth (holding the old PSRAM slices) is dropped before build_synth runs.
    crate::psram::reset_alloc();

    let d = &preset.delay;
    let time_l = d.time;
    let time_r = d.time * 1.15;

    defmt::info!(
        "build_synth: delay enabled={} backend={}",
        d.enabled != 0,
        if USE_PSRAM_DELAY { "PSRAM" } else { "SRAM" }
    );

    // Both arms are boxed so the chain type is identical regardless of backend;
    // the vtable dispatch is once per 256-sample block (not per sample), so the
    // A/B delta is purely the buffer location. Delay::new hardcodes 44100 Hz, so
    // the SRAM arm pre-sizes to the real rate to avoid a realloc inside `.and()`.
    let make_delay = |time: f32| -> Box<dyn FrameProcessor<Mono> + Send> {
        if USE_PSRAM_DELAY {
            Box::new(PsramDelay::new(0.3, time, d.feedback, d.mix, SAMPLE_RATE))
        } else {
            let mut dl = Delay::new(
                0.3,
                AudioParam::Static(time),
                AudioParam::Static(d.feedback),
                AudioParam::Static(d.mix),
            );
            dl.set_sample_rate(SAMPLE_RATE);
            Box::new(dl)
        }
    };

    let delay_l = make_delay(time_l);
    let delay_r = make_delay(time_r);

    let delay_node = ParallelMixer::new(1.0, DualMono::new(delay_l, delay_r));
    let delay_bypass = Bypass::new(delay_node, d.enabled != 0);

    let r = &preset.reverb;
    let reverb =
        Reverb::new_with_params(AudioParam::Static(r.size), AudioParam::Static(r.damping), 0);

    let reverb_node = ParallelMixer::new(r.mix, reverb);
    let reverb_bypass = Bypass::new(reverb_node, r.enabled != 0);

    let widener = StereoWidener::new(AudioParam::Static(1.5));
    let gain = Gain::new_fixed(0.5);

    DspChain::new(voice, SAMPLE_RATE)
        .to_stereo()
        .and(delay_bypass)
        .and(reverb_bypass)
        .and(widener)
        .and(gain)
}

async fn print_stats(stack_ptr: usize) {
    let free = HEAP.free();
    let used = HEAP.used();
    log_status!("Core 1: DSP Chain initialized\r\n");
    log_status!("Core 1: Memory used: {} KB\r\n", used / 1024);
    log_status!("Core 1: Memory free: {} KB\r\n", free / 1024);

    let stack_slice =
        unsafe { core::slice::from_raw_parts(stack_ptr as *const u8, CORE1_STACK_SIZE) };
    let mut unused = 0;
    for &byte in stack_slice {
        if byte == 0x55 {
            unused += 1;
        } else {
            break;
        }
    }
    let used_stack = CORE1_STACK_SIZE - unused;
    log_status!(
        "Core 1: Stack used: {} / {} bytes\r\n",
        used_stack,
        CORE1_STACK_SIZE
    );
}

#[embassy_executor::task]
pub async fn core1_task(midi_control: Arc<MidiControl>, initial_preset: Preset, stack_ptr: usize) {
    disable_denormals();

    log_status!("Core 1: Starting...\r\n");
    midi_control.set_portamento(initial_preset.portamento);
    log_status!(
        "Core 1: Heap free before build: {} KB\r\n",
        HEAP.free() / 1024
    );

    let mut synth: Option<Box<dyn FrameProcessor<Stereo> + Send>> =
        Some(Box::new(build_synth(midi_control.clone(), initial_preset)));

    log_status!("Core 1: build_synth OK\r\n");

    print_stats(stack_ptr).await;
    log_status!("Core 1: DSP Running (STEREO) with Preset\r\n");

    let mut buffer = [0.0; BLOCK_SIZE];
    let mut frame_index: u64 = 0;

    let max_duration_us = (BLOCK_SIZE as f32 / 2.0 / SAMPLE_RATE * 1_000_000.0) as u64;

    // Track the *peak* per-buffer duration over each ~1s reporting window. Audio glitches
    // come from individual buffers overrunning the deadline, which a once-per-second
    // instantaneous sample almost always misses; the peak is the number that matters.
    let mut peak_duration_us: u64 = 0;

    // Announce we're live so core0 knows to wait for our park-ack around flash writes.
    CORE1_RUNNING.store(true, Ordering::Release);

    loop {
        // Park off PSRAM while core0 does a flash write + PSRAM reconfigure. The delay
        // buffer lives in PSRAM (shared QMI bus with flash), so running the synth during
        // a flash op would hit the clobbered CS1 config / post-op recovery window. We ack
        // that we've stopped touching PSRAM, then spin until core0 releases us. Output
        // underruns to silence meanwhile — same brief glitch a save already causes.
        if PSRAM_GATE_REQ.load(Ordering::Acquire) {
            PSRAM_GATE_ACK.store(true, Ordering::Release);
            while PSRAM_GATE_REQ.load(Ordering::Acquire) {
                cortex_m::asm::nop();
            }
            PSRAM_GATE_ACK.store(false, Ordering::Release);
            continue;
        }

        if let Ok(new_preset) = PRESET_CHANNEL.try_receive() {
            log_status!("Core 1: Switching Preset...\r\n");
            let _ = synth.take();
            print_stats(stack_ptr).await;
            midi_control.set_portamento(new_preset.portamento);

            synth = Some(Box::new(build_synth(midi_control.clone(), new_preset)));

            log_status!("Core 1: Preset Switched.\r\n");
            print_stats(stack_ptr).await;
        }

        let start_time = Instant::now();

        if let Some(s) = &mut synth {
            s.process(&mut buffer, frame_index);
        } else {
            buffer.fill(0.0);
        }

        let end_time = Instant::now();

        let duration = (end_time - start_time).as_micros();
        if duration > peak_duration_us {
            peak_duration_us = duration;
        }

        if frame_index % (SAMPLE_RATE as u64) < (BLOCK_SIZE as u64 / 2) {
            let load = (peak_duration_us as f32 / max_duration_us as f32) * 100.0;

            let underruns = AUDIO_UNDERRUNS.swap(0, Ordering::Relaxed);
            let queue_min = AUDIO_QUEUE_MIN.swap(u32::MAX, Ordering::Relaxed);

            let _ = LOG_CHANNEL.try_send(LogData {
                sample: buffer[0],
                duration_us: peak_duration_us,
                load_percent: load,
                underruns,
                queue_min: if queue_min == u32::MAX { 0 } else { queue_min },
            });

            // Mirror the peak over defmt/RTT so it's visible under `probe-rs run`
            // for the PSRAM-delay measurement (USB serial isn't captured there).
            defmt::info!(
                "Core1 peak {=u64} us / {=u64} us = {=f32} % | underruns {=u32} | delay {=str}",
                peak_duration_us,
                max_duration_us,
                load,
                underruns,
                if USE_PSRAM_DELAY { "PSRAM" } else { "SRAM" },
            );

            peak_duration_us = 0;
        }

        AUDIO_CHANNEL.send(AudioData { buffer }).await;

        frame_index = frame_index.wrapping_add((BLOCK_SIZE / 2) as u64);
    }
}
