//! My Synth's sound: one voice, the last key held sounding. The engine
//! sees only the bridge (`silkscreen::bridge`): it runs on the audio
//! thread, or in an `AudioWorklet`, so it never allocates or locks.

use silkscreen::ParamIndex;
use silkscreen::bridge::{Engine, NoteEvent};

use crate::{LEVEL, WAVE};

/// How long the level and the note's gate take to settle, so steps don't
/// click.
const GLIDE_MS: f32 = 5.0;

/// One voice. Build it with [`MyEngine::new`] at the host's sample rate.
#[derive(Debug)]
pub struct MyEngine {
    /// A one-pole step towards the gate and level targets, per sample.
    coeff: f32,
    sample_rate: f32,
    phase: f32,
    /// Phase step of the sounding key, kept through the release.
    step: f32,
    key: Option<u8>,
    velocity: f32,
    gate: f32,
    level: f32,
    level_target: f32,
    wave: usize,
}

impl MyEngine {
    pub fn new(sample_rate: f32) -> Self {
        let samples = (GLIDE_MS * 0.001 * sample_rate).max(1.0);
        Self {
            coeff: 1.0 - (-1.0 / samples).exp(),
            sample_rate,
            phase: 0.0,
            step: 0.0,
            key: None,
            velocity: 0.0,
            gate: 0.0,
            level: 0.0,
            level_target: 0.0,
            wave: 0,
        }
    }

    /// The key sounding, if any.
    pub fn key(&self) -> Option<u8> {
        self.key
    }
}

impl Engine for MyEngine {
    fn set_param(&mut self, index: ParamIndex, value: f32) {
        match index {
            // Stepped params arrive as whole numbers: the position.
            WAVE => self.wave = value as usize,
            LEVEL => self.level_target = 10f32.powf(value / 20.0),
            _ => {}
        }
    }

    fn note(&mut self, event: NoteEvent) {
        match event {
            NoteEvent::On { key, velocity, .. } => {
                self.key = Some(key);
                self.velocity = velocity;
                self.step = 440.0 * ((f32::from(key) - 69.0) / 12.0).exp2() / self.sample_rate;
            }
            NoteEvent::Off { key, .. } if self.key == Some(key) => self.key = None,
            _ => {}
        }
    }

    fn process(&mut self, channels: &mut [&mut [f32]]) {
        let frames = channels.first().map_or(0, |c| c.len());
        let gate_target = if self.key.is_some() {
            self.velocity
        } else {
            0.0
        };
        for i in 0..frames {
            self.gate += self.coeff * (gate_target - self.gate);
            self.level += self.coeff * (self.level_target - self.level);
            self.phase = (self.phase + self.step).fract();
            let shape = match self.wave {
                0 => (self.phase * std::f32::consts::TAU).sin(),
                1 => 1.0 - 4.0 * (self.phase - 0.5).abs(),
                _ if self.phase < 0.5 => 1.0,
                _ => -1.0,
            };
            let s = 0.25 * shape * self.gate * self.level;
            for ch in channels.iter_mut() {
                ch[i] = s;
            }
        }
    }

    fn reset(&mut self) {
        self.key = None;
        self.gate = 0.0;
        self.level = self.level_target;
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

    #[test]
    fn a_held_key_sounds_and_its_release_falls_silent() {
        let mut e = MyEngine::new(48_000.0);
        e.set_param(LEVEL, 0.0);
        e.reset();
        e.note(on(69));
        let mut buf = vec![0.0; 4800];
        e.process(&mut [&mut buf]);
        assert!(buf.iter().any(|s| s.abs() > 0.1), "the note sounds");
        e.note(NoteEvent::Off {
            channel: 0,
            key: 69,
            velocity: 0.0,
        });
        e.process(&mut [&mut buf]);
        assert!(buf[4000..].iter().all(|s| s.abs() < 1e-3), "and stops");
    }
}
