//! Read-only diagnostic for the QMI CS0 (flash / XIP) configuration.
//!
//! On RP2350 there is no RP2040-style boot2: the bootrom scans the QSPI flash at
//! reset, picks the fastest read mode that works, and writes a default XIP setup
//! function into bootram with the discovered mode + clock divisor. `embassy_rp::init`
//! does not retune CS0, so the flash runs at whatever the bootrom left.
//!
//! `log_flash_xip_config` logs the *live* QMI M0 registers so we can see the actual
//! divisor and read mode rather than inferring them. Purely observational.
//!
//! `speed_up_flash_xip` (phase 1) drops the CS0 clock divisor to 2 (clk_sys/2, the
//! hardware max) and sweeps RXDELAY to find the read-data sample point that works at
//! the higher SCK, verifying each candidate against a golden copy of flash before
//! trusting it. The read *mode* (EBh quad-IO) is left untouched — only the clock and
//! sample timing change.
//!
//! Measured phase-1 result (Pimoroni Pico Plus 2 W, clk_sys 150 MHz): bootrom left CS0
//! at clkdiv=3 (50 MHz), rxdelay=2. Phase 1 moved it to clkdiv=2 (75 MHz) with a
//! healthy 3-wide rxdelay eye (pass bits rx1..rx3), centered on rxdelay=2. Stable
//! end-to-end (PSRAM self-test + benchmarks unaffected).
//!
//! ---------------------------------------------------------------------------------
//! PHASE 2 (not implemented — continuous-read mode to drop the command prefix)
//! ---------------------------------------------------------------------------------
//! After phase 1 the read format is still: prefix_width=0 (single-lane) prefix_len=_8
//! addr_width=2 (quad) data_width=2 (quad) dummy_len=4, read cmd 0xEB. The 8-bit
//! single-lane `0xEB` prefix is re-sent on *every* cache-line fill — 8 SCK cycles of
//! pure overhead before the quad burst. The bootrom keeps it so flash stays in a
//! serial-command-compatible state (so erase/program "just work"). See the
//! `flash_select_xip_read_mode` ROM docs in embassy-rp `rom_data/rp235x.rs`.
//!
//! Continuous-read mode ("Performance Enhance" on Winbond W25Q parts) lets subsequent
//! reads skip the opcode: the QSPI device latches "stay in quad read" from the EBh
//! mode/"M" bits, so only address+dummy+data are sent. Rough saving on an uncached
//! 4-byte read: 8 (prefix) + 6 (addr) + 4 (dummy) + 8 (data) ≈ 26 SCK -> ~18 SCK,
//! ~30% fewer cycles per cache-line fill (less on cache-friendly code; the XIP cache
//! already hides repeats). Combined with the 50->75 MHz from phase 1 it's ~2x on cold
//! flash reads, and — because flash (CS0) and PSRAM (CS1) share the QMI bus — frees
//! bus cycles for the core1 PSRAM delay-line traffic.
//!
//! Sketch (all of it RAM-resident + interrupts-masked + golden-validated, exactly like
//! the phase-1 sweep, since we are again retiming the bus we execute from):
//!   1. Put the flash into continuous read mode: drive the EBh access with the mode
//!      bits = 0xA0 (M5:4=10) so the device stays in quad continuous read. On RP2350
//!      this is the QMI Mx_RFMT/Mx_RCMD "suffix"/mode-bit fields rather than a separate
//!      flash command; configure CS0 the way the embassy `psram` driver configures CS1
//!      quad mode (see `qmi.mem(1).rfmt()/.rcmd()` in embassy-rp `psram.rs`).
//!   2. Set Mx_RFMT.prefix_len = NONE (drop the 8-bit opcode) and keep prefix_width as
//!      appropriate; set the mode/suffix bits so the device sees the continuation code.
//!   3. Re-run the same golden-compare validation as phase 1 before trusting it, and
//!      keep the same restore-on-failure fallback.
//! CAUTION: with the command prefix gone, flash is no longer in a plain serial-command
//! state, so the `Storage` flash driver's erase/program path (embassy_rp::flash) must
//! still round-trip it back to serial mode for writes (the ROM connect/exit-xip +
//! flush + restore-XIP dance). Validate read AND a storage write/erase cycle before
//! shipping phase 2.

use defmt::info;
use embassy_rp::clocks;
use embassy_rp::pac;
use embassy_rp::pac::qmi::regs::Timing;
use embassy_rp::pac::qmi::vals::PrefixLen;

/// CS0 (flash) XIP window base.
const FLASH_BASE: usize = 0x1000_0000;
/// Words of flash compared per candidate when validating a timing. 1 KiB of the
/// vector table / boot region — always populated, with enough entropy (addresses,
/// not 0xFF fill) that a bad RXDELAY produces mismatches.
const SAMPLE_WORDS: usize = 256;
/// RXDELAY is a 3-bit field, so the whole space is 0..=7.
const RXDELAY_MAX: u8 = 7;
/// Target divisor: clk_sys/2 is the fastest the QMI can drive SCK.
const TARGET_CLKDIV: u8 = 2;

