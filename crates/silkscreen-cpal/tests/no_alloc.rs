//! The audio callback must not allocate (design/engine-bridge.md). A counting global
//! allocator watches `Runner::fill` on every feed, the resync path and a
//! buffer larger than a chunk.

use silkscreen_bridge::testing::{CountingAllocator, allocations};

use silkscreen_bridge::{Engine, EngineEvent, EngineLink, ParamIndex, bridge};
use silkscreen_cpal::{CHUNK, Feed, Runner};

#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator;

#[derive(Debug)]
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

#[test]
fn the_callback_never_allocates() {
    let (mut tx, rx) = rtrb::RingBuffer::new(8192);
    for i in 0..4096 {
        tx.push([i as f32, 0.0]).unwrap();
    }
    for feed in [Feed::Silence, Feed::TestTone, Feed::Input(rx)] {
        let (mut ui, end) = bridge(&[0.0; 8], 4);
        let mut runner = Runner::new(Halve, end, feed, 48_000.0);
        // Overflow the ring, so the first fill takes the resync path.
        for i in 0..20 {
            ui.send(EngineEvent::ParamValue {
                index: ParamIndex(i % 8),
                value: i as f32,
            });
        }
        ui.load(&[0.25; 8]);
        let mut stereo = vec![0.0f32; (CHUNK * 2 + 3) * 2];
        let mut wide = vec![0i16; 256 * 4];
        let n = allocations(|| {
            runner.fill(&mut stereo, 2);
            runner.fill(&mut wide, 4);
            runner.fill(&mut stereo[..2], 1);
        });
        assert_eq!(n, 0, "the callback allocated {n} times");
    }
}
