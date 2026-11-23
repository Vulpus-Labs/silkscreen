//! Undo/redo from gesture history (design/model.md).

use silkscreen_core::{ControlEvent, ControlId, Modifiers, Point, Vec2};
use silkscreen_model::{
    Binding, Controller, HostEvent, Model, ModelEvent, Origin, ParamIndex, ParamKnob, ParamSpec,
};

static SPECS: [ParamSpec; 2] = [
    ParamSpec::new("Mix", 0.0, 1.0, 0.5),
    ParamSpec::new("Gain", -60.0, 12.0, 0.0),
];
const MIX: ParamIndex = ParamIndex(0);
const GAIN: ParamIndex = ParamIndex(1);
const MIX_KNOB: ControlId = ControlId(0);
const GAIN_KNOB: ControlId = ControlId(1);

struct Rig {
    model: Model,
    controller: Controller,
}

impl Rig {
    fn new() -> Self {
        let bindings: [Box<dyn Binding>; 2] = [
            Box::new(ParamKnob::new(MIX)),
            Box::new(ParamKnob::new(GAIN)),
        ];
        Self {
            model: Model::new(&SPECS),
            controller: Controller::new(bindings),
        }
    }

    fn send(&mut self, id: ControlId, event: &ControlEvent) {
        self.controller.handle_control(&mut self.model, id, event);
    }

    fn drag(&mut self, id: ControlId, dys: &[f32]) {
        let mods = Modifiers::NONE;
        let pos = Point::new(0.0, 0.0);
        self.send(id, &ControlEvent::DragStart { pos, mods });
        for &dy in dys {
            self.send(
                id,
                &ControlEvent::Drag {
                    delta: Vec2::new(0.0, dy),
                    pos,
                    mods,
                },
            );
        }
        self.send(id, &ControlEvent::DragEnd { over: None, mods });
    }

    fn wheel(&mut self, id: ControlId, lines: f32) {
        self.send(
            id,
            &ControlEvent::Wheel {
                delta: Vec2::new(0.0, lines),
                mods: Modifiers::NONE,
            },
        );
    }

    fn double_click(&mut self, id: ControlId) {
        self.send(
            id,
            &ControlEvent::Click {
                count: 2,
                mods: Modifiers::NONE,
            },
        );
    }

    fn mix(&self) -> f32 {
        self.model.get(MIX)
    }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-5
}

#[test]
fn drag_reset_and_wheel_each_make_one_entry() {
    let mut r = Rig::new();
    r.drag(MIX_KNOB, &[-10.0, -10.0, -10.0]); // 0.5 → 0.65
    r.double_click(MIX_KNOB); // → 0.5
    r.wheel(MIX_KNOB, 2.0); // → 0.52
    assert!(close(r.mix(), 0.52));

    assert!(r.model.undo());
    assert!(close(r.mix(), 0.5), "undid wheel: {}", r.mix());
    assert!(r.model.undo());
    assert!(close(r.mix(), 0.65), "undid reset: {}", r.mix());
    assert!(r.model.undo());
    assert!(close(r.mix(), 0.5), "undid drag: {}", r.mix());
    assert!(!r.model.can_undo());
    assert!(!r.model.undo());
}

#[test]
fn gesture_ending_where_it_began_makes_no_entry() {
    let mut r = Rig::new();
    r.drag(MIX_KNOB, &[-20.0, 20.0]);
    r.double_click(MIX_KNOB); // reset at default: no change
    assert!(!r.model.can_undo());
}

#[test]
fn redo_reapplies_and_new_edit_clears_redo() {
    let mut r = Rig::new();
    r.drag(MIX_KNOB, &[-40.0]); // → 0.7
    r.model.undo();
    assert!(close(r.mix(), 0.5));
    assert!(r.model.redo());
    assert!(close(r.mix(), 0.7));
    assert!(!r.model.redo());

    r.model.undo();
    assert!(r.model.can_redo());
    r.wheel(GAIN_KNOB, 1.0);
    assert!(!r.model.can_redo(), "a new edit clears redo");
}

#[test]
fn undo_and_redo_are_bracketed_undo_gestures() {
    let mut r = Rig::new();
    r.drag(MIX_KNOB, &[-40.0]);
    let mut cursor = r.model.log().cursor();
    r.model.undo();
    r.model.redo();
    let events: Vec<ModelEvent> = r.model.log().read(&mut cursor).copied().collect();
    let undo = Origin::Undo;
    assert_eq!(
        events,
        [
            ModelEvent::GestureBegin {
                param: MIX,
                origin: undo
            },
            ModelEvent::ParamChanged {
                param: MIX,
                value: 0.5,
                origin: undo
            },
            ModelEvent::GestureEnd {
                param: MIX,
                origin: undo
            },
            ModelEvent::GestureBegin {
                param: MIX,
                origin: undo
            },
            ModelEvent::ParamChanged {
                param: MIX,
                value: r.mix(),
                origin: undo
            },
            ModelEvent::GestureEnd {
                param: MIX,
                origin: undo
            },
        ]
    );
    // Undo/redo themselves are not recorded as new entries.
    assert!(r.model.can_undo());
    r.model.undo();
    assert!(!r.model.can_undo());
}

