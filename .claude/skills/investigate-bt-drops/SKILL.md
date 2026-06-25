---
name: investigate-bt-drops
description: Runbook for investigating BLE-MIDI keyboard disconnects ("Connection Timeout") on the rp2350-synth firmware — capturing RTT + USB-serial logs, correlating drops with played notes, and isolating keyboard-side vs synth-side vs power causes. Use when the Bluetooth keyboard keeps dropping, audio cuts out on certain notes, or you want to resume the BLE-drop investigation.
---

# Investigating BLE-MIDI keyboard drops

The synth is a BLE **Central** that pairs with and subscribes to a BLE-MIDI keyboard
(`src/bt/mod.rs`). Symptom under investigation: the link drops with **"Connection Timeout"**,
audio cuts, and the keyboard only comes back after a **keyboard restart**. One report
correlates drops with **playing very low-frequency notes**.

## What we already know (session findings — start here)

- The drop is logged as `[host] disconnection event ... reason: Connection Timeout`
  (`src/bt/mod.rs` `run_gatt` returns → line ~442 `disconnected, rescanning`). Connection
  Timeout = the **supervision timer expired**: no packets received from the keyboard for the
  whole supervision window (~2000 ms), i.e. the keyboard stopped transmitting.
- **The synth is healthy through every drop.** In the RTT log, `Core1 peak` load stays flat
  at ~18.5% (never above ~24% outside preset switches) and **underruns stay 0** right up to
  the disconnect. So it is **not** core1 DSP overload starving core0's radio poll.
- No cyw43/host errors precede the drop; the synth **cleanly rescans and re-pairs** every
  time. No panic/fault.
- **Keyboard restart** (not synth restart) recovers it → points at the **keyboard** as the
  wedging party, not our radio.
- A UX mitigation is already in place: on disconnect the firmware injects an All Notes Off
  (CC123) so held notes don't stick (`src/bt/mod.rs`, just after `run_gatt`). This silences
  stuck notes but does **not** fix the drop.

Leading hypotheses still open:
1. **Power/brownout coupling** — loud low-frequency bass = large current swings; a shared
   USB supply could sag and reset/wedge the keyboard. (Explains the low-note correlation and
   the keyboard-restart recovery.)
2. **Keyboard firmware** — a bug on its lowest keys/octave.
3. **RF / 2.4 GHz coexistence** — less likely given the clean note↔drop correlation.

## The one capture you don't have yet

The per-note MIDI log (`log_midi!` → `NOTE ON: <n>`) goes over **USB-serial, NOT RTT**
(see `AGENTS.md`). The RTT/defmt stream (`cargo run --release`) shows the BLE lifecycle and
`Core1 peak` telemetry but **not which notes were played**. To confirm the low-note
correlation you must capture **both** streams at once and line up timestamps.

## Step-by-step

### 1. Capture RTT (BLE lifecycle + DSP load)
```
cargo run --release            # probe-rs run; streams defmt over RTT
```
Watch for:
- `[host] disconnection event ... reason: <X>` — record the **reason** (Connection Timeout =
  supervision; other reasons point elsewhere) and the timestamp.
- `[bt] param req: interval 7..8 ms, latency 0, timeout 2000 ms` then `param req accepted` —
  the negotiated **connection interval** and **supervision timeout** (keyboard-requested,
  firmware-accepted in `src/bt/mod.rs` ~line 296–304).
- `Core1 peak ... = <load>% | underruns <n>` — confirm load/underruns are normal at the drop
  (rules in/out DSP starvation).

### 2. Capture USB-serial MIDI log (which notes) — simultaneously
The synth enumerates a USB-serial port (UAC1 + MIDI + serial log device, `PicoDSP`).
```
ls /dev/tty.usbmodem*                  # macOS — find the port
cat /dev/tty.usbmodem*  | ts '[%H:%M:%.S]'   # or: screen /dev/tty.usbmodemXXXX 115200
```
(USB CDC ignores baud.) Look for `NOTE ON: <note> (<freq> Hz)` and `NOTE OFF` lines.

### 3. Correlate
Build a timeline: for each `Connection Timeout` in RTT, find the `NOTE ON` events in the
serial log just before it. Questions to answer:
- Do drops **always** follow notes below some MIDI number / frequency? Which exact notes?
- Is it the **note** or the **resulting loudness** (amplitude/bass energy)?

### 4. Test the power/brownout theory (cheap, do this first)
- Turn synth **volume down** and play the same low notes. If drops stop → power/amplitude
  coupling, not MIDI.
- Power the synth from a **separate adapter** (not the same hub/host as the keyboard charger).
- If you have a meter/scope, watch the 3V3 rail during loud low notes for sag.

### 5. Keyboard-side vs synth-side
- Confirm: does **only restarting the keyboard** recover it (synth keeps scanning fine)? →
  keyboard side. Does the synth radio ever error in RTT? → synth side.
- Try a **different BLE-MIDI keyboard / phone MIDI app** playing the same low notes. If the
  drop disappears, it's that keyboard's firmware.

### 6. Synth-side mitigations to consider (only if pursuing a firmware fix)
- **Supervision timeout**: the keyboard requests 2000 ms; a longer timeout tolerates brief
  keyboard transmit gaps. See connection-param handling in `src/bt/mod.rs` (`conn_cfg`
  ~line 420 and the param-request accept path). Could request/negotiate a longer
  `supervision_timeout`.
- **Verify the trouble patch is applied**: `vendor/trouble` is patched so the L2CAP
  Connection Parameter Update *response* echoes the request's signaling identifier (Core spec
  Vol 3 Part A §4.1) — without it the keyboard's RTX timer expires and it drops the link every
  ~30 s (see `AGENTS.md` "Vendored crates"). If you bumped the trouble revision, re-check the
  patch is still in place; a regression here looks exactly like a periodic Connection Timeout.

## Key code & docs
- `src/bt/mod.rs` — Central loop, `connect`/`pair`/`run_gatt`, connection params (`conn_cfg`
  ~420), `parse_ble_midi` (~147), `BLE_MIDI_CHANNEL.try_send` (~181), disconnect + all-notes-off
  (~440), `disconnected, rescanning` (~442).
- `src/common/shared.rs` — `BLE_MIDI_CHANNEL` (parsed 3-byte MIDI → `midi_task`), `BT_CONNECTED`.
- `src/control/midi.rs` — `handle_voice_message` (shared USB + BLE path); CC123/CC120
  all-notes/sound-off clears the note stack + `midi_control.reset()`.
- `vendor/trouble`, `vendor/cyw43` — locally patched; see `AGENTS.md`.
- `docs/audio-clicks-ble-contention.md` — prior BLE-radio/QMI bus contention work (background).

## Live monitoring while reproducing
Tail the probe-rs RTT output and surface only the signals that matter (faults, drops, audio
stress), staying quiet on routine reconnect chatter:
```
tail -n0 -F <rtt-log> | awk '
/panic|PANIC|HardFault|Fault|ERROR|Error:/ {print "CRASH: "$0; fflush(); next}
/Connection Timeout|disconnected, rescanning/ {print "BLE-DROP: "$0; fflush(); next}
/Core1 peak/ { for(i=1;i<=NF;i++){if($i=="underruns")u=$(i+1)+0; if($i=="=")l=$(i+1)+0}
  if(u>5||l>88) print "AUDIO-STRESS: load="l"% underruns="u; fflush(); next }'
```
Note: a clean boot shows no `Core1 peak` until core0 reaches the I2S feed (gated behind
cyw43 init, which can take tens of seconds — up to ~135 s per `AGENTS.md`). Slow startup is
**not** a hang.
