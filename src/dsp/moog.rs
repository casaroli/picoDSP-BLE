use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use infinitedsp_core::FrameProcessor;
use infinitedsp_core::core::audio_param::AudioParam;
use infinitedsp_core::core::channels::Mono;
use infinitedsp_core::core::dsp_chain::DspChain;
use infinitedsp_core::effects::filter::predictive_ladder::PredictiveLadderFilter;
use infinitedsp_core::effects::utility::gain::Gain;
use infinitedsp_core::effects::utility::offset::Offset;
use infinitedsp_core::synthesis::envelope::Adsr;
use infinitedsp_core::synthesis::lfo::Lfo;
use infinitedsp_core::synthesis::oscillator::{Oscillator, Waveform};

use crate::control::midi::{
    slot, AtomicParam, MidiControl, MidiFilterCutoff, MidiFilterResonance, MidiFreq, MidiGate,
    MidiVelocity,
};
use crate::data::presets::{OscSettings, Preset};

/// An `AudioParam` whose value is read live from a `MidiControl` continuous slot, so the
/// editor can sweep it via CC without rebuilding the voice. See `control::cc_map`.
fn live(midi: &Arc<MidiControl>, slot: usize) -> AudioParam {
    AudioParam::Dynamic(Box::new(AtomicParam::new(midi.clone(), slot)))
}

struct MoogOscillatorSection {
    osc1: Oscillator,
    osc2: Oscillator,
    osc3: Oscillator,
    noise: Oscillator,
    /// Oscillator + noise levels are read live each block (slots OSC1/2/3_LEVEL, NOISE) so the
    /// editor can sweep them via CC. An osc whose level is ~0 is skipped to save CPU, but the
    /// node always exists, so raising the level later brings it back in.
    control: Arc<MidiControl>,
    scratch_buffer: Vec<f32>,
}

impl MoogOscillatorSection {
    fn new(
        osc1: Oscillator,
        osc2: Oscillator,
        osc3: Oscillator,
        noise: Oscillator,
        control: Arc<MidiControl>,
    ) -> Self {
        Self {
            osc1,
            osc2,
            osc3,
            noise,
            control,
            scratch_buffer: vec![0.0; 256],
        }
    }
}

impl FrameProcessor<Mono> for MoogOscillatorSection {
    fn process(&mut self, buffer: &mut [f32], frame_index: u64) {
        let len = buffer.len();
        if self.scratch_buffer.len() < len {
            self.scratch_buffer.resize(len, 0.0);
        }

        let level1 = self.control.get_cont(slot::OSC1_LEVEL);
        let level2 = self.control.get_cont(slot::OSC2_LEVEL);
        let level3 = self.control.get_cont(slot::OSC3_LEVEL);
        let level_noise = self.control.get_cont(slot::NOISE);

        if level1 > 0.0001 {
            self.osc1.process(buffer, frame_index);
            for s in buffer.iter_mut() {
                *s *= level1;
            }
        } else {
            buffer.fill(0.0);
        }

        if level2 > 0.0001 {
            self.osc2
                .process(&mut self.scratch_buffer[0..len], frame_index);
            for (s, scratch) in buffer.iter_mut().zip(self.scratch_buffer.iter()) {
                *s += *scratch * level2;
            }
        }

        if level3 > 0.0001 {
            self.osc3
                .process(&mut self.scratch_buffer[0..len], frame_index);
            for (s, scratch) in buffer.iter_mut().zip(self.scratch_buffer.iter()) {
                *s += *scratch * level3;
            }
        }

        if level_noise > 0.0001 {
            self.noise
                .process(&mut self.scratch_buffer[0..len], frame_index);
            for (s, scratch) in buffer.iter_mut().zip(self.scratch_buffer.iter()) {
                *s += *scratch * level_noise;
            }
        }
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.osc1.set_sample_rate(sample_rate);
        self.osc2.set_sample_rate(sample_rate);
        self.osc3.set_sample_rate(sample_rate);
        self.noise.set_sample_rate(sample_rate);
    }

    fn reset(&mut self) {
        self.osc1.reset();
        self.osc2.reset();
        self.osc3.reset();
        self.noise.reset();
    }

    fn latency_samples(&self) -> u32 {
        0
    }
    fn name(&self) -> &str {
        "MoogOscillatorSection"
    }
    fn visualize(&self, _indent: usize) -> alloc::string::String {
        "MoogOscillatorSection".into()
    }
}

