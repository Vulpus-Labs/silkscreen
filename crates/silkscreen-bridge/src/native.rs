//! The native transport ([design/engine-bridge.md]): an rtrb ring for ordered UI events,
//! and atomic slot arrays for values.
//!
//! **Single-writer discipline**, per field of [`Shared`]:
//!
//! | field | written by | read by | ordering |
//! |---|---|---|---|
//! | `ui_slots` | UI (`send`) | engine (resync) | Relaxed values, published by `resync` |
//! | `resync` | UI sets, engine clears | engine | Release set / Acquire swap |
//! | `load` | UI sets, engine clears | engine | Release set / Acquire swap |
//! | `host_slots` | engine (`publish_host`) | UI (`poll_host`) | Relaxed values, published by `dirty` |
//! | `dirty` | engine sets bits, UI clears words | UI | Release `fetch_or` / Acquire `swap` |
//! | `overflows` | UI | either | Relaxed counter |
//!
//! The ring itself carries [`EngineEvent`]s directly; the 16-byte codec is
//! for transports between separately compiled modules (the web).
//!
//! [design/engine-bridge.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/engine-bridge.md

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use crate::telemetry::{TelemetryReader, TelemetryTap, telemetry_with_frame};
use crate::{EngineEvent, EngineLink, ParamIndex};

#[derive(Debug)]
struct Shared {
    ui_slots: Box<[AtomicU32]>,
    host_slots: Box<[AtomicU32]>,
    dirty: Box<[AtomicU64]>,
    resync: AtomicBool,
    /// A state load is waiting in `ui_slots`.
    load: AtomicBool,
    overflows: AtomicU64,
}

/// Create a connected pair. `initial` seeds every value (use the model's
/// current values); `ring_capacity` bounds the UI → engine event queue.
/// Its telemetry carries no engine frame: see [`bridge_with_frame`].
pub fn bridge(initial: &[f32], ring_capacity: usize) -> (UiLink, EngineEnd) {
    bridge_with_frame(initial, ring_capacity, 0)
}

/// [`bridge`], with room in its telemetry for an engine frame of `frame`
/// words (design/engine-bridge.md): `PluginUi::frame`.
///
/// # Panics
/// If `frame` is over [`MAX_FRAME`](crate::telemetry::MAX_FRAME).
pub fn bridge_with_frame(
    initial: &[f32],
    ring_capacity: usize,
    frame: usize,
) -> (UiLink, EngineEnd) {
    let slots = || {
        initial
            .iter()
            .map(|v| AtomicU32::new(v.to_bits()))
            .collect()
    };
    let shared = Arc::new(Shared {
        ui_slots: slots(),
        host_slots: slots(),
        dirty: (0..initial.len().div_ceil(64))
            .map(|_| AtomicU64::new(0))
            .collect(),
        resync: AtomicBool::new(false),
        load: AtomicBool::new(false),
        overflows: AtomicU64::new(0),
    });
    let (producer, consumer) = rtrb::RingBuffer::new(ring_capacity);
    let (tap, reader) = telemetry_with_frame(frame);
    (
        UiLink {
            shared: shared.clone(),
            producer,
            telemetry: reader,
        },
        EngineEnd {
            shared,
            consumer,
            current: initial.into(),
            telemetry: tap,
        },
    )
}

/// The UI's end: an [`EngineLink`]. Main thread only.
#[derive(Debug)]
pub struct UiLink {
    shared: Arc<Shared>,
    producer: rtrb::Producer<EngineEvent>,
    telemetry: TelemetryReader,
}

impl UiLink {
    /// Events dropped because the ring was full (each one forced a resync).
    pub fn overflow_count(&self) -> u64 {
        self.shared.overflows.load(Ordering::Relaxed)
    }
}

impl EngineLink for UiLink {
    fn send(&mut self, event: EngineEvent) {
        if let EngineEvent::ParamValue { index, value } = event {
            // The slot is written first, so a resync always sees this value.
            self.shared.ui_slots[index.index()].store(value.to_bits(), Ordering::Relaxed);
        }
        if self.producer.push(event).is_err() {
            self.shared.overflows.fetch_add(1, Ordering::Relaxed);
            self.shared.resync.store(true, Ordering::Release);
        }
    }

    fn poll_host(&mut self, report: &mut dyn FnMut(ParamIndex, f32)) {
        for (w, word) in self.shared.dirty.iter().enumerate() {
            let mut bits = word.swap(0, Ordering::Acquire);
            while bits != 0 {
                let bit = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                let i = w * 64 + bit;
                let value = f32::from_bits(self.shared.host_slots[i].load(Ordering::Relaxed));
                report(ParamIndex(i as u32), value);
            }
        }
    }

    fn load(&mut self, values: &[f32]) {
        for (slot, v) in self.shared.ui_slots.iter().zip(values) {
            slot.store(v.to_bits(), Ordering::Relaxed);
        }
        self.shared.load.store(true, Ordering::Release);
    }

