//! Telemetry: the output, audio → main, for scopes and meters ([design/engine-bridge.md]).
//!
//! One block of [`SharedWords`], so it lives in native atomics
//! ([`telemetry`]) or in the web transport's `SharedArrayBuffer`
//! ([`crate::sab`]) with the same code. Layout, in words from the block's
//! base:
//!
//! ```text
//! word   field
//!    0   written        samples written, mod 2^32           (tap)
//!    1   channels       output channels metered             (tap)
//!    2   peaks          2 × f32 bits: tap raises, reader takes
//!    4   engine peaks   ENGINE_METERS × 2 × f32 bits, likewise
//!   12   values         ENGINE_VALUES × f32 bits: the engine's latest
//!   28   scope          SCOPE_CAPACITY × f32 bits: the mono sum, a ring
//! ```
//!
//! **Single-writer discipline:** the tap writes the scope samples, then
//! `written` (release); the reader loads `written` (acquire), then the
//! samples. Peaks are raised with `fetch_max` on the bits, which orders as
//! the value for non-negative floats, and taken with `swap(0)`. Values
//! are stored whole and read whole: each is the latest the engine wrote. Each
//! sample is whole, but a reader copying the scope while the tap writes
//! may mix in what was written during the copy; the next frame replaces
//! it.
//!
//! [design/engine-bridge.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/engine-bridge.md

use std::sync::Arc;
use std::sync::atomic::AtomicU32;

use crate::words::{ReaderWords, SharedWords};

/// Samples the scope ring keeps: a window and the search for its trigger.
pub const SCOPE_CAPACITY: usize = 4096;
const MASK: u32 = SCOPE_CAPACITY as u32 - 1;

/// Output channels metered.
pub const MAX_CHANNELS: usize = 2;

/// Stereo meters an engine may report from inside itself (design/engine-bridge.md).
pub const ENGINE_METERS: usize = 4;

/// Values an engine may report for its editor to show (design/engine-bridge.md): an
/// envelope's stage, an LFO's phase, how many voices sound.
pub const ENGINE_VALUES: usize = 16;

const WRITTEN_AT: usize = 0;
const CHANNELS_AT: usize = 1;
const PEAKS_AT: usize = 2;
const ENGINE_PEAKS_AT: usize = PEAKS_AT + MAX_CHANNELS;
const VALUES_AT: usize = ENGINE_PEAKS_AT + ENGINE_METERS * 2;
const SCOPE_AT: usize = VALUES_AT + ENGINE_VALUES;

/// Size of a telemetry block, in words.
pub const TELEMETRY_WORDS: usize = SCOPE_AT + SCOPE_CAPACITY;

/// Create a connected pair over one block of native atomics.
pub fn telemetry() -> (TelemetryTap, TelemetryReader) {
    let words: Arc<[AtomicU32]> = (0..TELEMETRY_WORDS).map(|_| AtomicU32::new(0)).collect();
    (
        TelemetryTap::over(Arc::clone(&words), 0),
        TelemetryReader::over(Box::new(words), 0),
    )
}

/// The audio thread's end. Never allocates, locks or blocks.
#[derive(Debug)]
pub struct TelemetryTap<W = Arc<[AtomicU32]>> {
    words: W,
    base: usize,
    /// Samples written: this end is the only writer, so it keeps its own.
    written: u32,
}

impl<W: SharedWords> TelemetryTap<W> {
    /// The tap over the block at `base` in `words` ([`TELEMETRY_WORDS`]
    /// long from there).
    pub fn over(words: W, base: usize) -> Self {
        let written = words.load(base + WRITTEN_AT);
        Self {
            words,
            base,
            written,
        }
    }

    /// Record one block of output: the mono sum into the scope, each of
    /// the first [`MAX_CHANNELS`] channels' peak. Channels are read up to
    /// the shortest's length.
    pub fn write<C: AsRef<[f32]>>(&mut self, channels: &[C]) {
        self.write_output(channels, true);
    }

    /// The output's peaks, and its mono sum into the scope if `scope`.
    pub(crate) fn write_output<C: AsRef<[f32]>>(&mut self, channels: &[C], scope: bool) {
        let channels = &channels[..channels.len().min(MAX_CHANNELS)];
        let Some(frames) = channels.iter().map(|c| c.as_ref().len()).min() else {
            return;
        };
        let (w, base) = (&self.words, self.base);
        if scope {
            let share = 1.0 / channels.len() as f32;
            for i in 0..frames {
                let sum: f32 = channels.iter().map(|c| c.as_ref()[i]).sum();
                let slot = (self.written.wrapping_add(i as u32) & MASK) as usize;
                w.store(base + SCOPE_AT + slot, (sum * share).to_bits());
            }
            self.written = self.written.wrapping_add(frames as u32);
            w.store(base + WRITTEN_AT, self.written);
        }
        for (c, channel) in channels.iter().enumerate() {
            let level = channel.as_ref()[..frames]
                .iter()
                .fold(0.0_f32, |m, s| m.max(s.abs()));
            w.fetch_max(base + PEAKS_AT + c, level.to_bits());
        }
        w.store(base + CHANNELS_AT, channels.len() as u32);
    }

