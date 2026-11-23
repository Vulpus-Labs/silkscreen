//! An idle editor does nothing a frame: with a tooltip showing,
//! a tick and a render allocate nothing and paint nothing.

use silkscreen::core::tiny_skia::Pixmap;
use silkscreen_bridge::testing::{CountingAllocator, allocations};
use silkscreen_harness::Harness;
use widgets::faceplate::ids;

#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator;

#[test]
fn an_idle_frame_with_a_tooltip_allocates_nothing() {
    let mut h = Harness::new(widgets::editor, 1.0);
    let c = h.centre(ids::CUTOFF);
    h.move_to(c).advance(700.0);
    assert!(h.editor().tip().is_some());
    h.snapshot();
    let (w, hgt) = h.editor().physical_size();
    let mut frame = Pixmap::new(w, hgt).unwrap();
    let mut now = h.now();
    let editor = h.editor_mut();
    let n = allocations(|| {
        for _ in 0..10 {
            now += 16.0;
            editor.tick(now);
            assert!(editor.render(&mut frame.as_mut()).is_empty());
        }
    });
    assert_eq!(n, 0, "an idle frame allocated {n} times");
}
