//! `ParamFader` on a stepped param with many positions: a keyboard
//! split point over 128 notes.

use silkscreen_core::{
    ControlEvent, ControlId, ControlKind, ControlSpec, Layout, Modifiers, MouseButton, Point, Rect,
    Size, Vec2,
};
use silkscreen_model::{Binding, Controller, Model, ParamFader, ParamIndex, ParamSpec};

const NOTES: [&str; 8] = ["C", "D", "E", "F", "G", "A", "B", "C'"];
static SPECS: [ParamSpec; 1] = [ParamSpec::stepped("Split", &NOTES, 0)];
const FADER: ControlId = ControlId(0);
static CONTROLS: [ControlSpec; 1] = [ControlSpec::new(
    FADER,
    "split",
    ControlKind::Fader,
    // 20 wide: a 12-high cap, so 88 px of travel.
    Rect::new(0.0, 0.0, 20.0, 100.0),
)];
static LAYOUT: Layout = Layout::new(Size::new(20.0, 100.0), &CONTROLS);
const SPLIT: ParamIndex = ParamIndex(0);

fn rig() -> (Model, Controller) {
    let mut fader = ParamFader::new(SPLIT);
    fader
        .attach(&CONTROLS[0], &SPECS, &LAYOUT)
        .expect("a fader takes a stepped param");
    (
        Model::new(&SPECS),
        Controller::new(vec![Box::new(fader) as Box<dyn Binding>]),
    )
}

#[test]
fn a_drag_lands_on_positions() {
    let (mut model, mut c) = rig();
    let none = Modifiers::NONE;
    let at = Point::new(10.0, 94.0);
    for e in [
        ControlEvent::Press {
            pos: at,
            button: MouseButton::Left,
            mods: none,
        },
        ControlEvent::DragStart {
            pos: at,
            mods: none,
        },
        // Up 0.45 of the travel: 3.15 positions, so 3.
        ControlEvent::Drag {
            delta: Vec2::new(0.0, -39.6),
            pos: at,
            mods: none,
        },
        ControlEvent::DragEnd {
            over: Some(FADER),
            mods: none,
        },
    ] {
        c.handle_control(&mut model, FADER, &e);
    }
    assert_eq!(model.get(SPLIT), 3.0);
}

#[test]
fn the_wheel_steps_one_position() {
    let (mut model, mut c) = rig();
    let wheel = |dy| ControlEvent::Wheel {
        delta: Vec2::new(0.0, dy),
        mods: Modifiers::NONE,
    };
    c.handle_control(&mut model, FADER, &wheel(1.0));
    c.handle_control(&mut model, FADER, &wheel(1.0));
    assert_eq!(model.get(SPLIT), 2.0);
    c.handle_control(&mut model, FADER, &wheel(-1.0));
    assert_eq!(model.get(SPLIT), 1.0);
}
