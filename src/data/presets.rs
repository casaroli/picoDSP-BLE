use infinitedsp_core::synthesis::lfo::LfoWaveform;
use infinitedsp_core::synthesis::oscillator::Waveform;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Preset {
    pub name: [u8; 32],
    pub osc1: OscSettings,
    pub osc2: OscSettings,
    pub osc3: OscSettings,
    pub noise_level: f32,
    pub portamento: f32,
    pub filter: FilterSettings,
    pub amp: EnvelopeSettings,
    pub lfo_enabled: u32,
    pub lfo: LfoSettings,
    pub delay: DelaySettings,
    pub reverb: ReverbSettings,
    pub _padding: [u8; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct OscSettings {
    pub waveform: u32,
    pub level: f32,
    pub octave: f32,
    pub detune: f32,
    pub enable_vibrato: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FilterSettings {
    pub cutoff: f32,
    pub resonance: f32,
    pub env_amount: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct EnvelopeSettings {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct LfoSettings {
    pub frequency: f32,
    pub waveform: u32,
    pub vibrato_amount: f32,
    pub filter_amount: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DelaySettings {
    pub time: f32,
    pub feedback: f32,
    pub mix: f32,
    pub enabled: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ReverbSettings {
    pub size: f32,
    pub damping: f32,
    pub mix: f32,
    pub enabled: u32,
}

impl Preset {
    pub fn get_name(&self) -> &str {
        let len = self
            .name
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(self.name.len());
        core::str::from_utf8(&self.name[..len]).unwrap_or("Invalid")
    }
}

impl OscSettings {
    pub fn get_waveform(&self) -> Waveform {
        match self.waveform {
            0 => Waveform::Sine,
            1 => Waveform::Triangle,
            2 => Waveform::Saw,
            3 => Waveform::Square,
            4 => Waveform::WhiteNoise,
            _ => Waveform::Saw,
        }
    }

    pub fn is_vibrato_enabled(&self) -> bool {
        self.enable_vibrato != 0
    }
}

impl LfoSettings {
    pub fn get_waveform(&self) -> LfoWaveform {
        match self.waveform {
            0 => LfoWaveform::Sine,
            1 => LfoWaveform::Triangle,
            2 => LfoWaveform::Saw,
            3 => LfoWaveform::Square,
            _ => LfoWaveform::Sine,
        }
    }
}

pub fn make_name(s: &str) -> [u8; 32] {
    let mut name = [0u8; 32];
    let bytes = s.as_bytes();
    let len = bytes.len().min(32);
    name[0..len].copy_from_slice(&bytes[0..len]);
    name
}

fn osc(wf: Waveform, level: f32, octave: f32, detune: f32, vib: bool) -> OscSettings {
    OscSettings {
        waveform: match wf {
            Waveform::Sine => 0,
            Waveform::Triangle => 1,
            Waveform::Saw => 2,
            Waveform::Square => 3,
            Waveform::WhiteNoise => 4,
        },
        level,
        octave,
        detune,
        enable_vibrato: if vib { 1 } else { 0 },
    }
}

fn lfo(freq: f32, wf: LfoWaveform, vib: f32, filt: f32) -> LfoSettings {
    LfoSettings {
        frequency: freq,
        waveform: match wf {
            LfoWaveform::Sine => 0,
            LfoWaveform::Triangle => 1,
            LfoWaveform::Saw => 2,
            LfoWaveform::Square => 3,
            _ => 0,
        },
        vibrato_amount: vib,
        filter_amount: filt,
    }
}

fn delay_set(time: f32, feedback: f32, mix: f32, enabled: bool) -> DelaySettings {
    DelaySettings {
        time,
        feedback,
        mix,
        enabled: if enabled { 1 } else { 0 },
    }
}

fn reverb_set(size: f32, damping: f32, mix: f32, enabled: bool) -> ReverbSettings {
    ReverbSettings {
        size,
        damping,
        mix,
        enabled: if enabled { 1 } else { 0 },
    }
}

#[allow(clippy::too_many_arguments)]
fn filt(
    cutoff: f32,
    resonance: f32,
    env_amount: f32,
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
) -> FilterSettings {
    FilterSettings {
        cutoff,
        resonance,
        env_amount,
        attack,
        decay,
        sustain,
        release,
    }
}

fn env(attack: f32, decay: f32, sustain: f32, release: f32) -> EnvelopeSettings {
    EnvelopeSettings {
        attack,
        decay,
        sustain,
        release,
    }
}

impl Default for Preset {
    fn default() -> Self {
        Self {
            name: make_name("Init Patch"),
            osc1: osc(Waveform::Saw, 1.0, 0.0, 0.0, true),
            osc2: osc(Waveform::Saw, 0.0, 0.0, 0.0, true),
            osc3: osc(Waveform::Saw, 0.0, 0.0, 0.0, true),
            noise_level: 0.0,
            portamento: 0.0,
            filter: filt(20000.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0),
            amp: env(0.01, 0.1, 1.0, 0.1),
            lfo_enabled: 0,
            lfo: lfo(1.0, LfoWaveform::Sine, 0.0, 0.0),
            delay: delay_set(0.25, 0.3, 0.3, false),
            reverb: reverb_set(0.5, 0.5, 0.1, false),
            _padding: [0; 4],
        }
    }
}

/// The 128-preset factory showcase bank. Presets are grouped by category in this
/// order: Bass, Leads, Pads, Keys & Organs, Plucks, Brass & Strings, Bells & Mallets,
/// Arps & Sequences, Drones & Atmospheres, Sound FX & Risers, Percussion & Drums.
/// Several entries (Lucky Man, Tom Sawyer, Moog Scream, ...) are the original
/// hand-tuned classics, distributed into the matching category.
#[rustfmt::skip]
pub fn get_default_presets() -> [Preset; 128] {
    use LfoWaveform::{Saw as LSaw, Sine as LSine, Square as LSqr, Triangle as LTri};
    use Waveform::{Saw, Sine, Square, Triangle};

    // Small constructor to keep each preset to a few readable lines. Defaults cover
    // the common case (no glide, no vibrato, FX off); override via the fields.
    #[allow(clippy::too_many_arguments)]
    fn p(
        name: &str,
        osc1: OscSettings,
        osc2: OscSettings,
        osc3: OscSettings,
        noise_level: f32,
        portamento: f32,
        filter: FilterSettings,
        amp: EnvelopeSettings,
        lfo_enabled: bool,
        lfo: LfoSettings,
        delay: DelaySettings,
        reverb: ReverbSettings,
    ) -> Preset {
        Preset {
            name: make_name(name),
            osc1,
            osc2,
            osc3,
            noise_level,
            portamento,
            filter,
            amp,
            lfo_enabled: if lfo_enabled { 1 } else { 0 },
            lfo,
            delay,
            reverb,
            _padding: [0; 4],
        }
    }

    let no_lfo = lfo(1.0, LSine, 0.0, 0.0);
    let no_osc = osc(Saw, 0.0, 0.0, 0.0, false);

    [
        // ============================================================
        // BASS (18)
        // ============================================================
        p("Moog Bass",
          osc(Saw, 1.0, -3.0, 0.0, false), osc(Saw, 0.4, -3.0, 0.3, false), osc(Square, 0.5, -4.0, 0.0, false),
          0.0, 0.0, filt(80.0, 0.6, 3000.0, 0.001, 0.25, 0.0, 0.1), env(0.001, 0.2, 0.8, 0.1),
          false, no_lfo, delay_set(0.25, 0.3, 0.3, false), reverb_set(0.5, 0.5, 0.1, false)),
        p("Deep Sub",
          osc(Sine, 1.0, -2.0, 0.0, false), osc(Sine, 0.3, -1.0, 0.0, false), no_osc,
          0.0, 0.0, filt(800.0, 0.1, 0.0, 0.001, 0.1, 1.0, 0.1), env(0.002, 0.15, 0.9, 0.12),
          false, no_lfo, delay_set(0.25, 0.3, 0.3, false), reverb_set(0.5, 0.5, 0.1, false)),
        p("Reese Bass",
          osc(Saw, 1.0, -2.0, 1.5, false), osc(Saw, 1.0, -2.0, -1.5, false), osc(Saw, 0.6, -1.0, 0.4, false),
          0.0, 0.0, filt(400.0, 1.5, 1500.0, 0.005, 0.4, 0.7, 0.2), env(0.005, 0.3, 0.9, 0.2),
          true, lfo(0.4, LTri, 0.0, 300.0), delay_set(0.25, 0.3, 0.2, false), reverb_set(0.4, 0.5, 0.1, false)),
        p("Acid Bass",
          osc(Saw, 1.0, -1.0, 0.0, false), no_osc, no_osc,
          0.0, 0.55, filt(120.0, 6.0, 4500.0, 0.003, 0.2, 0.1, 0.1), env(0.003, 0.2, 0.9, 0.1),
          false, no_lfo, delay_set(0.18, 0.35, 0.22, true), reverb_set(0.4, 0.5, 0.08, false)),
        p("Rubber Bass",
          osc(Square, 1.0, -2.0, 0.0, false), osc(Saw, 0.5, -1.0, 0.2, false), no_osc,
          0.0, 0.0, filt(180.0, 3.0, 2500.0, 0.002, 0.18, 0.2, 0.12), env(0.002, 0.2, 0.7, 0.1),
          false, no_lfo, delay_set(0.25, 0.3, 0.2, false), reverb_set(0.4, 0.5, 0.08, false)),
        p("Wobble Bass",
          osc(Saw, 1.0, -1.0, 0.0, false), osc(Square, 0.7, -1.0, 0.3, false), osc(Saw, 0.5, -2.0, 0.0, false),
          0.0, 0.0, filt(300.0, 4.0, 1000.0, 0.005, 0.3, 0.7, 0.2), env(0.005, 0.2, 0.9, 0.2),
          true, lfo(3.0, LSine, 0.0, 1500.0), delay_set(0.25, 0.3, 0.3, false), reverb_set(0.5, 0.5, 0.1, false)),
        p("Square Sub",
          osc(Square, 1.0, -2.0, 0.0, false), osc(Sine, 0.5, -3.0, 0.0, false), no_osc,
          0.0, 0.0, filt(500.0, 0.5, 800.0, 0.002, 0.2, 0.8, 0.1), env(0.003, 0.15, 0.85, 0.1),
          false, no_lfo, delay_set(0.25, 0.3, 0.2, false), reverb_set(0.4, 0.5, 0.06, false)),
        p("Growl Bass",
          osc(Saw, 1.0, -2.0, 2.0, false), osc(Saw, 0.8, -2.0, -2.0, false), osc(Square, 0.6, -1.0, 0.0, false),
          0.1, 0.0, filt(160.0, 5.0, 3000.0, 0.004, 0.3, 0.4, 0.15), env(0.004, 0.25, 0.8, 0.15),
          true, lfo(6.0, LSine, 0.0, 600.0), delay_set(0.2, 0.3, 0.2, true), reverb_set(0.4, 0.5, 0.1, false)),
        p("Pluck Bass",
          osc(Saw, 1.0, -2.0, 0.0, false), osc(Square, 0.5, -1.0, 0.3, false), no_osc,
          0.0, 0.0, filt(250.0, 3.5, 5000.0, 0.001, 0.15, 0.0, 0.1), env(0.001, 0.2, 0.0, 0.12),
          false, no_lfo, delay_set(0.22, 0.3, 0.2, true), reverb_set(0.4, 0.5, 0.08, false)),
        p("FM Bass",
          osc(Sine, 1.0, -2.0, 0.0, false), osc(Square, 0.4, 0.0, 0.5, false), osc(Sine, 0.3, -1.0, 0.0, false),
          0.0, 0.0, filt(600.0, 2.0, 1200.0, 0.001, 0.2, 0.5, 0.12), env(0.001, 0.18, 0.7, 0.1),
          false, no_lfo, delay_set(0.25, 0.3, 0.15, false), reverb_set(0.4, 0.5, 0.07, false)),
        p("Detuned Bass",
          osc(Saw, 1.0, -2.0, 3.0, false), osc(Saw, 0.9, -2.0, -3.0, false), osc(Sine, 0.5, -3.0, 0.0, false),
          0.0, 0.0, filt(350.0, 2.0, 1500.0, 0.004, 0.25, 0.6, 0.15), env(0.004, 0.2, 0.85, 0.15),
          false, no_lfo, delay_set(0.25, 0.3, 0.2, false), reverb_set(0.4, 0.5, 0.1, false)),
        p("808 Boom",
          osc(Sine, 1.0, -3.0, 0.0, false), osc(Sine, 0.2, -2.0, 0.0, false), no_osc,
          0.0, 0.5, filt(1200.0, 0.3, 0.0, 0.001, 0.05, 1.0, 0.1), env(0.001, 1.2, 0.0, 0.6),
          false, no_lfo, delay_set(0.25, 0.3, 0.1, false), reverb_set(0.3, 0.5, 0.05, false)),
        p("Hoover Bass",
          osc(Saw, 1.0, -1.0, 4.0, false), osc(Saw, 1.0, -1.0, -4.0, false), osc(Square, 0.7, -2.0, 0.0, true),
          0.0, 0.3, filt(450.0, 3.0, 2000.0, 0.005, 0.3, 0.6, 0.2), env(0.005, 0.25, 0.85, 0.2),
          true, lfo(0.3, LSine, 6.0, 500.0), delay_set(0.25, 0.35, 0.25, true), reverb_set(0.5, 0.5, 0.12, true)),
        p("Dub Bass",
          osc(Sine, 1.0, -2.0, 0.0, false), osc(Triangle, 0.4, -1.0, 0.0, false), no_osc,
          0.0, 0.2, filt(220.0, 1.5, 600.0, 0.01, 0.3, 0.6, 0.2), env(0.01, 0.25, 0.7, 0.25),
          false, no_lfo, delay_set(0.3, 0.45, 0.35, true), reverb_set(0.6, 0.5, 0.18, true)),
        p("Talking Bass",
          osc(Saw, 1.0, -2.0, 0.0, false), osc(Square, 0.6, -1.0, 0.3, false), no_osc,
          0.0, 0.0, filt(300.0, 5.0, 2500.0, 0.005, 0.25, 0.5, 0.15), env(0.005, 0.2, 0.8, 0.15),
          true, lfo(2.5, LTri, 0.0, 1800.0), delay_set(0.25, 0.3, 0.2, false), reverb_set(0.4, 0.5, 0.1, false)),
        p("Synth Bass 1",
          osc(Saw, 1.0, -2.0, 0.0, false), osc(Saw, 0.5, -2.0, 0.5, false), osc(Square, 0.4, -1.0, 0.0, false),
          0.0, 0.0, filt(280.0, 2.5, 2200.0, 0.002, 0.22, 0.4, 0.12), env(0.002, 0.2, 0.75, 0.12),
          false, no_lfo, delay_set(0.25, 0.3, 0.15, false), reverb_set(0.4, 0.5, 0.08, false)),
        p("Saw Bass",
          osc(Saw, 1.0, -2.0, 0.0, false), osc(Saw, 0.6, -3.0, 0.0, false), no_osc,
          0.0, 0.0, filt(220.0, 2.0, 1800.0, 0.002, 0.2, 0.5, 0.12), env(0.002, 0.18, 0.8, 0.12),
          false, no_lfo, delay_set(0.25, 0.3, 0.12, false), reverb_set(0.4, 0.5, 0.07, false)),
        p("Mini Bass",
          osc(Triangle, 1.0, -2.0, 0.0, false), osc(Square, 0.6, -2.0, 0.4, false), osc(Saw, 0.4, -1.0, 0.0, false),
          0.0, 0.0, filt(260.0, 3.0, 2600.0, 0.001, 0.2, 0.3, 0.1), env(0.001, 0.2, 0.7, 0.1),
          false, no_lfo, delay_set(0.25, 0.3, 0.12, false), reverb_set(0.4, 0.5, 0.08, false)),

        // ============================================================
        // LEADS (18)  — incl. classics: Lucky Man, Tom Sawyer, Moog Scream, Octavarium
        // ============================================================
        p("Lucky Man",
          osc(Square, 1.0, 0.0, 0.0, true), osc(Square, 0.7, 0.0, 2.0, true), osc(Square, 0.7, 0.0, -2.0, true),
          0.0, 0.92, filt(200.0, 0.4, 3000.0, 0.1, 1.5, 0.4, 0.5), env(0.05, 0.2, 1.0, 0.5),
          true, lfo(5.0, LSine, 2.0, 0.0), delay_set(0.3, 0.3, 0.3, true), reverb_set(0.5, 0.5, 0.1, false)),
        p("Tom Sawyer",
          osc(Saw, 1.0, 0.0, 0.0, false), osc(Saw, 0.5, 0.0, 1.5, false), no_osc,
          0.0, 0.0, filt(80.0, 0.45, 5000.0, 0.03, 2.0, 0.1, 0.1), env(0.01, 0.1, 1.0, 0.2),
          false, no_lfo, delay_set(0.15, 0.2, 0.2, true), reverb_set(0.3, 0.5, 0.1, false)),
        p("Moog Scream",
          osc(Saw, 1.0, 0.0, 0.0, true), osc(Saw, 0.6, 0.0, 2.5, true), osc(Square, 0.8, 0.0, -2.5, true),
          0.15, 0.85, filt(100.0, 7.0, 6000.0, 0.005, 0.3, 0.2, 0.2), env(0.005, 0.2, 1.0, 0.2),
          true, lfo(0.15, LSine, 8.0, 0.0), delay_set(0.25, 0.3, 0.3, false), reverb_set(0.5, 0.5, 0.2, true)),
        p("Octavarium Lead",
          osc(Saw, 1.0, 0.0, 0.0, true), osc(Saw, 0.5, 0.0, 2.0, false), osc(Square, 0.3, 0.0, 0.0, false),
          0.0, 0.94, filt(500.0, 0.6, 4000.0, 0.01, 0.5, 0.6, 0.2), env(0.005, 0.1, 1.0, 0.2),
          true, lfo(5.5, LSine, 1.5, 0.0), delay_set(0.25, 0.3, 0.3, true), reverb_set(0.5, 0.5, 0.1, true)),
        p("Super Saw Lead",
          osc(Saw, 1.0, 0.0, 0.0, true), osc(Saw, 0.7, 0.0, 4.0, true), osc(Saw, 0.7, 0.0, -4.0, true),
          0.0, 0.0, filt(3000.0, 0.3, 3000.0, 0.01, 0.5, 0.7, 0.3), env(0.01, 0.2, 1.0, 0.3),
          true, lfo(5.5, LSine, 1.5, 0.0), delay_set(0.3, 0.3, 0.3, true), reverb_set(0.4, 0.5, 0.1, true)),
        p("Square Lead",
          osc(Square, 1.0, 0.0, 0.0, true), osc(Square, 0.4, 1.0, 0.0, false), no_osc,
          0.0, 0.0, filt(2500.0, 0.5, 2000.0, 0.005, 0.3, 0.8, 0.2), env(0.005, 0.15, 1.0, 0.2),
          true, lfo(5.0, LSine, 1.2, 0.0), delay_set(0.3, 0.3, 0.25, true), reverb_set(0.4, 0.5, 0.1, true)),
        p("Mini Lead",
          osc(Saw, 1.0, 0.0, 0.0, true), osc(Saw, 0.6, 0.0, 0.5, false), osc(Triangle, 0.4, 1.0, 0.0, false),
          0.0, 0.4, filt(1800.0, 2.0, 2500.0, 0.005, 0.4, 0.7, 0.2), env(0.005, 0.2, 1.0, 0.2),
          true, lfo(5.5, LSine, 1.5, 0.0), delay_set(0.3, 0.3, 0.2, true), reverb_set(0.4, 0.5, 0.1, true)),
        p("Sync Lead",
          osc(Saw, 1.0, 0.0, 0.0, true), osc(Square, 0.7, 1.0, 7.0, false), osc(Saw, 0.4, 0.0, -3.0, false),
          0.0, 0.0, filt(1200.0, 4.0, 4000.0, 0.005, 0.5, 0.6, 0.2), env(0.005, 0.2, 1.0, 0.2),
          true, lfo(5.0, LSine, 1.0, 1500.0), delay_set(0.28, 0.3, 0.25, true), reverb_set(0.4, 0.5, 0.1, true)),
        p("Whistle Lead",
          osc(Sine, 1.0, 1.0, 0.0, true), osc(Sine, 0.3, 2.0, 0.0, false), no_osc,
          0.0, 0.3, filt(4000.0, 1.0, 1000.0, 0.02, 0.3, 0.9, 0.2), env(0.02, 0.2, 1.0, 0.25),
          true, lfo(6.0, LSine, 2.5, 0.0), delay_set(0.3, 0.35, 0.3, true), reverb_set(0.6, 0.4, 0.2, true)),
        p("Portamento Lead",
          osc(Saw, 1.0, 0.0, 0.0, true), osc(Square, 0.6, 0.0, 0.0, false), no_osc,
          0.0, 0.95, filt(900.0, 3.0, 3500.0, 0.01, 0.5, 0.7, 0.25), env(0.01, 0.2, 1.0, 0.3),
          true, lfo(5.0, LSine, 2.0, 0.0), delay_set(0.3, 0.35, 0.3, true), reverb_set(0.5, 0.5, 0.15, true)),
        p("Bright Saw Lead",
          osc(Saw, 1.0, 0.0, 0.0, true), osc(Saw, 0.5, 1.0, 2.0, true), no_osc,
          0.0, 0.0, filt(5000.0, 1.0, 2000.0, 0.005, 0.3, 0.8, 0.2), env(0.005, 0.15, 1.0, 0.2),
          true, lfo(5.5, LSine, 1.2, 0.0), delay_set(0.3, 0.3, 0.25, true), reverb_set(0.4, 0.5, 0.1, true)),
        p("Soft Lead",
          osc(Triangle, 1.0, 0.0, 0.0, true), osc(Sine, 0.5, 1.0, 0.0, false), no_osc,
          0.0, 0.2, filt(2500.0, 0.5, 1500.0, 0.03, 0.4, 0.9, 0.3), env(0.03, 0.2, 1.0, 0.35),
          true, lfo(5.0, LSine, 1.0, 0.0), delay_set(0.3, 0.3, 0.25, true), reverb_set(0.5, 0.4, 0.2, true)),
        p("Fifth Lead",
          osc(Saw, 1.0, 0.0, 0.0, true), osc(Saw, 0.7, 0.585, 0.0, false), osc(Square, 0.4, 1.0, 0.0, false),
          0.0, 0.0, filt(2200.0, 1.5, 2500.0, 0.008, 0.4, 0.7, 0.2), env(0.008, 0.2, 1.0, 0.25),
          true, lfo(5.2, LSine, 1.5, 0.0), delay_set(0.3, 0.3, 0.25, true), reverb_set(0.45, 0.5, 0.12, true)),
        p("Trance Lead",
          osc(Saw, 1.0, 0.0, 5.0, true), osc(Saw, 1.0, 0.0, -5.0, true), osc(Saw, 0.7, 1.0, 0.0, true),
          0.0, 0.0, filt(2800.0, 2.0, 3500.0, 0.005, 0.4, 0.8, 0.25), env(0.005, 0.2, 1.0, 0.3),
          true, lfo(6.0, LSine, 1.5, 800.0), delay_set(0.3, 0.4, 0.35, true), reverb_set(0.6, 0.4, 0.2, true)),
        p("Screaming Lead",
          osc(Saw, 1.0, 0.0, 0.0, true), osc(Saw, 0.8, 0.0, 3.0, true), osc(Square, 0.7, 1.0, -3.0, true),
          0.05, 0.5, filt(700.0, 8.0, 6000.0, 0.004, 0.4, 0.5, 0.2), env(0.004, 0.2, 1.0, 0.25),
          true, lfo(5.5, LSine, 3.0, 0.0), delay_set(0.3, 0.35, 0.3, true), reverb_set(0.5, 0.5, 0.18, true)),
        p("Pulse Lead",
          osc(Square, 1.0, 0.0, 0.3, true), osc(Square, 0.7, 0.0, -0.3, true), osc(Square, 0.5, 1.0, 0.0, false),
          0.0, 0.0, filt(2000.0, 1.5, 2500.0, 0.006, 0.35, 0.8, 0.2), env(0.006, 0.18, 1.0, 0.22),
          true, lfo(0.8, LTri, 0.0, 600.0), delay_set(0.3, 0.3, 0.25, true), reverb_set(0.4, 0.5, 0.12, true)),
        p("Vintage Lead",
          osc(Saw, 1.0, 0.0, 0.0, true), osc(Triangle, 0.6, 0.0, 1.0, false), osc(Square, 0.3, -1.0, 0.0, false),
          0.0, 0.6, filt(1500.0, 2.5, 3000.0, 0.01, 0.5, 0.6, 0.25), env(0.01, 0.2, 1.0, 0.3),
          true, lfo(4.8, LSine, 2.0, 0.0), delay_set(0.3, 0.32, 0.28, true), reverb_set(0.5, 0.5, 0.15, true)),
        p("Octave Lead",
          osc(Saw, 1.0, 0.0, 0.0, true), osc(Saw, 0.7, 1.0, 0.0, false), osc(Saw, 0.4, -1.0, 0.0, false),
          0.0, 0.0, filt(2600.0, 1.0, 2500.0, 0.006, 0.35, 0.8, 0.2), env(0.006, 0.18, 1.0, 0.22),
          true, lfo(5.5, LSine, 1.3, 0.0), delay_set(0.3, 0.3, 0.25, true), reverb_set(0.4, 0.5, 0.12, true)),

        // ============================================================
        // PADS (18)  — incl. classic Glass Pad
        // ============================================================
        p("Glass Pad",
          osc(Triangle, 0.9, 0.0, 0.0, false), osc(Sine, 0.7, 1.0, 0.2, false), osc(Triangle, 0.5, 0.0, -0.2, false),
          0.0, 0.0, filt(1200.0, 0.2, 1500.0, 0.8, 1.0, 0.8, 1.5), env(0.9, 0.5, 1.0, 1.8),
          true, lfo(0.3, LSine, 0.0, 400.0), delay_set(0.25, 0.3, 0.3, false), reverb_set(0.7, 0.4, 0.3, true)),
        p("Warm Pad",
          osc(Saw, 0.8, 0.0, 0.3, false), osc(Saw, 0.8, 0.0, -0.3, false), osc(Triangle, 0.5, -1.0, 0.0, false),
          0.0, 0.0, filt(900.0, 1.0, 1200.0, 1.0, 1.2, 0.85, 1.8), env(1.0, 0.6, 1.0, 2.0),
          true, lfo(0.25, LSine, 0.0, 300.0), delay_set(0.3, 0.3, 0.2, false), reverb_set(0.8, 0.4, 0.3, true)),
        p("String Pad",
          osc(Saw, 0.9, 0.0, 0.5, false), osc(Saw, 0.9, 0.0, -0.5, false), osc(Saw, 0.6, 1.0, 0.2, false),
          0.0, 0.0, filt(1600.0, 0.5, 1500.0, 0.6, 1.0, 0.85, 1.2), env(0.7, 0.5, 1.0, 1.4),
          true, lfo(0.4, LSine, 0.3, 400.0), delay_set(0.3, 0.3, 0.2, false), reverb_set(0.75, 0.45, 0.3, true)),
        p("Choir Pad",
          osc(Sine, 1.0, 0.0, 0.0, false), osc(Triangle, 0.6, 0.0, 0.4, false), osc(Sine, 0.5, 1.0, -0.4, false),
          0.05, 0.0, filt(2000.0, 1.5, 1000.0, 0.9, 1.0, 0.9, 1.6), env(0.9, 0.6, 1.0, 1.8),
          true, lfo(5.0, LSine, 1.5, 200.0), delay_set(0.3, 0.3, 0.2, false), reverb_set(0.85, 0.4, 0.35, true)),
        p("Sweep Pad",
          osc(Saw, 0.9, 0.0, 0.4, false), osc(Saw, 0.7, 0.0, -0.4, false), osc(Square, 0.5, -1.0, 0.0, false),
          0.0, 0.0, filt(400.0, 3.0, 4000.0, 2.0, 2.0, 0.8, 2.0), env(0.8, 1.0, 1.0, 2.0),
          true, lfo(0.15, LSine, 0.0, 1500.0), delay_set(0.3, 0.35, 0.3, true), reverb_set(0.8, 0.4, 0.3, true)),
        p("Dark Pad",
          osc(Saw, 0.9, -1.0, 0.3, false), osc(Triangle, 0.7, -1.0, -0.3, false), osc(Sine, 0.5, -2.0, 0.0, false),
          0.0, 0.0, filt(500.0, 1.5, 800.0, 1.2, 1.5, 0.7, 2.0), env(1.2, 0.8, 1.0, 2.5),
          true, lfo(0.12, LSine, 0.0, 250.0), delay_set(0.3, 0.4, 0.3, true), reverb_set(0.9, 0.35, 0.35, true)),
        p("Shimmer Pad",
          osc(Sine, 1.0, 0.0, 0.0, false), osc(Sine, 0.6, 1.0, 0.3, false), osc(Triangle, 0.4, 2.0, 0.0, false),
          0.0, 0.0, filt(3000.0, 0.5, 1500.0, 0.7, 1.0, 0.85, 1.6), env(0.8, 0.6, 1.0, 2.0),
          true, lfo(0.2, LSine, 0.0, 800.0), delay_set(0.3, 0.45, 0.35, true), reverb_set(0.9, 0.3, 0.4, true)),
        p("Analog Pad",
          osc(Saw, 0.9, 0.0, 0.6, false), osc(Square, 0.7, 0.0, -0.6, false), osc(Saw, 0.5, -1.0, 0.0, false),
          0.0, 0.0, filt(1100.0, 1.2, 1500.0, 0.7, 1.0, 0.8, 1.4), env(0.7, 0.6, 1.0, 1.6),
          true, lfo(0.5, LSine, 0.4, 350.0), delay_set(0.3, 0.3, 0.2, false), reverb_set(0.7, 0.45, 0.3, true)),
        p("Soft Pad",
          osc(Triangle, 1.0, 0.0, 0.2, false), osc(Sine, 0.7, 0.0, -0.2, false), osc(Triangle, 0.4, 1.0, 0.0, false),
          0.0, 0.0, filt(1500.0, 0.3, 800.0, 1.0, 1.0, 0.9, 1.8), env(1.0, 0.6, 1.0, 2.0),
          true, lfo(0.3, LSine, 0.0, 250.0), delay_set(0.3, 0.3, 0.2, false), reverb_set(0.8, 0.4, 0.3, true)),
        p("Evolving Pad",
          osc(Saw, 0.8, 0.0, 0.7, false), osc(Saw, 0.8, 0.0, -0.7, false), osc(Triangle, 0.6, 1.0, 0.3, false),
          0.0, 0.0, filt(700.0, 2.0, 3000.0, 1.5, 2.0, 0.75, 2.2), env(1.0, 1.0, 1.0, 2.5),
          true, lfo(0.08, LTri, 0.0, 2000.0), delay_set(0.3, 0.45, 0.3, true), reverb_set(0.85, 0.4, 0.35, true)),
        p("Octave Pad",
          osc(Saw, 0.9, 0.0, 0.3, false), osc(Saw, 0.6, 1.0, -0.3, false), osc(Triangle, 0.5, -1.0, 0.0, false),
          0.0, 0.0, filt(1400.0, 0.7, 1500.0, 0.8, 1.0, 0.85, 1.5), env(0.8, 0.6, 1.0, 1.7),
          true, lfo(0.4, LSine, 0.0, 400.0), delay_set(0.3, 0.3, 0.2, false), reverb_set(0.75, 0.45, 0.3, true)),
        p("Detuned Pad",
          osc(Saw, 0.9, 0.0, 1.0, false), osc(Saw, 0.9, 0.0, -1.0, false), osc(Saw, 0.7, 0.0, 2.0, false),
          0.0, 0.0, filt(1000.0, 1.0, 1500.0, 0.9, 1.0, 0.85, 1.6), env(0.9, 0.6, 1.0, 1.8),
          true, lfo(0.3, LSine, 0.5, 350.0), delay_set(0.3, 0.35, 0.25, true), reverb_set(0.8, 0.4, 0.3, true)),
        p("Ambient Pad",
          osc(Sine, 1.0, 0.0, 0.2, false), osc(Triangle, 0.6, 1.0, -0.2, false), osc(Sine, 0.4, 2.0, 0.0, false),
          0.0, 0.0, filt(2200.0, 0.5, 1200.0, 1.5, 1.5, 0.85, 2.5), env(1.5, 1.0, 1.0, 3.0),
          true, lfo(0.1, LSine, 0.0, 500.0), delay_set(0.3, 0.5, 0.4, true), reverb_set(0.95, 0.3, 0.45, true)),
        p("Voltage Pad",
          osc(Saw, 0.9, 0.0, 0.5, true), osc(Square, 0.7, 0.0, -0.5, true), osc(Saw, 0.5, -1.0, 0.0, true),
          0.0, 0.0, filt(800.0, 2.5, 2500.0, 1.0, 1.5, 0.8, 2.0), env(0.9, 0.8, 1.0, 2.2),
          true, lfo(0.2, LSine, 0.8, 900.0), delay_set(0.3, 0.4, 0.3, true), reverb_set(0.85, 0.4, 0.35, true)),
        p("Halo Pad",
          osc(Sine, 1.0, 1.0, 0.0, false), osc(Sine, 0.6, 2.0, 0.3, false), osc(Triangle, 0.4, 0.0, 0.0, false),
          0.0, 0.0, filt(3500.0, 0.4, 1000.0, 1.0, 1.2, 0.85, 2.0), env(1.0, 0.8, 1.0, 2.5),
          true, lfo(0.15, LSine, 0.0, 700.0), delay_set(0.3, 0.45, 0.35, true), reverb_set(0.95, 0.3, 0.45, true)),
        p("Wide Pad",
          osc(Saw, 0.9, 0.0, 2.0, false), osc(Saw, 0.9, 0.0, -2.0, false), osc(Triangle, 0.6, 0.0, 4.0, false),
          0.0, 0.0, filt(1300.0, 0.8, 1500.0, 0.8, 1.0, 0.85, 1.6), env(0.9, 0.6, 1.0, 1.8),
          true, lfo(0.35, LSine, 0.5, 400.0), delay_set(0.3, 0.4, 0.3, true), reverb_set(0.85, 0.4, 0.35, true)),
        p("Aurora Pad",
          osc(Triangle, 0.9, 0.0, 0.3, false), osc(Sine, 0.7, 1.0, -0.3, false), osc(Saw, 0.4, 0.585, 0.0, false),
          0.0, 0.0, filt(1800.0, 1.5, 2500.0, 1.3, 1.5, 0.8, 2.2), env(1.3, 0.9, 1.0, 2.5),
          true, lfo(0.12, LTri, 0.0, 1200.0), delay_set(0.3, 0.45, 0.35, true), reverb_set(0.9, 0.35, 0.4, true)),
        p("Mystic Pad",
          osc(Sine, 0.9, 0.0, 0.4, false), osc(Triangle, 0.7, 0.585, -0.4, false), osc(Sine, 0.5, 1.585, 0.0, false),
          0.0, 0.0, filt(1600.0, 1.0, 2000.0, 1.2, 1.4, 0.8, 2.0), env(1.2, 0.8, 1.0, 2.4),
          true, lfo(0.18, LSine, 0.4, 800.0), delay_set(0.3, 0.45, 0.32, true), reverb_set(0.9, 0.35, 0.4, true)),

        // ============================================================
        // KEYS & ORGANS (14)  — incl. classic Drawbar Organ
        // ============================================================
        p("Drawbar Organ",
          osc(Sine, 1.0, 0.0, 0.0, false), osc(Sine, 0.7, 1.0, 0.0, false), osc(Sine, 0.5, 1.585, 0.0, false),
          0.0, 0.0, filt(8000.0, 0.0, 0.0, 0.005, 0.0, 1.0, 0.05), env(0.005, 0.0, 1.0, 0.08),
          true, lfo(6.0, LSine, 0.5, 0.0), delay_set(0.25, 0.3, 0.3, false), reverb_set(0.4, 0.5, 0.15, true)),
        p("Rock Organ",
          osc(Sine, 1.0, 0.0, 0.0, false), osc(Sine, 0.8, 1.0, 0.0, false), osc(Square, 0.5, 2.0, 0.0, false),
          0.0, 0.0, filt(6000.0, 1.0, 0.0, 0.005, 0.0, 1.0, 0.05), env(0.005, 0.0, 1.0, 0.06),
          true, lfo(6.5, LSine, 0.8, 300.0), delay_set(0.25, 0.3, 0.2, false), reverb_set(0.4, 0.5, 0.15, true)),
        p("Full Organ",
          osc(Sine, 1.0, -1.0, 0.0, false), osc(Sine, 0.8, 0.0, 0.0, false), osc(Sine, 0.6, 1.0, 0.0, false),
          0.0, 0.0, filt(9000.0, 0.0, 0.0, 0.003, 0.0, 1.0, 0.04), env(0.003, 0.0, 1.0, 0.05),
          true, lfo(7.0, LSine, 0.6, 0.0), delay_set(0.25, 0.3, 0.15, false), reverb_set(0.45, 0.5, 0.18, true)),
        p("Electric Piano",
          osc(Sine, 1.0, 0.0, 0.0, false), osc(Sine, 0.5, 2.0, 0.0, false), osc(Triangle, 0.2, 0.0, 0.0, false),
          0.0, 0.0, filt(4000.0, 0.3, 1500.0, 0.001, 0.6, 0.3, 0.4), env(0.001, 0.8, 0.3, 0.4),
          false, no_lfo, delay_set(0.25, 0.25, 0.12, false), reverb_set(0.5, 0.45, 0.15, true)),
        p("Soft Clav",
          osc(Square, 1.0, 0.0, 0.0, false), osc(Square, 0.4, 1.0, 0.2, false), no_osc,
          0.0, 0.0, filt(1200.0, 2.5, 3000.0, 0.001, 0.25, 0.1, 0.15), env(0.001, 0.3, 0.2, 0.15),
          false, no_lfo, delay_set(0.22, 0.3, 0.15, true), reverb_set(0.4, 0.5, 0.1, true)),
        p("Pipe Organ",
          osc(Sine, 1.0, 0.0, 0.0, false), osc(Sine, 0.6, 1.0, 0.0, false), osc(Sine, 0.4, 2.0, 0.0, false),
          0.0, 0.0, filt(7000.0, 0.0, 0.0, 0.05, 0.0, 1.0, 0.15), env(0.05, 0.0, 1.0, 0.2),
          false, no_lfo, delay_set(0.25, 0.3, 0.15, false), reverb_set(0.9, 0.4, 0.4, true)),
        p("Church Organ",
          osc(Sine, 1.0, -1.0, 0.0, false), osc(Sine, 0.7, 0.0, 0.0, false), osc(Sine, 0.5, 1.585, 0.0, false),
          0.0, 0.0, filt(6500.0, 0.0, 0.0, 0.08, 0.0, 1.0, 0.2), env(0.08, 0.0, 1.0, 0.25),
          false, no_lfo, delay_set(0.3, 0.3, 0.15, false), reverb_set(0.95, 0.35, 0.45, true)),
        p("Jazz Organ",
          osc(Sine, 1.0, 0.0, 0.0, false), osc(Sine, 0.6, 1.0, 0.0, false), osc(Sine, 0.7, 0.585, 0.0, false),
          0.0, 0.0, filt(7500.0, 0.5, 0.0, 0.004, 0.0, 1.0, 0.05), env(0.004, 0.0, 1.0, 0.06),
          true, lfo(6.5, LSine, 0.7, 150.0), delay_set(0.25, 0.3, 0.15, false), reverb_set(0.5, 0.5, 0.18, true)),
        p("Percussive Organ",
          osc(Sine, 1.0, 0.0, 0.0, false), osc(Sine, 0.5, 2.0, 0.0, false), osc(Sine, 0.7, 1.0, 0.0, false),
          0.0, 0.0, filt(8000.0, 0.5, 2000.0, 0.001, 0.15, 1.0, 0.05), env(0.001, 0.2, 0.8, 0.06),
          true, lfo(6.0, LSine, 0.5, 0.0), delay_set(0.25, 0.3, 0.12, false), reverb_set(0.4, 0.5, 0.12, true)),
        p("Combo Organ",
          osc(Square, 1.0, 0.0, 0.0, false), osc(Square, 0.6, 1.0, 0.0, false), osc(Saw, 0.3, 2.0, 0.0, false),
          0.0, 0.0, filt(5000.0, 0.5, 0.0, 0.005, 0.0, 1.0, 0.05), env(0.005, 0.0, 1.0, 0.06),
          true, lfo(6.5, LSine, 1.0, 200.0), delay_set(0.25, 0.3, 0.15, false), reverb_set(0.4, 0.5, 0.12, true)),
        p("Reed Organ",
          osc(Saw, 0.8, 0.0, 0.0, false), osc(Sine, 0.7, 1.0, 0.2, false), osc(Triangle, 0.4, 0.0, -0.2, false),
          0.0, 0.0, filt(4500.0, 0.5, 500.0, 0.04, 0.0, 1.0, 0.12), env(0.04, 0.0, 1.0, 0.15),
          true, lfo(5.0, LSine, 0.6, 0.0), delay_set(0.25, 0.3, 0.15, false), reverb_set(0.6, 0.5, 0.2, true)),
        p("Tonewheel",
          osc(Sine, 1.0, 0.0, 0.0, false), osc(Sine, 0.8, 1.0, 0.0, false), osc(Sine, 0.6, 2.585, 0.0, false),
          0.0, 0.0, filt(8500.0, 0.0, 0.0, 0.004, 0.0, 1.0, 0.05), env(0.004, 0.0, 1.0, 0.06),
          true, lfo(7.2, LSine, 0.7, 250.0), delay_set(0.25, 0.3, 0.12, false), reverb_set(0.45, 0.5, 0.15, true)),
        p("Bell Piano",
          osc(Sine, 1.0, 0.0, 0.0, false), osc(Sine, 0.5, 2.0, 1.0, false), osc(Triangle, 0.3, 1.0, 0.0, false),
          0.0, 0.0, filt(5000.0, 0.2, 1000.0, 0.001, 1.0, 0.2, 0.6), env(0.001, 1.0, 0.2, 0.8),
          false, no_lfo, delay_set(0.3, 0.3, 0.2, true), reverb_set(0.6, 0.4, 0.25, true)),
        p("Synth Clav",
          osc(Square, 1.0, 0.0, 0.0, false), osc(Saw, 0.5, 0.0, 0.3, false), osc(Square, 0.3, 1.0, 0.0, false),
          0.0, 0.0, filt(1500.0, 3.0, 4000.0, 0.001, 0.2, 0.0, 0.12), env(0.001, 0.25, 0.0, 0.12),
          false, no_lfo, delay_set(0.2, 0.3, 0.15, true), reverb_set(0.4, 0.5, 0.1, true)),

        // ============================================================
        // PLUCKS (10)  — incl. classic Snappy Pluck
        // ============================================================
        p("Snappy Pluck",
          osc(Saw, 1.0, 0.0, 0.0, false), osc(Square, 0.5, 0.0, 0.3, false), no_osc,
          0.0, 0.0, filt(300.0, 3.0, 5000.0, 0.001, 0.18, 0.0, 0.15), env(0.001, 0.25, 0.0, 0.2),
          false, no_lfo, delay_set(0.25, 0.35, 0.3, true), reverb_set(0.4, 0.5, 0.1, true)),
        p("Nylon Pluck",
          osc(Triangle, 1.0, 0.0, 0.0, false), osc(Sine, 0.5, 1.0, 0.0, false), no_osc,
          0.0, 0.0, filt(2500.0, 0.5, 2000.0, 0.001, 0.4, 0.0, 0.3), env(0.001, 0.5, 0.0, 0.3),
          false, no_lfo, delay_set(0.25, 0.25, 0.15, true), reverb_set(0.5, 0.45, 0.18, true)),
        p("Synth Pluck",
          osc(Saw, 1.0, 0.0, 0.0, false), osc(Saw, 0.5, 0.0, 3.0, false), osc(Square, 0.3, 1.0, 0.0, false),
          0.0, 0.0, filt(800.0, 2.0, 4000.0, 0.001, 0.25, 0.0, 0.2), env(0.001, 0.3, 0.0, 0.25),
          false, no_lfo, delay_set(0.3, 0.35, 0.3, true), reverb_set(0.5, 0.5, 0.15, true)),
        p("Glass Pluck",
          osc(Sine, 1.0, 0.0, 0.0, false), osc(Sine, 0.5, 2.0, 0.0, false), osc(Triangle, 0.3, 1.0, 0.0, false),
          0.0, 0.0, filt(5000.0, 0.5, 2000.0, 0.001, 0.3, 0.0, 0.3), env(0.001, 0.4, 0.0, 0.4),
          false, no_lfo, delay_set(0.3, 0.35, 0.3, true), reverb_set(0.65, 0.4, 0.25, true)),
        p("Square Pluck",
          osc(Square, 1.0, 0.0, 0.0, false), osc(Square, 0.4, 1.0, 0.2, false), no_osc,
          0.0, 0.0, filt(1000.0, 1.5, 3500.0, 0.001, 0.2, 0.0, 0.18), env(0.001, 0.25, 0.0, 0.2),
          false, no_lfo, delay_set(0.28, 0.35, 0.28, true), reverb_set(0.45, 0.5, 0.12, true)),
        p("Koto Pluck",
          osc(Triangle, 1.0, 0.0, 0.0, false), osc(Saw, 0.3, 0.0, 0.5, false), osc(Sine, 0.3, 1.0, 0.0, false),
          0.0, 0.0, filt(3000.0, 1.0, 3000.0, 0.001, 0.3, 0.0, 0.25), env(0.001, 0.4, 0.0, 0.3),
          false, no_lfo, delay_set(0.3, 0.3, 0.2, true), reverb_set(0.55, 0.45, 0.2, true)),
        p("Harp Pluck",
          osc(Sine, 1.0, 0.0, 0.0, false), osc(Triangle, 0.5, 1.0, 0.0, false), osc(Sine, 0.3, 2.0, 0.0, false),
          0.0, 0.0, filt(4000.0, 0.3, 2000.0, 0.001, 0.6, 0.0, 0.5), env(0.001, 0.7, 0.0, 0.6),
          false, no_lfo, delay_set(0.3, 0.3, 0.2, true), reverb_set(0.7, 0.4, 0.25, true)),
        p("Muted Pluck",
          osc(Triangle, 1.0, 0.0, 0.0, false), osc(Square, 0.3, 0.0, 0.2, false), no_osc,
          0.0, 0.0, filt(900.0, 1.5, 1500.0, 0.001, 0.12, 0.0, 0.1), env(0.001, 0.15, 0.0, 0.12),
          false, no_lfo, delay_set(0.25, 0.3, 0.15, true), reverb_set(0.4, 0.5, 0.08, true)),
        p("Bright Pluck",
          osc(Saw, 1.0, 0.0, 0.0, false), osc(Saw, 0.6, 1.0, 2.0, false), no_osc,
          0.0, 0.0, filt(6000.0, 1.0, 3000.0, 0.001, 0.2, 0.0, 0.2), env(0.001, 0.25, 0.0, 0.22),
          false, no_lfo, delay_set(0.3, 0.35, 0.3, true), reverb_set(0.5, 0.45, 0.15, true)),
        p("Bass Pluck",
          osc(Saw, 1.0, -1.0, 0.0, false), osc(Square, 0.5, -1.0, 0.3, false), no_osc,
          0.0, 0.0, filt(500.0, 2.5, 4000.0, 0.001, 0.2, 0.0, 0.15), env(0.001, 0.25, 0.0, 0.18),
          false, no_lfo, delay_set(0.25, 0.3, 0.2, true), reverb_set(0.4, 0.5, 0.1, true)),

        // ============================================================
        // BRASS & STRINGS (12)  — incl. classics Fat Brass, PWM Strings
        // ============================================================
        p("Fat Brass",
          osc(Saw, 1.0, 0.0, 0.0, true), osc(Saw, 0.8, 0.0, 3.0, true), osc(Saw, 0.8, 0.0, -3.0, true),
          0.0, 0.0, filt(400.0, 0.3, 4000.0, 0.08, 0.4, 0.6, 0.3), env(0.03, 0.2, 1.0, 0.3),
          true, lfo(5.0, LSine, 1.0, 0.0), delay_set(0.25, 0.3, 0.3, false), reverb_set(0.4, 0.5, 0.12, true)),
        p("PWM Strings",
          osc(Square, 0.9, 0.0, 0.5, false), osc(Square, 0.9, 0.0, -0.5, false), osc(Square, 0.6, 1.0, 0.3, false),
          0.0, 0.0, filt(2000.0, 0.2, 1200.0, 0.4, 0.8, 0.8, 0.8), env(0.5, 0.5, 1.0, 0.9),
          true, lfo(0.5, LSine, 0.5, 300.0), delay_set(0.25, 0.3, 0.3, false), reverb_set(0.6, 0.5, 0.2, true)),
        p("Synth Brass",
          osc(Saw, 1.0, 0.0, 0.0, true), osc(Square, 0.6, 0.0, 1.0, true), osc(Saw, 0.5, 1.0, 0.0, false),
          0.0, 0.0, filt(600.0, 1.5, 4500.0, 0.05, 0.3, 0.7, 0.25), env(0.04, 0.2, 1.0, 0.25),
          true, lfo(5.0, LSine, 1.2, 0.0), delay_set(0.25, 0.3, 0.2, false), reverb_set(0.45, 0.5, 0.15, true)),
        p("Brass Section",
          osc(Saw, 1.0, 0.0, 2.0, true), osc(Saw, 0.9, 0.0, -2.0, true), osc(Saw, 0.7, 1.0, 0.0, true),
          0.0, 0.0, filt(700.0, 1.0, 4000.0, 0.07, 0.4, 0.7, 0.3), env(0.06, 0.25, 1.0, 0.3),
          true, lfo(5.2, LSine, 1.5, 0.0), delay_set(0.25, 0.3, 0.2, true), reverb_set(0.5, 0.5, 0.18, true)),
        p("Soft Strings",
          osc(Saw, 0.8, 0.0, 0.4, false), osc(Saw, 0.8, 0.0, -0.4, false), osc(Triangle, 0.5, 1.0, 0.0, false),
          0.0, 0.0, filt(1800.0, 0.5, 1200.0, 0.5, 0.8, 0.85, 1.0), env(0.6, 0.5, 1.0, 1.2),
          true, lfo(0.4, LSine, 0.4, 300.0), delay_set(0.3, 0.3, 0.2, false), reverb_set(0.75, 0.45, 0.3, true)),
        p("Saw Strings",
          osc(Saw, 0.9, 0.0, 0.6, false), osc(Saw, 0.9, 0.0, -0.6, false), osc(Saw, 0.6, 0.0, 1.5, false),
          0.0, 0.0, filt(2200.0, 0.7, 1500.0, 0.4, 0.7, 0.85, 0.9), env(0.5, 0.5, 1.0, 1.0),
          true, lfo(0.5, LSine, 0.5, 350.0), delay_set(0.3, 0.3, 0.2, false), reverb_set(0.7, 0.45, 0.28, true)),
        p("Octave Brass",
          osc(Saw, 1.0, 0.0, 0.0, true), osc(Saw, 0.7, 1.0, 2.0, true), osc(Square, 0.5, -1.0, 0.0, false),
          0.0, 0.0, filt(650.0, 1.2, 4000.0, 0.06, 0.35, 0.7, 0.3), env(0.05, 0.25, 1.0, 0.3),
          true, lfo(5.0, LSine, 1.2, 0.0), delay_set(0.25, 0.3, 0.2, true), reverb_set(0.5, 0.5, 0.15, true)),
        p("Trumpet Lead",
          osc(Saw, 1.0, 0.0, 0.0, true), osc(Square, 0.5, 0.0, 0.5, true), no_osc,
          0.0, 0.3, filt(800.0, 2.0, 4500.0, 0.04, 0.3, 0.6, 0.2), env(0.03, 0.2, 1.0, 0.2),
          true, lfo(5.5, LSine, 2.0, 0.0), delay_set(0.28, 0.3, 0.2, true), reverb_set(0.5, 0.5, 0.18, true)),
        p("Horn Section",
          osc(Saw, 1.0, 0.0, 1.0, true), osc(Saw, 0.8, 0.0, -1.0, true), osc(Triangle, 0.5, -1.0, 0.0, true),
          0.0, 0.0, filt(550.0, 0.8, 3500.0, 0.1, 0.5, 0.7, 0.35), env(0.08, 0.3, 1.0, 0.35),
          true, lfo(4.8, LSine, 1.0, 0.0), delay_set(0.25, 0.3, 0.2, true), reverb_set(0.55, 0.5, 0.2, true)),
        p("String Ensemble",
          osc(Saw, 0.9, 0.0, 0.7, false), osc(Saw, 0.9, 0.0, -0.7, false), osc(Saw, 0.7, 1.0, 0.4, false),
          0.0, 0.0, filt(2000.0, 0.4, 1500.0, 0.6, 1.0, 0.85, 1.2), env(0.7, 0.6, 1.0, 1.4),
          true, lfo(0.45, LSine, 0.6, 400.0), delay_set(0.3, 0.3, 0.25, true), reverb_set(0.8, 0.4, 0.3, true)),
        p("Cinematic Strings",
          osc(Saw, 0.9, 0.0, 1.0, false), osc(Saw, 0.9, -1.0, -1.0, false), osc(Triangle, 0.6, 0.0, 0.5, false),
          0.0, 0.0, filt(1500.0, 0.6, 2500.0, 1.2, 1.2, 0.85, 2.0), env(1.0, 0.8, 1.0, 2.2),
          true, lfo(0.2, LSine, 0.5, 600.0), delay_set(0.3, 0.45, 0.3, true), reverb_set(0.9, 0.35, 0.4, true)),
        p("Brass Stab",
          osc(Saw, 1.0, 0.0, 1.5, true), osc(Saw, 0.8, 0.0, -1.5, true), osc(Square, 0.6, 0.0, 0.0, false),
          0.0, 0.0, filt(900.0, 1.5, 4000.0, 0.005, 0.2, 0.0, 0.15), env(0.005, 0.25, 0.0, 0.18),
          false, no_lfo, delay_set(0.25, 0.3, 0.2, true), reverb_set(0.5, 0.5, 0.15, true)),

        // ============================================================
        // BELLS & MALLETS (10)  — incl. classic Crystal Bell
        // ============================================================
        p("Crystal Bell",
          osc(Sine, 1.0, 0.0, 0.0, false), osc(Sine, 0.5, 1.0, 1.0, false), osc(Sine, 0.3, 2.0, 0.0, false),
          0.0, 0.0, filt(6000.0, 0.1, 0.0, 0.001, 1.5, 0.0, 1.5), env(0.001, 1.5, 0.0, 1.8),
          false, no_lfo, delay_set(0.3, 0.25, 0.2, true), reverb_set(0.7, 0.4, 0.3, true)),
        p("Glass Bell",
          osc(Sine, 1.0, 0.0, 0.0, false), osc(Sine, 0.4, 2.585, 0.0, false), osc(Triangle, 0.2, 1.0, 0.0, false),
          0.0, 0.0, filt(7000.0, 0.2, 0.0, 0.001, 1.2, 0.0, 1.2), env(0.001, 1.2, 0.0, 1.5),
          false, no_lfo, delay_set(0.3, 0.3, 0.25, true), reverb_set(0.8, 0.35, 0.35, true)),
        p("Tubular Bell",
          osc(Sine, 1.0, 0.0, 0.0, false), osc(Sine, 0.6, 1.585, 0.0, false), osc(Sine, 0.3, 3.0, 0.0, false),
          0.0, 0.0, filt(5500.0, 0.1, 0.0, 0.001, 2.5, 0.1, 2.5), env(0.001, 2.5, 0.1, 3.0),
          false, no_lfo, delay_set(0.3, 0.3, 0.25, true), reverb_set(0.9, 0.3, 0.4, true)),
        p("Music Box",
          osc(Sine, 1.0, 1.0, 0.0, false), osc(Sine, 0.4, 2.0, 0.5, false), osc(Triangle, 0.2, 3.0, 0.0, false),
          0.0, 0.0, filt(8000.0, 0.2, 0.0, 0.001, 0.8, 0.0, 0.8), env(0.001, 0.8, 0.0, 1.0),
          false, no_lfo, delay_set(0.3, 0.3, 0.25, true), reverb_set(0.75, 0.35, 0.3, true)),
        p("Marimba",
          osc(Sine, 1.0, 0.0, 0.0, false), osc(Sine, 0.3, 1.585, 0.0, false), osc(Triangle, 0.2, 1.0, 0.0, false),
          0.0, 0.0, filt(4000.0, 0.3, 1000.0, 0.001, 0.4, 0.0, 0.3), env(0.001, 0.45, 0.0, 0.35),
          false, no_lfo, delay_set(0.25, 0.25, 0.15, true), reverb_set(0.5, 0.45, 0.18, true)),
        p("Vibraphone",
          osc(Sine, 1.0, 0.0, 0.0, true), osc(Sine, 0.4, 2.0, 0.0, true), osc(Triangle, 0.2, 1.0, 0.0, false),
          0.0, 0.0, filt(5000.0, 0.2, 500.0, 0.001, 1.2, 0.3, 1.0), env(0.001, 1.2, 0.3, 1.2),
          true, lfo(5.0, LSine, 2.0, 0.0), delay_set(0.3, 0.3, 0.2, true), reverb_set(0.7, 0.4, 0.28, true)),
        p("Kalimba",
          osc(Sine, 1.0, 0.0, 0.0, false), osc(Triangle, 0.3, 1.0, 0.0, false), osc(Sine, 0.2, 2.585, 0.0, false),
          0.0, 0.0, filt(3500.0, 0.5, 1500.0, 0.001, 0.5, 0.0, 0.4), env(0.001, 0.5, 0.0, 0.45),
          false, no_lfo, delay_set(0.28, 0.3, 0.2, true), reverb_set(0.55, 0.45, 0.22, true)),
        p("Glockenspiel",
          osc(Sine, 1.0, 1.0, 0.0, false), osc(Sine, 0.5, 3.0, 0.0, false), osc(Sine, 0.2, 2.0, 1.0, false),
          0.0, 0.0, filt(9000.0, 0.1, 0.0, 0.001, 0.7, 0.0, 0.6), env(0.001, 0.7, 0.0, 0.7),
          false, no_lfo, delay_set(0.3, 0.3, 0.2, true), reverb_set(0.8, 0.35, 0.32, true)),
        p("Temple Bell",
          osc(Sine, 1.0, 0.0, 0.0, false), osc(Sine, 0.6, 1.585, 2.0, false), osc(Sine, 0.4, 2.585, 0.0, false),
          0.0, 0.0, filt(4500.0, 0.2, 0.0, 0.001, 3.0, 0.0, 3.0), env(0.001, 3.0, 0.0, 3.5),
          false, no_lfo, delay_set(0.3, 0.35, 0.3, true), reverb_set(0.95, 0.3, 0.45, true)),
        p("Celesta",
          osc(Sine, 1.0, 1.0, 0.0, false), osc(Sine, 0.4, 2.0, 0.0, false), osc(Triangle, 0.25, 0.0, 0.0, false),
          0.0, 0.0, filt(7500.0, 0.2, 0.0, 0.001, 1.0, 0.0, 0.9), env(0.001, 1.0, 0.0, 1.0),
          false, no_lfo, delay_set(0.3, 0.3, 0.2, true), reverb_set(0.7, 0.4, 0.3, true)),

        // ============================================================
        // ARPS & SEQUENCES (8)  — incl. classic Acid Line
        // ============================================================
        p("Acid Line",
          osc(Saw, 1.0, 0.0, 0.0, false), no_osc, no_osc,
          0.0, 0.6, filt(120.0, 6.0, 4000.0, 0.005, 0.25, 0.2, 0.1), env(0.005, 0.2, 1.0, 0.1),
          false, no_lfo, delay_set(0.18, 0.35, 0.25, true), reverb_set(0.5, 0.5, 0.1, false)),
        p("Sequence Saw",
          osc(Saw, 1.0, 0.0, 0.0, false), osc(Saw, 0.4, 0.0, 0.4, false), no_osc,
          0.0, 0.0, filt(700.0, 3.0, 4000.0, 0.001, 0.18, 0.0, 0.12), env(0.001, 0.2, 0.0, 0.12),
          false, no_lfo, delay_set(0.1875, 0.4, 0.3, true), reverb_set(0.4, 0.5, 0.12, true)),
        p("Pluck Arp",
          osc(Saw, 1.0, 0.0, 0.0, false), osc(Square, 0.4, 1.0, 0.0, false), no_osc,
          0.0, 0.0, filt(900.0, 2.0, 4500.0, 0.001, 0.15, 0.0, 0.1), env(0.001, 0.18, 0.0, 0.12),
          false, no_lfo, delay_set(0.1875, 0.45, 0.35, true), reverb_set(0.45, 0.5, 0.15, true)),
        p("Square Arp",
          osc(Square, 1.0, 0.0, 0.0, false), osc(Square, 0.4, 1.0, 0.2, false), no_osc,
          0.0, 0.0, filt(1200.0, 1.5, 3500.0, 0.001, 0.12, 0.0, 0.1), env(0.001, 0.15, 0.0, 0.1),
          false, no_lfo, delay_set(0.125, 0.45, 0.35, true), reverb_set(0.4, 0.5, 0.12, true)),
        p("Octave Arp",
          osc(Saw, 1.0, 0.0, 0.0, false), osc(Saw, 0.5, 1.0, 0.0, false), osc(Square, 0.3, -1.0, 0.0, false),
          0.0, 0.0, filt(1000.0, 2.0, 4000.0, 0.001, 0.15, 0.0, 0.1), env(0.001, 0.18, 0.0, 0.12),
          false, no_lfo, delay_set(0.1875, 0.45, 0.35, true), reverb_set(0.45, 0.5, 0.15, true)),
        p("Digital Arp",
          osc(Square, 1.0, 0.0, 0.0, false), osc(Saw, 0.3, 2.0, 0.0, false), osc(Sine, 0.3, 1.0, 0.0, false),
          0.0, 0.0, filt(2500.0, 1.0, 3000.0, 0.001, 0.1, 0.0, 0.08), env(0.001, 0.12, 0.0, 0.1),
          false, no_lfo, delay_set(0.125, 0.5, 0.4, true), reverb_set(0.5, 0.45, 0.18, true)),
        p("Soft Arp",
          osc(Triangle, 1.0, 0.0, 0.0, false), osc(Sine, 0.5, 1.0, 0.0, false), no_osc,
          0.0, 0.0, filt(1500.0, 0.5, 2000.0, 0.001, 0.25, 0.0, 0.2), env(0.001, 0.3, 0.0, 0.2),
          false, no_lfo, delay_set(0.25, 0.4, 0.35, true), reverb_set(0.6, 0.45, 0.22, true)),
        p("Chase Sequence",
          osc(Saw, 1.0, 0.0, 2.0, false), osc(Saw, 0.7, 0.0, -2.0, false), osc(Square, 0.4, 1.0, 0.0, false),
          0.0, 0.0, filt(800.0, 4.0, 4000.0, 0.001, 0.12, 0.0, 0.1), env(0.001, 0.15, 0.0, 0.1),
          true, lfo(0.25, LSqr, 0.0, 2000.0), delay_set(0.1875, 0.5, 0.4, true), reverb_set(0.5, 0.45, 0.18, true)),

        // ============================================================
        // DRONES & ATMOSPHERES (10)  — incl. classic Sci-Fi Drone
        // ============================================================
        p("Sci-Fi Drone",
          osc(Saw, 0.9, 0.0, 0.5, true), osc(Saw, 0.9, -1.0, -0.5, true), osc(Triangle, 0.6, 1.0, 1.0, true),
          0.0, 0.0, filt(600.0, 0.4, 2000.0, 2.5, 2.0, 0.7, 2.5), env(2.0, 1.0, 1.0, 3.0),
          true, lfo(0.1, LSine, 1.0, 800.0), delay_set(0.3, 0.45, 0.3, true), reverb_set(0.9, 0.3, 0.35, true)),
        p("Deep Drone",
          osc(Saw, 1.0, -2.0, 0.5, false), osc(Saw, 0.8, -2.0, -0.5, false), osc(Sine, 0.6, -3.0, 0.0, false),
          0.0, 0.0, filt(400.0, 1.0, 1000.0, 3.0, 2.0, 0.8, 3.0), env(2.5, 1.5, 1.0, 3.5),
          true, lfo(0.05, LSine, 0.0, 400.0), delay_set(0.3, 0.5, 0.3, true), reverb_set(0.95, 0.3, 0.4, true)),
        p("Ocean Drone",
          osc(Triangle, 0.8, 0.0, 0.3, false), osc(Sine, 0.7, -1.0, -0.3, false), no_osc,
          0.3, 0.0, filt(800.0, 1.5, 1500.0, 3.0, 2.5, 0.7, 3.5), env(3.0, 2.0, 1.0, 4.0),
          true, lfo(0.07, LSine, 0.0, 600.0), delay_set(0.3, 0.5, 0.35, true), reverb_set(0.95, 0.3, 0.45, true)),
        p("Metallic Drone",
          osc(Square, 0.8, 0.0, 1.0, false), osc(Saw, 0.7, 0.585, -1.0, false), osc(Square, 0.5, 1.585, 0.0, false),
          0.0, 0.0, filt(1200.0, 3.0, 2000.0, 2.0, 2.0, 0.8, 3.0), env(1.5, 1.0, 1.0, 3.0),
          true, lfo(0.15, LSine, 0.0, 1000.0), delay_set(0.3, 0.5, 0.35, true), reverb_set(0.9, 0.35, 0.4, true)),
        p("Dark Atmosphere",
          osc(Saw, 0.9, -1.0, 0.4, false), osc(Triangle, 0.7, -2.0, -0.4, false), osc(Sine, 0.5, -1.0, 0.0, false),
          0.1, 0.0, filt(350.0, 2.0, 1500.0, 3.0, 2.5, 0.6, 4.0), env(2.5, 2.0, 1.0, 4.0),
          true, lfo(0.04, LTri, 0.0, 700.0), delay_set(0.3, 0.55, 0.35, true), reverb_set(0.98, 0.25, 0.45, true)),
        p("Space Wind",
          no_osc, no_osc, no_osc,
          0.8, 0.0, filt(600.0, 4.0, 3000.0, 2.5, 2.5, 0.7, 3.0), env(2.5, 1.5, 1.0, 3.5),
          true, lfo(0.08, LSine, 0.0, 2000.0), delay_set(0.3, 0.5, 0.35, true), reverb_set(0.95, 0.3, 0.45, true)),
        p("Tension Drone",
          osc(Saw, 0.9, 0.0, 0.0, false), osc(Saw, 0.9, 0.585, 6.0, false), osc(Square, 0.6, 1.0, -6.0, false),
          0.0, 0.0, filt(700.0, 3.0, 2500.0, 2.0, 2.0, 0.75, 3.0), env(1.8, 1.2, 1.0, 3.0),
          true, lfo(0.06, LSine, 0.0, 900.0), delay_set(0.3, 0.5, 0.3, true), reverb_set(0.92, 0.3, 0.4, true)),
        p("Underwater",
          osc(Sine, 1.0, -1.0, 0.2, false), osc(Triangle, 0.6, 0.0, -0.2, false), no_osc,
          0.1, 0.0, filt(500.0, 2.0, 1000.0, 2.0, 2.0, 0.7, 3.0), env(2.0, 1.5, 1.0, 3.0),
          true, lfo(0.5, LSine, 0.0, 400.0), delay_set(0.3, 0.55, 0.4, true), reverb_set(0.95, 0.3, 0.45, true)),
        p("Cosmic Wash",
          osc(Sine, 0.9, 0.0, 0.3, false), osc(Sine, 0.7, 1.0, -0.3, false), osc(Triangle, 0.5, 2.0, 0.5, false),
          0.0, 0.0, filt(2500.0, 0.5, 2000.0, 2.5, 2.0, 0.8, 3.5), env(2.5, 1.5, 1.0, 4.0),
          true, lfo(0.05, LSine, 0.0, 1200.0), delay_set(0.3, 0.55, 0.4, true), reverb_set(0.98, 0.25, 0.5, true)),
        p("Cinematic Pad",
          osc(Saw, 0.9, 0.0, 0.8, false), osc(Saw, 0.9, -1.0, -0.8, false), osc(Sine, 0.6, 1.0, 0.0, false),
          0.0, 0.0, filt(1000.0, 1.0, 3000.0, 2.0, 2.0, 0.8, 3.0), env(1.8, 1.2, 1.0, 3.5),
          true, lfo(0.06, LSine, 0.0, 1500.0), delay_set(0.3, 0.5, 0.35, true), reverb_set(0.95, 0.3, 0.45, true)),

        // ============================================================
        // SOUND FX & RISERS (6)  — incl. classic Riser FX
        // ============================================================
        p("Riser FX",
          osc(Saw, 0.7, 0.0, 0.0, false), no_osc, no_osc,
          0.5, 0.0, filt(200.0, 4.0, 6000.0, 2.0, 2.0, 1.0, 1.0), env(1.5, 0.5, 1.0, 0.5),
          true, lfo(0.2, LTri, 0.0, 2000.0), delay_set(0.3, 0.4, 0.3, true), reverb_set(0.8, 0.4, 0.3, true)),
        p("Down Sweep",
          osc(Saw, 0.8, 1.0, 0.0, false), osc(Square, 0.5, 0.0, 0.0, false), no_osc,
          0.2, 0.95, filt(8000.0, 3.0, -6000.0, 0.001, 2.0, 0.0, 1.0), env(0.001, 2.0, 0.0, 1.0),
          true, lfo(0.3, LSaw, 0.0, 3000.0), delay_set(0.3, 0.45, 0.35, true), reverb_set(0.85, 0.4, 0.35, true)),
        p("Laser Zap",
          osc(Square, 1.0, 2.0, 0.0, false), osc(Saw, 0.5, 1.0, 0.0, false), no_osc,
          0.0, 0.9, filt(6000.0, 5.0, -4000.0, 0.001, 0.3, 0.0, 0.2), env(0.001, 0.35, 0.0, 0.2),
          true, lfo(30.0, LSaw, 0.0, 3000.0), delay_set(0.2, 0.5, 0.4, true), reverb_set(0.6, 0.4, 0.25, true)),
        p("Alarm",
          osc(Square, 1.0, 1.0, 0.0, false), osc(Square, 0.6, 2.0, 0.0, false), no_osc,
          0.0, 0.0, filt(4000.0, 2.0, 0.0, 0.005, 0.0, 1.0, 0.05), env(0.005, 0.0, 1.0, 0.05),
          true, lfo(4.0, LSqr, 0.0, 3000.0), delay_set(0.25, 0.3, 0.2, true), reverb_set(0.5, 0.5, 0.15, true)),
        p("Wind Noise",
          no_osc, no_osc, no_osc,
          1.0, 0.0, filt(800.0, 5.0, 4000.0, 1.0, 1.0, 0.8, 1.5), env(1.0, 0.5, 1.0, 1.5),
          true, lfo(0.3, LSine, 0.0, 3000.0), delay_set(0.3, 0.4, 0.3, true), reverb_set(0.9, 0.35, 0.4, true)),
        p("UFO",
          osc(Sine, 1.0, 0.0, 0.0, true), osc(Sine, 0.6, 1.0, 0.0, true), no_osc,
          0.0, 0.0, filt(3000.0, 4.0, 2000.0, 0.01, 0.5, 1.0, 0.5), env(0.05, 0.3, 1.0, 0.5),
          true, lfo(7.0, LSine, 15.0, 1500.0), delay_set(0.3, 0.5, 0.4, true), reverb_set(0.8, 0.4, 0.35, true)),

        // ============================================================
        // PERCUSSION & DRUMS (4)
        // ============================================================
        p("Synth Kick",
          osc(Sine, 1.0, -2.0, 0.0, false), osc(Sine, 0.3, -1.0, 0.0, false), no_osc,
          0.0, 0.0, filt(2000.0, 1.0, 4000.0, 0.001, 0.08, 0.0, 0.08), env(0.001, 0.18, 0.0, 0.12),
          false, no_lfo, delay_set(0.25, 0.3, 0.0, false), reverb_set(0.3, 0.5, 0.05, false)),
        p("Synth Tom",
          osc(Sine, 1.0, -1.0, 0.0, false), osc(Triangle, 0.4, 0.0, 0.0, false), no_osc,
          0.1, 0.0, filt(1500.0, 1.5, 3000.0, 0.001, 0.2, 0.0, 0.15), env(0.001, 0.25, 0.0, 0.18),
          false, no_lfo, delay_set(0.25, 0.3, 0.1, false), reverb_set(0.4, 0.5, 0.1, true)),
        p("Zap Snare",
          osc(Triangle, 0.7, 0.0, 0.0, false), no_osc, no_osc,
          0.8, 0.0, filt(2500.0, 2.0, 3000.0, 0.001, 0.15, 0.0, 0.12), env(0.001, 0.2, 0.0, 0.15),
          false, no_lfo, delay_set(0.2, 0.3, 0.15, true), reverb_set(0.5, 0.5, 0.12, true)),
        p("Noise Hat",
          no_osc, no_osc, no_osc,
          1.0, 0.0, filt(9000.0, 3.0, 0.0, 0.001, 0.05, 0.0, 0.04), env(0.001, 0.06, 0.0, 0.05),
          false, no_lfo, delay_set(0.18, 0.25, 0.1, true), reverb_set(0.4, 0.5, 0.08, true)),
    ]
}
