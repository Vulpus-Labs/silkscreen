//! The audio callback's work, apart from any device: apply the UI's
//! values, feed the engine its input, render, record telemetry, and lay
//! the result out in the device's channels. Tested without a device.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use silkscreen_bridge::{DenormalGuard, Engine, EngineEnd, EngineSide};

/// Frames rendered per engine call. A device buffer of any size is worked
/// through in chunks of at most this many, so the scratch buffers are
/// allocated once.
pub const CHUNK: usize = 512;

/// The test tone's pitch and level: a quiet 220 Hz sawtooth.
const TONE_HZ: f32 = 220.0;
const TONE_LEVEL: f32 = 0.15;

/// What feeds the engine's input.
#[derive(Debug)]
pub enum Feed {
    /// Zeros: an instrument, or an effect left silent.
    Silence,
    /// A quiet sawtooth, for hearing an effect without a microphone.
    TestTone,
    /// Stereo frames from the input device's stream. A frame missing when
    /// it's needed repeats the last one.
    Input(rtrb::Consumer<[f32; 2]>),
}

/// Drives an engine from the audio callback. No method after
/// [`new`](Self::new) allocates, locks or blocks.
#[derive(Debug)]
pub struct Runner<E> {
    engine: E,
    end: EngineEnd,
    feed: Feed,
    last_input: [f32; 2],
    tone_phase: f32,
    tone_step: f32,
    left: Box<[f32]>,
    right: Box<[f32]>,
    /// Set if the engine panicked; it renders silence from then on.
    failed: Arc<AtomicBool>,
}

impl<E: Engine> Runner<E> {
    /// Seed `engine` from the bridge (a pending load's values, if one is
    /// waiting) and reset it, as a plugin host does at activation.
    pub fn new(mut engine: E, end: EngineEnd, feed: Feed, sample_rate: f32) -> Self {
        end.seed_engine(&mut engine);
        Self {
            engine,
            end,
            feed,
            last_input: [0.0; 2],
            tone_phase: 0.0,
            tone_step: TONE_HZ / sample_rate,
            left: vec![0.0; CHUNK].into_boxed_slice(),
            right: vec![0.0; CHUNK].into_boxed_slice(),
            failed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Set once the engine has panicked, for the app to report.
    pub fn failed(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.failed)
    }

    /// Fill one device buffer: `data` interleaved over `channels`. Stereo
    /// goes to the first two channels and silence to any others; a mono
    /// device gets the average.
    pub fn fill<T>(&mut self, data: &mut [T], channels: usize)
    where
        T: cpal::SizedSample + cpal::FromSample<f32>,
    {
        let _guard = DenormalGuard::new();
        if self.failed.load(Ordering::Relaxed) {
            data.fill(T::EQUILIBRIUM);
            return;
        }
        // An unwind out of the callback would abort the app; render silence
        // from here on instead.
        let ok = catch_unwind(AssertUnwindSafe(|| self.render(data, channels))).is_ok();
        if !ok {
            self.failed.store(true, Ordering::Relaxed);
            data.fill(T::EQUILIBRIUM);
        }
    }

    fn render<T>(&mut self, data: &mut [T], channels: usize)
    where
        T: cpal::SizedSample + cpal::FromSample<f32>,
    {
        // A standalone app has no host to record gestures for.
        self.end.apply_ui(&mut self.engine);
        let channels = channels.max(1);
        let frames = data.len() / channels;
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(CHUNK);
            self.feed_input(n);
            let (left, right) = (&mut self.left[..n], &mut self.right[..n]);
            self.engine.process(&mut [&mut *left, &mut *right]);
            self.end
                .telemetry()
                .write_block(&mut self.engine, &[&*left, &*right]);
            for (k, frame) in data[done * channels..(done + n) * channels]
                .chunks_exact_mut(channels)
                .enumerate()
            {
                if channels == 1 {
                    frame[0] = T::from_sample(0.5 * (left[k] + right[k]));
                } else {
                    frame[0] = T::from_sample(left[k]);
                    frame[1] = T::from_sample(right[k]);
                    for c in &mut frame[2..] {
                        *c = T::EQUILIBRIUM;
                    }
                }
            }
            done += n;
        }
    }

