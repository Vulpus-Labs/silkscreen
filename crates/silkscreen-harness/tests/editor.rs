//! The `Editor` host contract, driven through the harness, with a
//! hand-written layout (no build script).

use silkscreen::bridge::{EngineEvent, NullLink};
use silkscreen::core::tiny_skia::{Color, Pixmap};
use silkscreen::core::{
    ControlKind, ControlSpec, Layout, Modifiers, Point, RasterSource, Rect, Size,
};
use silkscreen::model::binding;
use silkscreen::{
    Binding, ControlId, Editor, EditorState, EditorView, EngineLink, IRect, ParamIndex, ParamKnob,
    ParamSpec,
};
use silkscreen_harness::{Harness, SnapshotError, check_snapshot_with};

static SPECS: [ParamSpec; 2] = [
    ParamSpec::new("Gain", -60.0, 12.0, 0.0),
    ParamSpec::new("Pan", -1.0, 1.0, 0.0),
];
const GAIN: ParamIndex = ParamIndex(0);
const PAN: ParamIndex = ParamIndex(1);

static CONTROLS: [ControlSpec; 3] = [
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
    // A second view of gain: proves damage follows params, not controls.
    ControlSpec::new(
        ControlId(2),
        "gain_mirror",
        ControlKind::Knob,
        Rect::new(110.0, 10.0, 20.0, 20.0),
    ),
];
static LAYOUT: Layout = Layout::new(Size::new(140.0, 60.0), &CONTROLS);
const KNOB_GAIN: ControlId = ControlId(0);
const KNOB_PAN: ControlId = ControlId(1);
const KNOB_MIRROR: ControlId = ControlId(2);

struct FlatRaster(Option<Pixmap>);

impl RasterSource for FlatRaster {
    fn size(&self) -> Size {
        LAYOUT.size
    }
    fn faceplate(&mut self, scale: f32) -> &Pixmap {
        let (w, h) = LAYOUT.size.to_physical(scale);
        if self
            .0
            .as_ref()
            .is_none_or(|p| (p.width(), p.height()) != (w, h))
        {
            let mut p = Pixmap::new(w, h).unwrap();
            p.fill(Color::from_rgba8(30, 30, 34, 255));
            self.0 = Some(p);
        }
        self.0.as_ref().unwrap()
    }
}

fn state(link: Box<dyn EngineLink>) -> EditorState {
    let bindings: [Box<dyn Binding>; 3] = [
        Box::new(ParamKnob::new(GAIN)),
        Box::new(ParamKnob::new(PAN)),
        Box::new(ParamKnob::new(GAIN)),
    ];
    EditorState::new(&LAYOUT, &SPECS, bindings, link)
}

fn editor(link: Box<dyn EngineLink>) -> Editor {
    Editor::new(state(link), Box::new(FlatRaster(None)))
}

fn rect(id: ControlId, scale: f32) -> IRect {
    LAYOUT.control(id).rect.to_physical(scale)
}

#[test]
fn drag_reaches_model_and_engine_bracketed() {
    let mut h = Harness::new(editor, 1.0);
    h.drag(KNOB_GAIN, -50.0);
    // 200px = 72 dB, so 50px up from 0 dB = +18 dB, clamped to +12.
    assert_eq!(h.model().get(GAIN), 12.0);
    let events = h.engine_events();
    assert_eq!(
        events.first(),
        Some(&EngineEvent::GestureBegin { index: GAIN })
    );
    assert_eq!(
        events.last(),
        Some(&EngineEvent::GestureEnd { index: GAIN })
    );
    assert!(
        events[1..events.len() - 1]
            .iter()
            .all(|e| matches!(e, EngineEvent::ParamValue { index, .. } if *index == GAIN))
    );
    assert_eq!(
        events[events.len() - 2],
        EngineEvent::ParamValue {
            index: GAIN,
            value: 12.0
        }
    );
}

#[test]
fn param_change_damages_every_control_showing_it() {
    let mut h = Harness::new(editor, 1.0);
    h.move_to(Point::new(0.0, 0.0));
    h.snapshot();
    h.wheel(KNOB_GAIN, 1.0);
    h.snapshot();
    let damage = h.last_damage().to_vec();
    assert!(damage.contains(&rect(KNOB_GAIN, 1.0)), "{damage:?}");
    assert!(damage.contains(&rect(KNOB_MIRROR, 1.0)), "{damage:?}");
    assert!(!damage.contains(&rect(KNOB_PAN, 1.0)), "{damage:?}");
}

