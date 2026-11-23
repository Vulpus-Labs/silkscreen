//! `SyncedKnob`: the time running free, the division in length order
//! with sync on.

use silkscreen_core::{ControlEvent, ControlId, Modifiers, Point, Vec2};
use silkscreen_model::{Binding, Controller, Model, Origin, ParamIndex, ParamSpec, SyncedKnob};

const TIME: ParamIndex = ParamIndex(0);
const DIVISION: ParamIndex = ParamIndex(1);
const SYNC: ParamIndex = ParamIndex(2);
const KNOB: ControlId = ControlId(0);

/// Divisions listed out of length order: 1/4, 1/16, 1/8.
static SPECS: [ParamSpec; 3] = [
    ParamSpec::new("Time", 0.0, 1.0, 0.5),
    ParamSpec::stepped("Division", &["1/4", "1/16", "1/8"], 0),
    ParamSpec::toggle("Sync", false),
];
const LENGTHS: [f32; 3] = [1.0, 0.25, 0.5];

fn rig() -> (Model, Controller) {
    let bindings: Vec<Box<dyn Binding>> =
        vec![Box::new(SyncedKnob::new(TIME, DIVISION, SYNC, &LENGTHS))];
    (Model::new(&SPECS), Controller::new(bindings))
}

fn drag(model: &mut Model, controller: &mut Controller, dy: f32) {
    let mods = Modifiers::NONE;
    let pos = Point::new(0.0, 0.0);
    for event in [
        ControlEvent::DragStart { pos, mods },
        ControlEvent::Drag {
            delta: Vec2::new(0.0, dy),
            pos,
            mods,
        },
        ControlEvent::DragEnd { over: None, mods },
    ] {
        controller.handle_control(model, KNOB, &event);
    }
}

#[test]
fn running_free_it_turns_the_time() {
    let (mut model, mut controller) = rig();
    drag(&mut model, &mut controller, -40.0);
    assert!(model.get(TIME) > 0.5);
    assert_eq!(model.get(DIVISION), 0.0, "untouched");
    assert_eq!(
        controller.binding(KNOB).describe(&model),
        None,
        "the time's own readout"
    );
}

#[test]
fn synced_clockwise_is_always_longer() {
    let (mut model, mut controller) = rig();
    model.edit(Origin::Ui).set(SYNC, 1.0);
    // From the shortest, 1/16 (index 1), up a step at a time.
    model.edit(Origin::Ui).set(DIVISION, 1.0);
    assert_eq!(controller.binding(KNOB).display(&model).value, 0.0);
    drag(&mut model, &mut controller, -100.0);
    assert_eq!(model.get(DIVISION), 2.0, "1/8 next");
    drag(&mut model, &mut controller, -100.0);
    assert_eq!(model.get(DIVISION), 0.0, "then 1/4");
    assert_eq!(model.get(TIME), 0.5, "the time untouched");
    assert_eq!(
        controller.binding(KNOB).describe(&model).as_deref(),
        Some("Division  1/4")
    );
}

#[test]
fn sync_turned_on_mid_drag_still_closes_the_time_gesture() {
    use silkscreen_model::ModelEvent;
    let (mut model, mut controller) = rig();
    let mut cursor = model.log().cursor();
    let mods = Modifiers::NONE;
    let pos = Point::new(0.0, 0.0);
    controller.handle_control(&mut model, KNOB, &ControlEvent::DragStart { pos, mods });
    controller.handle_control(
        &mut model,
        KNOB,
        &ControlEvent::Drag {
            delta: Vec2::new(0.0, -10.0),
            pos,
            mods,
        },
    );
    // Automation turns sync on mid-drag.
    model.edit(Origin::Host).set(SYNC, 1.0);
    controller.handle_control(
        &mut model,
        KNOB,
        &ControlEvent::Drag {
            delta: Vec2::new(0.0, -10.0),
            pos,
            mods,
        },
    );
    controller.handle_control(
        &mut model,
        KNOB,
        &ControlEvent::DragEnd { over: None, mods },
    );
    let ends = model
        .log()
        .read(&mut cursor)
        .filter(|e| matches!(e, ModelEvent::GestureEnd { param, .. } if *param == TIME))
        .count();
    assert_eq!(ends, 1, "the time's gesture closed");
    assert!(model.undo(), "and is one undo step");
}

#[test]
fn empty_lengths_are_refused_at_attach() {
    use silkscreen_core::{ControlKind, ControlSpec, Layout, Rect, Size};
    static CONTROLS: [ControlSpec; 1] = [ControlSpec::new(
        KNOB,
        "time",
        ControlKind::Knob,
        Rect::new(0.0, 0.0, 10.0, 10.0),
    )];
    static LAYOUT: Layout = Layout::new(Size::new(10.0, 10.0), &CONTROLS);
    let mut knob = SyncedKnob::new(TIME, DIVISION, SYNC, &[]);
    assert!(knob.attach(&CONTROLS[0], &SPECS, &LAYOUT).is_err());
}