    /// The engine's input for the next `n` frames, into the scratch.
    fn feed_input(&mut self, n: usize) {
        let (left, right) = (&mut self.left[..n], &mut self.right[..n]);
        match &mut self.feed {
            Feed::Silence => {
                left.fill(0.0);
                right.fill(0.0);
            }
            Feed::TestTone => {
                for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                    let s = TONE_LEVEL * (2.0 * self.tone_phase - 1.0);
                    self.tone_phase = (self.tone_phase + self.tone_step).fract();
                    (*l, *r) = (s, s);
                }
            }
            Feed::Input(rx) => {
                for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                    if let Ok(frame) = rx.pop() {
                        self.last_input = frame;
                    }
                    [*l, *r] = self.last_input;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use silkscreen_bridge::{EngineEvent, ParamIndex};
    use silkscreen_bridge::{EngineLink, bridge};

    /// Multiplies by param 0.
    #[derive(Debug)]
    struct Gain(f32);

    impl Engine for Gain {
        fn set_param(&mut self, index: ParamIndex, value: f32) {
            if index.0 == 0 {
                self.0 = value;
            }
        }
        fn process(&mut self, channels: &mut [&mut [f32]]) {
            for ch in channels.iter_mut() {
                for x in ch.iter_mut() {
                    *x *= self.0;
                }
            }
        }
    }

    /// Panics on its first block.
    #[derive(Debug)]
    struct Faulty;

    impl Engine for Faulty {
        fn set_param(&mut self, _: ParamIndex, _: f32) {}
        fn process(&mut self, _: &mut [&mut [f32]]) {
            panic!("engine fault");
        }
    }

    fn input(frames: &[[f32; 2]]) -> Feed {
        let (mut tx, rx) = rtrb::RingBuffer::new(4096);
        for &f in frames {
            tx.push(f).unwrap();
        }
        Feed::Input(rx)
    }

    #[test]
    fn input_passes_through_the_engine_to_stereo_output() {
        let (_ui, end) = bridge(&[0.5], 16);
        let mut r = Runner::new(Gain(1.0), end, input(&[[1.0, -1.0], [0.5, 0.25]]), 48_000.0);
        let mut out = [9.0f32; 4];
        r.fill(&mut out, 2);
        assert_eq!(out, [0.5, -0.5, 0.25, 0.125], "seeded at 0.5");
    }

    #[test]
    fn ui_values_apply_before_the_buffer_and_input_underruns_hold() {
        let (mut ui, end) = bridge(&[1.0], 16);
        let mut r = Runner::new(Gain(1.0), end, input(&[[1.0, 1.0]]), 48_000.0);
        ui.send(EngineEvent::ParamValue {
            index: ParamIndex(0),
            value: 2.0,
        });
        let mut out = [0.0f32; 6];
        r.fill(&mut out, 2);
        // One frame of input, then the last frame repeats.
        assert_eq!(out, [2.0; 6]);
    }

    #[test]
    fn mono_and_wide_devices() {
        let (_ui, end) = bridge(&[1.0], 16);
        let mut r = Runner::new(Gain(1.0), end, input(&[[1.0, 0.5], [0.0, 1.0]]), 48_000.0);
        let mut mono = [0.0f32; 1];
        r.fill(&mut mono, 1);
        assert_eq!(mono, [0.75]);
        let mut four = [9.0f32; 4];
        r.fill(&mut four, 4);
        assert_eq!(four, [0.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn buffers_larger_than_a_chunk_are_rendered_whole() {
        let (_ui, end) = bridge(&[1.0], 16);
        let frames: Vec<[f32; 2]> = (0..CHUNK * 2 + 7).map(|i| [i as f32, 0.0]).collect();
        let mut r = Runner::new(Gain(1.0), end, input(&frames), 48_000.0);
        let mut out = vec![0.0f32; frames.len() * 2];
        r.fill(&mut out, 2);
        assert!(out.chunks(2).enumerate().all(|(i, f)| f[0] == i as f32));
    }

    #[test]
    fn integer_devices_get_converted_samples() {
        let (_ui, end) = bridge(&[1.0], 16);
        let mut r = Runner::new(Gain(1.0), end, input(&[[0.5, -1.0]]), 48_000.0);
        let mut out = [0i16; 2];
        r.fill(&mut out, 2);
        assert_eq!(out, [16384, -32768]);
    }

    #[test]
    fn the_test_tone_is_a_quiet_sawtooth() {
        let (_ui, end) = bridge(&[1.0], 16);
        let mut r = Runner::new(Gain(1.0), end, Feed::TestTone, 48_000.0);
        let mut out = vec![0.0f32; 2 * 48_000];
        r.fill(&mut out, 2);
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((peak - TONE_LEVEL).abs() < 0.01, "{peak}");
        // 220 rising ramps a second: count the resets.
        let resets = out
            .chunks(2)
            .collect::<Vec<_>>()
            .windows(2)
            .filter(|w| w[1][0] < w[0][0] - 0.1)
            .count();
        assert!((219..=221).contains(&resets), "{resets}");
    }

    #[test]
    fn the_output_reaches_telemetry() {
        let (mut ui, end) = bridge(&[1.0], 16);
        let mut r = Runner::new(Gain(1.0), end, input(&[[0.25, -0.75]]), 48_000.0);
        let mut out = [0.0f32; 64];
        r.fill(&mut out, 2);
        let peaks = ui.telemetry().unwrap().take_peaks();
        assert_eq!(peaks.levels, [0.25, 0.75]);
    }

    #[test]
    fn a_panicking_engine_falls_silent_and_says_so() {
        let (_ui, end) = bridge(&[], 16);
        let mut r = Runner::new(Faulty, end, Feed::TestTone, 48_000.0);
        let failed = r.failed();
        let mut out = [9.0f32; 8];
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        r.fill(&mut out, 2);
        std::panic::set_hook(hook);
        assert!(failed.load(Ordering::Relaxed));
        assert_eq!(out, [0.0; 8]);
        r.fill(&mut out, 2);
        assert_eq!(out, [0.0; 8], "silent from then on");
    }
}
