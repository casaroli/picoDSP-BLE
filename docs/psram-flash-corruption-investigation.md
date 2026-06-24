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

## 7. The corruption test (paste-ready) and how to reproduce

This is the exact, runnable harness used to reach the §3b / §4 conclusions. Drop it
into `main.rs` **right after `storage.init().await`** (so `psram_region` and `storage`
are in scope, and PSRAM is up). It is **non-destructive** to your stored presets — it
rewrites the storage sector with its own contents purely to trigger the flash
erase+write path.

Important caveats baked into the design:
- Write **more than the 16 KiB XIP cache** (here 64 KiB) and only verify the first
  32 KiB, so the checked words were genuinely **evicted to PSRAM** before the flash op
  (not still sitting clean in the write-back cache, which would hide the corruption).
- For a *clean* read of what's actually in PSRAM, this build leaves
  `storage.write_raw()` calling `after_flash_write()` → `reinit()` (restores the CS1
  config, §3a). That's required for the post-flash reads to use a valid config; it does
  **not** repair the corrupted content. To observe the raw clobbered-config state too,
  temporarily comment the `after_flash_write()` call in `storage::write_raw` and call
  `psram::reinit()` yourself after the dumps.
- Optional uncached CS1 window `0x1500_0000` lets you read straight from PSRAM bypassing
  the cache (note: single-beat uncached reads are inherently a bit marginal on this
  board even pre-flash — see §4 #8 — so trust the **cached** counts as the oracle).

```rust
{
    let base = psram_region.base() as *mut u32; // cached CS1 = 0x1100_0000
    let ubase = 0x1500_0000usize as *mut u32;   // uncached CS1 (0x1400_0000 nocache + CS1 offset)
    let n = 16384usize;  // 64 KiB written
    let check = 8192usize; // verify first 32 KiB (past the 16 KiB cache => really in PSRAM)

    let pat = |i: usize| 0xC0DE_0000u32.wrapping_add(i as u32);
    for i in 0..n {
        unsafe { core::ptr::write_volatile(base.add(i), pat(i)) };
    }
    let count = |p: *mut u32| -> usize {
        let mut m = 0;
        for i in 0..check {
            if unsafe { core::ptr::read_volatile(p.add(i)) } != pat(i) {
                m += 1;
            }
        }
        m
    };

    // (1) before any flash op: cached reads should be perfect (uncached ~30, marginal).
    let pre = count(base);
    let pre_u = count(ubase);

    // (2) non-destructive flash erase+write (rewrite the storage sector with itself).
    let mut sector = [0u8; 4096];
    storage.read_raw(&mut sector).await;
    storage.write_raw(&sector).await; // calls after_flash_write() -> reinit() (restores config)
    let post = count(base);   // expect ~20-24 mismatches: CORRUPTED CONTENT
    let post_u = count(ubase);
    defmt::info!(
        "PSRAM verify: pre cached {}/{} uncached {}/{} | post cached {}/{} uncached {}/{}",
        pre, check, pre_u, check, post, check, post_u, check
    );

    // (3) DECISIVE: write a fresh pattern post-flash and read it back.
    // 0 mismatches here proves reads/writes are fine and the §2 errors were lost content.
    for i in 0..check {
        unsafe { core::ptr::write_volatile(base.add(i), 0xBEEF_0000u32.wrapping_add(i as u32)) };
    }
    cortex_m::asm::dsb();
    let mut rw = 0usize;
    for i in 0..check {
        if unsafe { core::ptr::read_volatile(base.add(i)) } != 0xBEEF_0000u32.wrapping_add(i as u32) {
            rw += 1;
        }
    }
    defmt::info!("PSRAM post-flash REWRITE+read: {}/{} mismatch", rw, check); // expect 0
}
```

Expected output (the result this whole doc rests on):

```
PSRAM verify: pre cached 0/8192 uncached 24/8192 | post cached 20/8192 uncached 20/8192
PSRAM post-flash REWRITE+read: 0/8192 mismatch
```

i.e. **pre-flash cached reads perfect → post-flash ~20 corrupt → rewriting the same
addresses is perfect again.** The flash op lost the content of a few cells; the PSRAM
interface is fine.

Variations used during the investigation (§4): sweep `rxdelay` 0..7 or `clkdiv` 2..10 via
`embassy_rp::pac::QMI.mem(1).timing().modify(|w| { w.set_rxdelay(d); w.set_clkdiv(d); })`
(with a `cortex_m::asm::dsb()` after) and re-`count()` between settings — both were flat,
confirming it is not a read-timing problem.

Useful registers: `embassy_rp::pac::QMI.mem(1).{timing,rfmt,rcmd,wfmt,wcmd}()`,
`embassy_rp::pac::XIP_CTRL.ctrl().writable_m1()`, `embassy_rp::pac::PADS_QSPI.gpio(n)`
(n: 0=SCLK, 1–4=SD0–3, 5=SS). A `dump_m1(tag)` helper that logged all five M1 registers +
`writable_m1` produced the §3a table; re-add it if you need to compare config states.

## 8. Key code locations

- `src/psram.rs` — `init`, `reinit`, `after_flash_write`, `boost_qspi_pads`, bump allocator.
- `src/dsp/psram_delay.rs` — PSRAM-backed delay (`process` is RAM-resident).
- `src/data/storage.rs` — flash storage; calls `after_flash_write()` after writes.
- `src/tasks/core1.rs` — `SYNTH_RESET_REQ` handling (rebuild → re-zero delay).
- `src/common/shared.rs` — `SYNTH_RESET_REQ`.
- `src/flash_diag.rs` — read-only QMI CS0 (flash) XIP config / rxdelay sweep diagnostics.
- Reference: `embassy-rp-0.10.0/src/{psram.rs,qmi_cs1.rs,flash.rs}`; MicroPython
  `rp2_psram.c` (original driver lineage).
