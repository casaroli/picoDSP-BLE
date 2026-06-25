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

impl Default for Preset {
    fn default() -> Self {
        Self {
            name: make_name("Init Patch"),
            osc1: osc(Waveform::Saw, 1.0, 0.0, 0.0, true),
            osc2: osc(Waveform::Saw, 0.0, 0.0, 0.0, true),
            osc3: osc(Waveform::Saw, 0.0, 0.0, 0.0, true),
            noise_level: 0.0,
            portamento: 0.0,
            filter: FilterSettings {
                cutoff: 20000.0,
                resonance: 0.0,
                env_amount: 0.0,
                attack: 0.0,
                decay: 0.0,
                sustain: 1.0,
                release: 0.0,
            },
            amp: EnvelopeSettings {
                attack: 0.01,
                decay: 0.1,
                sustain: 1.0,
                release: 0.1,
            },
            lfo_enabled: 0,
            lfo: lfo(1.0, LfoWaveform::Sine, 0.0, 0.0),
            delay: delay_set(0.25, 0.3, 0.3, false),
            reverb: reverb_set(0.5, 0.5, 0.1, false),
            _padding: [0; 4],
        }
    }
}

pub fn get_default_presets() -> [Preset; 17] {
    [
        Preset {
            name: make_name("Lucky Man"),
            osc1: osc(Waveform::Square, 1.0, 0.0, 0.0, true),
            osc2: osc(Waveform::Square, 0.7, 0.0, 2.0, true),
            osc3: osc(Waveform::Square, 0.7, 0.0, -2.0, true),
            noise_level: 0.0,
            portamento: 0.92,
            filter: FilterSettings {
                cutoff: 200.0,
                resonance: 0.4,
                env_amount: 3000.0,
                attack: 0.1,
                decay: 1.5,
                sustain: 0.4,
                release: 0.5,
            },
            amp: EnvelopeSettings {
                attack: 0.05,
                decay: 0.2,
                sustain: 1.0,
                release: 0.5,
            },
            lfo_enabled: 1,
            lfo: lfo(5.0, LfoWaveform::Sine, 2.0, 0.0),
            delay: delay_set(0.4, 0.3, 0.3, true),
            reverb: reverb_set(0.5, 0.5, 0.1, false),
            _padding: [0; 4],
        },
        Preset {
            name: make_name("Tom Sawyer"),
            osc1: osc(Waveform::Saw, 1.0, 0.0, 0.0, false),
            osc2: osc(Waveform::Saw, 0.5, 0.0, 1.5, false),
            osc3: osc(Waveform::Sine, 0.0, 0.0, 0.0, false),
            noise_level: 0.0,
            portamento: 0.0,
            filter: FilterSettings {
                cutoff: 80.0,
                resonance: 0.45,
                env_amount: 5000.0,
                attack: 0.03,
                decay: 2.0,
                sustain: 0.1,
                release: 0.1,
            },
            amp: EnvelopeSettings {
                attack: 0.01,
                decay: 0.1,
                sustain: 1.0,
                release: 0.2,
            },
            lfo_enabled: 0,
            lfo: lfo(1.0, LfoWaveform::Sine, 0.0, 0.0),
            delay: delay_set(0.15, 0.2, 0.2, true),
            reverb: reverb_set(0.3, 0.5, 0.1, false),
            _padding: [0; 4],
        },
        Preset {
            name: make_name("Moog Scream"),
            osc1: osc(Waveform::Saw, 1.0, 0.0, 0.0, true),
            osc2: osc(Waveform::Saw, 0.6, 0.0, 2.5, true),
            osc3: osc(Waveform::Square, 0.8, 0.0, -2.5, true),
            noise_level: 0.15,
            portamento: 0.85,
            filter: FilterSettings {
                cutoff: 100.0,
                resonance: 0.75,
                env_amount: 6000.0,
                attack: 0.005,
                decay: 0.3,
                sustain: 0.2,
                release: 0.2,
            },
            amp: EnvelopeSettings {
                attack: 0.005,
                decay: 0.2,
                sustain: 1.0,
                release: 0.2,
            },
            lfo_enabled: 1,
            lfo: lfo(0.15, LfoWaveform::Sine, 8.0, 0.0),
            delay: delay_set(0.25, 0.3, 0.3, false),
            reverb: reverb_set(0.5, 0.5, 0.2, true),
            _padding: [0; 4],
        },
        Preset {
            name: make_name("Moog Bass"),
            osc1: osc(Waveform::Saw, 1.0, -3.0, 0.0, false),
            osc2: osc(Waveform::Saw, 0.4, -3.0, 0.3, false),
            osc3: osc(Waveform::Square, 0.5, -4.0, 0.0, false),
            noise_level: 0.0,
            portamento: 0.0,
            filter: FilterSettings {
                cutoff: 80.0,
                resonance: 0.6,
                env_amount: 3000.0,
                attack: 0.001,
                decay: 0.25,
                sustain: 0.0,
                release: 0.1,
            },
            amp: EnvelopeSettings {
                attack: 0.001,
                decay: 0.2,
                sustain: 0.8,
                release: 0.1,
            },
            lfo_enabled: 0,
            lfo: lfo(1.0, LfoWaveform::Sine, 0.0, 0.0),
            delay: delay_set(0.25, 0.3, 0.3, false),
            reverb: reverb_set(0.5, 0.5, 0.1, false),
            _padding: [0; 4],
        },
        Preset {
            name: make_name("Octavarium Lead"),
            osc1: osc(Waveform::Saw, 1.0, 0.0, 0.0, true),
            osc2: osc(Waveform::Saw, 0.5, 0.0, 2.0, false),
            osc3: osc(Waveform::Square, 0.3, 0.0, 0.0, false),
            noise_level: 0.0,
            portamento: 0.94,
            filter: FilterSettings {
                cutoff: 500.0,
                resonance: 0.6,
                env_amount: 4000.0,
                attack: 0.01,
                decay: 0.5,
                sustain: 0.6,
                release: 0.2,
            },
            amp: EnvelopeSettings {
                attack: 0.005,
                decay: 0.1,
                sustain: 1.0,
                release: 0.2,
            },
            lfo_enabled: 1,
            lfo: lfo(5.5, LfoWaveform::Sine, 1.5, 0.0),
            delay: delay_set(0.25, 0.3, 0.3, true),
            reverb: reverb_set(0.5, 0.5, 0.1, true),
            _padding: [0; 4],
        },
        // --- 12 extra factory presets ---------------------------------------
        // Three detuned saws + a brass-style filter swell.
        Preset {
            name: make_name("Fat Brass"),
            osc1: osc(Waveform::Saw, 1.0, 0.0, 0.0, true),
            osc2: osc(Waveform::Saw, 0.8, 0.0, 3.0, true),
            osc3: osc(Waveform::Saw, 0.8, 0.0, -3.0, true),
            noise_level: 0.0,
            portamento: 0.0,
            filter: FilterSettings {
                cutoff: 400.0,
                resonance: 0.3,
                env_amount: 4000.0,
                attack: 0.08,
                decay: 0.4,
                sustain: 0.6,
                release: 0.3,
            },
            amp: EnvelopeSettings {
                attack: 0.03,
                decay: 0.2,
                sustain: 1.0,
                release: 0.3,
            },
            lfo_enabled: 1,
            lfo: lfo(5.0, LfoWaveform::Sine, 1.0, 0.0),
            delay: delay_set(0.25, 0.3, 0.3, false),
            reverb: reverb_set(0.4, 0.5, 0.12, true),
            _padding: [0; 4],
        },
        // Soft triangle/sine pad, slow swell, drifting cutoff, long reverb.
        Preset {
            name: make_name("Glass Pad"),
            osc1: osc(Waveform::Triangle, 0.9, 0.0, 0.0, false),
            osc2: osc(Waveform::Sine, 0.7, 1.0, 0.2, false),
            osc3: osc(Waveform::Triangle, 0.5, 0.0, -0.2, false),
            noise_level: 0.0,
            portamento: 0.0,
            filter: FilterSettings {
                cutoff: 1200.0,
                resonance: 0.2,
                env_amount: 1500.0,
                attack: 0.8,
                decay: 1.0,
                sustain: 0.8,
                release: 1.5,
            },
            amp: EnvelopeSettings {
                attack: 0.9,
                decay: 0.5,
                sustain: 1.0,
                release: 1.8,
            },
            lfo_enabled: 1,
            lfo: lfo(0.3, LfoWaveform::Sine, 0.0, 400.0),
            delay: delay_set(0.25, 0.3, 0.3, false),
            reverb: reverb_set(0.7, 0.4, 0.3, true),
            _padding: [0; 4],
        },
        // TB-303-style mono acid line: single saw, high resonance, glide.
        Preset {
            name: make_name("Acid Line"),
            osc1: osc(Waveform::Saw, 1.0, 0.0, 0.0, false),
            osc2: osc(Waveform::Saw, 0.0, 0.0, 0.0, false),
            osc3: osc(Waveform::Saw, 0.0, 0.0, 0.0, false),
            noise_level: 0.0,
            portamento: 0.6,
            filter: FilterSettings {
                cutoff: 120.0,
                resonance: 0.8,
                env_amount: 4000.0,
                attack: 0.005,
                decay: 0.25,
                sustain: 0.2,
                release: 0.1,
            },
            amp: EnvelopeSettings {
                attack: 0.005,
                decay: 0.2,
                sustain: 1.0,
                release: 0.1,
            },
            lfo_enabled: 0,
            lfo: lfo(1.0, LfoWaveform::Sine, 0.0, 0.0),
            delay: delay_set(0.18, 0.35, 0.25, true),
            reverb: reverb_set(0.5, 0.5, 0.1, false),
            _padding: [0; 4],
        },
        // Bright detuned super-saw lead with vibrato and delay.
        Preset {
            name: make_name("Super Saw Lead"),
            osc1: osc(Waveform::Saw, 1.0, 0.0, 0.0, true),
            osc2: osc(Waveform::Saw, 0.7, 0.0, 4.0, true),
            osc3: osc(Waveform::Saw, 0.7, 0.0, -4.0, true),
            noise_level: 0.0,
            portamento: 0.0,
            filter: FilterSettings {
                cutoff: 3000.0,
                resonance: 0.3,
                env_amount: 3000.0,
                attack: 0.01,
                decay: 0.5,
                sustain: 0.7,
                release: 0.3,
            },
            amp: EnvelopeSettings {
                attack: 0.01,
                decay: 0.2,
                sustain: 1.0,
                release: 0.3,
            },
            lfo_enabled: 1,
            lfo: lfo(5.5, LfoWaveform::Sine, 1.5, 0.0),
            delay: delay_set(0.3, 0.3, 0.3, true),
            reverb: reverb_set(0.4, 0.5, 0.1, true),
            _padding: [0; 4],
        },
        // Pure sine sub bass with an octave-up sine for presence.
        Preset {
            name: make_name("Deep Sub"),
            osc1: osc(Waveform::Sine, 1.0, -2.0, 0.0, false),
            osc2: osc(Waveform::Sine, 0.3, -1.0, 0.0, false),
            osc3: osc(Waveform::Sine, 0.0, 0.0, 0.0, false),
            noise_level: 0.0,
            portamento: 0.0,
            filter: FilterSettings {
                cutoff: 800.0,
                resonance: 0.1,
                env_amount: 0.0,
                attack: 0.001,
                decay: 0.1,
                sustain: 1.0,
                release: 0.1,
            },
            amp: EnvelopeSettings {
                attack: 0.002,
                decay: 0.15,
                sustain: 0.9,
                release: 0.12,
            },
            lfo_enabled: 0,
            lfo: lfo(1.0, LfoWaveform::Sine, 0.0, 0.0),
            delay: delay_set(0.25, 0.3, 0.3, false),
            reverb: reverb_set(0.5, 0.5, 0.1, false),
            _padding: [0; 4],
        },
        // Detuned squares for a PWM-style string ensemble, slow attack.
        Preset {
            name: make_name("PWM Strings"),
            osc1: osc(Waveform::Square, 0.9, 0.0, 0.5, false),
            osc2: osc(Waveform::Square, 0.9, 0.0, -0.5, false),
            osc3: osc(Waveform::Square, 0.6, 1.0, 0.3, false),
            noise_level: 0.0,
            portamento: 0.0,
            filter: FilterSettings {
                cutoff: 2000.0,
                resonance: 0.2,
                env_amount: 1200.0,
                attack: 0.4,
                decay: 0.8,
                sustain: 0.8,
                release: 0.8,
            },
            amp: EnvelopeSettings {
                attack: 0.5,
                decay: 0.5,
                sustain: 1.0,
                release: 0.9,
            },
            lfo_enabled: 1,
            lfo: lfo(0.5, LfoWaveform::Sine, 0.5, 300.0),
            delay: delay_set(0.25, 0.3, 0.3, false),
            reverb: reverb_set(0.6, 0.5, 0.2, true),
            _padding: [0; 4],
        },
        // Short percussive pluck with a snappy filter envelope and delay.
        Preset {
            name: make_name("Snappy Pluck"),
            osc1: osc(Waveform::Saw, 1.0, 0.0, 0.0, false),
            osc2: osc(Waveform::Square, 0.5, 0.0, 0.3, false),
            osc3: osc(Waveform::Saw, 0.0, 0.0, 0.0, false),
            noise_level: 0.0,
            portamento: 0.0,
            filter: FilterSettings {
                cutoff: 300.0,
                resonance: 0.5,
                env_amount: 5000.0,
                attack: 0.001,
                decay: 0.18,
                sustain: 0.0,
                release: 0.15,
            },
            amp: EnvelopeSettings {
                attack: 0.001,
                decay: 0.25,
                sustain: 0.0,
                release: 0.2,
            },
            lfo_enabled: 0,
            lfo: lfo(1.0, LfoWaveform::Sine, 0.0, 0.0),
            delay: delay_set(0.25, 0.35, 0.3, true),
            reverb: reverb_set(0.4, 0.5, 0.1, true),
            _padding: [0; 4],
        },
        // Sine partials stacked an octave and two octaves up for a bell.
        Preset {
            name: make_name("Crystal Bell"),
            osc1: osc(Waveform::Sine, 1.0, 0.0, 0.0, false),
            osc2: osc(Waveform::Sine, 0.5, 1.0, 1.0, false),
            osc3: osc(Waveform::Sine, 0.3, 2.0, 0.0, false),
            noise_level: 0.0,
            portamento: 0.0,
            filter: FilterSettings {
                cutoff: 6000.0,
                resonance: 0.1,
                env_amount: 0.0,
                attack: 0.001,
                decay: 1.5,
                sustain: 0.0,
                release: 1.5,
            },
            amp: EnvelopeSettings {
                attack: 0.001,
                decay: 1.5,
                sustain: 0.0,
                release: 1.8,
            },
            lfo_enabled: 0,
            lfo: lfo(1.0, LfoWaveform::Sine, 0.0, 0.0),
            delay: delay_set(0.3, 0.25, 0.2, true),
            reverb: reverb_set(0.7, 0.4, 0.3, true),
            _padding: [0; 4],
        },
        // Noise + saw with a slow filter-envelope/LFO sweep — a riser FX.
        Preset {
            name: make_name("Riser FX"),
            osc1: osc(Waveform::Saw, 0.7, 0.0, 0.0, false),
            osc2: osc(Waveform::Saw, 0.0, 0.0, 0.0, false),
            osc3: osc(Waveform::Saw, 0.0, 0.0, 0.0, false),
            noise_level: 0.5,
            portamento: 0.0,
            filter: FilterSettings {
                cutoff: 200.0,
                resonance: 0.6,
                env_amount: 6000.0,
                attack: 2.0,
                decay: 2.0,
                sustain: 1.0,
                release: 1.0,
            },
            amp: EnvelopeSettings {
                attack: 1.5,
                decay: 0.5,
                sustain: 1.0,
                release: 0.5,
            },
            lfo_enabled: 1,
            lfo: lfo(0.2, LfoWaveform::Triangle, 0.0, 2000.0),
            delay: delay_set(0.3, 0.4, 0.3, true),
            reverb: reverb_set(0.8, 0.4, 0.3, true),
            _padding: [0; 4],
        },
        // Saw/square bass with a heavy LFO cutoff wobble.
        Preset {
            name: make_name("Wobble Bass"),
            osc1: osc(Waveform::Saw, 1.0, -1.0, 0.0, false),
            osc2: osc(Waveform::Square, 0.7, -1.0, 0.3, false),
            osc3: osc(Waveform::Saw, 0.5, -2.0, 0.0, false),
            noise_level: 0.0,
            portamento: 0.0,
            filter: FilterSettings {
                cutoff: 300.0,
                resonance: 0.7,
                env_amount: 1000.0,
                attack: 0.005,
                decay: 0.3,
                sustain: 0.7,
                release: 0.2,
            },
            amp: EnvelopeSettings {
                attack: 0.005,
                decay: 0.2,
                sustain: 0.9,
                release: 0.2,
            },
            lfo_enabled: 1,
            lfo: lfo(3.0, LfoWaveform::Sine, 0.0, 1500.0),
            delay: delay_set(0.25, 0.3, 0.3, false),
            reverb: reverb_set(0.5, 0.5, 0.1, false),
            _padding: [0; 4],
        },
        // Drawbar organ: 8' + an octave + an octave-and-a-fifth (2^1.585) sine.
        Preset {
            name: make_name("Drawbar Organ"),
            osc1: osc(Waveform::Sine, 1.0, 0.0, 0.0, false),
            osc2: osc(Waveform::Sine, 0.7, 1.0, 0.0, false),
            osc3: osc(Waveform::Sine, 0.5, 1.585, 0.0, false),
            noise_level: 0.0,
            portamento: 0.0,
            filter: FilterSettings {
                cutoff: 8000.0,
                resonance: 0.0,
                env_amount: 0.0,
                attack: 0.005,
                decay: 0.0,
                sustain: 1.0,
                release: 0.05,
            },
            amp: EnvelopeSettings {
                attack: 0.005,
                decay: 0.0,
                sustain: 1.0,
                release: 0.08,
            },
            lfo_enabled: 1,
            lfo: lfo(6.0, LfoWaveform::Sine, 0.5, 0.0),
            delay: delay_set(0.25, 0.3, 0.3, false),
            reverb: reverb_set(0.4, 0.5, 0.15, true),
            _padding: [0; 4],
        },
        // Slowly evolving detuned drone with deep LFO filter motion and big space.
        Preset {
            name: make_name("Sci-Fi Drone"),
            osc1: osc(Waveform::Saw, 0.9, 0.0, 0.5, true),
            osc2: osc(Waveform::Saw, 0.9, -1.0, -0.5, true),
            osc3: osc(Waveform::Triangle, 0.6, 1.0, 1.0, true),
            noise_level: 0.0,
            portamento: 0.0,
            filter: FilterSettings {
                cutoff: 600.0,
                resonance: 0.4,
                env_amount: 2000.0,
                attack: 2.5,
                decay: 2.0,
                sustain: 0.7,
                release: 2.5,
            },
            amp: EnvelopeSettings {
                attack: 2.0,
                decay: 1.0,
                sustain: 1.0,
                release: 3.0,
            },
            lfo_enabled: 1,
            lfo: lfo(0.1, LfoWaveform::Sine, 1.0, 800.0),
            delay: delay_set(0.5, 0.45, 0.3, true),
            reverb: reverb_set(0.9, 0.3, 0.35, true),
            _padding: [0; 4],
        },
    ]
}
