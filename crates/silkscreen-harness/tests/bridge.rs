//! The editor wired to a real native bridge.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use silkscreen::bridge::{EngineEnd, EngineEvent, bridge};
use silkscreen::core::tiny_skia::{Color, Pixmap};
use silkscreen::core::{
    ControlKind, ControlSpec, Layout, Modifiers, MouseButton, Point, RasterSource, Rect, Size,
};
use silkscreen::{
    Binding, ControlId, Editor, EditorState, EngineLink, ParamIndex, ParamKnob, ParamSpec, RawKind,
};
use silkscreen_harness::Harness;

static SPECS: [ParamSpec; 2] = [
    ParamSpec::new("Gain", -60.0, 12.0, 0.0),
    ParamSpec::new("Pan", -1.0, 1.0, 0.0),
];
const GAIN: ParamIndex = ParamIndex(0);
const PAN: ParamIndex = ParamIndex(1);

static CONTROLS: [ControlSpec; 2] = [
    ControlSpec::new(
        ControlId(0),
        "gain",
        ControlKind::Knob,
        Rect::new(10.0, 10.0, 40.0, 40.0),
    ),
    ControlSpec::new(
        ControlId(1),
        "pan",
        ControlKind::Knob,
        Rect::new(60.0, 10.0, 40.0, 40.0),
    ),
];
static LAYOUT: Layout = Layout::new(Size::new(110.0, 60.0), &CONTROLS);
const KNOB_GAIN: ControlId = ControlId(0);
const KNOB_PAN: ControlId = ControlId(1);

struct Flat(Option<Pixmap>);
impl RasterSource for Flat {
    fn size(&self) -> Size {
        LAYOUT.size
    }
    fn faceplate(&mut self, scale: f32) -> &Pixmap {
        self.0.get_or_insert_with(|| {
            let (w, h) = LAYOUT.size.to_physical(scale);
            let mut p = Pixmap::new(w, h).unwrap();
            p.fill(Color::BLACK);
            p
        })
    }
}

fn editor(link: Box<dyn EngineLink>) -> Editor {
    let bindings: [Box<dyn Binding>; 2] = [
        Box::new(ParamKnob::new(GAIN)),
        Box::new(ParamKnob::new(PAN)),
    ];
    Editor::new(
        EditorState::new(&LAYOUT, &SPECS, bindings, link),
        Box::new(Flat(None)),
    )
}

fn wired() -> (Harness, EngineEnd) {
    let defaults: Vec<f32> = SPECS.iter().map(|s| s.default).collect();
    let (ui, engine) = bridge(&defaults, 256);
    (Harness::with_link(editor, Box::new(ui), 1.0), engine)
}

fn drained(engine: &mut EngineEnd) -> Vec<EngineEvent> {
    let mut out = Vec::new();
    engine.drain(|e| out.push(e));
    out
}

/// Press on `id` and move far enough to start a drag, leaving it open.
fn start_drag(h: &mut Harness, id: ControlId) {
    let c = h.centre(id);
    h.move_to(c);
    h.raw(RawKind::PointerDown {
        pos: c,
        button: MouseButton::Left,
        mods: Modifiers::NONE,
    });
    h.move_to(Point::new(c.x, c.y - 20.0));
}

fn finish_drag(h: &mut Harness, id: ControlId) {
    let c = h.centre(id);
    h.move_to(Point::new(c.x, c.y - 40.0));
    h.raw(RawKind::PointerUp {
        pos: Point::new(c.x, c.y - 40.0),
        button: MouseButton::Left,
        mods: Modifiers::NONE,
    });
}

#[test]
fn engine_thread_sees_the_drag_in_order() {
    let defaults: Vec<f32> = SPECS.iter().map(|s| s.default).collect();
    let (ui, mut engine) = bridge(&defaults, 256);
    let stop = Arc::new(AtomicBool::new(false));
    let audio = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            let mut seen = Vec::new();
            loop {
                let finished = stop.load(Ordering::Acquire);
                engine.drain(|e| seen.push(e));
                if finished {
                    return (seen, engine.read(GAIN));
                }
                std::thread::yield_now();
            }
        })
    };

    let mut h = Harness::with_link(editor, Box::new(ui), 1.0);
    h.drag(KNOB_GAIN, -20.0);
    let model_gain = h.model().get(GAIN);
    stop.store(true, Ordering::Release);
    let (seen, engine_gain) = audio.join().unwrap();

    let shape: String = seen
        .iter()
        .map(|e| match e {
            EngineEvent::GestureBegin { .. } => 'B',
            EngineEvent::ParamValue { .. } => 'V',
            EngineEvent::GestureEnd { .. } => 'E',
            EngineEvent::Shown { .. } => 'S',
            EngineEvent::PatchLoad => 'P',
        })
        .collect();
    assert_eq!(shape, "BVVVVE");
    assert!(seen.iter().all(|e| e.index() == Some(GAIN)));
    assert_eq!(engine_gain, model_gain);
}

#[test]
fn host_value_mid_drag_on_another_param_is_not_echoed() {
    let (mut h, mut engine) = wired();
    start_drag(&mut h, KNOB_PAN);
    drained(&mut engine);

    engine.publish_host(GAIN, -6.0);
    h.advance(16.0);
    assert_eq!(h.model().get(GAIN), -6.0, "model follows the host");
    h.snapshot();
    // Covered by a damage rect: one of its own, or merged with the drag
    // readout's (an overlay draws once).
    let knob = LAYOUT.control(KNOB_GAIN).rect.to_physical(1.0);
    assert!(
        h.last_damage()
            .iter()
            .any(|d| d.intersect(&knob) == Some(knob)),
        "the gain knob is repainted"
    );

    finish_drag(&mut h, KNOB_PAN);
    let events = drained(&mut engine);
    assert!(
        events.iter().all(|e| e.index() == Some(PAN)),
        "nothing for GAIN echoed: {events:?}"
    );
    assert_eq!(events.last(), Some(&EngineEvent::GestureEnd { index: PAN }));
    assert_eq!(engine.read(GAIN), -6.0);
}

#[test]
fn host_value_mid_drag_on_the_same_param_is_applied_then_overwritten() {
    // Documented behaviour: last writer wins. A host in touch-automation
    // mode doesn't write a param the user is holding.
    let (mut h, mut engine) = wired();
    start_drag(&mut h, KNOB_PAN);
    engine.publish_host(PAN, -0.5);
    h.advance(16.0);
    assert_eq!(h.model().get(PAN), -0.5);

    finish_drag(&mut h, KNOB_PAN);
    // The drag continues from its own position, not the host's.
    assert!(h.model().get(PAN) > 0.0);
    assert_eq!(engine.read(PAN), -0.5, "engine not yet drained");
    drained(&mut engine);
    assert_eq!(engine.read(PAN), h.model().get(PAN));
}
