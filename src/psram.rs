//! External PSRAM (APS6404L, 8 MiB) on QMI CS1 / GPIO47 — Pimoroni Pico Plus 2 W.
//!
//! Thin wrapper over embassy-rp's built-in `psram` driver, plus a power-on
//! self-test and bandwidth benchmark used to validate the part before we rely on
//! it for DSP buffers (delay lines, samples, etc.).
//!
//! The embassy driver is the MicroPython / Michael Bell timing lineage (same as
//! the reference we started from), but RAM-resident with hand-written asm for the
//! direct-mode init sequence and proper core1-pausing, so we prefer it over a
//! hand-ported `.data` driver.

use core::sync::atomic::{AtomicUsize, Ordering};

use defmt::info;
use embassy_rp::Peri;
use embassy_rp::clocks;
use embassy_rp::peripherals::{PIN_47, QMI_CS1};
use embassy_rp::psram::{Config, Psram};
use embassy_rp::qmi_cs1::QmiCs1;
use embassy_time::{Duration, Instant, block_for};

/// Cached, memory-mapped base of QMI CS1 (PSRAM) on RP2350.
/// Kept for the next phase (placing DSP buffers in PSRAM).
#[allow(dead_code)]
pub const PSRAM_BASE: usize = 0x1100_0000;

/// A successfully initialised, memory-mapped PSRAM region.
#[derive(Clone, Copy)]
pub struct PsramRegion {
    base: usize,
    size: usize,
}

impl PsramRegion {
    #[inline]
    pub fn base(&self) -> usize {
        self.base
    }

    #[inline]
    pub fn size(&self) -> usize {
        self.size
    }

    #[inline]
    #[allow(dead_code)] // used once we allocate DSP buffers into PSRAM
    pub fn as_mut_ptr(&self) -> *mut u8 {
        self.base as *mut u8
    }
}

/// Detect + initialise the PSRAM and return its mapped region.
///
/// Panics if the APS6404L is not detected — on this board PSRAM is expected, so a
/// missing/failed part is fatal (matches the reference driver's behaviour).
/// APS6404L maximum memory operating frequency (matches `Config::aps6404l`'s
/// `max_mem_freq`). The QMI SCK is clk_sys divided down to stay at or below this.
const APS6404L_MAX_MEM_HZ: u32 = 133_000_000;

/// QMI CS1 clock divisor the embassy psram driver derives for a given clk_sys —
/// `ceil(clk_sys / 133 MHz)`, floored at 2 above 100 MHz clk_sys (see embassy-rp
/// `psram.rs`). Mirrored here only so the boot log can report the real SCK.
fn psram_divisor(clk_sys_hz: u32) -> u32 {
    let mut divisor = (clk_sys_hz + APS6404L_MAX_MEM_HZ - 1) / APS6404L_MAX_MEM_HZ;
    if divisor == 1 && clk_sys_hz > 100_000_000 {
        divisor = 2;
    }
    divisor
}

/// PSRAM controller config shared by init and reinit.
fn psram_config() -> Config {
    let mut config = Config::aps6404l();
    // The built-in default assumes a 125 MHz system clock; feed it the *actual*
    // clock so the derived divisor / rxdelay / max_select / min_deselect are right.
    config.clock_hz = clocks::clk_sys_freq();
    config
}

