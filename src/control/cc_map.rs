//! Authoritative CC ↔ DSP-parameter map, shared by the live-edit path and (in spirit) the
//! `picoDSP-Edit` editor, which mirrors these numbers and scalings. One CC per parameter,
//! 7-bit value 0..=127 scaled to the parameter's range here on the device.
//!
//! Two classes of parameter:
//!   * **continuous** — written into a `MidiControl` live slot (read per-block by the DSP),
//!     so a slider drag updates smoothly with no voice rebuild. `apply_cc` returns
//!     `Some(false)`.
//!   * **structural** — changes the chain shape (oscillator waveform/vibrato, FX enable), so
//!     it's written into the working `Preset` and the caller rebuilds the voice. `apply_cc`
//!     returns `Some(true)`.
//!
//! `apply_cc` always keeps the working `Preset` in sync (for *every* mapped CC, continuous or
//! not) so a later structural rebuild re-seeds the live slots to the values the user dialled
//! in, never snapping back to the stored preset.

use crate::control::midi::{slot, MidiControl};
use crate::data::presets::Preset;

// Reused, semantically-standard CCs (also drivable by a normal MIDI controller).
pub const CC_PORTAMENTO: u8 = 5;
pub const CC_RESONANCE: u8 = 71;
pub const CC_CUTOFF: u8 = 74;

// Continuous params — undefined CC range 16..=31 then 102..=110.
pub const CC_OSC1_LEVEL: u8 = 16;
pub const CC_OSC1_OCTAVE: u8 = 17;
pub const CC_OSC1_DETUNE: u8 = 18;
pub const CC_OSC2_LEVEL: u8 = 19;
pub const CC_OSC2_OCTAVE: u8 = 20;
pub const CC_OSC2_DETUNE: u8 = 21;
pub const CC_OSC3_LEVEL: u8 = 22;
pub const CC_OSC3_OCTAVE: u8 = 23;
pub const CC_OSC3_DETUNE: u8 = 24;
pub const CC_NOISE: u8 = 25;
pub const CC_FILT_ENV_AMT: u8 = 26;
pub const CC_FILT_ATTACK: u8 = 27;
pub const CC_FILT_DECAY: u8 = 28;
pub const CC_FILT_SUSTAIN: u8 = 29;
pub const CC_FILT_RELEASE: u8 = 30;
pub const CC_AMP_ATTACK: u8 = 31;
pub const CC_AMP_DECAY: u8 = 102;
pub const CC_AMP_SUSTAIN: u8 = 103;
pub const CC_AMP_RELEASE: u8 = 104;
pub const CC_DELAY_TIME: u8 = 105;
pub const CC_DELAY_FEEDBACK: u8 = 106;
pub const CC_DELAY_MIX: u8 = 107;
pub const CC_REVERB_SIZE: u8 = 108;
pub const CC_REVERB_DAMPING: u8 = 109;
pub const CC_REVERB_MIX: u8 = 110;

// LFO continuous params — undefined CCs 86,88,89.
pub const CC_LFO_FREQ: u8 = 86;
pub const CC_LFO_VIB_AMT: u8 = 88;
pub const CC_LFO_FILT_AMT: u8 = 89;

// Structural params — undefined CC range 111..=118, plus LFO enable/waveform at 85/87.
pub const CC_OSC1_WAVEFORM: u8 = 111;
pub const CC_OSC1_VIBRATO: u8 = 112;
pub const CC_OSC2_WAVEFORM: u8 = 113;
pub const CC_OSC2_VIBRATO: u8 = 114;
pub const CC_OSC3_WAVEFORM: u8 = 115;
pub const CC_OSC3_VIBRATO: u8 = 116;
pub const CC_DELAY_ENABLED: u8 = 117;
pub const CC_REVERB_ENABLED: u8 = 118;
pub const CC_LFO_ENABLED: u8 = 85;
pub const CC_LFO_WAVEFORM: u8 = 87;

/// Longest delay the ring buffer can produce (matches `core1::build_synth` and the editor's
/// `MAX_DELAY_SECONDS`); CC105 maps 0..=127 onto 0..this.
pub const MAX_DELAY_SECONDS: f32 = 0.3;

