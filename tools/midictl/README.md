# midictl

Tiny host-side MIDI sender for driving the **rp2350-synth** over USB-MIDI during
development — load presets, hold notes, send CCs, or raw bytes from the command line.
Handy for scripted testing when you don't have a DAW/keyboard wired up.

It auto-targets the first MIDI output port whose name contains `picodsp`
(the firmware enumerates as `PicoDSP …`).

## Build / run

The repo root `.cargo/config.toml` forces the embedded target, so this tool has its own
`.cargo/config.toml` pinning the **host** target. It's set to `aarch64-apple-darwin`
(Apple Silicon); change it to your host if different (`rustc -vV | grep host`).

```sh
cd tools/midictl
cargo run -- list                 # list MIDI output ports
cargo run -- pc 12                # Program Change -> load preset 12
cargo run -- note 60 8            # hold middle C (note 60) for 8 seconds
cargo run -- note 72 4 100        # note 72, 4 s, velocity 100
cargo run -- cc 74 64             # Control Change 74 (filter cutoff) = 64
cargo run -- raw 0x90 0x3C 0x64   # raw bytes (Note On, ch1)
```

For a one-shot binary: `cargo build --release` then run `target/<host>/release/midictl …`.

## Commands

| Command | Bytes sent | Notes |
|---|---|---|
| `list` | — | print available output ports |
| `pc <program>` | `C0 prog` | select a preset (0-based index) |
| `note <note> [secs] [vel]` | `90 note vel` … `80 note 00` | Note On, hold `secs` (default 3), Note Off |
| `cc <controller> <value>` | `B0 ctrl val` | Control Change |
| `raw <byte> [byte …]` | the bytes | decimal or `0x`-hex |

## Port selection

Set `MIDI_PORT` to match a different port-name substring:

```sh
MIDI_PORT="my synth" cargo run -- pc 0
```

## Synth-specific reference

- **Preset cycling:** a CC67 (soft pedal) press→release advances to the next stored preset:
  `cargo run -- cc 67 127 && cargo run -- cc 67 0`
- **Filter:** CC74 = cutoff, CC71 = resonance; CC1 = mod wheel; CC5 = portamento time;
  CC64 = sustain.
