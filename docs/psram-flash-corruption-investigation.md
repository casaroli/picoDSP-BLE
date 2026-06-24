# PSRAM data corruption after flash writes — investigation log

Status: **open problem, partial workaround in place.**

A flash erase/program corrupts a small amount of *already-stored* PSRAM content.
For the delay buffer this is masked by a workaround (rebuild → re-zero the buffer),
but **the underlying problem is unsolved**: we cannot yet write to flash and have
PSRAM keep the data it held before. This matters as soon as PSRAM holds anything
that isn't disposable (samples, wavetables, looper/recorder buffers, anything we'd
want persisted or reconstructed rather than zeroed).

---

## 1. Hardware / software context

- Board: **Pimoroni Pico Plus 2 W**, RP2350B.
- PSRAM: **APS6404L, 8 MiB**, on **QMI CS1 / GPIO47**.
- Flash: QSPI on **QMI CS0**, presets stored in the last 64 KiB.
- **Flash and PSRAM share the one QMI controller and the QSPI bus** (SCLK, SD0–3),
  with separate chip selects (CS0 flash, CS1 PSRAM) and separate M-register banks
  (M0 = flash, M1 = PSRAM).
- Stack: `embassy-rp 0.10.0`, `rp-pac 7.0.0`.
- PSRAM brought up via `embassy_rp::psram::Psram::new` (cached window `0x1100_0000`).
- Flash via `embassy_rp::flash::Flash` (Async/DMA). Presets via `src/data/storage.rs`.
- Delay ring buffers live in PSRAM: bump allocator in `src/psram.rs`, delay node in
  `src/dsp/psram_delay.rs`.

## 2. Symptom

With the delay backed by PSRAM, after a **flash write** (save preset, or change delay
settings + save to device), the delay produces **loud sustained noise / clicks and no
clean sound**. Turning the delay **off** restores clean audio (it stops touching
PSRAM). The noise is **persistent**, not a brief blip — because the delay's **feedback
loop recirculates** the corrupted samples so they never wash out.

## 3. Confirmed mechanism

A flash erase/program has **two** distinct effects on PSRAM:

### 3a. It clobbers the CS1 (PSRAM) controller config
`embassy_rp::flash` erase/program goes through the bootrom sequence
`flash_exit_xip` → `flash_flush_cache` → `flash_enter_cmd_xip`
(`embassy-rp-0.10.0/src/flash.rs`, the `.data.ram_func` routines around L631/696/815).
That sequence reconfigures the QMI and re-enters XIP **for CS0 (flash) only**. It does
**not** restore the CS1 M1 registers or `XIP_CTRL.writable_m1`.

Measured M1 register state (read via a `dump_m1` helper, since removed):

| state | timing | rfmt | rcmd | wfmt | wcmd | writable_m1 |
|---|---|---|---|---|---|---|
| after boot init | `0x60242202` | `0x000612aa` | `0x0eb` | `0x000012aa` | `0x38` | true |
| after flash (clobbered) | `0x60007203` | `0x000492a8` | `0x0eb` | `0x000012aa` | `0x38` | true |
| after our reinit | `0x60242202` | `0x000612aa` | `0x0eb` | `0x000012aa` | `0x38` | true |

Note the clobbered `timing` low byte is `0x03` (clkdiv reset to 3) and `rfmt` changed.
**Restoring these registers is necessary but not sufficient** (see §3b / §4).

- **Flash *reads* do NOT clobber** CS1: `Flash::read` uses `background_read`
  (XIP-stream DMA) / `blocking_read` is a plain memcpy from the XIP window — neither
  runs the exit/enter-xip sequence. Only **erase/program** clobber.
- **No concurrency race**: `embassy_rp::flash` calls `multicore::pause_core1()` for the
  duration of the op (`flash.rs:946`), so core1 is paused during it.

