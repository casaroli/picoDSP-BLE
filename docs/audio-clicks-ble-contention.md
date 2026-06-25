# Audio clicks under BLE radio activity (open issue)

## Symptom

Audible clicks in the I2S audio output, correlated with cyw43 BLE radio activity:

- **At idle** (BLE keyboard connected, no traffic): occasional low-volume clicks.
- **While playing** over the BLE keyboard: more frequent, random (non-onset) clicks
  during sustained notes.
- **Worst case — a CC stream from the keyboard** (e.g. a mod-wheel / continuous
  controller sweep): a lot of clicking, even with the voice silent.

The clicks are **inaudible with BT disabled** (confirmed by temporarily not spawning
`bluetooth_task` and playing a held note over USB-MIDI — dead clean). They were always
present but **masked by saw/square waveforms**; clean **sine** tones (after the fast-sine
fix) exposed them.

## Why the underrun counter doesn't see it

`AUDIO_UNDERRUNS` only counts the software `AUDIO_CHANNEL` (core1 -> core0) going empty.
The click is a **PIO I2S FIFO underflow** in the *output DMA*, which is a different thing
and stays at `underruns 0`. So "0 underruns" does **not** mean glitch-free here.

## Root cause(s)

Audio output path: core1 DSP -> `AUDIO_CHANNEL` -> `main_task` on core0 -> `PioI2sOut`
double-buffer (DMA_CH1, PIO0). The cyw43 radio is `PioSpi` on PIO1 + **DMA_CH2**, polled
by the vendored cyw43 runner (BT IRQ line isn't wired, so it busy-polls).

Two distinct contention mechanisms:

1. **DMA bus contention.** cyw43 SPI bursts (DMA_CH2) and core1's heavy SRAM/PSRAM DSP
   traffic delay the I2S output DMA (DMA_CH1) enough to underflow the ~8-sample PIO FIFO
   (~166 µs of slack). *Mitigated* (see below).

2. **XIP instruction-fetch contention (the dominant remaining mechanism).** The BLE host
   stack (TrouBLE Runner + bt-hci) and the MIDI handlers ran from **flash (XIP)**. cyw43 and
   the DSP were already relocated to RAM, but this code was not. While the radio is on, that
   code executes constantly — processing advertising reports **even when only scanning, not
   connected** (matches "BT on, not connected -> clicks"), and notifications/CC when
   connected. Its **instruction fetches go over the QMI bus**, which the core1 PSRAM delay
   shares -> the DSP stalls -> the audio output glitches. This scales with radio traffic
   (worst on a dense CC stream) and is independent of whether a note is playing.

## What fixed it

Three changes, all committed:

1. **`BUSCTRL.bus_priority` DMA_R/DMA_W = high** + **I2S DMA channel `HIGH_PRIORITY`**
   (re-asserted each block in `main_task`, since embassy rewrites `ctrl_trig` every
   `write()`) — DMA outranks the cores on the bus fabric and the I2S DMA outranks cyw43's
   DMA in the scheduler. → idle / connection-event clicks gone.
   (`fix(audio): cut BLE-radio audio clicks via DMA bus priority + lighter poll`)
2. **cyw43 adaptive-poll budget 64 -> 8** (`vendor/cyw43/src/runner.rs`) — steady-state play
   drops to the gentle 8 ms idle poll between notes instead of polling at 250 µs
   continuously. → most during-play clicks gone. (same commit)
3. **Run the BLE-host + MIDI hot path from SRAM** (`memory.x` `.ram_code` + `.data.ram_func`)
   — the real fix for the residual clicks. Relocated `bt_hci` and the hot `trouble_host`
   modules (`host`/`att`/`channel_manager`/`connection_manager`/`packet_pool`/`central`/
   `connection`/`gatt`/`l2cap`/`pdu`/`codec`/`cursor`/`scan`), this crate's `control::midi`
   module, and `parse_ble_midi`/`adv_contains_uuid`. **Deliberately skipped
   `trouble_host::security_manager` (~53 KB, only runs once per pairing — cold)** to stay in
   the RAM budget. Paid for it by trimming the heap **256 KB -> 208 KB** (safe: core1 drops
   the old synth before building the new, so peak heap is ~110 KB; ~91 KB stack headroom).
   → **clicks gone, including the scanning/CC-flood case** (confirmed on hardware).

## Status: RESOLVED.

## Appendix — attempted fix that did NOT work: audio on an interrupt executor

(Pursued before #3 was found, when the cause was mis-attributed to I2S re-queue wake
latency on the thread executor. The actual cause was XIP contention, fixed by #3.)

The intended proper fix for #2: run the I2S output loop on a **high-priority interrupt
executor** (a spare `SWI_IRQ`) so it preempts the cyw43/BLE/MIDI work on core0's thread
executor and always re-queues within the FIFO window.

It was implemented (made `PioI2sOut` `'static` via `StaticCell` for the PIO `Common` +
program; moved the loop into a `#[task] audio_task` on `EXECUTOR_AUDIO`) and **reverted**:

- The `audio_task` ran (logged "started"), but it introduced a **non-deterministic
  multicore startup race** that wedged **core1's `build_synth`** — core1 never reached its
  first block (no `build_synth`/`Core1 peak` logs), so the whole synth went silent. The
  race was timing-sensitive (toggled by adding a single log line).
- Gating the audio spawn on `CORE1_RUNNING`, and spawning it after the core1 launch, did
  not resolve it.
- Suspected: contention on the shared global-allocator critical section / spinlock between
  core1's `build_synth` heap allocations and the audio interrupt executor coming up, or the
  high-priority SWI perturbing the multicore (SIO FIFO) launch handshake. Not root-caused.

Note: an apparent "boot hang at 3.9 s" during this work was a red herring — it was the
**pre-existing intermittent slow cyw43 `control.init`** (tens of seconds), not a hang and
not caused by these changes.

## Ideas for next time

- Re-attempt the interrupt executor but bring it up **only after core1 is fully live**, and
  investigate the allocator critical-section interaction (e.g. avoid heap allocation on the
  audio path; pre-allocate everything before starting the interrupt executor).
- Alternatively, **move the I2S output to core1** (the DSP core, isolated from BLE): core1
  already produces the audio, so it could drive the DMA directly and drop `AUDIO_CHANNEL`
  + core0 from the audio path entirely. Bigger restructure of the core1 loop.
- **Wire up the cyw43 BT IRQ line** so the runner stops busy-polling SPI entirely — removes
  the polling-induced bus flooding at the source (the runner comment notes "interrupts
  aren't working yet for bluetooth").
- Reduce core0 flood: gate/remove `log_midi!` formatting on the hot MIDI path during dense
  CC; consider a larger/chained I2S DMA ring for more re-queue slack.
