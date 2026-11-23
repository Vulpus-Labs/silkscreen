//! A one-pole parameter smoother, so a knob's steps don't click. Also used
//! by the tone example.

/// A value that glides towards its target, one sample at a time.
#[derive(Clone, Copy, Debug)]
pub struct Smoothed {
    current: f32,
    target: f32,
    /// The share of the remaining distance covered each sample.
    coeff: f32,
}

/// Closer than this, the glide lands on its target rather than creeping
/// towards it for ever.
const SNAP: f32 = 1.0e-6;

impl Smoothed {
    /// Settled at `initial`, gliding with a time constant of `ms`.
    pub fn new(initial: f32, ms: f32, sample_rate: f32) -> Self {
        let samples = (ms * 0.001 * sample_rate).max(1.0);
        Self {
            current: initial,
            target: initial,
            coeff: 1.0 - (-1.0 / samples).exp(),
        }
    }

    /// Where to glide to.
    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Where it is gliding to.
    pub fn target(&self) -> f32 {
        self.target
    }

    /// Jump to `value` at once.
    pub fn snap(&mut self, value: f32) {
        self.current = value;
        self.target = value;
    }

    /// One sample's step; the new value.
    #[inline]
    pub fn tick(&mut self) -> f32 {
        self.current += self.coeff * (self.target - self.current);
        if (self.target - self.current).abs() < SNAP {
            self.current = self.target;
        }
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settles_within_five_time_constants() {
        let mut s = Smoothed::new(0.0, 5.0, 48_000.0);
        s.set_target(1.0);
        let mut v = 0.0;
        for _ in 0..1200 {
            v = s.tick();
        }
        assert!((v - 1.0).abs() < 0.01, "{v}");
    }

    #[test]
    fn lands_exactly() {
        let mut s = Smoothed::new(1.0, 20.0, 1500.0);
        s.set_target(0.0);
        assert!((0..10_000).any(|_| s.tick() == 0.0));
    }

    #[test]
    fn snap_is_immediate() {
        let mut s = Smoothed::new(0.0, 100.0, 48_000.0);
        s.snap(0.5);
        assert_eq!(s.tick(), 0.5);
    }
}
