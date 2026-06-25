use crate::common::shared::{BLE_MIDI_CHANNEL, SystemCommand, COMMAND_CHANNEL, PRESET_CHANNEL};
use crate::data::storage::{
    Storage, MAGIC as STORAGE_MAGIC, STORAGE_IMAGE_SIZE, VERSION as STORAGE_VERSION,
};
use crate::usb::logger::{LED_SIGNAL_CHANNEL, MIDI_LOG_CHANNEL};
use alloc::sync::Arc;
use alloc::vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use embassy_futures::select::{select3, Either3};
use embassy_rp::peripherals::USB;
use embassy_rp::usb::Driver;
use embassy_time::Instant;
use embassy_usb::class::midi::{Receiver, Sender};
use infinitedsp_core::core::channels::Mono;
use infinitedsp_core::FrameProcessor;

macro_rules! log_midi {
    ($($arg:tt)*) => {
        {
            let mut msg = heapless::String::<64>::new();
            let now = Instant::now().as_millis();
            if core::fmt::write(&mut msg, format_args!("\r\n[{}] ", now)).is_ok() {
                if core::fmt::write(&mut msg, format_args!($($arg)*)).is_ok() {
                    let _ = MIDI_LOG_CHANNEL.try_send(msg);
                }
            }
        }
    };
}

const NOTE_OFF: u8 = 0x80;
const NOTE_ON: u8 = 0x90;
const CONTROL_CHANGE: u8 = 0xB0;
const PROGRAM_CHANGE: u8 = 0xC0;
const PITCH_BEND: u8 = 0xE0;
const SYSEX_START: u8 = 0xF0;
const SYSEX_END: u8 = 0xF7;

const CC_MOD_WHEEL: u8 = 1;
const CC_PORTAMENTO_TIME: u8 = 5;
const CC_SUSTAIN: u8 = 64;
/// Soft pedal (una corda). A full press→release gesture cycles to the next stored preset.
const CC_SOFT_PEDAL: u8 = 67;
const CC_FILTER_RESONANCE: u8 = 71;
const CC_FILTER_CUTOFF: u8 = 74;
const CC_ALL_SOUND_OFF: u8 = 120;
const CC_ALL_NOTES_OFF: u8 = 123;

const SYSEX_ID: u8 = 0x7D;
const SYSEX_MODEL: u8 = 0x01;
const CMD_DUMP_REQ: u8 = 0x01;
const CMD_WRITE_REQ: u8 = 0x02;
const CMD_WRITE_SUCCESS: u8 = 0x03;
const CMD_WRITE_ERROR: u8 = 0x04;

const ERR_BAD_LENGTH: u8 = 0x01;
const ERR_BAD_MAGIC: u8 = 0x02;

fn midi_to_freq(note: u8) -> f32 {
    440.0 * libm::powf(2.0, (note as f32 - 69.0) / 12.0)
}

struct NoteStack {
    notes: heapless::Vec<u8, 16>,
    pending_off: heapless::Vec<u8, 16>,
    sustain_active: bool,
    soft_pedal_held: bool,
}

impl NoteStack {
    fn new() -> Self {
        Self {
            notes: heapless::Vec::new(),
            pending_off: heapless::Vec::new(),
            sustain_active: false,
            soft_pedal_held: false,
        }
    }

    /// Update the soft-pedal (CC67) latch and report whether a full press→release gesture
    /// just completed — the edge used to advance to the next preset.
    fn soft_pedal(&mut self, pressed: bool) -> bool {
        let released = self.soft_pedal_held && !pressed;
        self.soft_pedal_held = pressed;
        released
    }

    fn note_on(&mut self, note: u8) {
        if let Some(pos) = self.pending_off.iter().position(|&n| n == note) {
            self.pending_off.remove(pos);
        }
        if !self.notes.contains(&note) {
            let _ = self.notes.push(note);
        }
    }

