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

2. **I2S re-queue wake latency (the remaining issue).** The double-buffer is re-queued by
   `main_task` on core0's **thread executor**. The re-queue deadline after a buffer
   completes is only the PIO FIFO drain (~166 µs). When a BLE traffic burst (CC flood,
   notifications) keeps core0 busy — cyw43 SPI reads + trouble-host GATT + `midi_task` +
   `log_midi!` — `main_task` wakes too late and the FIFO underflows. This is why it scales
   with radio traffic and is worst on dense CC.

## What was fixed (committed)

`fix(audio): cut BLE-radio audio clicks via DMA bus priority + lighter poll`:

- **`BUSCTRL.bus_priority` DMA_R/DMA_W = high** — DMA masters outrank the cores on the bus
  fabric so core1 can't starve the audio DMA.
- **I2S DMA channel `HIGH_PRIORITY`** (re-asserted each block in `main_task`, since embassy
  rewrites `ctrl_trig` on every `write()`) — I2S outranks cyw43's DMA in the DMA scheduler.
  → **Idle / connection-event clicks: gone.**
- **cyw43 adaptive-poll budget 64 -> 8** (`vendor/cyw43/src/runner.rs`) — steady-state play
  drops back to the gentle 8 ms idle poll between notes instead of polling at 250 µs
  continuously, cutting the bus flooding. Init stays fast (continuous activity keeps the
  budget refreshed). → **During-play clicks: largely gone.**

## What's still open

**Dense CC streams still click** (mechanism #2 above — core0 thread-executor wake latency
under BLE flood). Not yet fixed.

## Attempted fix that did NOT work: audio on an interrupt executor

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
