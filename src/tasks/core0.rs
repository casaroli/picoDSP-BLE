use core::mem;

use alloc::sync::Arc;
use embassy_executor::Spawner;
use embassy_rp::Peri;
use embassy_rp::peripherals::{PIO0, USB};
use embassy_rp::pio_programs::i2s::PioI2sOut;
use embassy_rp::usb::Driver;
use static_cell::StaticCell;

use crate::common::shared::{
    AUDIO_CHANNEL, AUDIO_QUEUE_MIN, AUDIO_UNDERRUNS, BLOCK_SIZE, HEAP_SIZE, SAMPLE_RATE,
};
use crate::control::midi::{MidiControl, midi_task};
use crate::data::storage::Storage;
use crate::usb::device;
use crate::usb::logger;

pub async fn main_task(
    spawner: Spawner,
    usb: Peri<'static, USB>,
    mut i2s: PioI2sOut<'_, PIO0, 0>,
    midi_control: Arc<MidiControl>,
    storage: Storage<'static>,
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
            )
            .unwrap(),
        );
    }

    loop {
        let dma_future = i2s.write(front_buffer);

        // Observe the output queue depth before we block on it: empty == the DSP producer
        // failed to keep up for this block == an audible underrun. Track the low-water-mark
        // and count empties so the log shows whether bursts are actually causing glitches.
        let fill = AUDIO_CHANNEL.len() as u32;
        AUDIO_QUEUE_MIN.fetch_min(fill, core::sync::atomic::Ordering::Relaxed);
        if fill == 0 {
            AUDIO_UNDERRUNS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        }

        let audio_data = AUDIO_CHANNEL.receive().await;

        const I16_SCALE: f32 = i16::MAX as f32;

        for (dst, frame) in back_buffer.iter_mut().zip(audio_data.buffer.chunks_exact(2)) {
            let l = (frame[0] * I16_SCALE) as i16 as u16 as u32;
            let r = (frame[1] * I16_SCALE) as i16 as u16 as u32;
            *dst = (l << 16) | r;
        }

        dma_future.await;
        mem::swap(&mut back_buffer, &mut front_buffer);
    }
}