    fn note_off(&mut self, note: u8) {
        if self.sustain_active {
            if self.notes.contains(&note) && !self.pending_off.contains(&note) {
                let _ = self.pending_off.push(note);
            }
        } else {
            if let Some(pos) = self.notes.iter().position(|&n| n == note) {
                self.notes.remove(pos);
            }
        }
    }

    fn set_sustain(&mut self, active: bool) {
        self.sustain_active = active;
        if !active {
            for note in self.pending_off.iter() {
                if let Some(pos) = self.notes.iter().position(|n| n == note) {
                    self.notes.remove(pos);
                }
            }
            self.pending_off.clear();
        }
    }

    fn clear(&mut self) {
        self.notes.clear();
        self.pending_off.clear();
        self.sustain_active = false;
    }

    fn active_note(&self) -> Option<u8> {
        self.notes.last().copied()
    }
}

pub struct MidiControl {
    target_freq_bits: AtomicU32,
    gate: AtomicBool,
    gate_reset: AtomicBool,
    portamento_amount_bits: AtomicU32,
    pitch_bend_bits: AtomicU32,
    mod_wheel_bits: AtomicU32,
    velocity_bits: AtomicU32,
    parameter_1_bits: AtomicU32,
    parameter_2_bits: AtomicU32,
}

impl MidiControl {
    pub fn new() -> Self {
        Self {
            target_freq_bits: AtomicU32::new(440.0f32.to_bits()),
            gate: AtomicBool::new(false),
            gate_reset: AtomicBool::new(false),
            portamento_amount_bits: AtomicU32::new(0.0f32.to_bits()),
            pitch_bend_bits: AtomicU32::new(1.0f32.to_bits()),
            mod_wheel_bits: AtomicU32::new(0.0f32.to_bits()),
            velocity_bits: AtomicU32::new(1.0f32.to_bits()),
            parameter_1_bits: AtomicU32::new(0.5f32.to_bits()),
            parameter_2_bits: AtomicU32::new(0.0f32.to_bits()),
        }
    }

    pub fn set_freq(&self, freq: f32) {
        self.target_freq_bits
            .store(freq.to_bits(), Ordering::Relaxed);
    }

    pub fn set_gate(&self, gate: bool) {
        self.gate.store(gate, Ordering::Relaxed);
        if !gate {
            self.gate_reset.store(true, Ordering::Relaxed);
        }
    }

    pub fn take_gate_reset(&self) -> bool {
        self.gate_reset.swap(false, Ordering::Relaxed)
    }

    pub fn set_portamento(&self, amount: f32) {
        self.portamento_amount_bits
            .store(amount.to_bits(), Ordering::Relaxed);
    }

    pub fn set_pitch_bend(&self, bend_factor: f32) {
        self.pitch_bend_bits
            .store(bend_factor.to_bits(), Ordering::Relaxed);
    }

    pub fn set_mod_wheel(&self, value: f32) {
        self.mod_wheel_bits
            .store(value.to_bits(), Ordering::Relaxed);
    }

    /// Latch the normalized (0..1) velocity of the most recent note-on. Held until the next
    /// note-on, so legato fall-back notes keep the current velocity.
    pub fn set_velocity(&self, value: f32) {
        self.velocity_bits.store(value.to_bits(), Ordering::Relaxed);
    }

    pub fn set_parameter_1(&self, value: f32) {
        self.parameter_1_bits
            .store(value.to_bits(), Ordering::Relaxed);
    }

    pub fn set_parameter_2(&self, value: f32) {
        self.parameter_2_bits
            .store(value.to_bits(), Ordering::Relaxed);
    }

    pub fn reset(&self) {
        self.gate.store(false, Ordering::Relaxed);
        self.gate_reset.store(false, Ordering::Relaxed);
        self.pitch_bend_bits
            .store(1.0f32.to_bits(), Ordering::Relaxed);
        self.mod_wheel_bits
            .store(0.0f32.to_bits(), Ordering::Relaxed);
    }

