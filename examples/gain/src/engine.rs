//! The gain example's DSP. Depends only on the bridge's `Engine` seam.

use crate::smooth::Smoothed;
use silkscreen::ParamIndex;
use silkscreen::bridge::Engine;

use crate::GAIN;

/// Smoothing time for gain changes, so knob moves don't click.
const SMOOTHING_MS: f32 = 20.0;

#[derive(Debug)]
pub struct GainEngine {
    gain: Smoothed,
}

impl GainEngine {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            gain: Smoothed::new(1.0, SMOOTHING_MS, sample_rate),
        }
    }

    /// The linear gain being approached.
    pub fn target(&self) -> f32 {
        self.gain.target()
    }
}

pub fn db_to_linear(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

impl Engine for GainEngine {
    fn set_param(&mut self, index: ParamIndex, value: f32) {
        if index == GAIN {
            self.gain.set_target(db_to_linear(value));
        }
    }

    fn process(&mut self, channels: &mut [&mut [f32]]) {
        let frames = channels.first().map_or(0, |c| c.len());
        for i in 0..frames {
            let gain = self.gain.tick();
            for ch in channels.iter_mut() {
                ch[i] *= gain;
            }
        }
    }

    fn reset(&mut self) {
        self.gain.snap(self.gain.target());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converges_on_the_target_gain() {
        let mut e = GainEngine::new(48_000.0);
        e.set_param(GAIN, -6.0);
        let mut l = vec![1.0f32; 48_000];
        let mut r = vec![1.0f32; 48_000];
        e.process(&mut [&mut l, &mut r]);
        let expected = db_to_linear(-6.0);
        assert!((l[47_999] - expected).abs() < 1e-4, "{}", l[47_999]);
        assert_eq!(l, r);
        // Smoothed: the first sample has barely moved from unity.
        assert!(l[0] > 0.99);
    }

    #[test]
    fn ignores_unknown_params() {
        let mut e = GainEngine::new(48_000.0);
        e.set_param(ParamIndex(7), -60.0);
        assert_eq!(e.target(), 1.0);
    }
}