#[test]
fn idle_render_paints_nothing() {
    let mut h = Harness::new(editor, 1.5);
    h.snapshot();
    assert!(h.last_damage().is_empty());
    h.advance(100.0);
    h.snapshot();
    assert!(h.last_damage().is_empty());
}

#[test]
fn hover_damages_the_hovered_control() {
    let mut h = Harness::new(editor, 1.0);
    h.move_to(Point::new(0.0, 0.0));
    h.snapshot();
    h.move_to(LAYOUT.control(KNOB_PAN).rect.center());
    h.snapshot();
    assert_eq!(h.last_damage(), [rect(KNOB_PAN, 1.0)]);
    h.move_to(Point::new(0.0, 0.0));
    h.snapshot();
    assert_eq!(h.last_damage(), [rect(KNOB_PAN, 1.0)]);
}

#[test]
fn host_changes_update_model_and_view_but_not_engine() {
    let mut h = Harness::new(editor, 1.0);
    h.snapshot();
    h.host_change(PAN, -0.5).advance(16.0);
    assert_eq!(h.model().get(PAN), -0.5);
    assert!(h.engine_events().is_empty(), "host values must not echo");
    h.snapshot();
    assert_eq!(h.last_damage(), [rect(KNOB_PAN, 1.0)]);
}

#[test]
fn state_load_updates_the_view_and_asks_the_engine_to_load() {
    let mut h = Harness::new(editor, 1.0);
    h.drag(KNOB_PAN, -20.0);
    h.move_to(Point::new(0.0, 0.0));
    h.snapshot();
    let _ = h.engine_events();
    assert!(h.editor().state.model().can_undo());

    // Gain is in the state; pan is missing, so it returns to its default.
    h.host_load(&[(GAIN, -6.0)]);
    h.snapshot();
    let damage = h.last_damage().to_vec();
    for id in [KNOB_GAIN, KNOB_PAN, KNOB_MIRROR] {
        assert!(damage.contains(&rect(id, 1.0)), "{id:?}: {damage:?}");
    }
    assert_eq!((h.model().get(GAIN), h.model().get(PAN)), (-6.0, 0.0));
    assert!(h.engine_events().is_empty(), "no edits, no brackets");
    assert_eq!(h.engine_loads(), [vec![-6.0, 0.0]]);
    assert!(!h.editor().state.model().can_undo());
}

#[test]
fn double_click_resets_and_cmd_click_resets() {
    let mut h = Harness::new(editor, 1.0);
    h.drag(KNOB_PAN, -50.0);
    assert_eq!(h.model().get(PAN), 0.5);
    h.advance(1000.0).double_click(KNOB_PAN);
    assert_eq!(h.model().get(PAN), 0.0);

    h.drag(KNOB_PAN, -50.0).advance(1000.0);
    h.click_with(KNOB_PAN, Modifiers::CMD);
    assert_eq!(h.model().get(PAN), 0.0);
}

#[test]
fn view_can_be_dropped_and_recreated_over_live_state() {
    let mut ed = editor(Box::new(NullLink));
    let press = |ed: &mut Editor, t, kind| ed.handle(&silkscreen::RawEvent::new(t, kind));
    let c = LAYOUT.control(KNOB_PAN).rect.center();
    press(
        &mut ed,
        0.0,
        silkscreen::RawKind::PointerDown {
            pos: c,
            button: silkscreen::core::MouseButton::Left,
            mods: Modifiers::NONE,
        },
    );
    press(
        &mut ed,
        16.0,
        silkscreen::RawKind::PointerMove {
            pos: Point::new(c.x, c.y - 100.0),
            mods: Modifiers::NONE,
        },
    );
    assert!(ed.state.model().in_gesture(PAN));

    // Close the window mid-drag: cancel ends the gesture, state survives.
    let Editor {
        mut state,
        mut view,
    } = ed;
    view.cancel(&mut state);
    drop(view);
    assert!(!state.model().in_gesture(PAN));
    assert_eq!(state.model().get(PAN), 1.0);

    // State keeps working with no view.
    state.handle_host(silkscreen::HostEvent::ParamChanged {
        param: GAIN,
        value: -6.0,
    });
    let _ = state.pump();

    let mut view = EditorView::new(&mut state, Box::new(FlatRaster(None)));
    view.set_scale(2.0);
    let (w, h) = view.physical_size();
    assert_eq!((w, h), (280, 120));
    let mut frame = Pixmap::new(w, h).unwrap();
    let painted = view.render(&mut state, &mut frame.as_mut()).to_vec();
    assert_eq!(painted, [IRect::frame(280, 120)]);
    assert_eq!(state.model().get(GAIN), -6.0);
}