pub fn init(qmi_cs1: Peri<'static, QMI_CS1>, cs1_pin: Peri<'static, PIN_47>) -> PsramRegion {
    let cs1 = QmiCs1::new(qmi_cs1, cs1_pin);

    let config = psram_config();
    let size = config.mem_size;

    match Psram::new(cs1, config) {
        Ok(psram) => {
            // base_address() is the cached CS1 window (PSRAM_BASE).
            let region = PsramRegion {
                base: psram.base_address() as usize,
                size: psram.size(),
            };
            // Report the *actual* QMI CS1 SCK, not just clk_sys — the bus runs at
            // clk_sys / divisor, where the driver picks divisor = ceil(clk_sys / 133 MHz)
            // (forced to >=2 above 100 MHz clk_sys). This is what answers "is PSRAM at
            // full speed": 200 MHz clk_sys -> divisor 2 -> 100 MHz SCK.
            let sys_hz = clocks::clk_sys_freq();
            let divisor = psram_divisor(sys_hz);
            info!(
                "PSRAM: APS6404L OK — {} MiB @ {=usize:#x}, clk_sys {} MHz / div {} = SCK {} MHz (max {} MHz)",
                region.size / (1024 * 1024),
                region.base,
                sys_hz / 1_000_000,
                divisor,
                sys_hz / divisor / 1_000_000,
                APS6404L_MAX_MEM_HZ / 1_000_000,
            );
            region
        }
        Err(e) => {
            // size is captured only to make the message useful.
            let _ = size;
            defmt::panic!("PSRAM init failed: {:?}", e);
        }
    }
}

/// Lock core1 off the PSRAM for the duration of a flash write + reconfigure, then
/// release with [`unlock_core1`]. The PSRAM-backed delay buffer shares the QMI bus
/// with flash; a flash op clobbers the CS1 config and leaves a post-op recovery
/// window, so core1 must not touch PSRAM across the whole sequence. We request the
/// park and `await` core1's ack — async (not a busy spin) so core0's executor keeps
/// draining the audio channel; otherwise core1 could block in `AUDIO_CHANNEL.send`
/// and never reach the loop top to ack (deadlock). If core1 isn't running yet (boot
/// self-test) no ack will come, so we don't wait.
pub async fn lock_core1_off_psram() {
    use crate::common::shared::{CORE1_RUNNING, PSRAM_GATE_ACK, PSRAM_GATE_REQ};
    PSRAM_GATE_REQ.store(true, Ordering::Release);
    if CORE1_RUNNING.load(Ordering::Acquire) {
        while !PSRAM_GATE_ACK.load(Ordering::Acquire) {
            embassy_time::Timer::after(Duration::from_micros(50)).await;
        }
    }
}

/// Release core1 after a flash write + reconfigure (see [`lock_core1_off_psram`]).
/// Call only after [`after_flash_write`] has restored the CS1 config, so core1
/// resumes with valid PSRAM access.
pub fn unlock_core1() {
    crate::common::shared::PSRAM_GATE_REQ.store(false, Ordering::Release);
}

/// Settle the PSRAM and fully re-init the CS1 controller after a flash erase/program,
/// preserving prior PSRAM contents. Caller must have core1 parked
/// ([`lock_core1_off_psram`]) so the PSRAM is idle through the recovery window.
pub fn after_flash_write() {
    // Two ingredients are required together (see
    // docs/psram-flash-corruption-investigation.md):
    //   1. a short IDLE SETTLE after the flash op — accessing PSRAM too soon corrupts a
    //      handful of already-stored words (0 ms -> ~20 corrupt, 100 µs -> 0; we use a
    //      comfortable 1 ms, dwarfed by the tens of ms the erase already stalls audio);
    //   2. a FULL `reinit()` (Psram::new: chip reset 0xf5 / re-detect / re-enter QPI 0x35
    //      / M1 regs) — re-applying just the clobbered M1 registers after the settle is
    //      NOT enough (measured 28 corrupt words); the chip-level re-init is what makes
    //      the content survive.
    block_for(Duration::from_millis(1));
    reinit();
    // reinit + settle preserves prior PSRAM contents, so there's nothing to rebuild —
    // the old SYNTH_RESET_REQ delay-rezero path is gone.
}

/// Re-establish PSRAM access after a flash op clobbered the CS1 config. Re-runs the
/// full embassy init (chip reset 0xf5, re-detect, re-enter QPI 0x35, reconfigure M1).
/// `Psram::new` pauses core1 and runs its timing-critical parts from RAM, so it's
/// safe to call from flash. The peripherals were consumed at boot, so steal them
/// back — we statically know nothing else owns QMI CS1 / GPIO47.
pub fn reinit() {
    boost_qspi_pads();
    let cs1 = QmiCs1::new(unsafe { QMI_CS1::steal() }, unsafe { PIN_47::steal() });
    // Ensure core1's resume from the preceding flash op's pause is globally visible before
    // Psram::new issues its own `pause_core1` — otherwise the FIFO handshake can nest/deadlock.
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
    match Psram::new(cs1, psram_config()) {
        Ok(_) => info!("PSRAM: re-init after flash op OK"),
        Err(e) => defmt::error!("PSRAM re-init after flash failed: {:?}", e),
    }
}