    pub fn get_target_freq(&self) -> f32 {
        f32::from_bits(self.target_freq_bits.load(Ordering::Relaxed))
    }

    pub fn get_portamento_amount(&self) -> f32 {
        f32::from_bits(self.portamento_amount_bits.load(Ordering::Relaxed))
    }

    pub fn get_pitch_bend(&self) -> f32 {
        f32::from_bits(self.pitch_bend_bits.load(Ordering::Relaxed))
    }

    #[allow(dead_code)]
    pub fn get_mod_wheel(&self) -> f32 {
        f32::from_bits(self.mod_wheel_bits.load(Ordering::Relaxed))
    }

    pub fn get_velocity(&self) -> f32 {
        f32::from_bits(self.velocity_bits.load(Ordering::Relaxed))
    }

    pub fn get_parameter_1(&self) -> f32 {
        f32::from_bits(self.parameter_1_bits.load(Ordering::Relaxed))
    }

    pub fn get_parameter_2(&self) -> f32 {
        f32::from_bits(self.parameter_2_bits.load(Ordering::Relaxed))
    }

    pub fn get_gate(&self) -> f32 {
        if self.gate.load(Ordering::Relaxed) {
            1.0
        } else {
            0.0
        }
    }
}

pub struct MidiFreq {
    control: Arc<MidiControl>,
    current_freq: f32,
}

impl MidiFreq {
    pub fn new(control: Arc<MidiControl>) -> Self {
        let initial_freq = control.get_target_freq();
        Self {
            control,
            current_freq: initial_freq,
        }
    }
}

impl FrameProcessor<Mono> for MidiFreq {
    fn process(&mut self, buffer: &mut [f32], _frame_index: u64) {
        let target = self.control.get_target_freq();
        let amount = self.control.get_portamento_amount();
        let bend = self.control.get_pitch_bend();

        const CHUNK_SIZE: usize = 32;
        let factor = 1.0 - amount.clamp(0.0, 0.999);

        for chunk in buffer.chunks_mut(CHUNK_SIZE) {
            let diff = target - self.current_freq;

            if diff.abs() < 0.1 {
                self.current_freq = target;
            } else {
                self.current_freq += diff * factor;
            }

            let final_freq = self.current_freq * bend;

            for sample in chunk.iter_mut() {
                *sample = final_freq;
            }
        }
    }

    fn set_sample_rate(&mut self, _sample_rate: f32) {}

    fn reset(&mut self) {
        self.current_freq = self.control.get_target_freq();
    }

    fn latency_samples(&self) -> u32 {
        0
    }
    fn name(&self) -> &str {
        "MidiFreq"
    }
    fn visualize(&self, _indent: usize) -> alloc::string::String {
        "MidiFreq".into()
    }
}

pub struct MidiGate(pub Arc<MidiControl>);
impl FrameProcessor<Mono> for MidiGate {
    fn process(&mut self, buffer: &mut [f32], _frame_index: u64) {
        let g = self.0.get_gate();
        let reset = self.0.take_gate_reset();

        for (i, sample) in buffer.iter_mut().enumerate() {
            if reset && i < 4 {
                *sample = 0.0;
            } else {
                *sample = g;
            }
        }
    }
    fn set_sample_rate(&mut self, _sample_rate: f32) {}
    fn reset(&mut self) {}
    fn latency_samples(&self) -> u32 {
        0
    }
    fn name(&self) -> &str {
        "MidiGate"
    }
    fn visualize(&self, _indent: usize) -> alloc::string::String {
        "MidiGate".into()
    }
}

/// Emits the latched note-on velocity (0..1) as a control signal. Multiplied into the amp
/// envelope so harder hits are louder — fixed, always-on velocity sensitivity.
pub struct MidiVelocity(pub Arc<MidiControl>);
impl FrameProcessor<Mono> for MidiVelocity {
    fn process(&mut self, buffer: &mut [f32], _frame_index: u64) {
        let v = self.0.get_velocity();
        for sample in buffer.iter_mut() {
            *sample = v;
        }
    }
    fn set_sample_rate(&mut self, _sample_rate: f32) {}
    fn reset(&mut self) {}
    fn latency_samples(&self) -> u32 {
        0
    }
    fn name(&self) -> &str {
        "MidiVelocity"
    }
    fn visualize(&self, _indent: usize) -> alloc::string::String {
        "MidiVelocity".into()
    }
}