### 3b. It corrupts a few already-stored PSRAM words
**This is the real problem and is independent of 3a.** After a flash op, ~20–24
scattered words of previously-written PSRAM read back wrong. Critically, the PSRAM
**is fully functional afterward** — fresh writes/reads are perfect (see §4 test #7).
So the cells' *content* is corrupted; the interface is fine.

Error signature:
- Count: ~14–34 words out of 8192 checked (**varies run-to-run**).
- Pattern: **high 16 bits zeroed, low 16 bits correct**, e.g. `0xC0DE0252` → `0x00000252`.
- Addresses: **scattered** (~every 45 words), **not fixed** between runs.
- Leading hypothesis: the APS6404 is **pseudo-static (self-refreshing DRAM)** and the
  long flash erase (~tens of ms) disturbs refresh / a few cells decay. Variable +
  scattered + content-only is consistent with a refresh/disturb mechanism rather than
  fixed bad cells or a timing/signal-integrity problem.

## 4. Experiments tried (and why each was rejected)

All via a boot-time harness (since removed from `main.rs`): write a sentinel pattern
to a PSRAM region **larger than the 16 KiB XIP cache** (so early words are evicted to
real PSRAM, not left dirty in write-back cache), do a **non-destructive** flash
erase+write (rewrite the storage sector with its own contents), then read back. Uncached
CS1 window used for some reads: **`0x1500_0000`** (= `0x14000000` nocache base + `0x01000000` CS1 offset).

| # | Hypothesis / fix tried | Result | Verdict |
|---|---|---|---|
| 1 | Re-apply M1 registers only (`reapply_config`) | post-flash 24–200 errs; regs byte-identical to boot | registers restored, reads still wrong → not (just) registers |
| 2 | Full chip re-init: `Psram::new` again (reset 0xf5 → detect → QPI 0x35 → M1) | post-flash 14–200 errs | re-init does **not** fix it |
| 3 | Slower clock: clkdiv 3 (50 MHz) at init | ~14–24 errs | barely changed; the "200 vs 22" was run-to-run variance, not clkdiv |
| 4 | rxdelay sweep post-flash (uncached, rx0–7) | rx0–4 = **24 (identical)**, rx5 = 325, rx6–7 = all-fail | **rxdelay-insensitive** in the eye → not sample timing |
| 5 | clkdiv sweep post-flash (runtime, clkdiv 2–10 = 75→15 MHz) | **all exactly 24** | **clock-insensitive** → not signal integrity / timing |
| 6 | Boost shared QSPI data-pad drive (`PADS_QSPI` SD0–3 + SCLK → 12 mA, slewfast) | 22 errs | did **not** help |
| 7 | **Write fresh pattern post-flash, read back** | **0/8192 errors** | **decisive: reads+writes are perfect; the earlier errors are corrupted *content*** |
| 8 | pre-flash vs post-flash, cached vs uncached | pre: cached **0**, uncached ~30; post: cached ~24, uncached ~24 | uncached single-beat reads are inherently marginal even pre-flash; cached burst reads are perfect pre-flash and degrade only because the *content* is corrupt |

**Net conclusion:** the flash op corrupts a small, variable set of pre-existing PSRAM
cells. It is not a config, timing, clock, rxdelay, or pad-drive problem — those were all
ruled out. The data in the cells is genuinely lost and re-writing the same addresses
restores them.

## 5. Current workaround (in code)

Only viable because the delay's data is **disposable**:

- `src/psram.rs::after_flash_write()` — called by storage after every flash write:
  1. `reinit()` → `boost_qspi_pads()` + `Psram::new` (peripherals re-acquired via
     `QMI_CS1::steal()` / `PIN_47::steal()`) to restore the CS1 config (§3a).
  2. Sets `shared::SYNTH_RESET_REQ`.
- `src/data/storage.rs` — `format()` and `write_raw()` call `after_flash_write()`.
- `src/tasks/core1.rs` — on `SYNTH_RESET_REQ`, rebuilds the synth, which re-allocates and
  **re-zeros** the PSRAM delay buffer, clearing the corrupt words and breaking the
  feedback loop.

Expect a brief audible glitch *during* a save (flash halts audio ~tens of ms + the synth
rebuild); it should then recover clean. **This does not preserve PSRAM data** — it
discards and rebuilds it.

## 6. The open problem (for future investigation)

**Goal:** write to flash and have PSRAM still hold the same data afterward (after
restoring CS1 config). Required before we trust PSRAM with non-disposable data.

Avenues not yet tried:
- **Reference implementations that coexist flash + PSRAM:** MicroPython `rp2_psram`
  (the driver lineage we started from) runs a flash filesystem *and* a PSRAM heap. Check
  whether it (a) sees this corruption, (b) quiesces/handles PSRAM around flash ops, or
  (c) does something to preserve refresh. Same for pico-sdk PSRAM examples and Pimoroni's.
- **RP2350 datasheet / errata:** look for QMI flash+PSRAM coexistence notes, the APS6404
  `tCEM` (max CS-low) / refresh requirements, and what `flash_exit_xip` / direct mode do
  to the shared clock/bus while CS1 is idle.
- **Refresh-during-flash hypothesis:** measure whether corruption count scales with flash
  **erase time** (long) vs **program time** (short) — test erase-only vs program-only. If
  it tracks erase duration, it's refresh decay during the op.
- **Bus/idle state during the op:** verify CS1 is genuinely deasserted and the chip can
  self-refresh while the flash op holds the bus; try explicitly idling/parking PSRAM
  before the op.
- **Quiesce + checksum approach:** if corruption is unavoidable, store critical PSRAM data
  with ECC/checksums and a flash-backed copy, and repair the few corrupt words after each
  flash op instead of trusting PSRAM across the op.
- **Address distribution:** log the exact corrupt addresses across many runs — are they
  uniformly random, or biased to rows/pages/banks? That points at the mechanism.

## 7. How to reproduce / re-add the test harness

The diagnostic block lived in `main.rs` right after `storage.init().await`. Sketch:

```rust
// Write > 16 KiB so early words are evicted to real PSRAM (not dirty in write-back cache).
let base = psram_region.base() as *mut u32;            // cached CS1 = 0x1100_0000
let ubase = 0x1500_0000usize as *mut u32;              // uncached CS1
let n = 16384; let check = 8192;
for i in 0..n { unsafe { core::ptr::write_volatile(base.add(i), 0xC0DE_0000 + i as u32) }; }
let pre = /* count mismatches reading base[0..check] */;          // expect 0 (cached)
let mut s = [0u8; 4096];
storage.read_raw(&mut s).await; storage.write_raw(&s).await;      // non-destructive flash erase+write
// (write_raw now calls after_flash_write() -> reinit(); for raw diagnosis, bypass it)
let post = /* count mismatches reading base[0..check] */;          // ~20-24 wrong (corrupt content)
// Definitive: rewrite fresh + read back -> 0 errors (interface fine, content was lost).
for i in 0..check { unsafe { core::ptr::write_volatile(base.add(i), 0xBEEF_0000 + i as u32) }; }
let rw = /* count mismatches */;                                   // 0
```

Useful registers: `embassy_rp::pac::QMI.mem(1).{timing,rfmt,rcmd,wfmt,wcmd}()`,
`embassy_rp::pac::XIP_CTRL.ctrl().writable_m1()`, `embassy_rp::pac::PADS_QSPI.gpio(n)`
(n: 0=SCLK, 1–4=SD0–3, 5=SS).

## 8. Key code locations

- `src/psram.rs` — `init`, `reinit`, `after_flash_write`, `boost_qspi_pads`, bump allocator.
- `src/dsp/psram_delay.rs` — PSRAM-backed delay (`process` is RAM-resident).
- `src/data/storage.rs` — flash storage; calls `after_flash_write()` after writes.
- `src/tasks/core1.rs` — `SYNTH_RESET_REQ` handling (rebuild → re-zero delay).
- `src/common/shared.rs` — `SYNTH_RESET_REQ`.
- `src/flash_diag.rs` — read-only QMI CS0 (flash) XIP config / rxdelay sweep diagnostics.
- Reference: `embassy-rp-0.10.0/src/{psram.rs,qmi_cs1.rs,flash.rs}`; MicroPython
  `rp2_psram.c` (original driver lineage).
