//! A render that merges damage under a panel allocates nothing,
//! and dimming a control adds none.

use std::alloc::{GlobalAlloc, Layout as AllocLayout, System};
use std::cell::Cell;

use silkscreen_core::tiny_skia::{Pixmap, PixmapMut, Transform};
use silkscreen_core::{
    Compositor, ControlId, ControlKind, ControlSpec, IRect, Layout, Overlay, RasterSource, Rect,
    Size, WidgetState,
};

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

/// The system allocator, counting this thread's allocations while
/// [`allocations`] runs.
struct Counting;

// Forwards to the system allocator unchanged.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: AllocLayout) -> *mut u8 {
        if COUNTING.with(Cell::get) {
            COUNT.with(|c| c.set(c.get() + 1));
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: AllocLayout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

fn allocations(f: impl FnOnce()) -> usize {
    COUNT.with(|c| c.set(0));
    COUNTING.with(|c| c.set(true));
    f();
    COUNTING.with(|c| c.set(false));
    COUNT.with(Cell::get)
}

static LAYOUT: Layout = Layout::new(Size::new(100.0, 60.0), &[]);

struct Plain(Pixmap);

impl RasterSource for Plain {
    fn size(&self) -> Size {
        LAYOUT.size
    }
    fn faceplate(&mut self, _scale: f32) -> &Pixmap {
        &self.0
    }
}

/// A panel that draws nothing, so only the compositor's own work counts.
struct Panel(Rect);

impl Overlay for Panel {
    fn rect(&self) -> Rect {
        self.0
    }
    fn draw(&self, _: &mut PixmapMut<'_>, _: Transform) {}
}

#[test]
fn a_render_with_two_rects_under_a_panel_allocates_nothing() {
    let mut c = Compositor::new(&LAYOUT);
    let (w, h) = c.frame_size();
    let mut raster = Plain(Pixmap::new(w, h).unwrap());
    let mut target = Pixmap::new(w, h).unwrap();
    c.set_overlays(vec![Box::new(Panel(Rect::new(5.0, 5.0, 80.0, 40.0)))]);
    let mut frame = |c: &mut Compositor| {
        c.damage(IRect::new(10, 10, 20, 4));
        c.damage(IRect::new(10, 30, 20, 4));
        let painted = c.render(&mut target.as_mut(), &mut raster, &[]).len();
        assert_eq!(painted, 1, "the two rows merge under the panel");
    };
    // The first frames size the buffers.
    frame(&mut c);
    frame(&mut c);
    assert_eq!(allocations(|| frame(&mut c)), 0);
}

static KNOB: [ControlSpec; 1] = [ControlSpec::new(
    ControlId(0),
    "knob",
    ControlKind::Knob,
    Rect::new(10.0, 10.0, 40.0, 40.0),
)];
static DIM_LAYOUT: Layout = Layout::new(Size::new(100.0, 60.0), &KNOB);

/// The knob's own drawing allocates (tiny-skia's paths); dimming it adds
/// nothing once warmed up.
#[test]
fn dimming_a_control_adds_no_allocations() {
    let count = |dimmed: bool| {
        let mut c = Compositor::new(&DIM_LAYOUT);
        let (w, h) = c.frame_size();
        let mut raster = Plain(Pixmap::new(w, h).unwrap());
        let mut target = Pixmap::new(w, h).unwrap();
        let mut state = WidgetState::value(0.5);
        state.dimmed = dimmed;
        let states = [state];
        let mut frame = |c: &mut Compositor| {
            c.damage_all();
            c.render(&mut target.as_mut(), &mut raster, &states);
        };
        frame(&mut c);
        frame(&mut c);
        allocations(|| frame(&mut c))
    };
    assert_eq!(count(true), count(false));
}