pub struct MidiFilterCutoff(pub Arc<MidiControl>);
impl FrameProcessor<Mono> for MidiFilterCutoff {
    fn process(&mut self, buffer: &mut [f32], _frame_index: u64) {
        let val = self.0.get_parameter_1();
        let freq = 20.0 * libm::powf(1000.0, val);
        for sample in buffer.iter_mut() {
            *sample = freq;
        }
    }
    fn set_sample_rate(&mut self, _sample_rate: f32) {}
    fn reset(&mut self) {}
    fn latency_samples(&self) -> u32 {
        0
    }
    fn name(&self) -> &str {
        "MidiFilterCutoff"
    }
    fn visualize(&self, _indent: usize) -> alloc::string::String {
        "MidiFilterCutoff".into()
    }
}

pub struct MidiFilterResonance(pub Arc<MidiControl>);
impl FrameProcessor<Mono> for MidiFilterResonance {
    fn process(&mut self, buffer: &mut [f32], _frame_index: u64) {
        let val = self.0.get_parameter_2();
        let q = 0.707 + (val * 9.3);
        for sample in buffer.iter_mut() {
            *sample = q;
        }
    }
    fn set_sample_rate(&mut self, _sample_rate: f32) {}
    fn reset(&mut self) {}
    fn latency_samples(&self) -> u32 {
        0
    }
    fn name(&self) -> &str {
        "MidiFilterResonance"
    }
    fn visualize(&self, _indent: usize) -> alloc::string::String {
        "MidiFilterResonance".into()
    }
}

/// Load the preset at `index` from flash, map its parameters onto the live controls and
/// hand it to the DSP via `PRESET_CHANNEL`. Shared by Program Change and the soft-pedal
/// preset cycling. The caller owns `current_preset_index`.
async fn load_and_apply_preset(storage: &mut Storage<'static>, index: usize, midi_control: &MidiControl) {
    if let Some(preset) = storage.load_preset(index).await {
        log_midi!("Loaded: {}\r\n", preset.get_name());
        let cutoff_norm = libm::log10f(preset.filter.cutoff / 20.0) / libm::log10f(1000.0);
        midi_control.set_parameter_1(cutoff_norm.clamp(0.0, 1.0));
        let res_norm = (preset.filter.resonance - 0.707) / 9.3;
        midi_control.set_parameter_2(res_norm.clamp(0.0, 1.0));
        midi_control.set_portamento(preset.portamento);
        let _ = PRESET_CHANNEL.try_send(preset);
    } else {
        log_midi!("Preset {} not found\r\n", index);
    }
}

/// Streams a SysEx message out as USB-MIDI packets: buffers content bytes into groups of
/// three and emits the correct end-of-SysEx CIN. Used for the dump response.
struct SysexTx<'a> {
    sender: &'a mut Sender<'static, Driver<'static, USB>>,
    buf: [u8; 3],
    len: usize,
}

impl<'a> SysexTx<'a> {
    async fn push(&mut self, b: u8) {
        self.buf[self.len] = b;
        self.len += 1;
        if self.len == 3 {
            let _ = self
                .sender
                .write_packet(&[0x04, self.buf[0], self.buf[1], self.buf[2]])
                .await;
            self.len = 0;
        }
    }

    /// Flush the trailing 0..=2 buffered bytes plus the terminator (0xF7) with the matching CIN.
    async fn end(&mut self) {
        let p = match self.len {
            0 => [0x05, SYSEX_END, 0x00, 0x00],
            1 => [0x06, self.buf[0], SYSEX_END, 0x00],
            _ => [0x07, self.buf[0], self.buf[1], SYSEX_END],
        };
        let _ = self.sender.write_packet(&p).await;
        self.len = 0;
    }
}