    /// Append `samples` (mono) to the scope: an engine's own scope source
    /// ([design/engine-bridge.md]) in place of the output.
    ///
    /// [design/engine-bridge.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/engine-bridge.md
    pub fn write_scope(&mut self, samples: &[f32]) {
        let (w, base) = (&self.words, self.base);
        for (i, s) in samples.iter().enumerate() {
            let slot = (self.written.wrapping_add(i as u32) & MASK) as usize;
            w.store(base + SCOPE_AT + slot, s.to_bits());
        }
        self.written = self.written.wrapping_add(samples.len() as u32);
        w.store(base + WRITTEN_AT, self.written);
    }

    /// Raise engine meter `n`'s peaks to `levels` (magnitudes). Meters
    /// past [`ENGINE_METERS`] are ignored.
    pub fn write_engine_peak(&mut self, n: usize, levels: [f32; 2]) {
        if n >= ENGINE_METERS {
            return;
        }
        for (c, level) in levels.into_iter().enumerate() {
            self.words.fetch_max(
                self.base + ENGINE_PEAKS_AT + n * 2 + c,
                level.abs().to_bits(),
            );
        }
    }
}

impl<W: SharedWords> TelemetryTap<W> {
    /// Set engine value `n` to `value`. Values past [`ENGINE_VALUES`] are
    /// ignored.
    pub fn write_engine_value(&mut self, n: usize, value: f32) {
        if n < ENGINE_VALUES {
            self.words.store(self.base + VALUES_AT + n, value.to_bits());
        }
    }
}

/// Each channel's peak since the last take.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Peaks {
    /// Channels the tap has seen: 0 before any output.
    pub channels: usize,
    /// Linear, `|sample|`; channels past `channels` are 0.
    pub levels: [f32; MAX_CHANNELS],
}

/// The main thread's end.
pub struct TelemetryReader {
    words: Box<dyn ReaderWords>,
    base: usize,
    /// `written` as of the last [`scope`](Self::scope).
    seen: u32,
}

impl std::fmt::Debug for TelemetryReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TelemetryReader")
            .field("base", &self.base)
            .field("seen", &self.seen)
            .finish_non_exhaustive()
    }
}

impl TelemetryReader {
    /// The reader of the block at `base` in `words`.
    pub fn over(words: Box<dyn ReaderWords>, base: usize) -> Self {
        Self {
            words,
            base,
            seen: 0,
        }
    }

    /// Samples written so far, mod 2^32.
    pub fn written(&self) -> u64 {
        u64::from(self.words.load(self.base + WRITTEN_AT))
    }

    /// Copy the latest [`SCOPE_CAPACITY`] samples into `out`, oldest
    /// first (zeros before any output). Returns whether any arrived since
    /// the last copy.
    pub fn scope(&mut self, out: &mut [f32; SCOPE_CAPACITY]) -> bool {
        let written = self.words.load(self.base + WRITTEN_AT);
        let mut ring = [0u32; SCOPE_CAPACITY];
        self.words.load_range(self.base + SCOPE_AT, &mut ring);
        for (i, o) in out.iter_mut().enumerate() {
            *o = f32::from_bits(ring[(written.wrapping_add(i as u32) & MASK) as usize]);
        }
        let fresh = written != self.seen;
        self.seen = written;
        fresh
    }

    /// Take engine meter `n`'s peaks since the last take, leaving zero.
    /// Meters past [`ENGINE_METERS`] read silent.
    pub fn take_engine_peaks(&self, n: usize) -> [f32; 2] {
        if n >= ENGINE_METERS {
            return [0.0; 2];
        }
        std::array::from_fn(|c| {
            f32::from_bits(self.words.swap(self.base + ENGINE_PEAKS_AT + n * 2 + c, 0))
        })
    }

    /// Every engine value, as last written.
    pub fn engine_values(&self, out: &mut [f32; ENGINE_VALUES]) {
        let mut bits = [0u32; ENGINE_VALUES];
        self.words.load_range(self.base + VALUES_AT, &mut bits);
        for (o, b) in out.iter_mut().zip(bits) {
            *o = f32::from_bits(b);
        }
    }

