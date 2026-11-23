//! Two threads, a small ring, many events: the final state must survive
//! overflow and resync.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use silkscreen_bridge::{EngineEvent, EngineLink, ParamIndex, bridge};

const EVENTS: u32 = 10_000;
const PARAMS: u32 = 8;

#[test]
fn final_state_survives_concurrent_overflow() {
    let (mut ui, mut engine) = bridge(&[0.0; PARAMS as usize], 32);
    let done = Arc::new(AtomicBool::new(false));

    let engine_thread = {
        let done = done.clone();
        std::thread::spawn(move || {
            let mut delivered = 0u64;
            loop {
                let finished = done.load(Ordering::Acquire);
                engine.drain(|_| delivered += 1);
                // The engine also publishes host values while the UI writes.
                engine.publish_host(ParamIndex(PARAMS - 1), delivered as f32);
                if finished {
                    break;
                }
                std::thread::yield_now();
            }
            engine
        })
    };

    for i in 1..=EVENTS {
        ui.send(EngineEvent::ParamValue {
            index: ParamIndex(i % (PARAMS - 1)),
            value: i as f32,
        });
    }
    done.store(true, Ordering::Release);
    let mut engine = engine_thread.join().unwrap();
    engine.drain(|_| {});

    // Each UI param holds the last value sent to it.
    for p in 0..PARAMS - 1 {
        let last = (1..=EVENTS).rev().find(|i| i % (PARAMS - 1) == p).unwrap();
        assert_eq!(engine.read(ParamIndex(p)), last as f32, "param {p}");
    }
    // The host-published param reached the UI side with its final value.
    let mut seen = None;
    ui.poll_host(&mut |p, v| {
        assert_eq!(p, ParamIndex(PARAMS - 1));
        seen = Some(v);
    });
    assert_eq!(seen, Some(engine.read(ParamIndex(PARAMS - 1))));
}