/// Send the full storage image to the editor as one CMD_WRITE_REQ SysEx, nibbleizing on the
/// fly (no 2x encode buffer).
async fn send_sysex_dump(sender: &mut Sender<'static, Driver<'static, USB>>, image: &[u8]) {
    let mut tx = SysexTx {
        sender,
        buf: [0; 3],
        len: 0,
    };
    tx.push(SYSEX_START).await;
    tx.push(SYSEX_ID).await;
    tx.push(SYSEX_MODEL).await;
    tx.push(CMD_WRITE_REQ).await;
    for &b in image {
        tx.push((b >> 4) & 0x0F).await;
        tx.push(b & 0x0F).await;
    }
    tx.end().await;
}

/// Send a one-byte (success) or two-byte (error) status SysEx back to the editor.
async fn send_sysex_status(
    sender: &mut Sender<'static, Driver<'static, USB>>,
    cmd: u8,
    err: Option<u8>,
) {
    let _ = sender
        .write_packet(&[0x04, SYSEX_START, SYSEX_ID, SYSEX_MODEL])
        .await;
    let p = match err {
        None => [0x06, cmd, SYSEX_END, 0x00],
        Some(e) => [0x07, cmd, e, SYSEX_END],
    };
    let _ = sender.write_packet(&p).await;
}

/// Handle a complete SysEx command from the editor (picoDSP-Edit): full-bank dump or write.
/// `image` is the de-nibbleized storage image buffer (`STORAGE_IMAGE_SIZE`); for a WRITE it
/// already holds the decoded `raw_len` bytes, for a DUMP it is reused as the read scratch.
async fn handle_sysex(
    cmd: u8,
    image: &mut [u8],
    raw_len: usize,
    overflow: bool,
    storage: &mut Storage<'static>,
    sender: &mut Sender<'static, Driver<'static, USB>>,
    midi_control: &MidiControl,
    current_preset_index: usize,
) {
    match cmd {
        CMD_DUMP_REQ => {
            log_midi!("SysEx: Dump Request\r\n");
            let n = STORAGE_IMAGE_SIZE.min(image.len());
            storage.read_raw(&mut image[..n]).await;
            send_sysex_dump(sender, &image[..n]).await;
            log_midi!("SysEx: Dump Sent ({} bytes)\r\n", n);
        }
        CMD_WRITE_REQ => {
            defmt::info!("SysEx WRITE_REQ from Edit: {=usize} decoded bytes", raw_len);
            if overflow || raw_len != STORAGE_IMAGE_SIZE {
                log_midi!("SysEx: Invalid Length ({})\r\n", raw_len);
                defmt::warn!(
                    "SysEx REJECTED bad length: {=usize} bytes (expected {=usize})",
                    raw_len,
                    STORAGE_IMAGE_SIZE
                );
                send_sysex_status(sender, CMD_WRITE_ERROR, Some(ERR_BAD_LENGTH)).await;
                return;
            }
            let magic = u32::from_le_bytes([image[0], image[1], image[2], image[3]]);
            let version = u32::from_le_bytes([image[4], image[5], image[6], image[7]]);
            if magic == STORAGE_MAGIC && version == STORAGE_VERSION {
                storage.write_raw(&image[..raw_len]).await;
                log_midi!("SysEx: Write Success\r\n");
                defmt::info!(
                    "SysEx write OK -> flash; reloading active preset {=usize}",
                    current_preset_index
                );
                send_sysex_status(sender, CMD_WRITE_SUCCESS, None).await;
                load_and_apply_preset(storage, current_preset_index, midi_control).await;
            } else {
                log_midi!(
                    "SysEx: Invalid Magic/Version ({:X}, {})\r\n",
                    magic,
                    version
                );
                defmt::warn!(
                    "SysEx REJECTED bad magic/version: magic={=u32:#x} version={=u32} (expected {=u32:#x}/{=u32})",
                    magic,
                    version,
                    STORAGE_MAGIC,
                    STORAGE_VERSION
                );
                send_sysex_status(sender, CMD_WRITE_ERROR, Some(ERR_BAD_MAGIC)).await;
            }
        }
        _ => {}
    }
}

