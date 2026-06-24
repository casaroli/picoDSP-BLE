#![no_std]
#![no_main]

extern crate alloc;

mod bt;
mod common;
mod control;
mod data;
mod dsp;
mod tasks;
mod usb;

use alloc::sync::Arc;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;
use embassy_rp::pio::Pio;
use embassy_rp::pio_programs::i2s::{PioI2sOut, PioI2sOutProgram};

use defmt_rtt as _;
use embassy_executor::Spawner;
use embedded_alloc::LlffHeap as Heap;
use panic_halt as _;

use embassy_rp::bind_interrupts;
use embassy_rp::flash::Flash;
use embassy_rp::multicore::{Stack, spawn_core1};
use embassy_rp::peripherals::{DMA_CH0, DMA_CH1, DMA_CH2, PIO0, PIO1, USB};
use embassy_rp::usb::InterruptHandler;

use static_cell::StaticCell;

use crate::common::shared::{CORE1_STACK_SIZE, HEAP_SIZE, disable_denormals};
use crate::control::midi::MidiControl;
use crate::data::storage::Storage;
use crate::tasks::{core0, core1};

#[global_allocator]
pub static HEAP: Heap = Heap::empty();

bind_interrupts!(pub struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
    PIO0_IRQ_0 => embassy_rp::pio::InterruptHandler<PIO0>;
    PIO1_IRQ_0 => embassy_rp::pio::InterruptHandler<PIO1>;
    DMA_IRQ_0 => embassy_rp::dma::InterruptHandler<DMA_CH0>, embassy_rp::dma::InterruptHandler<DMA_CH1>, embassy_rp::dma::InterruptHandler<DMA_CH2>;
});

fn init_heap() {
    static mut HEAP_MEM: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];
    unsafe {
        HEAP.init(addr_of_mut!(HEAP_MEM) as usize, HEAP_SIZE);
    }
}

// #[unsafe(link_section = ".sram4")]
static mut CORE1_STACK: Stack<CORE1_STACK_SIZE> = Stack::new();

// #[unsafe(link_section = ".sram5")]
static EXECUTOR1: StaticCell<embassy_executor::Executor> = StaticCell::new();

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    disable_denormals();

    init_heap();
    let p = embassy_rp::init(Default::default());

    let flash = Flash::new(p.FLASH, p.DMA_CH0, Irqs);
    let mut storage = Storage::new(flash);
    storage.init().await;

    let preset = storage
        .load_preset(4)
        .await
        .unwrap_or_else(|| crate::data::presets::get_default_presets()[4]);

    let midi_control = Arc::new(MidiControl::new());

    let cutoff_norm = libm::log10f(preset.filter.cutoff / 20.0) / libm::log10f(1000.0);
    midi_control.set_parameter_1(cutoff_norm.clamp(0.0, 1.0));
    let res_norm = (preset.filter.resonance - 0.707) / 9.3;
    midi_control.set_parameter_2(res_norm.clamp(0.0, 1.0));

    let midi_control_core1 = midi_control.clone();

    unsafe {
        let stack_ptr = addr_of_mut!(CORE1_STACK) as *mut u8;
        core::ptr::write_bytes(stack_ptr, 0x55, CORE1_STACK_SIZE);
    }

    let stack_ptr_val = addr_of_mut!(CORE1_STACK) as usize;

    let Pio {
        mut common, sm0, ..
    } = Pio::new(p.PIO0, Irqs);

    let bit_clock_pin = p.PIN_10;
    let left_right_clock_pin = p.PIN_11;
    let data_pin = p.PIN_9;

    const SAMPLE_RATE: u32 = 48_000;
    const BIT_DEPTH: u32 = 16;

    let program = PioI2sOutProgram::new(&mut common);
    let mut i2s = PioI2sOut::new(
        &mut common,
        sm0,
        p.DMA_CH1,
        Irqs,
        data_pin,
        bit_clock_pin,
        left_right_clock_pin,
        SAMPLE_RATE,
        BIT_DEPTH,
        &program,
    );
    i2s.start();

    spawn_core1(
        p.CORE1,
        unsafe { &mut *addr_of_mut!(CORE1_STACK) },
        move || {
            let executor = EXECUTOR1.init(embassy_executor::Executor::new());
            executor.run(|spawner| {
                spawner.spawn(core1::core1_task(midi_control_core1, preset, stack_ptr_val).unwrap())
            });
        },
    );

    // Bluetooth host (BLE-MIDI keyboard) on core0. CYW43 owns PIN_23/24/25/29 + PIO1 + DMA_CH2;
    // the onboard LED moves to CYW43 GPIO 0 (driven inside this task), so PIN_25 — the radio's
    // chip-select on the Pico Plus 2 W — is no longer used as a plain GPIO.
    spawner.spawn(
        bt::bluetooth_task(p.PIN_23, p.PIN_25, p.PIN_24, p.PIN_29, p.PIO1, p.DMA_CH2).unwrap(),
    );

    core0::main_task(spawner, p.USB, i2s, midi_control, storage).await;
}
