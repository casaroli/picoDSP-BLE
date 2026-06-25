# rp2350-synth

A polyphonic-voiced (mono-priority) **synthesizer firmware** for the **RP2350**
(specifically the [Pimoroni Pico Plus 2 W](https://shop.pimoroni.com/products/pimoroni-pico-plus-2-w)),
written in `no_std` Rust on top of [Embassy](https://embassy.dev). It runs across both cores: one
core drives USB and audio I/O, the other runs the DSP engine.

It plays from **USB-MIDI** and **BLE-MIDI** (it acts as a Bluetooth Central and connects to a
BLE-MIDI keyboard), outputs audio over **I2S** at 48 kHz / 16-bit, and ships with **17 factory
presets** stored in flash. The signal chain per voice is a 3-oscillator voice → ladder filter →
VCA → delay → reverb → stereo widener, built on
[`infinitedsp-core`](https://crates.io/crates/infinitedsp-core).

This project is a Rust port/evolution of **[picoDSP](https://github.com/Na1w/picoDSP)** — see
[Credits](#credits).

## Supported hardware

> **Only the [Pimoroni Pico Plus 2 W](https://shop.pimoroni.com/products/pimoroni-pico-plus-2-w)
> is supported.**

This is deliberate. The firmware depends on board-specific features of the Pico Plus 2 W:

- the **RP2350B** MCU (`rp235xb`),
- the **8 MiB APS6404L PSRAM** on QMI CS1 (backs the delay ring buffers), and
- the on-board **CYW43** radio for BLE-MIDI.

Other RP2350 boards (including the plain Raspberry Pi Pico 2 / 2 W) are **not** supported and
will not work without changes.

You also need:

- a **debug probe** (e.g. a Raspberry Pi Debug Probe, or a second Pico running `picotool`/CMSIS-DAP)
  wired to the target's SWD pins, and
- an **I2S DAC** wired to the configured I2S pins for audio output.

## Building & flashing

### Prerequisites

1. A Rust toolchain (stable) with the Cortex-M target:

   ```sh
   rustup target add thumbv8m.main-none-eabihf
   ```

2. [`probe-rs`](https://probe.rs) for flashing and log streaming:

   ```sh
   cargo install probe-rs-tools
   ```

The target and runner are already configured in `.cargo/config`
(`thumbv8m.main-none-eabihf`, `probe-rs run --chip RP235x`), so no extra flags are needed.

### Flash + stream logs

With the debug probe connected to the board:

```sh
cargo run --release
```

This builds, flashes, and then streams **defmt logs over RTT** (shown by `probe-rs`).

> **Note:** MIDI and storage logs go over **USB-serial**, not RTT — open the USB serial port to
> see them.

### Notes & troubleshooting

- The CYW43 radio is a separate chip. A soft reset or re-flash toggles its power pin, but it
  occasionally needs a **physical USB power-cycle** to clear a wedged state.
- **Intermittent slow startup:** CYW43 `control.init` sometimes takes tens of seconds before the
  host reports `initialized`. This is a known, pre-existing flaky behaviour — not a hang. Wait it
  out or power-cycle.

## Host tool: `midictl`

`tools/midictl` is a tiny [`midir`](https://crates.io/crates/midir) CLI to drive the synth over
USB-MIDI without a DAW. It auto-targets the `PicoDSP` port. Examples:

```sh
cd tools/midictl
cargo run -- pc 12              # load preset 12
cargo run -- note 60 8          # hold middle C for 8 beats
cargo run -- cc 67 127 && cargo run -- cc 67 0   # soft-pedal: cycle to next preset
```

See `tools/midictl/README.md` for details.

## Project layout

See [`AGENTS.md`](AGENTS.md) for an in-depth tour of the architecture (dual-core split, audio
path, MIDI/BLE handling, storage, PSRAM, and the locally-patched vendored crates).

## Credits

This firmware builds on the work of the original authors:

- **[picoDSP](https://github.com/Na1w/picoDSP)** by **[Na1w](https://github.com/Na1w)** — the
  original project this is based on. All credit for the original design and concept goes to its
  authors.
- **[`infinitedsp-core`](https://crates.io/crates/infinitedsp-core)** — the DSP engine powering
  the synth voice and effects chain.
- The **[Embassy](https://embassy.dev)** project and the wider Rust embedded ecosystem
  (`embassy-rp`, `cyw43`, `trouble-host`, and more).

If you are an original author and want your credit adjusted, please open an issue.

## License

Licensed under the [MIT License](LICENSE).
