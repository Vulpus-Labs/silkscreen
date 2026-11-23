//! `ParamKnob` through the controller and model, no rendering.
#![allow(clippy::needless_pass_by_value)]

use silkscreen_core::{ControlEvent, ControlId, Modifiers, Point, Vec2};
use silkscreen_model::{
    Binding, Controller, Cursor, HostEvent, Model, ModelEvent, Origin, ParamIndex, ParamKnob,
    ParamSpec, binding,
};

static SPECS: [ParamSpec; 2] = [
    ParamSpec::new("Mix", 0.0, 1.0, 0.5),
    ParamSpec::new("Gain", -60.0, 12.0, 0.0),
];
const MIX: ParamIndex = ParamIndex(0);
const GAIN: ParamIndex = ParamIndex(1);
const KNOB: ControlId = ControlId(0);

struct Rig {
    model: Model,
    controller: Controller,
    cursor: Cursor,
}

impl Rig {
    fn new() -> Self {
        let model = Model::new(&SPECS);
        let cursor = model.log().cursor();
        let bindings: Vec<Box<dyn Binding>> = vec![Box::new(ParamKnob::new(MIX))];
        Self {
            model,
            controller: Controller::new(bindings),
            cursor,
        }
    }

    fn send(&mut self, event: ControlEvent) {
        self.controller
            .handle_control(&mut self.model, KNOB, &event);
    }

    fn mix(&self) -> f32 {
        self.model.get(MIX)
    }

    /// Drag by `dys` in successive moves, with optional shift per move.
    fn drag(&mut self, moves: &[(f32, bool)]) {
        self.send(ControlEvent::DragStart {
            pos: Point::new(0.0, 0.0),
            mods: Modifiers::NONE,
        });
        for &(dy, shift) in moves {
            self.send(ControlEvent::Drag {
                delta: Vec2::new(0.0, dy),
                pos: Point::new(0.0, 0.0),
                mods: Modifiers {
                    shift,
                    ..Modifiers::NONE
                },
            });
        }
        self.send(ControlEvent::DragEnd {
            over: Some(KNOB),
            mods: Modifiers::NONE,
        });
    }

    fn events(&mut self) -> Vec<ModelEvent> {
        self.model.log().read(&mut self.cursor).copied().collect()
    }

    /// The log reduced to "B", "E" and "C" (begin, end, changed).
    fn shape(&mut self) -> String {
        self.events()
            .iter()
            .map(|e| match e {
                ModelEvent::GestureBegin { .. } => 'B',
                ModelEvent::GestureEnd { .. } => 'E',
                ModelEvent::ParamChanged { .. } => 'C',
            })
            .collect()
    }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-5
}

#[test]
fn drag_up_increases_and_clamps() {
    let mut r = Rig::new();
    r.drag(&[(-100.0, false)]);
    assert_eq!(r.mix(), 1.0);
}

#[test]
fn drag_down_full_range() {
    let mut r = Rig::new();
    r.drag(&[(-100.0, false)]);
    r.drag(&[(200.0, false)]);
    assert_eq!(r.mix(), 0.0);
}

#[test]
fn shift_is_fine() {
    let mut r = Rig::new();
    r.drag(&[(-100.0, true)]);
    assert!(close(r.mix(), 0.55), "{}", r.mix());
}

#[test]
fn switching_fine_mid_drag_does_not_jump() {
    let mut r = Rig::new();
    // 20px coarse = +0.1, then 20px fine = +0.01, then 20px coarse = +0.1.
    r.drag(&[(-20.0, false), (-20.0, true), (-20.0, false)]);
    assert!(close(r.mix(), 0.71), "{}", r.mix());
}

#[test]
fn overshoot_is_clamped_so_reversal_responds_immediately() {
    let mut r = Rig::new();
    // Up 300px (0.5 → 1.0, with 0.5 of overshoot), then down 20px.
    r.drag(&[(-300.0, false), (20.0, false)]);
    assert!(close(r.mix(), 0.9), "{}", r.mix());
}

#[test]
fn double_click_resets() {
    let mut r = Rig::new();
    r.drag(&[(-40.0, false)]);
    r.send(ControlEvent::Click {
        count: 1,
        mods: Modifiers::NONE,
    });
    assert!(close(r.mix(), 0.7));
    r.send(ControlEvent::Click {
        count: 2,
        mods: Modifiers::NONE,
    });
    assert_eq!(r.mix(), 0.5);
}

#[test]
fn cmd_click_resets() {
    let mut r = Rig::new();
    r.drag(&[(-40.0, false)]);
    r.send(ControlEvent::Click {
        count: 1,
        mods: Modifiers::CMD,
    });
    assert_eq!(r.mix(), 0.5);
}

#[test]
fn wheel_steps_one_percent_and_brackets() {
    let mut r = Rig::new();
    r.events();
    r.send(ControlEvent::Wheel {
        delta: Vec2::new(0.0, 1.0),
        mods: Modifiers::NONE,
    });
    assert!(close(r.mix(), 0.51));
    r.send(ControlEvent::Wheel {
        delta: Vec2::new(0.0, -3.0),
        mods: Modifiers::NONE,
    });
    assert!(close(r.mix(), 0.48));
    assert_eq!(r.shape(), "BCEBCE");
}