    fn telemetry(&mut self) -> Option<&mut TelemetryReader> {
        Some(&mut self.telemetry)
    }
}

/// The engine's end. Audio thread only. No method allocates or blocks.
#[derive(Debug)]
pub struct EngineEnd {
    shared: Arc<Shared>,
    consumer: rtrb::Consumer<EngineEvent>,
    /// The engine's view of every value, from either source.
    current: Box<[f32]>,
    telemetry: TelemetryTap,
}

impl EngineEnd {
    /// Where the host records each block's output for the editor
    /// ([design/engine-bridge.md]).
    ///
    /// [design/engine-bridge.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/engine-bridge.md
    pub fn telemetry(&mut self) -> &mut TelemetryTap {
        &mut self.telemetry
    }

    /// Deliver pending UI events in order. After an overflow, the queue is
    /// discarded and every param is delivered as a `ParamValue` from the UI
    /// slots instead: values are restored, gesture brackets in the
    /// discarded stretch are lost. Returns `true` if that happened, so the
    /// caller can close any gesture it still has open.
    pub fn drain(&mut self, mut deliver: impl FnMut(EngineEvent)) -> bool {
        let resynced = self.shared.resync.swap(false, Ordering::Acquire);
        if resynced {
            while self.consumer.pop().is_ok() {}
            for (i, slot) in self.shared.ui_slots.iter().enumerate() {
                let value = f32::from_bits(slot.load(Ordering::Relaxed));
                self.current[i] = value;
                deliver(EngineEvent::ParamValue {
                    index: ParamIndex(i as u32),
                    value,
                });
            }
        }
        while let Ok(event) = self.consumer.pop() {
            if let EngineEvent::ParamValue { index, value } = event {
                self.current[index.index()] = value;
            }
            deliver(event);
        }
        resynced
    }

    /// Apply a pending state load, if there is one: discard the queue, and
    /// hand every param's value to `apply`. These are not UI edits, so the
    /// caller must not report them to the host. Call before
    /// [`drain`](Self::drain). Returns `true` if a load was applied; gesture
    /// brackets in the discarded stretch are lost, as with a resync.
    pub fn take_load(&mut self, mut apply: impl FnMut(ParamIndex, f32)) -> bool {
        if !self.shared.load.swap(false, Ordering::Acquire) {
            return false;
        }
        while self.consumer.pop().is_ok() {}
        for (i, slot) in self.shared.ui_slots.iter().enumerate() {
            let value = f32::from_bits(slot.load(Ordering::Relaxed));
            self.current[i] = value;
            apply(ParamIndex(i as u32), value);
        }
        true
    }

    /// The value to seed a fresh engine with: [`read`](Self::read), or the
    /// pending load's value if one is waiting. Seeding from this means the
    /// engine starts at loaded state instead of gliding to it.
    pub fn seed(&self, index: ParamIndex) -> f32 {
        if self.shared.load.load(Ordering::Acquire) {
            f32::from_bits(self.shared.ui_slots[index.index()].load(Ordering::Relaxed))
        } else {
            self.read(index)
        }
    }

    /// Record a value that came from the host (automation, its generic UI),
    /// for the editor to pick up. Never call this for values from
    /// [`drain`](Self::drain): that would echo UI edits back as host edits.
    pub fn publish_host(&mut self, index: ParamIndex, value: f32) {
        let i = index.index();
        self.current[i] = value;
        self.shared.host_slots[i].store(value.to_bits(), Ordering::Relaxed);
        self.shared.dirty[i / 64].fetch_or(1 << (i % 64), Ordering::Release);
    }

    /// The latest value of `index` from either source.
    pub fn read(&self, index: ParamIndex) -> f32 {
        self.current[index.index()]
    }

    pub fn param_count(&self) -> usize {
        self.current.len()
    }

