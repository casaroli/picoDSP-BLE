use crate::data::presets::Preset;
use core::sync::atomic::{AtomicBool, AtomicU32};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;

/// Set by core0 after a flash erase/program completes. A flash op corrupts a few
/// already-stored words of the PSRAM-backed delay buffer (and the corruption then
/// recirculates through the delay feedback), so core1 must rebuild the synth to
/// re-zero the delay buffer once the flash op is done. See [`crate::psram::reinit`].
pub static SYNTH_RESET_REQ: AtomicBool = AtomicBool::new(false);

/// Count of audio-output underruns (AUDIO_CHANNEL found empty when core0 needed a block).
/// A nonzero value is a click/stutter; this is the metric that actually matters, not the
/// core1 peak load (short bursts are absorbed by the output queue).
pub static AUDIO_UNDERRUNS: AtomicU32 = AtomicU32::new(0);
/// Low-water-mark of AUDIO_CHANNEL fill level since last report (how close we got to empty).
pub static AUDIO_QUEUE_MIN: AtomicU32 = AtomicU32::new(u32::MAX);

pub const SAMPLE_RATE: f32 = 48000.0;
// BLE statics (CYW43 state + TrouBLE HostResources) cost ~34 KB of static RAM.
// The main ARM stack lives between BSS top and 0x20080000 and needs ~45 KB of
// headroom for the CYW43 firmware-loading and TrouBLE init paths (going too high
// caused a silent main-stack overflow that stopped USB enumerating).
//
// Two changes reclaimed room: the delay ring buffers now live in PSRAM (see
// PsramDelay), freeing ~115 KB of heap usage; and ~78 KB of hot code
// (cyw43 + infinitedsp_core DSP chain + libm) is relocated to RAM (.ram_code) to
// keep instruction fetch off the QMI bus. The heap is cut from 400 KB to 256 KB
// to pay for the relocated code while keeping stack headroom:
// BSS(~64 KB) + heap(256 KB) + .ram_code(~78 KB) + .data leaves ~100 KB of stack.
pub const HEAP_SIZE: usize = 256000;
pub const BLOCK_SIZE: usize = 256;
pub const CORE1_STACK_SIZE: usize = 16384;

pub struct AudioData {
    pub buffer: [f32; BLOCK_SIZE],
}

#[derive(Clone, Copy)]
pub enum SystemCommand {
    ResetStorage,
}

pub static AUDIO_CHANNEL: Channel<CriticalSectionRawMutex, AudioData, 4> = Channel::new();
pub static PRESET_CHANNEL: Channel<CriticalSectionRawMutex, Preset, 1> = Channel::new();
pub static COMMAND_CHANNEL: Channel<CriticalSectionRawMutex, SystemCommand, 2> = Channel::new();

/// Decoded channel-voice MIDI messages `[status, data1, data2]` arriving over BLE-MIDI.
/// The BLE task parses notifications into these and the `midi_task` consumes them through
/// the same handler the USB-MIDI path uses, so both transports converge on one code path.
pub static BLE_MIDI_CHANNEL: Channel<CriticalSectionRawMutex, [u8; 3], 16> = Channel::new();

pub fn disable_denormals() {
    unsafe {
        let fpscr: u32;
        core::arch::asm!("vmrs {}, fpscr", out(reg) fpscr);
        let new_fpscr = fpscr | (1 << 24) | (1 << 25);
        core::arch::asm!("vmsr fpscr, {}", in(reg) new_fpscr);
    }
}