/// Outcome of the RXDELAY sweep at `TARGET_CLKDIV`.
#[derive(Clone, Copy)]
struct SweepResult {
    /// Bit `i` set == reads validated at RXDELAY `i` (the data-eye width).
    pass_bitmap: u8,
    /// Chosen sample point (center of the widest passing run), or `None` if nothing
    /// validated (in which case the bootrom timing was restored).
    chosen: Option<u8>,
}

/// Log the QMI CS0 (flash) clock divisor, RX sample delay, and XIP read format.
///
/// `clkdiv` is the SCK period in system-clock cycles (1..=255 direct, 0 == 256), so
/// the effective QSPI clock is `clk_sys / clkdiv`. Maximum supported flash speed is
/// `clkdiv = 2` (clk_sys/2). The bootrom default is typically larger, and also keeps
/// an 8-bit serial command prefix on every access (PrefixLen::_8), which a hand-tuned
/// continuous-read / QPI setup would drop.
pub fn log_flash_xip_config() {
    let m0 = pac::QMI.mem(0);
    let timing = m0.timing().read();
    let rfmt = m0.rfmt().read();
    let rcmd = m0.rcmd().read();

    let clkdiv = timing.clkdiv();
    let sys_hz = clocks::clk_sys_freq();
    // clkdiv == 0 encodes a divisor of 256.
    let divisor: u32 = if clkdiv == 0 { 256 } else { clkdiv as u32 };
    let qspi_hz = sys_hz / divisor;

    info!(
        "FLASH XIP: clkdiv={} -> SCK {} MHz (clk_sys {} MHz), rxdelay={}",
        clkdiv,
        qspi_hz / 1_000_000,
        sys_hz / 1_000_000,
        timing.rxdelay(),
    );
    // The rp-pac enums don't implement defmt::Format in this build, so log the raw
    // field encodings. Widths: 0=single 1=dual 2=quad. prefix_len: 0=none 1=8-bit.
    info!(
        "FLASH XIP read fmt: prefix_width={} prefix_len={} addr_width={} data_width={} dummy_len={} (read cmd {=u8:#04x})",
        rfmt.prefix_width().to_bits(),
        rfmt.prefix_len().to_bits(),
        rfmt.addr_width().to_bits(),
        rfmt.data_width().to_bits(),
        rfmt.dummy_len().to_bits(),
        rcmd.prefix(),
    );

    // Quick verdict so the log is self-explanatory without cross-referencing the datasheet.
    let max_speed = divisor <= 2;
    let has_cmd_prefix = matches!(rfmt.prefix_len(), PrefixLen::_8);
    info!(
        "FLASH XIP: at_max_clk={} (clkdiv {} of min 2), per-access_8bit_cmd_prefix={}",
        max_speed, clkdiv, has_cmd_prefix,
    );
}

/// Phase 1 flash speed-up: set CS0 CLKDIV=2 (clk_sys/2) and pick a working RXDELAY.
///
/// Must be called early in `main` (single core, before core1/PSRAM/DMA/USB are
/// running) so the only flash traffic is this core's instruction fetch — which the
/// sweep routine moves into SRAM for the duration. Leaves the divisor at the bootrom
/// default if no RXDELAY validates at the higher clock, so it can never brick boot.
pub fn speed_up_flash_xip() {
    let orig = pac::QMI.mem(0).timing().read();
    let cur = orig.clkdiv();
    // clkdiv==0 encodes 256; any value already <= target is nothing to do.
    if cur != 0 && cur <= TARGET_CLKDIV {
        info!("FLASH XIP: already at clkdiv={}, no speed-up needed", cur);
        return;
    }

    // Resolve the ROM cache-flush trampoline to its boot-ROM address *now*, while the
    // flash timing is still good. The generated wrapper lives in flash; calling it
    // during a bad-timing candidate would fault. The resolved pointer targets boot ROM
    // (not on the QMI bus), so it is safe to call from the SRAM critical region.
    let flush = embassy_rp::rom_data::flash_flush_cache::ptr();

    // Capture a golden copy of the validation region at the current (good) timing.
    let mut golden = [0u32; SAMPLE_WORDS];
    let src = FLASH_BASE as *const u32;
    unsafe {
        flush();
        for (i, g) in golden.iter_mut().enumerate() {
            *g = core::ptr::read_volatile(src.add(i));
        }
    }

    // Mask interrupts: the embassy time-driver ISR (and any other) is flash-resident
    // and would fault if it fired while a candidate timing is bad. The sweep is a few
    // hundred memory reads per candidate, so the masked window is sub-millisecond.
    let result = critical_section::with(|_| unsafe { sweep_rxdelay_at_target(flush, golden.as_ptr(), orig.0) });

    // Bit i (rxdelay i) set == that sample point read flash correctly at clkdiv=2. A
    // wide run of 1s is a healthy data eye; a single passing bit means the margin is
    // thin and the speed-up should be treated with suspicion (temperature/voltage may
    // push it out of the window).
    info!(
        "FLASH XIP rxdelay sweep @clkdiv={}: pass[rx7..rx0]={=u8:08b} ({} of 8 ok, 1=read OK)",
        TARGET_CLKDIV,
        result.pass_bitmap,
        result.pass_bitmap.count_ones(),
    );

    match result.chosen {
        Some(rx) => {
            let mhz = clocks::clk_sys_freq() / (TARGET_CLKDIV as u32) / 1_000_000;
            info!(
                "FLASH XIP: clkdiv {}->{} OK ({} MHz), rxdelay {}->{} (centered in passing window)",
                cur, TARGET_CLKDIV, mhz, orig.rxdelay(), rx,
            );
        }
        None => info!(
            "FLASH XIP: clkdiv={} unstable for all rxdelay; kept bootrom clkdiv={}",
            TARGET_CLKDIV, cur,
        ),
    }
}

