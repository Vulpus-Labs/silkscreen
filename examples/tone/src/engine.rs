//! The tone example's DSP: one voice with last-note priority. Depends
//! only on the bridge's `Engine` seam and the gain example's smoother.

use gain::engine::db_to_linear;
use gain::smooth::Smoothed;
use silkscreen::ParamIndex;
use silkscreen::bridge::{Engine, NoteEvent};

use crate::held::HeldKeys;
use crate::{LEVEL, OCTAVE, WAVE};

/// Peak level at full velocity and 0 dB.
const PEAK: f32 = 0.25;
/// Envelope and level smoothing, so notes and knob moves don't click.
const SMOOTHING_MS: f32 = 5.0;
/// Pitch bend range, each way.
const BEND_SEMITONES: f32 = 2.0;

/// One sine voice with last-note priority. Never allocates.
#[derive(Debug)]
pub struct ToneEngine {
    sample_rate: f32,
    phase: f32,
    held: HeldKeys,
    velocity: f32,
    bend: f32,
    level: Smoothed,
    env: Smoothed,
    /// `WAVE` position: 0 sine, 1 triangle, 2 square.
    wave: usize,
    octave_down: bool,
    /// Phase step of the last sounding key, kept through the release.
    step: f32,
}

impl ToneEngine {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            phase: 0.0,
            held: HeldKeys::default(),
            velocity: 0.0,
            bend: 0.0,
            level: Smoothed::new(1.0, SMOOTHING_MS, sample_rate),
            env: Smoothed::new(0.0, SMOOTHING_MS, sample_rate),
            step: 0.0,
            wave: 0,
            octave_down: false,
        }
    }

    /// The key sounding, if any.
    pub fn key(&self) -> Option<u8> {
        self.held.any().then(|| self.held.sounding())
    }

    /// Frequency of the sounding key with the current bend.
    pub fn frequency(&self) -> Option<f32> {
        let key = f32::from(self.key()?);
        let octave = if self.octave_down { -12.0 } else { 0.0 };
        Some(440.0 * ((key - 69.0 + octave + self.bend * BEND_SEMITONES) / 12.0).exp2())
    }
}

impl Engine for ToneEngine {
    fn set_param(&mut self, index: ParamIndex, value: f32) {
        match index {
            LEVEL => self.level.set_target(db_to_linear(value)),
            // Discrete values arrive as exact whole numbers (design/model.md).
            WAVE => self.wave = value as usize,
            OCTAVE => self.octave_down = value >= 0.5,
            _ => {}
        }
    }

    fn note(&mut self, event: NoteEvent) {
        match event {
            NoteEvent::On { key, velocity, .. } => {
                self.held.down(key);
                self.velocity = velocity;
            }
            NoteEvent::Off { key, .. } => self.held.up(key),
            NoteEvent::PitchBend { value, .. } => self.bend = value,
            NoteEvent::Midi { .. } => {}
        }
    }

    fn process(&mut self, channels: &mut [&mut [f32]]) {
        let frames = channels.first().map_or(0, |c| c.len());
        let gate = if self.held.any() { self.velocity } else { 0.0 };
        self.env.set_target(gate);
        if let Some(f) = self.frequency() {
            self.step = f / self.sample_rate;
        }
        for i in 0..frames {
            let (env, level) = (self.env.tick(), self.level.tick());
            self.phase = (self.phase + self.step).fract();
            let shape = match self.wave {
                0 => (self.phase * std::f32::consts::TAU).sin(),
                1 => 1.0 - 4.0 * (self.phase - 0.5).abs(),
                _ => {
                    if self.phase < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
            };
            let s = shape * env * level * PEAK;
            for ch in channels.iter_mut() {
                ch[i] = s;
            }
        }
    }

    fn reset(&mut self) {
        self.level.snap(self.level.target());
        self.env.snap(0.0);
        self.held.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on(key: u8) -> NoteEvent {
        NoteEvent::On {
            channel: 0,
            key,
            velocity: 1.0,
        }
    }

    fn off(key: u8) -> NoteEvent {
        NoteEvent::Off {
            channel: 0,
            key,
            velocity: 0.0,
        }
    }

    #[test]
    fn last_note_priority() {
        let mut e = ToneEngine::new(48_000.0);
        e.note(on(60));
        e.note(on(64));
        assert_eq!(e.key(), Some(64));
        e.note(off(64));
        assert_eq!(e.key(), Some(60), "falls back to the held note");
        e.note(off(60));
        assert_eq!(e.key(), None);
    }

    #[test]
    fn a_full_stack_drops_the_oldest() {
        let mut e = ToneEngine::new(48_000.0);
        for k in 0..=HeldKeys::CAPACITY as u8 {
            e.note(on(k));
        }
        e.note(off(HeldKeys::CAPACITY as u8));
        assert_eq!(e.key(), Some(HeldKeys::CAPACITY as u8 - 1));
    }

    #[test]
    fn octave_down_halves_the_pitch() {
        let mut e = ToneEngine::new(48_000.0);
        e.note(on(69));
        e.set_param(OCTAVE, 1.0);
        assert_eq!(e.frequency(), Some(220.0));
    }

    #[test]
    fn each_wave_shape_peaks_at_full_scale() {
        for wave in 0..3 {
            let mut e = ToneEngine::new(48_000.0);
            e.set_param(WAVE, wave as f32);
            e.note(on(69));
            let mut out = vec![0.0f32; 4800];
            e.process(&mut [&mut out]);
            let peak = out[2400..].iter().fold(0f32, |m, s| m.max(s.abs()));
            assert!((peak - PEAK).abs() < 0.01, "wave {wave}: {peak}");
        }
    }

    #[test]
    fn bend_moves_pitch_two_semitones() {
        let mut e = ToneEngine::new(48_000.0);
        e.note(on(69));
        assert_eq!(e.frequency(), Some(440.0));
        e.note(NoteEvent::PitchBend {
            channel: 0,
            value: 1.0,
            raw: 16383,
        });
        let up = e.frequency().unwrap();
        assert!((up - 440.0 * (2f32 / 12.0).exp2()).abs() < 1e-3, "{up}");
    }
}
