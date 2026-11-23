//! Generated control kinds, radio groups, and the binding/kind check
//! (design/model.md).

use silkscreen::bridge::NullLink;
use silkscreen::core::ControlKind;
use silkscreen::{Binding, EditorState, ParamKnob};
use widgets::{PARAMS, WAVE, controls, faceplate};

#[test]
fn every_kind_reaches_the_layout() {
    let kinds: Vec<&str> = faceplate::CONTROLS
        .iter()
        .map(|c| c.kind.as_str())
        .collect();
    assert_eq!(
        kinds,
        [
            "knob", "knob", "fader", "selector", "toggle", "button", "selector", "radio", "radio",
            "radio", "dropdown", "preset", "action", "action", "action"
        ]
    );
    assert_eq!(
        faceplate::radios::TUNING,
        [
            faceplate::ids::TUNING_0,
            faceplate::ids::TUNING_1,
            faceplate::ids::TUNING_2
        ]
    );
    let group = faceplate::LAYOUT.radio_group("tuning");
    assert_eq!(group.len(), 3);
    assert_eq!(
        group[2].kind,
        ControlKind::Radio {
            group: "tuning",
            position: 2
        }
    );
}

#[test]
fn the_editor_builds() {
    let state = widgets::state(Box::new(NullLink));
    assert_eq!(state.model().get(WAVE), 0.0);
}

fn build_with(change: impl FnOnce(&mut faceplate::Controls<Box<dyn Binding>>)) {
    let mut c = controls();
    change(&mut c);
    let _ = EditorState::new(
        &faceplate::LAYOUT,
        &PARAMS,
        c.into_array(),
        Box::new(NullLink),
    );
}

#[test]
#[should_panic(
    expected = "control \"knob.level\" uses ParamKnob on param \"Stop\", which is discrete (a toggle)"
)]
fn a_knob_on_a_toggle_is_refused() {
    build_with(|c| c.level = Box::new(ParamKnob::new(widgets::STOP)));
}

/// A stepped knob is a wave rotary: accepted.
#[test]
fn a_knob_on_a_stepped_param_is_accepted() {
    build_with(|c| c.level = Box::new(ParamKnob::new(WAVE)));
}

#[test]
#[should_panic(expected = "control \"fader.cutoff\" is a fader but is bound with ParamKnob")]
fn a_knob_binding_on_a_fader_is_refused() {
    build_with(|c| c.cutoff = Box::new(ParamKnob::new(widgets::CUTOFF)));
}

#[test]
#[should_panic(
    expected = "control \"knob.level\" is bound to param 9 but the param table has only 9 entries"
)]
fn an_unknown_param_is_refused() {
    build_with(|c| c.level = Box::new(ParamKnob::new(silkscreen::ParamIndex(9))));
}

#[test]
#[should_panic(
    expected = "control \"toggle.stop\" is bound to param 12 but the param table has only 9 entries"
)]
fn a_custom_binding_with_an_unknown_param_is_refused() {
    build_with(|c| {
        c.stop = Box::new(silkscreen::model::binding::from_fn(
            &[silkscreen::ParamIndex(12)],
            |_, _| {},
            |_| silkscreen::core::WidgetState::value(0.0),
        ));
    });
}
