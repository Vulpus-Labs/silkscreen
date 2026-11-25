//! The engine side must not allocate (design/engine-bridge.md). A counting global
//! allocator watches every engine-side call, including the resync path.

use silkscreen_bridge::testing::{CountingAllocator, allocations};

use silkscreen_bridge::{EngineEvent, EngineLink, ParamIndex, bridge};

#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator;

#[test]
fn the_counter_sees_allocations() {
    let n = allocations(|| drop(std::hint::black_box(vec![1u8; 16])));
    assert_eq!(n, 1);
}

#[test]
fn engine_side_never_allocates() {
    let (mut ui, mut engine) = bridge(&[0.0; 100], 8);
    for i in 0..20 {
        ui.send(EngineEvent::ParamValue {
            index: ParamIndex(i % 100),
            value: i as f32,
        });
    }
    // Overflowed: this drain takes the resync path.
    let mut sum = 0.0;
    let n = allocations(|| {
        engine.drain(|e| {
            if let EngineEvent::ParamValue { value, .. } = e {
                sum += value;
            }
        });
        for i in 0..100 {
            engine.publish_host(ParamIndex(i), i as f32);
            sum += engine.read(ParamIndex(i));
        }
        sum += engine.overflow_count() as f32;
    });
    assert_eq!(n, 0, "engine side allocated {n} times");
    assert!(sum > 0.0);

    // And the ordinary, non-resync path.
    ui.send(EngineEvent::GestureBegin {
        index: ParamIndex(1),
    });
    ui.send(EngineEvent::ParamValue {
        index: ParamIndex(1),
        value: 0.5,
    });
    let n = allocations(|| {
        engine.drain(|_| {});
    });
    assert_eq!(n, 0);
}

#[test]
fn web_engine_side_never_allocates() {
    use silkscreen_bridge::sab::{SabEngine, SabLayout, SabUi};
    use std::sync::Arc;
    use std::sync::atomic::AtomicU32;

    let layout = SabLayout::new(100, 8).unwrap();
    let words: Arc<[AtomicU32]> = (0..layout.words()).map(|_| AtomicU32::new(0)).collect();
    let mut ui = SabUi::create(words.clone(), layout, &[0.0; 100]).unwrap();
    let mut engine = SabEngine::attach(words).unwrap();
    for i in 0..20 {
        ui.send(EngineEvent::ParamValue {
            index: ParamIndex(i % 100),
            value: i as f32,
        });
    }
    ui.load(&[0.5; 100]);
    let mut sum = 0.0;
    let n = allocations(|| {
        engine.take_load(|_, v| sum += v);
        // Overflowed: this drain takes the resync path.
        engine.drain(|_| sum += 1.0);
        for i in 0..100 {
            engine.publish_host(ParamIndex(i), i as f32);
            sum += engine.read(ParamIndex(i)) + engine.seed(ParamIndex(i));
        }
        sum += engine.overflow_count() as f32;
    });
    assert_eq!(n, 0, "web engine side allocated {n} times");
    assert!(sum > 0.0);

    ui.send(EngineEvent::GestureBegin {
        index: ParamIndex(1),
    });
    let n = allocations(|| {
        engine.drain(|_| {});
    });
    assert_eq!(n, 0);
}

#[test]
fn web_engine_render_never_allocates() {
    use silkscreen_bridge::Engine;
    use silkscreen_bridge::sab::{SabLayout, SabUi};
    use silkscreen_bridge::web_engine::{QUANTUM, WebEngine};
    use std::sync::Arc;
    use std::sync::atomic::AtomicU32;

    struct Halve;
    impl Engine for Halve {
        fn set_param(&mut self, _: ParamIndex, _: f32) {}
        fn process(&mut self, channels: &mut [&mut [f32]]) {
            for ch in channels.iter_mut() {
                for x in ch.iter_mut() {
                    *x *= 0.5;
                }
            }
        }
    }

    let layout = SabLayout::new(4, 16).unwrap();
    let words: Arc<[AtomicU32]> = (0..layout.words()).map(|_| AtomicU32::new(0)).collect();
    let mut ui = SabUi::create(words.clone(), layout, &[0.0; 4]).unwrap();
    let mut engine = WebEngine::new(Halve, words, 2).unwrap();
    ui.send(EngineEvent::ParamValue {
        index: ParamIndex(1),
        value: 0.5,
    });
    let mut midi = ui.midi_out().unwrap();
    for key in 0..32 {
        midi.send([0x90, key, 100]);
        midi.send([0x80, key, 0]);
    }
    let n = allocations(|| {
        engine.buffer(0).fill(1.0);
        engine.render(QUANTUM);
        engine.host_set_param(ParamIndex(2), 0.25);
        engine.reset();
    });
    assert_eq!(n, 0, "WebEngine allocated {n} times");
    assert_eq!(engine.buffer(0)[0], 0.5);
}

#[test]
fn publishing_a_frame_never_allocates() {
    let (_ui, mut engine) = silkscreen_bridge::bridge_with_frame(&[0.0], 8, 70);
    let frame = [0.5; 70];
    let n = allocations(|| {
        for _ in 0..100 {
            engine.telemetry().write_frame(&frame);
        }
    });
    assert_eq!(n, 0, "write_frame allocated {n} times");
}