/// Map a normalized 0..1 onto a linear range.
fn lin(n: f32, lo: f32, hi: f32) -> f32 {
    lo + n * (hi - lo)
}

/// Apply a 7-bit CC to the live controls and the working preset.
///
/// Returns `Some(true)` if the CC was a **structural** DSP param (caller must rebuild the
/// voice), `Some(false)` if it was a **continuous** DSP param (already live), or `None` if the
/// CC isn't a DSP parameter (the caller handles mod-wheel / sustain / etc.).
pub fn apply_cc(cc: u8, value: u8, p: &mut Preset, ctrl: &MidiControl) -> Option<bool> {
    let n = value as f32 / 127.0;
    match cc {
        // ---- continuous ---------------------------------------------------------------
        CC_PORTAMENTO => {
            p.portamento = n;
            ctrl.set_portamento(n);
        }
        CC_RESONANCE => {
            p.filter.resonance = n;
            ctrl.set_parameter_2(n);
        }
        CC_CUTOFF => {
            // Device exponential map (matches MidiFilterCutoff); the live slot holds the
            // normalized value, the preset the resolved Hz.
            p.filter.cutoff = 20.0 * libm::powf(1000.0, n);
            ctrl.set_parameter_1(n);
        }

        CC_OSC1_LEVEL => set_level(p, ctrl, 0, n),
        CC_OSC2_LEVEL => set_level(p, ctrl, 1, n),
        CC_OSC3_LEVEL => set_level(p, ctrl, 2, n),
        CC_OSC1_OCTAVE => set_octave(p, ctrl, 0, n),
        CC_OSC2_OCTAVE => set_octave(p, ctrl, 1, n),
        CC_OSC3_OCTAVE => set_octave(p, ctrl, 2, n),
        CC_OSC1_DETUNE => set_detune(p, ctrl, 0, n),
        CC_OSC2_DETUNE => set_detune(p, ctrl, 1, n),
        CC_OSC3_DETUNE => set_detune(p, ctrl, 2, n),

        CC_NOISE => {
            p.noise_level = n;
            ctrl.set_cont(slot::NOISE, n);
        }

        CC_FILT_ENV_AMT => {
            let v = lin(n, -10000.0, 10000.0);
            p.filter.env_amount = v;
            ctrl.set_cont(slot::FILT_ENV_AMT, v);
        }
        CC_FILT_ATTACK => {
            let v = lin(n, 0.0, 5.0);
            p.filter.attack = v;
            ctrl.set_cont(slot::FILT_ATTACK, v);
        }
        CC_FILT_DECAY => {
            let v = lin(n, 0.0, 5.0);
            p.filter.decay = v;
            ctrl.set_cont(slot::FILT_DECAY, v);
        }
        CC_FILT_SUSTAIN => {
            p.filter.sustain = n;
            ctrl.set_cont(slot::FILT_SUSTAIN, n);
        }
        CC_FILT_RELEASE => {
            let v = lin(n, 0.0, 5.0);
            p.filter.release = v;
            ctrl.set_cont(slot::FILT_RELEASE, v);
        }

        CC_AMP_ATTACK => {
            let v = lin(n, 0.0, 5.0);
            p.amp.attack = v;
            ctrl.set_cont(slot::AMP_ATTACK, v);
        }
        CC_AMP_DECAY => {
            let v = lin(n, 0.0, 5.0);
            p.amp.decay = v;
            ctrl.set_cont(slot::AMP_DECAY, v);
        }
        CC_AMP_SUSTAIN => {
            p.amp.sustain = n;
            ctrl.set_cont(slot::AMP_SUSTAIN, n);
        }
        CC_AMP_RELEASE => {
            let v = lin(n, 0.0, 5.0);
            p.amp.release = v;
            ctrl.set_cont(slot::AMP_RELEASE, v);
        }

        CC_DELAY_TIME => {
            let v = lin(n, 0.0, MAX_DELAY_SECONDS);
            p.delay.time = v;
            ctrl.set_cont(slot::DELAY_TIME, v);
        }
        CC_DELAY_FEEDBACK => {
            p.delay.feedback = n;
            ctrl.set_cont(slot::DELAY_FEEDBACK, n);
        }
        CC_DELAY_MIX => {
            p.delay.mix = n;
            ctrl.set_cont(slot::DELAY_MIX, n);
        }

        CC_REVERB_SIZE => {
            p.reverb.size = n;
            ctrl.set_cont(slot::REVERB_SIZE, n);
        }
        CC_REVERB_DAMPING => {
            p.reverb.damping = n;
            ctrl.set_cont(slot::REVERB_DAMPING, n);
        }
        CC_REVERB_MIX => {
            p.reverb.mix = n;
            ctrl.set_cont(slot::REVERB_MIX, n);
        }

        CC_LFO_FREQ => {
            let v = lin(n, 0.0, 20.0);
            p.lfo.frequency = v;
            ctrl.set_cont(slot::LFO_FREQ, v);
        }
        CC_LFO_VIB_AMT => {
            let v = lin(n, 0.0, 20.0);
            p.lfo.vibrato_amount = v;
            ctrl.set_cont(slot::LFO_VIB_AMT, v);
        }
        CC_LFO_FILT_AMT => {
            let v = lin(n, 0.0, 5000.0);
            p.lfo.filter_amount = v;
            ctrl.set_cont(slot::LFO_FILT_AMT, v);
        }

        // ---- structural (rebuild voice) ------------------------------------------------
        CC_OSC1_WAVEFORM => {
            p.osc1.waveform = (value.min(4)) as u32;
            return Some(true);
        }
        CC_OSC2_WAVEFORM => {
            p.osc2.waveform = (value.min(4)) as u32;
            return Some(true);
        }
        CC_OSC3_WAVEFORM => {
            p.osc3.waveform = (value.min(4)) as u32;
            return Some(true);
        }
        CC_OSC1_VIBRATO => {
            p.osc1.enable_vibrato = (value >= 64) as u32;
            return Some(true);
        }
        CC_OSC2_VIBRATO => {
            p.osc2.enable_vibrato = (value >= 64) as u32;
            return Some(true);
        }
        CC_OSC3_VIBRATO => {
            p.osc3.enable_vibrato = (value >= 64) as u32;
            return Some(true);
        }
        CC_DELAY_ENABLED => {
            p.delay.enabled = (value >= 64) as u32;
            return Some(true);
        }
        CC_REVERB_ENABLED => {
            p.reverb.enabled = (value >= 64) as u32;
            return Some(true);
        }
        CC_LFO_ENABLED => {
            p.lfo_enabled = (value >= 64) as u32;
            return Some(true);
        }
        CC_LFO_WAVEFORM => {
            // LfoWaveform has 4 variants (Sine/Triangle/Saw/Square).
            p.lfo.waveform = (value.min(3)) as u32;
            return Some(true);
        }

        _ => return None,
    }
    Some(false)
}

fn osc_mut(p: &mut Preset, idx: usize) -> &mut crate::data::presets::OscSettings {
    match idx {
        0 => &mut p.osc1,
        1 => &mut p.osc2,
        _ => &mut p.osc3,
    }
}

fn set_level(p: &mut Preset, ctrl: &MidiControl, idx: usize, n: f32) {
    osc_mut(p, idx).level = n;
    ctrl.set_cont(slot::osc_level(idx), n);
}

fn set_octave(p: &mut Preset, ctrl: &MidiControl, idx: usize, n: f32) {
    let oct = lin(n, -2.0, 2.0);
    osc_mut(p, idx).octave = oct;
    // The live slot holds the frequency multiplier 2^octave (a plain Gain on the pitch).
    ctrl.set_cont(slot::osc_octave(idx), libm::powf(2.0, oct));
}

fn set_detune(p: &mut Preset, ctrl: &MidiControl, idx: usize, n: f32) {
    let det = lin(n, -100.0, 100.0);
    osc_mut(p, idx).detune = det;
    ctrl.set_cont(slot::osc_detune(idx), det);
}
