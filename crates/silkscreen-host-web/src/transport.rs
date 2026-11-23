//! The UI end of the web transport ([design/hosts.md]): the bridge's
//! [`SabUi`] over a `SharedArrayBuffer`, through JS `Atomics`.
//!
//! [design/hosts.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/hosts.md

use js_sys::{Atomics, Int32Array, SharedArrayBuffer};
use silkscreen::bridge::sab::{LayoutError, SabLayout, SabUi, SharedWords};
use wasm_bindgen::JsCast;

/// A `SharedArrayBuffer` seen as 32-bit words, each accessed with
/// `Atomics` (sequentially consistent).
#[derive(Debug, Clone)]
pub struct SabWords {
    words: Int32Array,
}

impl SabWords {
    pub fn new(buffer: &SharedArrayBuffer) -> Self {
        Self {
            words: Int32Array::new(buffer),
        }
    }

    pub fn buffer(&self) -> SharedArrayBuffer {
        // Always a `SharedArrayBuffer`: `new` only takes one.
        self.words.buffer().unchecked_into()
    }
}

// `Atomics` throws only for a non-integer array or an index out of range;
// the array is an `Int32Array` and the layout bounds every index.
const IN_RANGE: &str = "silkscreen: shared buffer index in range";

impl SharedWords for SabWords {
    fn len(&self) -> usize {
        self.words.length() as usize
    }
    fn load(&self, i: usize) -> u32 {
        Atomics::load(&self.words, i as u32).expect(IN_RANGE) as u32
    }
    fn store(&self, i: usize, value: u32) {
        Atomics::store(&self.words, i as u32, value as i32).expect(IN_RANGE);
    }
    fn swap(&self, i: usize, value: u32) -> u32 {
        Atomics::exchange(&self.words, i as u32, value as i32).expect(IN_RANGE) as u32
    }
    fn fetch_or(&self, i: usize, value: u32) -> u32 {
        Atomics::or(&self.words, i as u32, value as i32).expect(IN_RANGE) as u32
    }
    fn fetch_add(&self, i: usize, value: u32) -> u32 {
        Atomics::add(&self.words, i as u32, value as i32).expect(IN_RANGE) as u32
    }
    fn compare_exchange(&self, i: usize, current: u32, new: u32) -> Result<u32, u32> {
        let before = Atomics::compare_exchange(&self.words, i as u32, current as i32, new as i32)
            .expect(IN_RANGE) as u32;
        if before == current {
            Ok(before)
        } else {
            Err(before)
        }
    }
    /// One bulk copy (a scope window is 4096 words): each element is read
    /// whole, as `Atomics.load` would, but not in one atomic step, which
    /// the trait allows.
    fn load_range(&self, start: usize, out: &mut [u32]) {
        let end = (start + out.len()) as u32;
        js_sys::Uint32Array::new(&self.words.buffer())
            .subarray(start as u32, end)
            .copy_to(out);
    }
}

/// A new `SharedArrayBuffer` laid out for `initial.len()` params and a
/// ring of `capacity` records, and the UI's link over it. Hand the buffer
/// to the worklet (`processorOptions`); the engine attaches with
/// [`SabEngine::attach`](silkscreen::bridge::sab::SabEngine::attach).
///
/// Constructing a `SharedArrayBuffer` needs a cross-origin isolated page
/// (COOP and COEP headers).
pub fn sab_link(initial: &[f32], capacity: usize) -> Result<SabUi<SabWords>, LayoutError> {
    let layout = SabLayout::new(initial.len(), capacity)?;
    let words = SabWords::new(&SharedArrayBuffer::new(layout.bytes() as u32));
    SabUi::create(words, layout, initial)
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;
    use silkscreen::ParamIndex;
    use silkscreen::bridge::sab::SabEngine;
    use silkscreen::bridge::{EngineEvent, EngineLink};
    use wasm_bindgen_test::wasm_bindgen_test;

    /// Both ends over one real `SharedArrayBuffer`, through two separate
    /// `Int32Array` views, as the UI and worklet have.
    #[wasm_bindgen_test]
    fn both_ends_meet_through_a_shared_array_buffer() {
        let mut ui = sab_link(&[0.0, 1.0, 2.0], 8).unwrap();
        let engine_view = SabWords::new(&ui_buffer(&ui));
        let mut engine = SabEngine::attach(engine_view).unwrap();
        assert_eq!(engine.read(ParamIndex(2)), 2.0);

        let a = ParamIndex(1);
        let sent = [
            EngineEvent::GestureBegin { index: a },
            EngineEvent::ParamValue {
                index: a,
                value: -0.5,
            },
            EngineEvent::GestureEnd { index: a },
        ];
        for e in sent {
            ui.send(e);
        }
        let mut got = Vec::new();
        assert!(!engine.drain(|e| got.push(e)));
        assert_eq!(got, sent);

        engine.publish_host(ParamIndex(0), 0.25);
        let mut polled = Vec::new();
        ui.poll_host(&mut |p, v| polled.push((p, v)));
        assert_eq!(polled, [(ParamIndex(0), 0.25)]);

        // Overflow → resync, through the same buffer.
        for i in 0..20 {
            ui.send(EngineEvent::ParamValue {
                index: ParamIndex(2),
                value: i as f32,
            });
        }
        assert!(engine.drain(|_| {}));
        assert_eq!(engine.read(ParamIndex(2)), 19.0);
        assert_eq!(engine.overflow_count(), 12);

        // Telemetry: the scope through `load_range`, peaks through
        // `compare_exchange` (fetch_max) and `swap`.
        let ramp: Vec<f32> = (0..300).map(|i| i as f32).collect();
        engine.telemetry().write(&[&ramp[..]]);
        engine.telemetry().write(&[[-500.0f32]]);
        engine.telemetry().write_engine_peak(3, [0.25, -0.75]);
        let reader = ui.telemetry().unwrap();
        let mut scope = [0.0; silkscreen::bridge::telemetry::SCOPE_CAPACITY];
        assert!(reader.scope(&mut scope));
        let n = scope.len();
        assert_eq!(&scope[n - 3..], &[298.0, 299.0, -500.0]);
        assert_eq!(reader.take_peaks().levels[0], 500.0);
        assert_eq!(reader.take_engine_peaks(3), [0.25, 0.75]);

        // MIDI from the page.
        let mut midi = ui.midi_out().unwrap();
        assert!(midi.send([0x90, 64, 127]));
        let mut notes = Vec::new();
        engine.drain_midi(|n| notes.push(n));
        assert_eq!(
            notes,
            [silkscreen::bridge::NoteEvent::On {
                channel: 0,
                key: 64,
                velocity: 1.0
            }]
        );
    }

    fn ui_buffer(ui: &SabUi<SabWords>) -> SharedArrayBuffer {
        ui.words().buffer()
    }
}