    /// Take each channel's peak since the last take, leaving zero.
    pub fn take_peaks(&self) -> Peaks {
        let channels = (self.words.load(self.base + CHANNELS_AT) as usize).min(MAX_CHANNELS);
        let mut levels = [0.0; MAX_CHANNELS];
        for (c, level) in levels.iter_mut().enumerate().take(channels) {
            *level = f32::from_bits(self.words.swap(self.base + PEAKS_AT + c, 0));
        }
        Peaks { channels, levels }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_values_read_as_last_written() {
        let (mut tap, reader) = telemetry();
        tap.write_engine_value(0, 0.25);
        tap.write_engine_value(3, -1.5);
        tap.write_engine_value(0, 0.5);
        tap.write_engine_value(ENGINE_VALUES, 9.0); // ignored
        let mut out = [f32::NAN; ENGINE_VALUES];
        reader.engine_values(&mut out);
        assert_eq!((out[0], out[3], out[1]), (0.5, -1.5, 0.0));
    }

    #[test]
    fn the_scope_holds_the_mono_sum_oldest_first() {
        let (mut tap, mut reader) = telemetry();
        let mut out = [9.0; SCOPE_CAPACITY];
        assert!(!reader.scope(&mut out), "nothing written yet");
        assert!(out.iter().all(|&s| s == 0.0));
        tap.write(&[[1.0, 3.0], [3.0, 5.0]]);
        assert!(reader.scope(&mut out));
        assert_eq!(&out[SCOPE_CAPACITY - 2..], &[2.0, 4.0]);
        assert!(!reader.scope(&mut out), "nothing new");
    }

    #[test]
    fn the_ring_wraps() {
        let (mut tap, mut reader) = telemetry();
        let ramp: Vec<f32> = (0..SCOPE_CAPACITY + 10).map(|i| i as f32).collect();
        tap.write(&[&ramp[..]]);
        let mut out = [0.0; SCOPE_CAPACITY];
        reader.scope(&mut out);
        assert_eq!(out[0], 10.0);
        assert_eq!(out[SCOPE_CAPACITY - 1], (SCOPE_CAPACITY + 9) as f32);
    }

    #[test]
    fn peaks_are_the_largest_magnitude_since_the_last_take() {
        let (mut tap, reader) = telemetry();
        assert_eq!(reader.take_peaks(), Peaks::default());
        tap.write(&[[0.5, -0.9], [0.1, 0.2]]);
        tap.write(&[[0.3, 0.3], [-0.4, 0.0]]);
        let p = reader.take_peaks();
        assert_eq!(p.channels, 2);
        assert_eq!(p.levels, [0.9, 0.4]);
        assert_eq!(reader.take_peaks().levels, [0.0, 0.0], "taken");
    }

    #[test]
    fn engine_meters_are_raised_and_taken_on_their_own() {
        let (mut tap, reader) = telemetry();
        tap.write_engine_peak(1, [0.5, -0.25]);
        tap.write_engine_peak(1, [0.2, 0.75]);
        tap.write_engine_peak(ENGINE_METERS, [1.0, 1.0]);
        assert_eq!(reader.take_engine_peaks(0), [0.0, 0.0]);
        assert_eq!(reader.take_engine_peaks(1), [0.5, 0.75]);
        assert_eq!(reader.take_engine_peaks(1), [0.0, 0.0], "taken");
        assert_eq!(reader.take_engine_peaks(ENGINE_METERS), [0.0, 0.0]);
        assert_eq!(reader.take_peaks().channels, 0, "the output is apart");
    }

    #[test]
    fn a_mono_output_meters_one_channel() {
        let (mut tap, reader) = telemetry();
        tap.write(&[[0.25, -0.5]]);
        assert_eq!(
            reader.take_peaks(),
            Peaks {
                channels: 1,
                levels: [0.5, 0.0]
            }
        );
    }

    #[test]
    fn a_writer_at_full_speed_never_tears_the_count_or_loses_a_peak() {
        const BLOCKS: usize = 20_000;
        const FRAMES: usize = 64;
        let (mut tap, mut reader) = telemetry();
        let writer = std::thread::spawn(move || {
            let mut next = 0.0_f32;
            let mut left = [0.0; FRAMES];
            for b in 0..BLOCKS {
                for s in &mut left {
                    // A ramp: consecutive samples differ by one.
                    *s = next;
                    next += 1.0;
                }
                // One spike of 1e7 in one block, on the right channel.
                let right = if b == BLOCKS / 2 {
                    [1e7; FRAMES]
                } else {
                    [0.5; FRAMES]
                };
                tap.write(&[&left[..], &right[..]]);
            }
        });
        let mut out = [0.0; SCOPE_CAPACITY];
        let mut last = 0;
        let mut spike = false;
        while !writer.is_finished() {
            let written = reader.written();
            assert!(written >= last && written % FRAMES as u64 == 0, "{written}");
            last = written;
            reader.scope(&mut out);
            // A copy racing the writer may mix stretches, but every
            // sample is one the tap wrote, (k + 0.5) / 2 or (k + 1e7) / 2,
            // or the ring's initial zero.
            for &v in &out {
                let k = 2.0 * v - 0.5;
                assert!(k == k.trunc() || v == 0.0 || v >= 5e6, "a torn sample: {v}");
            }
            spike |= reader.take_peaks().levels[1] == 1e7;
        }
        writer.join().unwrap();
        spike |= reader.take_peaks().levels[1] == 1e7;
        assert!(spike, "the spike's peak was taken");
        assert_eq!(reader.written(), (BLOCKS * FRAMES) as u64);
        // At rest, a copy is the latest stretch in order: the ramp steps by ½.
        reader.scope(&mut out);
        assert!(out.windows(2).all(|w| w[1] - w[0] == 0.5));
    }
}