/// Set the shared QSPI data pads (SCLK + SD0..3) to max drive / fast slew.
/// Hygiene — flash and PSRAM share these pads and a flash op can lower their drive.
fn boost_qspi_pads() {
    use embassy_rp::pac;
    // PADS_QSPI.gpio(n): 0=SCLK, 1=SD0, 2=SD1, 3=SD2, 4=SD3, 5=SS.
    for n in 0..=4usize {
        pac::PADS_QSPI.gpio(n).modify(|w| {
            w.set_drive(pac::pads::vals::Drive::_12M_A);
            w.set_slewfast(true);
            w.set_ie(true);
        });
    }
    cortex_m::asm::dsb();
}

// --- Minimal bump allocator over PSRAM ------------------------------------
//
// Used to back DSP buffers (delay lines) with PSRAM instead of the SRAM heap.
// Single producer (core1's `build_synth`); never frees individually. Call
// `reset_alloc()` before rebuilding the synth so preset switches don't leak —
// safe only once every previously-handed-out slice has been dropped.

static ALLOC_BASE: AtomicUsize = AtomicUsize::new(0);
static ALLOC_NEXT: AtomicUsize = AtomicUsize::new(0);
static ALLOC_END: AtomicUsize = AtomicUsize::new(0);

/// Arm the bump allocator over the given region. Must be called before
/// `spawn_core1` so the stores are visible to core1 (the spawn handshake
/// provides the cross-core barrier).
pub fn init_alloc(region: &PsramRegion) {
    ALLOC_BASE.store(region.base(), Ordering::Release);
    ALLOC_NEXT.store(region.base(), Ordering::Release);
    ALLOC_END.store(region.base() + region.size(), Ordering::Release);
}

/// Reset the bump pointer to the region base. Only safe when no slice from a
/// previous `alloc_f32_slice` is still alive.
pub fn reset_alloc() {
    ALLOC_NEXT.store(ALLOC_BASE.load(Ordering::Acquire), Ordering::Release);
}

/// Bump-allocate a zeroed `&'static mut [f32]` of `len` elements from PSRAM.
/// Panics if the region is exhausted. `len*4` is always a multiple of 4 and the
/// base is 16-aligned, so returned pointers stay f32-aligned.
pub fn alloc_f32_slice(len: usize) -> &'static mut [f32] {
    let bytes = len * core::mem::size_of::<f32>();
    let start = ALLOC_NEXT.fetch_add(bytes, Ordering::AcqRel);
    let end = ALLOC_END.load(Ordering::Acquire);
    assert!(start != 0, "PSRAM allocator not initialised");
    assert!(start + bytes <= end, "PSRAM bump allocator out of memory");
    let ptr = start as *mut f32;
    let slice = unsafe { core::slice::from_raw_parts_mut(ptr, len) };
    slice.fill(0.0);
    slice
}

/// Address-dependent test pattern so stuck/aliased/shorted address lines are caught
/// (a plain incrementing counter would miss many aliasing faults).
#[inline(always)]
fn pattern(word_index: usize) -> u32 {
    (word_index as u32).wrapping_mul(0x9E37_79B1) ^ 0x1111_1100
}

