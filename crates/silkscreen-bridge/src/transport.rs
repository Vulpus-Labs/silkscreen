//! Host transport as the engine sees it, once per block.

/// The host's transport at the start of a block. Hosts that supply none,
/// or omit a field, get [`Transport::default`]'s value for it: 120 BPM,
/// stopped, beat 0, 4/4.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transport {
    /// Beats per minute.
    pub tempo: f64,
    pub playing: bool,
    /// Song position in beats (quarter notes) at the block's first sample.
    pub beats: f64,
    /// Time signature as (numerator, denominator).
    pub time_signature: (u16, u16),
}

impl Transport {
    pub const FALLBACK_TEMPO: f64 = 120.0;
}

impl Default for Transport {
    fn default() -> Self {
        Self {
            tempo: Self::FALLBACK_TEMPO,
            playing: false,
            beats: 0.0,
            time_signature: (4, 4),
        }
    }
}