    /// Events dropped because the ring was full.
    pub fn overflow_count(&self) -> u64 {
        self.shared.overflows.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: ParamIndex = ParamIndex(0);
    const B: ParamIndex = ParamIndex(1);

    fn drained(engine: &mut EngineEnd) -> Vec<EngineEvent> {
        let mut out = Vec::new();
        engine.drain(|e| out.push(e));
        out
    }

    fn polled(ui: &mut UiLink) -> Vec<(ParamIndex, f32)> {
        let mut out = Vec::new();
        ui.poll_host(&mut |p, v| out.push((p, v)));
        out
    }

    #[test]
    fn events_arrive_in_order() {
        let (mut ui, mut engine) = bridge(&[0.0, 0.0], 16);
        let events = [
            EngineEvent::GestureBegin { index: A },
            EngineEvent::ParamValue {
                index: A,
                value: 0.25,
            },
            EngineEvent::ParamValue {
                index: A,
                value: 0.5,
            },
            EngineEvent::GestureEnd { index: A },
        ];
        for e in events {
            ui.send(e);
        }
        assert_eq!(drained(&mut engine), events);
        assert_eq!(engine.read(A), 0.5);
        assert!(drained(&mut engine).is_empty());
    }

    #[test]
    fn initial_values_seed_both_ends() {
        let (_ui, engine) = bridge(&[1.5, -3.0], 4);
        assert_eq!(engine.read(A), 1.5);
        assert_eq!(engine.read(B), -3.0);
    }

    #[test]
    fn overflow_resyncs_to_latest_ui_values() {
        let (mut ui, mut engine) = bridge(&[0.0, 0.0], 4);
        for i in 0..10 {
            ui.send(EngineEvent::ParamValue {
                index: A,
                value: i as f32,
            });
        }
        ui.send(EngineEvent::ParamValue {
            index: B,
            value: 7.0,
        });
        assert_eq!(ui.overflow_count(), 7);
        assert_eq!(engine.overflow_count(), 7);

        let mut events = Vec::new();
        assert!(engine.drain(|e| events.push(e)), "reports the resync");
        assert_eq!(
            events,
            [
                EngineEvent::ParamValue {
                    index: A,
                    value: 9.0
                },
                EngineEvent::ParamValue {
                    index: B,
                    value: 7.0
                },
            ]
        );
        assert_eq!((engine.read(A), engine.read(B)), (9.0, 7.0));
        // The flag is cleared and normal delivery resumes.
        assert!(!engine.drain(|_| {}));
        ui.send(EngineEvent::GestureBegin { index: A });
        assert_eq!(
            drained(&mut engine),
            [EngineEvent::GestureBegin { index: A }]
        );
    }

    #[test]
    fn host_values_are_reported_once_per_publish() {
        let (mut ui, mut engine) = bridge(&[0.0; 70], 4);
        engine.publish_host(ParamIndex(3), 0.5);
        engine.publish_host(ParamIndex(69), 0.25);
        engine.publish_host(ParamIndex(3), 0.75);
        assert_eq!(
            polled(&mut ui),
            [(ParamIndex(3), 0.75), (ParamIndex(69), 0.25)]
        );
        assert!(polled(&mut ui).is_empty());
        assert_eq!(engine.read(ParamIndex(3)), 0.75);
    }

    #[test]
    fn a_repeated_host_value_is_still_reported() {
        // Host sets X, the UI moves the param, the host sets X again: the
        // second X must reach the model.
        let (mut ui, mut engine) = bridge(&[0.0], 4);
        engine.publish_host(A, 0.5);
        assert_eq!(polled(&mut ui), [(A, 0.5)]);
        ui.send(EngineEvent::ParamValue {
            index: A,
            value: 0.9,
        });
        drained(&mut engine);
        engine.publish_host(A, 0.5);
        assert_eq!(polled(&mut ui), [(A, 0.5)]);
    }

    #[test]
    fn a_load_supersedes_queued_events_and_is_not_a_ui_edit() {
        let (mut ui, mut engine) = bridge(&[0.0, 0.0], 16);
        ui.send(EngineEvent::GestureBegin { index: A });
        ui.send(EngineEvent::ParamValue {
            index: A,
            value: 0.9,
        });
        ui.load(&[0.25, 0.5]);
        assert_eq!((engine.seed(A), engine.seed(B)), (0.25, 0.5));
        assert_eq!(engine.read(A), 0.0, "not applied until taken");

        let mut loaded = Vec::new();
        assert!(engine.take_load(|p, v| loaded.push((p, v))));
        assert_eq!(loaded, [(A, 0.25), (B, 0.5)]);
        assert_eq!((engine.read(A), engine.read(B)), (0.25, 0.5));
        assert!(drained(&mut engine).is_empty(), "the queue was discarded");
        assert!(!engine.take_load(|_, _| {}), "taken once");
        assert!(polled(&mut ui).is_empty(), "not a host change either");

        // Events after the load are delivered normally.
        ui.send(EngineEvent::ParamValue {
            index: B,
            value: 0.75,
        });
        assert_eq!(
            drained(&mut engine),
            [EngineEvent::ParamValue {
                index: B,
                value: 0.75
            }]
        );
    }

    #[test]
    fn ui_values_are_never_reported_as_host_values() {
        let (mut ui, mut engine) = bridge(&[0.0], 4);
        ui.send(EngineEvent::ParamValue {
            index: A,
            value: 0.9,
        });
        drained(&mut engine);
        assert!(polled(&mut ui).is_empty());
    }
}

/// The UI end moves to the thread that opens the editor (a standalone
/// window's builder must be `Send`), and the engine end to the audio thread.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod send_check {
    #[test]
    fn ends_are_send() {
        fn send<T: Send>() {}
        send::<super::UiLink>();
        send::<super::EngineEnd>();
    }
}
