# AGENTS.md — rp2350-synth

Guide for agents working on this repo. Read this first.

## What this is

A polyphonic-voiced (mono-priority) **synthesizer firmware** for the **RP2350**
(Raspberry Pi Pico Plus 2 W). `no_std` Rust, **Embassy** async, dual-core. It plays from
**USB-MIDI** and **BLE-MIDI** (a Bluetooth keyboard), outputs audio over **I2S**, and stores
**17 factory presets** in flash.

## Build / flash / run

- **Flash + stream logs:** `cargo run --release` (runner is `probe-rs run --chip RP235x`).
- Target: `thumbv8m.main-none-eabihf`. Log level: `DEFMT_LOG=info` (set in `.cargo/config`).
- Logs are **defmt over RTT** (shown by `probe-rs`). MIDI/storage logs (`log_midi!`,
  `log_status!`) go over **USB-serial**, not RTT.
- The board's CYW43 radio is a separate chip; a soft reset / re-flash toggles its PWR pin but
  occasionally it needs a **physical USB power-cycle** (e.g. wedged radio, or to clear state).
- **Intermittent slow startup:** cyw43 `control.init` sometimes takes tens of seconds (seen
  up to ~135 s) before `[host] initialized`. This is a known, pre-existing flaky behaviour —
  not a hang. Don't mistake it for a crash; wait it out or power-cycle.

### Host tool: `tools/midictl`

Tiny `midir` CLI to drive the synth over USB-MIDI without a DAW. Auto-targets the `PicoDSP`
port. Has its own `.cargo/config.toml` (host target). Examples:
`cargo run -- pc 12` (load preset), `cargo run -- note 60 8` (hold note),
`cargo run -- cc 67 127 && cargo run -- cc 67 0` (soft-pedal preset cycle). See its README.

## Architecture

- **core0** (`src/tasks/core0.rs` `main_task`): USB device (UAC1 audio + MIDI + serial log),
  the **I2S output feed** (DSP block -> double-buffered DMA), storage, and spawns the
  BLE host. Also runs the cyw43 runner.
- **core1** (`src/tasks/core1.rs` `core1_task`): the **DSP**. `build_synth` builds an
  `infinitedsp-core` chain (3 osc voice -> ladder filter -> VCA -> delay -> reverb -> widener)
  and the loop produces `BLOCK_SIZE` (256) blocks into `AUDIO_CHANNEL`.
- **Audio path:** core1 -> `AUDIO_CHANNEL` (cap 4) -> core0 `main_task` -> `PioI2sOut`
  (PIO0 + **DMA_CH1**), 48 kHz / 16-bit. `AUDIO_UNDERRUNS` counts the *software* channel
  emptying — it does **not** see PIO-FIFO output underflows (the audio "clicks"; see below).
- **MIDI:** USB-MIDI and BLE-MIDI both funnel into `handle_voice_message`
  (`src/control/midi.rs`). `MidiControl` (atomics) carries freq/gate/params to core1.
  CC67 (soft pedal) press->release **cycles presets**.
- **BLE** (`src/bt/mod.rs`, core0): cyw43 radio (`PioSpi` PIO1 + **DMA_CH2**) +
  TrouBLE host as a **Central** that scans for, pairs with (LE Legacy JustWorks + bonding),
  and subscribes to a BLE-MIDI keyboard; decodes BLE-MIDI notifications into
  `BLE_MIDI_CHANNEL`. Requests a 15–30 ms connection interval (keyboard pulls to ~7.5 ms).
- **Storage** (`src/data/storage.rs`): header (magic/version/count) + N x 200-byte `Preset`
  in a 64 KB reserved flash region at top of flash. **Capacity `MAX_PRESETS = 128`** (a 128
  bank = 16 + 128*200 = 25616 B -> spans 7 of the 4 KB sectors); `format`/`write_raw`/`read_raw`
  are multi-sector, `load_preset`/`num_presets` index linearly. Bump `VERSION` to force a
  reformat. Currently `VERSION = 9`, 17 factory presets (`src/data/presets.rs`
  `get_default_presets`) — can grow toward 128. The SysEx full-bank editor transfer
  (`midi_task` `handle_sysex`, talks to picoDSP-Edit) moves the whole 128-capacity image:
  the incoming WRITE is de-nibbleized on the fly into one `STORAGE_IMAGE_SIZE` (~28 KB)
  buffer and the DUMP is nibbleized on the fly, so the 2x-larger nibbleized stream never sits
  in RAM next to the ~110 KB synth. `write_raw` writes **one sector per fully independent
  gate→erase→write→reinit cycle** (see PSRAM note). Note: BLE/USB Program Change is 7-bit, so
  presets >=128 aren't PC-addressable. Editor side: `../picoDSP-Edit` `STORAGE_SIZE`/`VERSION`
  must match this firmware.