#[test]
fn log_is_compacted_as_consumers_read() {
    let mut h = Harness::new(editor, 1.0);
    for _ in 0..10 {
        h.drag(KNOB_GAIN, -5.0).advance(500.0);
    }
    h.snapshot();
    assert!(
        h.model().log().is_empty(),
        "both cursors have read everything"
    );
}

#[test]
fn custom_bindings_receive_events() {
    let make = |link: Box<dyn EngineLink>| {
        let toggle = binding::from_fn(
            &[PAN],
            |ev, edit| {
                if matches!(ev, silkscreen::core::ControlEvent::Click { .. }) {
                    edit.reset(PAN);
                }
            },
            |m| silkscreen::core::WidgetState::value(m.get_normalized(PAN)),
        );
        let bindings: [Box<dyn Binding>; 3] = [
            Box::new(ParamKnob::new(GAIN)),
            Box::new(toggle),
            Box::new(ParamKnob::new(GAIN)),
        ];
        Editor::new(
            EditorState::new(&LAYOUT, &SPECS, bindings, link),
            Box::new(FlatRaster(None)),
        )
    };
    let mut h = Harness::new(make, 1.0);
    h.host_change(PAN, 0.7).advance(16.0);
    h.click(KNOB_PAN);
    assert_eq!(h.model().get(PAN), 0.0);
    // reset() is a begin/set/end bracket, all on PAN.
    let params: Vec<_> = h.engine_events().iter().map(EngineEvent::index).collect();
    assert_eq!(params, [Some(PAN); 3]);
}

#[test]
fn snapshot_missing_bless_match_and_mismatch() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("snapshots-selftest");
    let _ = std::fs::remove_dir_all(&dir);
    let mut h = Harness::new(editor, 1.0);
    let frame = h.snapshot().clone();

    match check_snapshot_with(&frame, &dir, "frame", false) {
        Err(SnapshotError::Missing { .. }) => {}
        other => panic!("{other:?}"),
    }
    check_snapshot_with(&frame, &dir, "frame", true).unwrap();
    check_snapshot_with(&frame, &dir, "frame", false).unwrap();

    h.drag(KNOB_GAIN, -30.0);
    let changed = h.snapshot().clone();
    let err = check_snapshot_with(&changed, &dir, "frame", false).unwrap_err();
    let SnapshotError::PixelMismatch {
        differing,
        actual_path,
        diff_path,
        ..
    } = &err
    else {
        panic!("{err:?}")
    };
    assert!(*differing > 0);
    assert!(actual_path.exists() && diff_path.exists());
    assert!(err.to_string().contains("SILKSCREEN_BLESS=1"));

    // Blessing clears the leftovers.
    check_snapshot_with(&changed, &dir, "frame", true).unwrap();
    assert!(!actual_path.exists() && !diff_path.exists());
}

#[test]
fn undo_reaches_the_engine_as_a_gesture() {
    let mut h = Harness::new(editor, 1.0);
    h.drag(KNOB_PAN, -50.0);
    h.engine_events();
    assert!(h.editor_mut().undo());
    assert_eq!(h.model().get(PAN), 0.0);
    assert_eq!(
        h.engine_events(),
        [
            EngineEvent::GestureBegin { index: PAN },
            EngineEvent::ParamValue {
                index: PAN,
                value: 0.0
            },
            EngineEvent::GestureEnd { index: PAN },
        ]
    );
    // And the view repaints the knob.
    h.snapshot();
    assert!(h.last_damage().contains(&rect(KNOB_PAN, 1.0)));
    assert!(h.editor_mut().redo());
    assert_eq!(h.model().get(PAN), 0.5);
}

/// A preset load tells the engine a whole patch follows, before the
/// load's gestures, so an engine can fade the old patch out.
#[test]
fn a_preset_load_marks_a_patch_load_before_its_gestures() {
    let mut h = Harness::new(editor, 1.0);
    h.snapshot();
    let _ = h.engine_events();
    let preset = silkscreen::model::Preset::capture("Quiet", &SPECS, [-12.0, 0.0]);
    h.editor_mut().state.load_preset(&preset);
    let events = h.engine_events();
    assert_eq!(events[0], EngineEvent::PatchLoad, "{events:?}");
    assert_eq!(events[1], EngineEvent::GestureBegin { index: GAIN });
    assert!(
        !events[1..].contains(&EngineEvent::PatchLoad),
        "one mark: {events:?}"
    );
}