#[test]
fn every_edit_path_is_bracketed() {
    let mut r = Rig::new();
    r.drag(&[(-10.0, false), (-10.0, true), (5.0, false)]);
    assert_eq!(r.shape(), "BCCCE");
    r.send(ControlEvent::Click {
        count: 2,
        mods: Modifiers::NONE,
    });
    assert_eq!(r.shape(), "BCE");
    r.send(ControlEvent::Wheel {
        delta: Vec2::new(0.0, 1.0),
        mods: Modifiers::NONE,
    });
    assert_eq!(r.shape(), "BCE");
    // A drag that ends where it started still closes its gesture.
    r.drag(&[(-10.0, false), (10.0, false)]);
    assert_eq!(r.shape(), "BCCE");
    assert!(!r.model.in_gesture(MIX));
}

#[test]
fn stray_drag_events_without_start_are_ignored() {
    let mut r = Rig::new();
    r.events();
    r.send(ControlEvent::Drag {
        delta: Vec2::new(0.0, -50.0),
        pos: Point::new(0.0, 0.0),
        mods: Modifiers::NONE,
    });
    r.send(ControlEvent::DragEnd {
        over: None,
        mods: Modifiers::NONE,
    });
    assert_eq!(r.shape(), "");
}

#[test]
fn host_events_apply_without_bindings() {
    let mut r = Rig::new();
    r.controller.handle_host(
        &mut r.model,
        HostEvent::ParamChanged {
            param: GAIN,
            value: -6.0,
        },
    );
    assert_eq!(r.model.get(GAIN), -6.0);
    assert_eq!(
        r.events(),
        [ModelEvent::ParamChanged {
            param: GAIN,
            value: -6.0,
            origin: Origin::Host
        }]
    );
}

#[test]
fn display_reports_normalized_value() {
    let r = Rig::new();
    let b = r.controller.binding(KNOB);
    assert_eq!(b.display(&r.model).value, 0.5);
    assert_eq!(b.params(), [MIX]);
}

#[test]
fn closure_bindings() {
    let mut model = Model::new(&SPECS);
    let toggle = binding::from_fn(
        &[MIX],
        |ev, edit| {
            if let ControlEvent::Click { .. } = ev {
                let next = if edit.get(MIX) > 0.5 { 0.0 } else { 1.0 };
                edit.begin(MIX);
                edit.set(MIX, next);
                edit.end(MIX);
            }
        },
        |m| silkscreen_core::WidgetState::value(m.get(MIX)),
    );
    let mut controller = Controller::new([Box::new(toggle) as Box<dyn Binding>]);
    let click = ControlEvent::Click {
        count: 1,
        mods: Modifiers::NONE,
    };
    controller.handle_control(&mut model, KNOB, &click);
    assert_eq!(model.get(MIX), 1.0);
    controller.handle_control(&mut model, KNOB, &click);
    assert_eq!(model.get(MIX), 0.0);
    assert_eq!(controller.binding(KNOB).params(), [MIX]);
}

#[test]
fn described_bindings_say_their_own_tooltip() {
    use silkscreen_model::BindingExt;
    let model = Model::new(&SPECS);
    let plain = ParamKnob::new(ParamIndex(0));
    assert_eq!(plain.describe(&model), None);
    let said = ParamKnob::new(ParamIndex(0))
        .describe_with(|m| Some(format!("at {}", m.get(ParamIndex(0)))));
    assert_eq!(said.describe(&model).as_deref(), Some("at 0.5"));
    assert_eq!(said.params(), plain.params());
}

/// A knob on a stepped param (a wave rotary): drags land on
/// positions, the wheel steps one, the widget gets the detent count.
#[test]
fn a_knob_drives_a_stepped_param_position_by_position() {
    use silkscreen_core::{ControlKind, ControlSpec, Layout, Rect, Size};
    static WAVE_SPECS: [ParamSpec; 1] = [ParamSpec::stepped(
        "Wave",
        &["Sine", "Tri", "Saw", "Pulse"],
        0,
    )];
    static CONTROLS: [ControlSpec; 1] = [ControlSpec::new(
        KNOB,
        "wave",
        ControlKind::Knob,
        Rect::new(0.0, 0.0, 40.0, 40.0),
    )];
    static LAYOUT: Layout = Layout::new(Size::new(40.0, 40.0), &CONTROLS);
    let wave = ParamIndex(0);
    let mut knob = ParamKnob::new(wave);
    knob.attach(&CONTROLS[0], &WAVE_SPECS, &LAYOUT)
        .expect("a knob takes a stepped param");
    let mut model = Model::new(&WAVE_SPECS);
    assert_eq!(knob.display(&model).steps, 4, "the widget draws 4 detents");
    let mut controller = Controller::new(vec![Box::new(knob) as Box<dyn Binding>]);
    let mut send = |model: &mut Model, e: ControlEvent| controller.handle_control(model, KNOB, &e);

    send(
        &mut model,
        ControlEvent::DragStart {
            pos: Point::new(0.0, 0.0),
            mods: Modifiers::NONE,
        },
    );
    // Up 110 of 200 px: 0.55 of the range, nearest position 2.
    send(
        &mut model,
        ControlEvent::Drag {
            delta: Vec2::new(0.0, -110.0),
            pos: Point::new(0.0, 0.0),
            mods: Modifiers::NONE,
        },
    );
    send(
        &mut model,
        ControlEvent::DragEnd {
            over: Some(KNOB),
            mods: Modifiers::NONE,
        },
    );
    assert_eq!(model.get(wave), 2.0);
    send(
        &mut model,
        ControlEvent::Wheel {
            delta: Vec2::new(0.0, 1.0),
            mods: Modifiers::NONE,
        },
    );
    assert_eq!(model.get(wave), 3.0);
    send(
        &mut model,
        ControlEvent::Wheel {
            delta: Vec2::new(0.0, 1.0),
            mods: Modifiers::NONE,
        },
    );
    assert_eq!(model.get(wave), 3.0, "clamped at the last position");
}