- **PSRAM** (`src/psram.rs`): 8 MiB APS6404L on QMI CS1, backs the delay ring buffers. Shares
  the QMI bus with flash — flash writes need a settle + CS1 reinit + a core1 PSRAM gate
  (`PSRAM_GATE_REQ/ACK`). The reinit (`Psram::new`) does its own `multicore::pause_core1`, so
  a **multi-sector** write must be done as **independent per-sector cycles** (release core1
  between sectors) with `dsb` barriers, else reinit's pause nests with the gate/embassy flash
  pauses and FIFO-deadlocks. See `docs/psram-flash-corruption-investigation.md`.

## Vendored crates (`vendor/`) — patched via `[patch]` in `Cargo.toml`

Do not assume upstream behaviour; these are **locally patched**. If you bump a revision,
**re-apply the patch**.

- **`vendor/cyw43`** — busy-poll runner with an **adaptive poll cadence**: fast (250 µs)
  while traffic flows, decaying to an 8 ms idle period via a small budget (`BT_FAST_POLL_BUDGET
  = 8`). Fixes a ~50 s startup *and* limits steady-state bus flooding (audio clicks).
- **`vendor/trouble`** (trouble-host) — patched so the **L2CAP Connection Parameter Update
  *response* echoes the request's signaling identifier** (Core spec Vol 3 Part A §4.1).
  Without it the keyboard's RTX timer expires and it drops the link every ~30 s. Built with
  the `legacy-pairing` feature (the keyboard is LE-Legacy, no Secure Connections).
- **`vendor/infinitedsp-core`** (0.9.0) — the **sine oscillator's per-sample `libm::sinf`**
  (~1100 cycles on RP2350; ~90 % of a block!) replaced with a **fast polynomial**
  (`fast_sin_norm`, ~16 % cost). Without it, sine-based presets underflow into silence+noise.

## Hot code relocated to SRAM (`memory.x` `.ram_code` + `.data.ram_func`)

To keep instruction fetch off the QMI/XIP bus that the core1 PSRAM delay shares (the cause
of audio clicks), the **hot paths are copied to RAM at boot** (`init_ram_code`):
`cyw43`, `infinitedsp_core` (DSP), `libm`, **`bt_hci` + the hot `trouble_host` modules**
(host/att/channel_manager/connection_manager/etc. — but NOT the cold `security_manager`),
and this crate's **`control::midi`** module + `parse_ble_midi`/`adv_contains_uuid`. Adding a
new hot path? Add its symbol pattern to `.ram_code` (or tag a fn `.data.ram_func`) and watch
the RAM budget. Heap is **208 KB** (trimmed from 256 to pay for the relocation; ~91 KB stack
headroom). One synth uses ~110 KB heap; core1 drops the old synth before building the new.

## Known issues

- **BLE-radio audio clicks**: RESOLVED — see `docs/audio-clicks-ble-contention.md` (DMA bus
  priority + lighter cyw43 poll + running the BLE/MIDI hot path from SRAM). Read it before
  touching the audio path or the relocation.
- **PSRAM-across-flash corruption**: largely worked around; details in
  `docs/psram-flash-corruption-investigation.md`.

## Conventions

- **Commit directly to the current branch** (incl. `master`); do not auto-create branches.
- Match surrounding code style. Keep the firmware `no_std`.
- When verifying changes, prefer flashing and reading the actual logs/audio over assuming.

## Boot diagnostics

The noisy boot diagnostics have been **removed** (committed): flash-XIP config logging,
`psram::bench`, and the PSRAM-across-flash verify block, for a quieter/faster boot. The
protective `psram::self_test` and the functional `speed_up_flash_xip` remain. If you need to
re-investigate flash/PSRAM timing or corruption, re-add the relevant probe (see
`docs/psram-flash-corruption-investigation.md`).
