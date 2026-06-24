#![no_std]
#![no_main]

extern crate alloc;

mod bt;
mod common;
mod control;
mod data;
mod dsp;
mod flash_diag;
mod psram;
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

// Symbols from the `.ram_code` section in memory.x (cyw43/cyw43_pio text placed
// in RAM with a FLASH load address).
unsafe extern "C" {
    static __sram_code_start: u8;
    static __sram_code_end: u8;
    static __sram_code_load: u8;
}

/// Copy the RAM-resident cyw43 code from its FLASH load address into SRAM.
/// MUST run before any cyw43 code executes (i.e. before the BT task spawns).
fn init_ram_code() {
    unsafe {
        let start = &raw const __sram_code_start as usize;
        let end = &raw const __sram_code_end as usize;
        let load = &raw const __sram_code_load as usize;
        let len = end - start;
        core::ptr::copy_nonoverlapping(load as *const u8, start as *mut u8, len);
        // Ensure the copied instructions are visible before they're fetched.
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
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
    // Relocate cyw43 code into SRAM before anything touches the radio.
    init_ram_code();
    let p = embassy_rp::init(Default::default());

    // Log the bootrom flash XIP config, raise CS0 to clk_sys/2 with an RXDELAY sweep,
    // then log again to confirm the new divisor. Runs single-core before core1/PSRAM/
    // DMA/USB start, so the sweep is the only flash user while it retimes CS0.
    flash_diag::log_flash_xip_config();
    flash_diag::speed_up_flash_xip();
    flash_diag::log_flash_xip_config();

    // Bring up external PSRAM (APS6404L, 8 MiB on QMI CS1 / GPIO47) before core1
    // is spawned, then validate it. The init pauses core1 internally (a no-op
    // here since it isn't running yet) and runs from RAM, so flash XIP is safe.
    let psram_region = psram::init(p.QMI_CS1, p.PIN_47);
    if psram::self_test(&psram_region).is_err() {
        defmt::panic!("PSRAM self-test failed — refusing to continue");
    }
    psram::bench(&psram_region);
    // Arm the PSRAM bump allocator before spawning core1 so its build_synth can
    // back the delay ring buffers with PSRAM.
    psram::init_alloc(&psram_region);

    let flash = Flash::new(p.FLASH, p.DMA_CH0, Irqs);
    let mut storage = Storage::new(flash);
    storage.init().await;

    // --- PSRAM-across-flash verification (remove once confirmed) --------------
    // Sentinel -> non-destructive flash erase+write -> after_flash_write() (1 ms
    // idle settle + lightweight CS1 register restore) -> read back. Expect 0
    // mismatches: PSRAM content now survives a flash write. Runs single-core
    // before core1 spawns, so nothing else touches PSRAM during the check.
    {
        let base = psram_region.base() as *mut u32; // cached CS1 = 0x1100_0000
        let n = 16384usize; // write 64 KiB (> 16 KiB XIP cache)
        let check = 8192usize; // verify first 32 KiB (genuinely evicted to PSRAM)
        let pat = |i: usize| 0xC0DE_0000u32.wrapping_add(i as u32);
        let count = |p: *mut u32| -> usize {
            let mut m = 0;
            for i in 0..check {
                if unsafe { core::ptr::read_volatile(p.add(i)) } != pat(i) {
                    m += 1;
                }
            }
            m
        };

        for i in 0..n {
            unsafe { core::ptr::write_volatile(base.add(i), pat(i)) };
        }
        cortex_m::asm::dsb();
        let pre = count(base);

        let mut sector = [0u8; 4096];
        storage.read_raw(&mut sector).await;
        storage.write_raw(&sector).await; // -> after_flash_write(): settle + restore

        let post = count(base);
        defmt::info!(
            "PSRAM across-flash verify: pre {}/{} post {}/{} (expect 0/0)",
            pre,
            check,
            post,
            check
        );
    }

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
