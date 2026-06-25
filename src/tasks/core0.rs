use core::mem;
use core::sync::atomic::Ordering;

use alloc::sync::Arc;
use embassy_executor::Spawner;
use embassy_rp::Peri;
use embassy_rp::peripherals::{PIO0, USB};
use embassy_rp::pio_programs::i2s::PioI2sOut;
use embassy_rp::usb::Driver;
use embassy_time::{Duration, with_timeout};
use static_cell::StaticCell;

use crate::common::shared::{
    AUDIO_CHANNEL, AUDIO_QUEUE_MIN, AUDIO_UNDERRUNS, BLOCK_SIZE, HEAP_SIZE, SAMPLE_RATE,
    USB_AUDIO_CHANNEL, USB_AUDIO_STREAMING,
};
use crate::control::midi::{MidiControl, midi_task};
use crate::data::storage::Storage;
use crate::usb::device::{self, UsbMicrophone};
use crate::usb::logger;

pub async fn main_task(
    spawner: Spawner,
    usb: Peri<'static, USB>,
    mut i2s: PioI2sOut<'_, PIO0, 0>,
    midi_control: Arc<MidiControl>,
    storage: Storage<'static>,
    needs_format: bool,
) {
    let usb_device = {
        let driver = Driver::new(usb, crate::Irqs);
        Some(device::init(spawner, driver))
    };

    const BUFFER_SIZE: usize = BLOCK_SIZE / 2; // u32 I2S frames per DMA buffer
    static DMA_BUFFER: StaticCell<[u32; BUFFER_SIZE * 2]> = StaticCell::new();
    let dma_buffer = DMA_BUFFER.init_with(|| [0u32; BUFFER_SIZE * 2]);
    let (mut back_buffer, mut front_buffer) = dma_buffer.split_at_mut(BUFFER_SIZE);

    if let Some(device) = usb_device {
        spawner
            .spawn(logger::logger_task(device.sender, SAMPLE_RATE, BLOCK_SIZE, HEAP_SIZE).unwrap());

        spawner.spawn(
            midi_task(
                device.midi_receiver,
                device.midi_sender,
                midi_control,
                storage,
                needs_format,
            )
            .unwrap(),
        );

        // Mirror the audio to the host over USB (UAC1) in addition to the DAC. Best-effort:
        // it consumes the USB_AUDIO_CHANNEL tee the I2S loop feeds below, and only writes
        // while the host actually has the stream open.
        spawner.spawn(microphone_task(device.microphone).unwrap());
    }

    loop {
        let dma_future = i2s.write(front_buffer);

        // Give the I2S output DMA preferential treatment in the DMA scheduler so the cyw43
        // radio's SPI bursts (DMA_CH2, also DMA-driven) can't delay it and underflow the PIO
        // FIFO. The I2S DMA is DMA_CH1; embassy rewrites its ctrl_trig on every write(),
        // clearing the bit, so re-assert it each block.
        embassy_rp::pac::DMA
            .ch(1)
            .ctrl_trig()
            .modify(|w| w.set_high_priority(true));

        // Observe the output queue depth before we block on it: empty == the DSP producer
        // failed to keep up for this block == an audible underrun. Track the low-water-mark
        // and count empties so the log shows whether bursts are actually causing glitches.
        let fill = AUDIO_CHANNEL.len() as u32;
        AUDIO_QUEUE_MIN.fetch_min(fill, core::sync::atomic::Ordering::Relaxed);
        if fill == 0 {
            AUDIO_UNDERRUNS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        }

        let audio_data = AUDIO_CHANNEL.receive().await;

        // Tee the block to the USB microphone stream. Non-blocking: if USB is closed or the
        // mic task can't keep up, the send is dropped so the DAC's DMA cadence is untouched.
        let _ = USB_AUDIO_CHANNEL.try_send(audio_data);

        for (dst, frame) in back_buffer
            .iter_mut()
            .zip(audio_data.buffer.chunks_exact(2))
        {
            let l = (frame[0] * I16_SCALE) as i16 as u16 as u32;
            let r = (frame[1] * I16_SCALE) as i16 as u16 as u32;
            *dst = (l << 16) | r;
        }

        dma_future.await;
        mem::swap(&mut back_buffer, &mut front_buffer);
    }
}

/// Stream the audio tee to the host over USB (UAC1 isochronous IN). Secondary to the DAC:
/// it never back-pressures the I2S loop (that loop only `try_send`s). Repacketizes the
/// 256-sample (128-frame) DSP blocks into 48-frame / 1 ms USB packets, matching the host's
/// SOF cadence. Only writes while the host has the stream open (`USB_AUDIO_STREAMING`); a
/// stalled host is bounded by a write timeout so the task can't wedge and stall the tee.
#[embassy_executor::task]
async fn microphone_task(mut microphone: UsbMicrophone) {
    const FRAMES_PER_PACKET: usize = (SAMPLE_RATE as usize) / 1000; // 48 frames = 1 ms @ 48 kHz

    let mut block: [f32; BLOCK_SIZE] = [0.0; BLOCK_SIZE];
    // Sample index within `block`; start past the end to force an initial receive.
    let mut idx = BLOCK_SIZE;

    loop {
        // While the host hasn't opened the stream, the iso IN endpoint isn't polled, so a
        // write would block forever. Keep the tee drained instead so the I2S `try_send`
        // never fills up and starts dropping.
        if !USB_AUDIO_STREAMING.load(Ordering::Relaxed) {
            let _ = USB_AUDIO_CHANNEL.receive().await;
            idx = BLOCK_SIZE;
            continue;
        }

        let mut packet = [0u8; FRAMES_PER_PACKET * 2 * 2]; // stereo, 16-bit LE
        let mut collected = 0;

        while collected < FRAMES_PER_PACKET {
            if idx >= BLOCK_SIZE {
                block = USB_AUDIO_CHANNEL.receive().await.buffer;
                idx = 0;
            }

            let avail_frames = (BLOCK_SIZE - idx) / 2;
            let n = avail_frames.min(FRAMES_PER_PACKET - collected);
            for _ in 0..n {
                let l = (block[idx].clamp(-1.0, 1.0) * I16_SCALE) as i16;
                let r = (block[idx + 1].clamp(-1.0, 1.0) * I16_SCALE) as i16;
                idx += 2;

                let b = collected * 4;
                packet[b..b + 2].copy_from_slice(&l.to_le_bytes());
                packet[b + 2..b + 4].copy_from_slice(&r.to_le_bytes());
                collected += 1;
            }
        }

        // Bound the write: if the host stopped polling without dropping to alt 0 (e.g. app
        // hang) this returns rather than blocking the task — which would back the tee up.
        if with_timeout(Duration::from_millis(10), microphone.write_packet(&packet))
            .await
            .is_err()
        {
            idx = BLOCK_SIZE;
        }
    }
}

const I16_SCALE: f32 = i16::MAX as f32;
