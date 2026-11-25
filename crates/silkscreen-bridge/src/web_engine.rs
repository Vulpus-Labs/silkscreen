//! An [`Engine`] driven from an `AudioWorklet` ([design/hosts.md]): the engine
//! module's side of the web host.
//!
//! [`WebEngine`] owns the engine, its [`SabEngine`] end and the audio
//! buffers the worklet copies through. It is target-independent, so it is
//! tested natively; [`export_web_engine!`](crate::export_web_engine) wraps
//! one in the C-ABI exports the worklet calls.
//!
//! [design/hosts.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/hosts.md

use crate::sab::{LayoutError, SabEngine, SharedWords};
use crate::{Engine, EngineSide, ParamIndex};

/// Frames in a Web Audio render quantum.
pub const QUANTUM: usize = 128;

/// The most channels a [`WebEngine`] renders: those telemetry meters.
pub use crate::telemetry::MAX_CHANNELS;

/// An engine, the engine end of the shared buffer, and one quantum of
/// audio per channel. Nothing after [`new`](Self::new) allocates.
#[derive(Debug)]
pub struct WebEngine<E, W> {
    engine: E,
    end: SabEngine<W>,
    channels: usize,
    buffers: [[f32; QUANTUM]; MAX_CHANNELS],
}

impl<E: Engine, W: SharedWords + Clone> WebEngine<E, W> {
    /// Attach to the shared buffer and seed `engine` with its values (a
    /// pending load's, if one is waiting), then reset it so it starts there.
    pub fn new(mut engine: E, words: W, channels: usize) -> Result<Self, LayoutError> {
        let end = SabEngine::attach(words)?;
        end.seed_engine(&mut engine);
        Ok(Self {
            engine,
            end,
            channels: channels.clamp(1, MAX_CHANNELS),
            buffers: [[0.0; QUANTUM]; MAX_CHANNELS],
        })
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Channel `ch`'s buffer: the worklet writes the input here before
    /// [`render`](Self::render) and reads the output after.
    pub fn buffer(&mut self, ch: usize) -> &mut [f32; QUANTUM] {
        &mut self.buffers[ch.min(MAX_CHANNELS - 1)]
    }

    /// Apply the UI's changes, deliver the page's notes, then render `frames` (at most [`QUANTUM`])
    /// in place, and record them, and the engine's meters, as telemetry. Gesture brackets are dropped: a browser has no host to
    /// record automation.
    pub fn render(&mut self, frames: usize) {
        self.end.apply_ui(&mut self.engine);
        let engine = &mut self.engine;
        // Notes after values, so a note starts with the latest settings.
        self.end.drain_midi(|note| engine.note(note));
        let frames = frames.min(QUANTUM);
        let [l, r] = &mut self.buffers;
        let tap = self.end.telemetry();
        if self.channels == 1 {
            engine.process(&mut [&mut l[..frames]]);
            tap.write_block(engine, &[&l[..frames]]);
        } else {
            engine.process(&mut [&mut l[..frames], &mut r[..frames]]);
            tap.write_block(engine, &[&l[..frames], &r[..frames]]);
        }
    }

    /// A value from outside the editor (the page standing in for a host:
    /// MIDI learn, automation): applied to the engine and reported to the
    /// editor, as a host's automation is.
    pub fn host_set_param(&mut self, index: ParamIndex, value: f32) {
        if index.index() < self.end.param_count() {
            self.engine.set_param(index, value);
            self.end.publish_host(index, value);
        }
    }

    /// Clear the engine's state, e.g. on resuming after a suspend.
    pub fn reset(&mut self) {
        self.engine.reset();
    }
}

/// Export a C ABI that runs `$make(sample_rate)`, an `$engine`, in an
/// `AudioWorklet`. Use once, in the engine module's crate: a `cdylib` that
/// depends only on `silkscreen-bridge` and the engine. Expands to nothing
/// off wasm32.
///
/// ```ignore
/// silkscreen_bridge::export_web_engine!(GainEngine, GainEngine::new);
/// ```
///
/// The exports, all prefixed `silkscreen_engine_`:
///
/// | export | does |
/// |---|---|
/// | `init(sample_rate: f32, channels: u32) -> u32` | attach to the shared buffer through the `silkscreen_sab` imports; 0 on success, else the length of the message `error` points to |
/// | `error() -> *const u8` | the last `init` failure, UTF-8 |
/// | `quantum() -> u32` | frames per render: 128 |
/// | `buffer(ch: u32) -> *mut f32` | channel `ch`'s buffer, `quantum` frames; null before `init` |
/// | `render(frames: u32)` | apply the UI's changes, render the buffers in place |
/// | `host_set_param(index: u32, value: f32)` | a value from the page, reported to the editor |
/// | `reset()` | clear engine state |
#[macro_export]
macro_rules! export_web_engine {
    ($engine:ty, $make:expr) => {
        #[cfg(target_arch = "wasm32")]
        #[allow(unsafe_code)]
        mod silkscreen_web_engine_exports {
            use super::*;
            use ::std::cell::RefCell;
            use $crate::ParamIndex;
            use $crate::sab::ImportedWords;
            use $crate::web_engine::{QUANTUM, WebEngine};

            ::std::thread_local! {
                // One engine per module instance; wasm32 without atomics has
                // one thread, so this is a plain static.
                static ENGINE: RefCell<Option<WebEngine<$engine, ImportedWords>>> =
                    const { RefCell::new(None) };
                static ERROR: RefCell<::std::string::String> =
                    const { RefCell::new(::std::string::String::new()) };
            }

            fn with<R>(f: impl FnOnce(&mut WebEngine<$engine, ImportedWords>) -> R) -> Option<R> {
                ENGINE.with(|e| e.borrow_mut().as_mut().map(f))
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn silkscreen_engine_init(sample_rate: f32, channels: u32) -> u32 {
                let make: fn(f32) -> $engine = $make;
                match WebEngine::new(make(sample_rate), ImportedWords, channels as usize) {
                    Ok(engine) => {
                        ENGINE.with(|e| *e.borrow_mut() = Some(engine));
                        0
                    }
                    Err(err) => ERROR.with(|m| {
                        let mut m = m.borrow_mut();
                        *m = ::std::format!("silkscreen engine: {err}");
                        m.len() as u32
                    }),
                }
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn silkscreen_engine_error() -> *const u8 {
                ERROR.with(|m| m.borrow().as_ptr())
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn silkscreen_engine_quantum() -> u32 {
                QUANTUM as u32
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn silkscreen_engine_buffer(ch: u32) -> *mut f32 {
                with(|e| e.buffer(ch as usize).as_mut_ptr()).unwrap_or(::core::ptr::null_mut())
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn silkscreen_engine_render(frames: u32) {
                with(|e| e.render(frames as usize));
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn silkscreen_engine_host_set_param(index: u32, value: f32) {
                with(|e| e.host_set_param(ParamIndex(index), value));
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn silkscreen_engine_reset() {
                with(WebEngine::reset);
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicU32;

    use super::*;
    use crate::EngineEvent;
    use crate::EngineLink;
    use crate::sab::{SabLayout, SabUi};

    /// Multiplies by param 0; counts resets; a note on sets the gain to
    /// its velocity, after any param change in the same block.
    #[derive(Debug, Default)]
    struct Gain {
        gain: f32,
        resets: u32,
    }

    impl Engine for Gain {
        fn set_param(&mut self, index: ParamIndex, value: f32) {
            if index.0 == 0 {
                self.gain = value;
            }
        }
        fn process(&mut self, channels: &mut [&mut [f32]]) {
            for ch in channels.iter_mut() {
                for x in ch.iter_mut() {
                    *x *= self.gain;
                }
            }
        }
        fn reset(&mut self) {
            self.resets += 1;
        }
        fn note(&mut self, event: crate::NoteEvent) {
            if let crate::NoteEvent::On { velocity, .. } = event {
                self.gain = velocity;
            }
        }
    }

    type Words = Arc<[AtomicU32]>;

    fn setup(initial: &[f32], channels: usize) -> (SabUi<Words>, WebEngine<Gain, Words>) {
        let layout = SabLayout::new(initial.len(), 16).unwrap();
        let words: Words = (0..layout.words()).map(|_| AtomicU32::new(0)).collect();
        let ui = SabUi::create(words.clone(), layout, initial).unwrap();
        let engine = WebEngine::new(Gain::default(), words, channels).unwrap();
        (ui, engine)
    }

    #[test]
    fn a_new_engine_is_seeded_from_the_buffer_and_reset() {
        let (_ui, mut e) = setup(&[0.5, 0.0], 2);
        assert_eq!(e.engine.resets, 1);
        e.buffer(0).fill(1.0);
        e.buffer(1).fill(-2.0);
        e.render(QUANTUM);
        assert!(e.buffer(0).iter().all(|&x| x == 0.5));
        assert!(e.buffer(1).iter().all(|&x| x == -1.0));
    }

    #[test]
    fn ui_changes_apply_before_the_block_renders() {
        let (mut ui, mut e) = setup(&[1.0], 1);
        ui.send(EngineEvent::GestureBegin {
            index: ParamIndex(0),
        });
        ui.send(EngineEvent::ParamValue {
            index: ParamIndex(0),
            value: 0.25,
        });
        e.buffer(0).fill(1.0);
        e.render(64);
        assert_eq!(e.buffer(0)[63], 0.25);
        assert_eq!(e.buffer(0)[64], 1.0, "only `frames` are rendered");
        ui.load(&[2.0]);
        e.buffer(0).fill(1.0);
        e.render(QUANTUM);
        assert_eq!(e.buffer(0)[0], 2.0, "a load applies too");
    }

    #[test]
    fn page_values_reach_the_engine_and_the_editor() {
        let (mut ui, mut e) = setup(&[1.0], 1);
        e.host_set_param(ParamIndex(0), 0.5);
        e.host_set_param(ParamIndex(9), 0.5); // out of range: ignored
        let mut polled = Vec::new();
        ui.poll_host(&mut |p, v| polled.push((p, v)));
        assert_eq!(polled, [(ParamIndex(0), 0.5)]);
        e.buffer(0).fill(1.0);
        e.render(QUANTUM);
        assert_eq!(e.buffer(0)[0], 0.5);
    }

    #[test]
    fn each_quantum_reaches_the_editor_as_telemetry() {
        let (mut ui, mut e) = setup(&[0.5], 2);
        e.buffer(0).fill(1.0);
        e.buffer(1).fill(-1.0);
        e.render(QUANTUM);
        let reader = ui.telemetry().unwrap();
        assert_eq!(reader.written(), QUANTUM as u64);
        assert_eq!(reader.take_peaks().levels, [0.5, 0.5]);
    }

    /// Publishes its block count, twice over, as a frame.
    #[derive(Default)]
    struct Counter(f32);

    impl Engine for Counter {
        fn set_param(&mut self, _: ParamIndex, _: f32) {}
        fn process(&mut self, _: &mut [&mut [f32]]) {
            self.0 += 1.0;
        }
        fn frame(&mut self, write: &mut dyn FnMut(&[f32])) {
            write(&[self.0, self.0]);
        }
    }

    #[test]
    fn each_quantum_publishes_the_engine_frame() {
        let layout = SabLayout::new(1, 16).unwrap().with_frame(2).unwrap();
        let words: Words = (0..layout.words()).map(|_| AtomicU32::new(0)).collect();
        let mut ui = SabUi::create(words.clone(), layout, &[0.0]).unwrap();
        let mut e = WebEngine::new(Counter::default(), words, 2).unwrap();
        e.render(QUANTUM);
        e.render(QUANTUM);
        let reader = ui.telemetry().unwrap();
        assert!(reader.poll_frame());
        assert_eq!(reader.frame(), &[2.0, 2.0]);
    }

    #[test]
    fn notes_reach_the_engine_after_values_and_before_the_block() {
        let (mut ui, mut e) = setup(&[1.0], 1);
        let mut midi = ui.midi_out().unwrap();
        ui.send(EngineEvent::ParamValue {
            index: ParamIndex(0),
            value: 0.25,
        });
        midi.send([0x90, 60, 127]);
        e.buffer(0).fill(0.5);
        e.render(QUANTUM);
        assert_eq!(e.buffer(0)[0], 0.5, "velocity 1 from the note, not 0.25");
    }

    #[test]
    fn channels_are_clamped_to_one_or_two() {
        assert_eq!(setup(&[1.0], 0).1.channels(), 1);
        assert_eq!(setup(&[1.0], 6).1.channels(), 2);
    }
}