pub fn new_moog_voice(
    sample_rate: f32,
    midi: Arc<MidiControl>,
    preset: Preset,
) -> impl FrameProcessor<Mono> + Send {
    // Seed every live continuous slot from the preset (cutoff, resonance, levels, octave,
    // detune, env amt, ADSRs, FX amounts). The DSP nodes below read these slots per-block, so
    // a CC sweep updates them live; a structural rebuild re-seeds here from the working preset.
    midi.seed_from_preset(&preset);

    // Single shared LFO config. Whether the LFO exists at all (lfo_enabled) and its waveform
    // are structural — toggling/changing them rebuilds the voice. Its rate and the vibrato /
    // filter depths are live: each LFO instance reads the LFO_FREQ slot for its rate and is
    // built with range -1..1 then scaled by a live depth Gain (LFO_VIB_AMT / LFO_FILT_AMT), so
    // a slider sweep updates smoothly. When disabled, no LFO nodes are built (the live slots
    // simply go unread until it's re-enabled).
    let lfo_on = preset.lfo_enabled != 0;
    let lfo_wave = preset.lfo.get_waveform();

    // Build a fresh LFO whose output is scaled live by `depth_slot` (one instance per use site,
    // since `AudioParam` owns its processor).
    let make_lfo = |depth_slot: usize| -> AudioParam {
        let mut l = Lfo::new(live(&midi, slot::LFO_FREQ), lfo_wave);
        l.set_range(-1.0, 1.0);
        l.set_sample_rate(sample_rate);
        AudioParam::Dynamic(Box::new(
            DspChain::new(l, sample_rate).and(Gain::new(live(&midi, depth_slot))),
        ))
    };

    let filter_lfo_node: Option<AudioParam> = lfo_on.then(|| make_lfo(slot::LFO_FILT_AMT));

    // `idx` (0,1,2) selects the oscillator's live octave/detune slots. Octave (a 2^oct
    // frequency multiplier) and detune are always inserted so they stay live; vibrato is
    // structural (its LFO is rebuilt when the per-osc toggle / lfo_enabled changes).
    let create_pitch = |idx: usize, params: &OscSettings| -> AudioParam {
        let mut chain = DspChain::new(MidiFreq::new(midi.clone()), sample_rate)
            .and(Gain::new(live(&midi, slot::osc_octave(idx))))
            .and(Offset::new_param(live(&midi, slot::osc_detune(idx))));

        if lfo_on && params.is_vibrato_enabled() {
            chain = chain.and(Offset::new_param(make_lfo(slot::LFO_VIB_AMT)));
        }

        AudioParam::Dynamic(Box::new(chain))
    };

    let mut osc1_node =
        Oscillator::new(create_pitch(0, &preset.osc1), preset.osc1.get_waveform());
    osc1_node.set_sample_rate(sample_rate);

    let mut osc2_node =
        Oscillator::new(create_pitch(1, &preset.osc2), preset.osc2.get_waveform());
    osc2_node.set_sample_rate(sample_rate);

    let mut osc3_node =
        Oscillator::new(create_pitch(2, &preset.osc3), preset.osc3.get_waveform());
    osc3_node.set_sample_rate(sample_rate);

    let mut noise_node = Oscillator::new(AudioParam::Static(0.0), Waveform::WhiteNoise);
    noise_node.set_sample_rate(sample_rate);

    // Levels (incl. noise) are read live from the control slots inside the section.
    let mixer = MoogOscillatorSection::new(osc1_node, osc2_node, osc3_node, noise_node, midi.clone());

    let filter_env = Adsr::new(
        AudioParam::Dynamic(Box::new(MidiGate(midi.clone()))),
        live(&midi, slot::FILT_ATTACK),
        live(&midi, slot::FILT_DECAY),
        live(&midi, slot::FILT_SUSTAIN),
        live(&midi, slot::FILT_RELEASE),
    );

    let cutoff_ctrl = MidiFilterCutoff(midi.clone());

    let mut cutoff_mod_chain = DspChain::new(cutoff_ctrl, sample_rate).and(Offset::new_param(
        AudioParam::Dynamic(Box::new(
            DspChain::new(filter_env, sample_rate).and(Gain::new(live(&midi, slot::FILT_ENV_AMT))),
        )),
    ));

    if let Some(lfo) = filter_lfo_node {
        cutoff_mod_chain = cutoff_mod_chain.and(Offset::new_param(lfo));
    }

    let resonance_ctrl = MidiFilterResonance(midi.clone());

    let filter_node = PredictiveLadderFilter::new(
        AudioParam::Dynamic(Box::new(cutoff_mod_chain)),
        AudioParam::Dynamic(Box::new(resonance_ctrl)),
    );

    let amp_env = Adsr::new(
        AudioParam::Dynamic(Box::new(MidiGate(midi.clone()))),
        live(&midi, slot::AMP_ATTACK),
        live(&midi, slot::AMP_DECAY),
        live(&midi, slot::AMP_SUSTAIN),
        live(&midi, slot::AMP_RELEASE),
    );

    // Scale the amp envelope by note velocity (fixed, always-on velocity sensitivity), then
    // use the product as the VCA gain so harder hits play louder.
    let amp_env_vel = DspChain::new(amp_env, sample_rate).and(Gain::new(AudioParam::Dynamic(
        Box::new(MidiVelocity(midi.clone())),
    )));
    let vca = Gain::new(AudioParam::Dynamic(Box::new(amp_env_vel)));

    DspChain::new(mixer, sample_rate).and(filter_node).and(vca)
}