#[test]
fn host_changes_are_never_recorded() {
    let mut r = Rig::new();
    r.controller.handle_host(
        &mut r.model,
        HostEvent::ParamChanged {
            param: GAIN,
            value: -12.0,
        },
    );
    let mut e = r.model.edit(Origin::Host);
    e.begin(GAIN);
    e.set(GAIN, -24.0);
    e.end(GAIN);
    assert!(!r.model.can_undo());
}

#[test]
fn undo_is_refused_mid_gesture() {
    let mut r = Rig::new();
    r.drag(MIX_KNOB, &[-20.0]);
    let pos = Point::new(0.0, 0.0);
    r.send(
        GAIN_KNOB,
        &ControlEvent::DragStart {
            pos,
            mods: Modifiers::NONE,
        },
    );
    assert!(!r.model.undo());
    r.send(
        GAIN_KNOB,
        &ControlEvent::DragEnd {
            over: None,
            mods: Modifiers::NONE,
        },
    );
    assert!(r.model.undo());
}

#[test]
fn depth_is_capped() {
    let mut r = Rig::new();
    r.model.set_undo_depth(3);
    for _ in 0..5 {
        r.wheel(MIX_KNOB, 1.0); // 0.51 … 0.55
    }
    let mut undone = 0;
    while r.model.undo() {
        undone += 1;
    }
    assert_eq!(undone, 3);
    assert!(close(r.mix(), 0.52));
}

#[test]
fn state_load_sets_every_param_and_clears_history() {
    let mut r = Rig::new();
    r.drag(MIX_KNOB, &[-20.0]);
    r.drag(GAIN_KNOB, &[-20.0]);
    r.model.undo();
    assert!(r.model.can_undo() && r.model.can_redo());

    let mut c = r.model.log().cursor();
    r.controller
        .handle_host(&mut r.model, HostEvent::StateLoaded(vec![(GAIN, -6.0)]));
    assert_eq!(r.model.get(GAIN), -6.0);
    assert_eq!(r.mix(), 0.5, "missing from the state: back to default");
    assert!(!r.model.can_undo() && !r.model.can_redo());

    let events: Vec<ModelEvent> = r.model.log().read(&mut c).copied().collect();
    assert_eq!(
        events,
        [
            ModelEvent::ParamChanged {
                param: MIX,
                value: 0.5,
                origin: Origin::Host
            },
            ModelEvent::ParamChanged {
                param: GAIN,
                value: -6.0,
                origin: Origin::Host
            },
        ],
        "host-origin values, no gesture brackets"
    );
}

#[test]
fn state_load_ignores_unknown_and_non_finite_entries() {
    let mut r = Rig::new();
    r.wheel(MIX_KNOB, 1.0);
    r.controller.handle_host(
        &mut r.model,
        HostEvent::StateLoaded(vec![(ParamIndex(9), 1.0), (MIX, f32::NAN), (GAIN, 100.0)]),
    );
    assert_eq!(r.mix(), 0.5);
    assert_eq!(r.model.get(GAIN), 12.0, "clamped like any host value");
}

#[test]
fn a_gesture_open_across_a_load_records_only_what_follows() {
    let mut r = Rig::new();
    let pos = Point::new(0.0, 0.0);
    let mods = Modifiers::NONE;
    r.send(MIX_KNOB, &ControlEvent::DragStart { pos, mods });
    r.controller
        .handle_host(&mut r.model, HostEvent::StateLoaded(vec![(MIX, 0.2)]));
    r.send(MIX_KNOB, &ControlEvent::DragEnd { over: None, mods });
    assert_eq!(r.mix(), 0.2);
    assert!(
        !r.model.can_undo(),
        "undo must not jump back across the load"
    );
}

#[test]
fn discrete_params_snap_through_edits_and_loads() {
    static DISCRETE: [ParamSpec; 2] = [
        ParamSpec::stepped("Wave", &["A", "B", "C", "D"], 0),
        ParamSpec::toggle("Stop", false),
    ];
    const WAVE: ParamIndex = ParamIndex(0);
    const STOP: ParamIndex = ParamIndex(1);
    let mut model = Model::new(&DISCRETE);
    let mut controller = Controller::new(Vec::<Box<dyn Binding>>::new());
    model.edit(Origin::Ui).set(WAVE, 1.6);
    assert_eq!(model.get(WAVE), 2.0);
    model.edit(Origin::Ui).set_normalized(WAVE, 0.9);
    assert_eq!(model.get(WAVE), 3.0);
    controller.handle_host(
        &mut model,
        HostEvent::ParamChanged {
            param: STOP,
            value: 0.7,
        },
    );
    assert_eq!(model.get(STOP), 1.0);
    controller.handle_host(&mut model, HostEvent::StateLoaded(vec![(WAVE, 0.4)]));
    assert_eq!((model.get(WAVE), model.get(STOP)), (0.0, 0.0));
}

#[test]
fn a_group_undoes_and_redoes_as_one_step() {
    let mut model = Model::new(&SPECS);
    model.edit(Origin::Ui).group(|e| {
        for (p, v) in [(MIX, 0.9), (GAIN, -6.0)] {
            e.begin(p);
            e.set(p, v);
            e.end(p);
        }
    });
    assert!(model.undo());
    assert_eq!(
        (model.get(MIX), model.get(GAIN)),
        (0.5, 0.0),
        "both, in one step"
    );
    assert!(!model.can_undo());
    assert!(model.redo());
    assert_eq!((model.get(MIX), model.get(GAIN)), (0.9, -6.0));
}
