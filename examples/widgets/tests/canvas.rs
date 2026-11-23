//! A canvas control (design/faceplates.md): the plugin's widget paints
//! from several params and the engine's telemetry.

use std::cell::RefCell;
use std::rc::Rc;

use silkscreen::core::{ControlEvent, Point, WidgetState};
use silkscreen::model::binding;
use silkscreen::{Binding, Unbound};
use silkscreen_harness::{Harness, assert_snapshot};
use widgets::canvas::ids;

fn harness_with(curve: Box<dyn Binding>) -> Harness {
    Harness::new(move |link| widgets::canvas_editor(link, curve), 2.0)
}

fn harness() -> Harness {
    harness_with(Box::new(Unbound))
}

/// Whether the last render repainted `id`'s rect at 2x.
fn repainted(h: &Harness, id: silkscreen::ControlId) -> bool {
    let r = widgets::canvas::LAYOUT.control(id).rect.to_physical(2.0);
    h.last_damage().iter().any(|d| d.intersects(&r))
}

#[test]
fn the_canvas_draws_from_the_model() {
    let mut h = harness();
    assert_snapshot!(h, "canvas_default@2x");
    h.drag(ids::ATTACK, -40.0);
    h.snapshot();
    assert!(repainted(&h, ids::CURVE), "a knob's param moves the curve");
    assert_snapshot!(h, "canvas_attack@2x");
}

#[test]
fn the_canvas_repaints_from_engine_telemetry() {
    let mut h = harness();
    h.advance(16.0);
    h.snapshot();
    h.advance(16.0);
    assert!(!h.editor().needs_render(), "nothing changed");
    // The engine says where its envelope is.
    h.engine_value(0, 0.4);
    h.advance(16.0);
    h.snapshot();
    assert!(repainted(&h, ids::CURVE));
    assert!(!repainted(&h, ids::ATTACK), "only the canvas");
}

#[test]
fn a_hover_reaches_the_canvas_binding_with_its_position() {
    let seen: Rc<RefCell<Vec<Point>>> = Rc::default();
    let log = Rc::clone(&seen);
    let curve = binding::from_fn(
        &[],
        move |event: &ControlEvent, _: &mut silkscreen::model::Edit<'_>| {
            if let ControlEvent::Hover { pos } = event {
                log.borrow_mut().push(*pos);
            }
        },
        |_: &silkscreen::Model| WidgetState::default(),
    );
    let mut h = harness_with(Box::new(curve));
    let at = h.centre(ids::CURVE);
    let next = Point::new(at.x + 10.0, at.y);
    h.move_to(at).move_to(next);
    assert_eq!(*seen.borrow(), [at, next]);
}