/// Handle a single channel-voice MIDI message. Shared by the USB-MIDI path and the
/// BLE-MIDI path so both transports drive the synth through identical logic.
async fn handle_voice_message(
    status: u8,
    d1: u8,
    d2: u8,
    notes: &mut NoteStack,
    midi_control: &MidiControl,
    storage: &mut Storage<'static>,
    current_preset_index: &mut usize,
) {
    log_midi!("MIDI: [{:02X}-{:02X}-{:02X}] - ", status, d1, d2);

    let cmd = status & 0xF0;

    match cmd {
        NOTE_ON if d2 > 0 => {
            let freq = midi_to_freq(d1);
            log_midi!("NOTE ON: {} ({} Hz)", d1, freq);
            notes.note_on(d1);
            midi_control.set_velocity((d2 as f32 / 127.0).clamp(0.0, 1.0));
            midi_control.set_freq(freq);
            midi_control.set_gate(true);
            let _ = LED_SIGNAL_CHANNEL.try_send(true);
        }
        NOTE_OFF | NOTE_ON => {
            let freq = midi_to_freq(d1);
            log_midi!("NOTE OFF: {}", freq);
            notes.note_off(d1);

            if let Some(last_note) = notes.active_note() {
                midi_control.set_freq(midi_to_freq(last_note));
                midi_control.set_gate(true);
            } else {
                midi_control.set_gate(false);
                let _ = LED_SIGNAL_CHANNEL.try_send(false);
            }
        }
        CONTROL_CHANGE => {
            let val_norm = d2 as f32 / 127.0;
            match d1 {
                CC_MOD_WHEEL => {
                    log_midi!("MOD WHEEL: {:.2}", val_norm);
                    midi_control.set_mod_wheel(val_norm);
                }
                CC_PORTAMENTO_TIME => {
                    let amount = val_norm;
                    log_midi!("PORTAMENTO: {:.2}", amount);
                    midi_control.set_portamento(amount);
                }
                CC_SUSTAIN => {
                    let sustain_on = d2 >= 64;
                    log_midi!("SUSTAIN: {}", if sustain_on { "ON" } else { "OFF" });
                    notes.set_sustain(sustain_on);

                    if !sustain_on {
                        if let Some(last_note) = notes.active_note() {
                            midi_control.set_freq(midi_to_freq(last_note));
                            midi_control.set_gate(true);
                        } else {
                            midi_control.set_gate(false);
                            let _ = LED_SIGNAL_CHANNEL.try_send(false);
                        }
                    }
                }
                CC_SOFT_PEDAL => {
                    // A full press (>=64) followed by release (<64) advances to the next
                    // stored preset, wrapping around. The pedal-up edge is the trigger so a
                    // single tap = one step, regardless of how long it's held.
                    if notes.soft_pedal(d2 >= 64) {
                        let count = storage.num_presets().await;
                        if count > 0 {
                            *current_preset_index = (*current_preset_index + 1) % count;
                            log_midi!("SOFT PEDAL: next preset {}\r\n", *current_preset_index);
                            defmt::info!("SOFT PEDAL -> preset {=usize}", *current_preset_index);
                            load_and_apply_preset(storage, *current_preset_index, midi_control).await;
                        }
                    }
                }
                CC_FILTER_RESONANCE => {
                    log_midi!("RESONANCE: {:.2}", val_norm);
                    defmt::info!("PARAM resonance <- {=f32}", val_norm);
                    midi_control.set_parameter_2(val_norm);
                }
                CC_FILTER_CUTOFF => {
                    log_midi!("CUTOFF: {:.2}", val_norm);
                    defmt::info!("PARAM cutoff <- {=f32}", val_norm);
                    midi_control.set_parameter_1(val_norm);
                }
                CC_ALL_SOUND_OFF | CC_ALL_NOTES_OFF => {
                    log_midi!("ALL NOTES/SOUND OFF");
                    notes.clear();
                    midi_control.reset();
                    let _ = LED_SIGNAL_CHANNEL.try_send(false);
                }
                _ => {}
            }
        }
        PROGRAM_CHANGE => {
            log_midi!("PROGRAM CHANGE: {}\r\n", d1);
            defmt::info!("PROGRAM CHANGE -> preset {=u8}", d1);
            *current_preset_index = d1 as usize;
            load_and_apply_preset(storage, d1 as usize, midi_control).await;
        }
        PITCH_BEND => {
            let val = ((d2 as u16) << 7) | (d1 as u16);
            log_midi!("PITCHBEND: {}", val);
            let norm = (val as f32 - 8192.0) / 8192.0;
            let factor = libm::powf(2.0, (norm * 2.0) / 12.0);
            midi_control.set_pitch_bend(factor);
        }
        _ => {}
    }
}