/// Write a pattern across the whole region and read it back twice. Returns the
/// byte offset of the first mismatch on failure.
#[inline(never)]
pub fn self_test(region: &PsramRegion) -> Result<(), usize> {
    let base = region.base() as *mut u32;
    let words = region.size() / 4;

    for i in 0..words {
        unsafe { core::ptr::write_volatile(base.add(i), pattern(i)) };
    }

    for pass in 0..2u8 {
        for i in 0..words {
            let got = unsafe { core::ptr::read_volatile(base.add(i)) };
            let want = pattern(i);
            if got != want {
                info!(
                    "PSRAM self-test FAIL pass {} @word {}: got {=u32:#x} want {=u32:#x}",
                    pass, i, got, want
                );
                return Err(i * 4);
            }
        }
    }

    info!(
        "PSRAM self-test PASS — {} MiB verified",
        region.size() / (1024 * 1024)
    );
    Ok(())
}

/// bytes/microsecond == MB/s exactly (1e6 us = 1 s, 1e6 bytes = 1 MB).
#[inline(always)]
fn mb_per_s(bytes: usize, micros: u64) -> u64 {
    if micros == 0 {
        0
    } else {
        bytes as u64 / micros
    }
}

/// Measure sequential write / sequential read / cache-hit read / random-read
/// latency over the mapped region and log the results.
///
/// Notes:
/// - Accesses are per-word `read_volatile`/`write_volatile`, i.e. the realistic
///   per-sample access pattern of a delay line — not a batched memcpy ceiling.
/// - This loop executes from flash XIP while hitting PSRAM XIP (CS1), so the
///   numbers already include QMI bus contention with instruction fetch, which is
///   exactly the condition the DSP loop would face.
#[inline(never)]
pub fn bench(region: &PsramRegion) {
    let base = region.base() as *mut u32;
    let words = region.size() / 4;
    let bytes = words * 4;

    // --- sequential write (whole region) ---
    let t0 = Instant::now();
    for i in 0..words {
        unsafe { core::ptr::write_volatile(base.add(i), i as u32) };
    }
    let wr_us = t0.elapsed().as_micros();

    // --- sequential read (whole region >> 16 KiB XIP cache => true read BW) ---
    let mut acc: u32 = 0;
    let t1 = Instant::now();
    for i in 0..words {
        acc = acc.wrapping_add(unsafe { core::ptr::read_volatile(base.add(i)) });
    }
    let rd_us = t1.elapsed().as_micros();
    core::hint::black_box(acc);

    info!(
        "PSRAM seq write: {} KiB in {} us -> {} MB/s",
        bytes / 1024,
        wr_us,
        mb_per_s(bytes, wr_us)
    );
    info!(
        "PSRAM seq read:  {} KiB in {} us -> {} MB/s",
        bytes / 1024,
        rd_us,
        mb_per_s(bytes, rd_us)
    );

    // --- cache-hit read: 8 KiB working set (fits in 16 KiB XIP cache), reread x1024 ---
    let small_words = 2048usize; // 8 KiB
    let iters = 1024usize;
    let mut acc2: u32 = 0;
    let t2 = Instant::now();
    for _ in 0..iters {
        for i in 0..small_words {
            acc2 = acc2.wrapping_add(unsafe { core::ptr::read_volatile(base.add(i)) });
        }
    }
    let crd_us = t2.elapsed().as_micros();
    core::hint::black_box(acc2);
    let cbytes = small_words * 4 * iters;
    info!(
        "PSRAM cached read (8 KiB x{}): {} us -> {} MB/s",
        iters,
        crd_us,
        mb_per_s(cbytes, crd_us)
    );

    // --- random 32-bit read latency (working set = whole 8 MiB) ---
    // words is a power of two for an 8 MiB region, so mask-index is uniform.
    let n = 100_000usize;
    let mask = words - 1;
    let mut idx = 0x1234_5usize;
    let mut acc3: u32 = 0;
    let t3 = Instant::now();
    for _ in 0..n {
        idx = idx.wrapping_mul(2_654_435_761).wrapping_add(1) & mask;
        acc3 = acc3.wrapping_add(unsafe { core::ptr::read_volatile(base.add(idx)) });
    }
    let rnd_us = t3.elapsed().as_micros();
    core::hint::black_box(acc3);
    info!(
        "PSRAM random read: {} reads in {} us -> {} ns/read",
        n,
        rnd_us,
        (rnd_us * 1000) / n as u64
    );
}