/// SRAM-resident RXDELAY sweep at `TARGET_CLKDIV`. Copied to RAM before `main` via
/// `.data.ram_func` (same mechanism as `PsramDelay::process`), so it executes with no
/// instruction fetch from flash — essential, because mid-sweep the flash timing is
/// deliberately set to candidates that may not read correctly.
///
/// Returns the per-RXDELAY pass bitmap plus the centered RXDELAY of the longest
/// passing run, with that timing left applied; or `chosen: None` (bootrom timing
/// restored) if nothing validated. Calls only the ROM `flush` pointer and does pure
/// register/RAM work in between — never a flash-resident function, which would fault
/// at a bad candidate.
///
/// # Safety
/// `golden` must point to `SAMPLE_WORDS` valid words. Caller must have interrupts
/// masked and no other flash/QMI users active.
#[unsafe(link_section = ".data.ram_func")]
#[inline(never)]
unsafe fn sweep_rxdelay_at_target(flush: unsafe extern "C" fn(), golden: *const u32, orig_bits: u32) -> SweepResult {
    let orig = Timing(orig_bits);
    let timing = pac::QMI.mem(0).timing();
    let src = FLASH_BASE as *const u32;

    // Build the candidate Timing from the original so every other field (min_deselect,
    // max_select, select_hold, pagebreak, cooldown, ...) is preserved.
    let apply = |rxdelay: u8| {
        let mut t = orig;
        t.set_clkdiv(TARGET_CLKDIV);
        t.set_rxdelay(rxdelay);
        timing.write_value(t);
        cortex_m::asm::dsb();
        unsafe { flush() };
        cortex_m::asm::dsb();
    };
    // Read `SAMPLE_WORDS` of flash and compare to golden. Raw-pointer reads avoid any
    // slice bounds-check panic path (which would be a flash call at a bad timing).
    // `src` (0x1000_0000) and `golden` (a [u32; _]) are both 4-aligned and `i*4`
    // preserves that, so the debug misaligned-read check is statically unreachable —
    // keeping the only outbound calls from this RAM function the ROM `flush` pointer.
    let validate = || -> bool {
        let mut i = 0usize;
        while i < SAMPLE_WORDS {
            if unsafe { core::ptr::read_volatile(src.add(i)) != *golden.add(i) } {
                return false;
            }
            i += 1;
        }
        true
    };

    let mut pass = [false; (RXDELAY_MAX as usize) + 1];
    let mut pass_bitmap = 0u8;
    let mut c: u8 = 0;
    while c <= RXDELAY_MAX {
        apply(c);
        let ok = validate();
        pass[c as usize] = ok;
        if ok {
            pass_bitmap |= 1 << c;
        }
        c += 1;
    }

    // Center of the longest run of consecutive passing RXDELAY values (the data eye
    // midpoint is the most robust sample point; the first passing edge is marginal).
    let mut best_len = 0u8;
    let mut best_center = 0u8;
    let mut run_start = 0u8;
    let mut run_len = 0u8;
    let mut i: u8 = 0;
    while i <= RXDELAY_MAX {
        if pass[i as usize] {
            if run_len == 0 {
                run_start = i;
            }
            run_len += 1;
            if run_len > best_len {
                best_len = run_len;
                best_center = run_start + (run_len - 1) / 2;
            }
        } else {
            run_len = 0;
        }
        i += 1;
    }

    if best_len == 0 {
        // Nothing worked at the target divisor — restore the bootrom timing.
        timing.write_value(orig);
        cortex_m::asm::dsb();
        unsafe { flush() };
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
        return SweepResult { pass_bitmap, chosen: None };
    }

    // Apply the chosen sample point and re-validate before handing control back to
    // flash-resident code at the new timing.
    apply(best_center);
    let ok = validate();
    cortex_m::asm::isb();
    if ok {
        SweepResult { pass_bitmap, chosen: Some(best_center) }
    } else {
        timing.write_value(orig);
        cortex_m::asm::dsb();
        unsafe { flush() };
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
        SweepResult { pass_bitmap, chosen: None }
    }
}
