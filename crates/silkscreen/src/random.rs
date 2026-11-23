//! Randomising a sound (design/presets.md): a small seeded generator,
//! wasm-safe (no clock or OS entropy: the view seeds it from the time
//! it's given), and what a plugin's randomiser hands back.

use silkscreen_model::ParamSpec;

pub mod names;

/// A sound a plugin's randomiser built (design/presets.md): every param's
/// value in spec order, its name, the user folder Save As offers, and a
/// comment ("Random Pad"), to which the editor adds the seed.
#[derive(Clone, Debug, PartialEq)]
pub struct RandomSound {
    pub values: Vec<f32>,
    pub name: String,
    pub folder: Option<String>,
    pub comment: Option<String>,
}

/// A random sound's seed (design/presets.md): 25 bits, written as five
/// Crockford base-32 characters ("3F2AK") in the sound's comment, so a
/// sound can be made again from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Seed(u32);

/// Crockford's base 32: no I, L, O or U.
const DIGITS: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

impl Seed {
    pub const LEN: usize = 5;
    const MASK: u64 = (1 << (5 * Self::LEN)) - 1;

    /// The seed in `x`'s low 25 bits.
    pub fn from_bits(x: u64) -> Self {
        Self((x & Self::MASK) as u32)
    }

    pub fn bits(self) -> u32 {
        self.0
    }

    /// The generator this seed's sound comes from.
    pub fn rng(self) -> Rng {
        Rng::new(u64::from(self.0))
    }

    /// Five base-32 characters, in either case; O reads as 0 and I or L
    /// as 1, as Crockford's alphabet allows.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.len() != Self::LEN {
            return None;
        }
        text.bytes()
            .try_fold(0u32, |n, c| {
                let c = match c.to_ascii_uppercase() {
                    b'O' => b'0',
                    b'I' | b'L' => b'1',
                    c => c,
                };
                let d = DIGITS.iter().position(|&x| x == c)?;
                Some(n << 5 | d as u32)
            })
            .map(Self)
    }
}

impl std::fmt::Display for Seed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s: String = (0..Self::LEN)
            .rev()
            .map(|i| DIGITS[(self.0 >> (5 * i) & 31) as usize] as char)
            .collect();
        f.write_str(&s)
    }
}

/// Every param uniformly random over its range, or on one of its
/// positions: the editor's sound when the plugin has no
/// randomiser.
pub fn uniform(rng: &mut Rng, specs: &[ParamSpec]) -> Vec<f32> {
    specs
        .iter()
        .map(|s| {
            let positions = s.position_labels().len();
            if positions > 0 {
                s.clamp(s.min + rng.below(positions) as f32)
            } else {
                s.denormalize(rng.unit())
            }
        })
        .collect()
}

/// xorshift64*: fast, small, and good enough for patches. The editor
/// depends on no engine crate, so an engine that wants the same generator
/// (for noise, say) keeps its own copy.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    /// A generator from `seed` (any value; zero is moved off).
    pub fn new(seed: u64) -> Self {
        // splitmix64 once, so nearby seeds diverge at once.
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        Self((z ^ (z >> 31)).max(1))
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `[0, 1)`.
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Uniform in `0..n` (0 if `n` is 0).
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }

    /// Uniform in `[lo, hi)`.
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }

    /// True with probability `p`.
    pub fn chance(&mut self, p: f32) -> bool {
        self.unit() < p
    }

    /// One of `choices`, by weight (any non-negative weights; the last
    /// weighted one if rounding leaves the draw past them all, so a
    /// zero weight is never drawn).
    ///
    /// # Panics
    /// If `choices` is empty.
    pub fn pick<T: Copy>(&mut self, choices: &[(T, f32)]) -> T {
        let total: f32 = choices.iter().map(|c| c.1).sum();
        let mut x = self.unit() * total;
        for &(c, w) in choices {
            if x < w {
                return c;
            }
            x -= w;
        }
        choices
            .iter()
            .rev()
            .find(|c| c.1 > 0.0)
            .unwrap_or(&choices[choices.len() - 1])
            .0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_weighted_pick_never_draws_a_zero_weight() {
        let mut rng = Rng::new(7);
        for _ in 0..10_000 {
            assert_ne!(rng.pick(&[('a', 1.0), ('b', 3.0), ('z', 0.0)]), 'z');
        }
        // And the weights hold: about three b's to every a.
        let bs = (0..10_000)
            .filter(|_| rng.pick(&[('a', 1.0), ('b', 3.0)]) == 'b')
            .count();
        assert!((7_000..8_000).contains(&bs), "{bs}");
    }

    #[test]
    fn seeds_round_trip_through_five_characters() {
        for bits in [0, 1, 31, 32, 0x1ff_ffff, 0x00ab_cdef] {
            let seed = Seed::from_bits(bits);
            let text = seed.to_string();
            assert_eq!(text.len(), Seed::LEN);
            assert_eq!(Seed::parse(&text), Some(seed), "{text}");
            assert_eq!(Seed::parse(&text.to_lowercase()), Some(seed));
        }
        assert_eq!(Seed::from_bits(0).to_string(), "00000");
        assert_eq!(Seed::from_bits(0x1ff_ffff).to_string(), "ZZZZZ");
        assert_eq!(Seed::parse("oIl00"), Seed::parse("01100"));
        for bad in ["", "0000", "000000", "0000U", "00!00"] {
            assert_eq!(Seed::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn it_is_seeded_and_spread() {
        let a: Vec<u64> = (0..4).map(|_| Rng::new(7).next_u64()).collect();
        assert!(
            a.windows(2).all(|w| w[0] == w[1]),
            "same seed, same numbers"
        );
        let mut r = Rng::new(1);
        let mut counts = [0usize; 4];
        for _ in 0..4000 {
            let u = r.unit();
            assert!((0.0..1.0).contains(&u));
            counts[r.below(4)] += 1;
        }
        assert!(
            counts.iter().all(|&c| (800..1200).contains(&c)),
            "{counts:?}"
        );
        assert_ne!(Rng::new(1).next_u64(), Rng::new(2).next_u64());
    }
}