#[embassy_executor::task]
pub async fn midi_task(
    mut receiver: Receiver<'static, Driver<'static, USB>>,
    mut sender: Sender<'static, Driver<'static, USB>>,
    midi_control: Arc<MidiControl>,
    mut storage: Storage<'static>,
    needs_format: bool,
) {
    // Deferred boot reformat. main() decided a reformat is due (version bump / blank flash) but
    // left the 7-sector bank write to here: format() drives embassy flash ops that pause core1
    // over the SIO FIFO, and that only works once core1 is actually running its loop (so it can
    // ack the PSRAM gate) AND the audio drain loop in main_task is live (so core1 doesn't block
    // in AUDIO_CHANNEL.send and miss the gate). Wait for core1, then format — identical to the
    // runtime ResetStorage path, which is the proven, deadlock-safe scenario.
    if needs_format {
        while !crate::common::shared::CORE1_RUNNING.load(core::sync::atomic::Ordering::Acquire) {
            embassy_time::Timer::after(embassy_time::Duration::from_millis(2)).await;
        }
        log_midi!("Boot: writing 128-preset factory bank...\r\n");
        storage.format().await;
        log_midi!("Boot: factory bank written.\r\n");
    }

    let mut buf = [0; 64];
    let mut notes = NoteStack::new();

    let mut current_preset_index = 4;

    // De-nibbleized SysEx image buffer. Incoming WRITE nibbles are decoded on the fly into
    // here (2 nibbles -> 1 byte) so the 2x-larger nibbleized stream is never held in RAM; a
    // DUMP reads the storage image into the same buffer. ~28 KB, sits alongside the synth.
    let mut sysex_image = vec![0u8; STORAGE_IMAGE_SIZE];
    let mut sysex_hdr = [0u8; 4]; // F0, manufacturer, model, command
    let mut sysex_hdr_len = 0usize;
    let mut sysex_raw_idx = 0usize; // decoded bytes in sysex_image
    let mut sysex_hi: i16 = -1; // pending high nibble (-1 = none)
    let mut sysex_overflow = false;
    let mut in_sysex = false;

    loop {
        // Service BLE-MIDI and commands even while USB is disconnected, so a BLE keyboard
        // can play the synth without a USB host attached.
        match select3(
            receiver.wait_connection(),
            COMMAND_CHANNEL.receive(),
            BLE_MIDI_CHANNEL.receive(),
        )
        .await
        {
            Either3::First(_) => {}
            Either3::Second(SystemCommand::ResetStorage) => {
                log_midi!("Command: Reset Storage...\r\n");
                storage.format().await;
                log_midi!("Storage Reset Complete.\r\n");
                continue;
            }
            Either3::Third(msg) => {
                handle_voice_message(
                    msg[0],
                    msg[1],
                    msg[2],
                    &mut notes,
                    midi_control.as_ref(),
                    &mut storage,
                    &mut current_preset_index,
                )
                .await;
                continue;
            }
        }

        loop {
            match select3(
                receiver.read_packet(&mut buf),
                COMMAND_CHANNEL.receive(),
                BLE_MIDI_CHANNEL.receive(),
            )
            .await
            {
                Either3::First(read_result) => match read_result {
                    Ok(n) => {
                        let data = &buf[..n];
                        for packet in data.chunks(4) {
                            if packet.len() < 4 {
                                continue;
                            }

                            let cin = packet[0] & 0x0F;
                            let status = packet[1];
                            let d1 = packet[2];
                            let d2 = packet[3];

                            if cin == 0x4 || cin == 0x5 || cin == 0x6 || cin == 0x7 {
                                // SysEx (USB-MIDI CINs 0x4=3 bytes, 0x5/0x6/0x7=end w/ 1/2/3).
                                // Decode the editor's nibbleized payload on the fly into the
                                // image buffer so we never hold the 2x nibbleized stream.
                                let nbytes = match cin {
                                    0x4 => 3,
                                    0x5 => 1,
                                    0x6 => 2,
                                    0x7 => 3,
                                    _ => 0,
                                };
                                for &b in &packet[1..1 + nbytes] {
                                    if b == SYSEX_START {
                                        in_sysex = true;
                                        sysex_hdr[0] = b;
                                        sysex_hdr_len = 1;
                                        sysex_raw_idx = 0;
                                        sysex_hi = -1;
                                        sysex_overflow = false;
                                    } else if !in_sysex {
                                        // stray byte outside a SysEx -> ignore
                                    } else if sysex_hdr_len < 4 {
                                        sysex_hdr[sysex_hdr_len] = b;
                                        sysex_hdr_len += 1;
                                    } else if b == SYSEX_END {
                                        in_sysex = false;
                                        if sysex_hi >= 0 {
                                            sysex_overflow = true; // dangling nibble
                                        }
                                        if sysex_hdr[1] == SYSEX_ID
                                            && sysex_hdr[2] == SYSEX_MODEL
                                        {
                                            handle_sysex(
                                                sysex_hdr[3],
                                                &mut sysex_image,
                                                sysex_raw_idx,
                                                sysex_overflow,
                                                &mut storage,
                                                &mut sender,
                                                midi_control.as_ref(),
                                                current_preset_index,
                                            )
                                            .await;
                                        }
                                    } else {
                                        // Payload nibble (0x00..=0x0F): combine pairs.
                                        if sysex_hi < 0 {
                                            sysex_hi = (b & 0x0F) as i16;
                                        } else {
                                            if sysex_raw_idx < sysex_image.len() {
                                                sysex_image[sysex_raw_idx] =
                                                    ((sysex_hi as u8) << 4) | (b & 0x0F);
                                                sysex_raw_idx += 1;
                                            } else {
                                                sysex_overflow = true;
                                            }
                                            sysex_hi = -1;
                                        }
                                    }
                                }
                                continue;
                            }

                            handle_voice_message(
                                status,
                                d1,
                                d2,
                                &mut notes,
                                midi_control.as_ref(),
                                &mut storage,
                                &mut current_preset_index,
                            )
                            .await;
                        }
                    }
                    Err(_) => {
                        break;
                    }
                },
                Either3::Second(cmd) => match cmd {
                    SystemCommand::ResetStorage => {
                        log_midi!("Command: Reset Storage...\r\n");
                        storage.format().await;
                        log_midi!("Storage Reset Complete.\r\n");
                    }
                },
                Either3::Third(msg) => {
                    handle_voice_message(
                        msg[0],
                        msg[1],
                        msg[2],
                        &mut notes,
                        midi_control.as_ref(),
                        &mut storage,
                        &mut current_preset_index,
                    )
                    .await;
                }
            }
        }
    }
}
