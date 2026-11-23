//! The scope and the meter draw the engine's output, fed on `tick`, and
//! repaint only when it changes (design/engine-bridge.md).

use silkscreen::bridge::NullLink;
use silkscreen::core::Point;
use silkscreen_harness::{Harness, assert_snapshot};
use widgets::live::ids;

fn sine(len: usize, hz: f64, amp: f32) -> Vec<f32> {
    (0..len)
        .map(|i| amp * (2.0 * std::f64::consts::PI * hz * i as f64 / 48_000.0).sin() as f32)
        .collect()
}

#[test]
fn the_scope_draws_a_steady_note_from_a_rising_crossing() {
    let mut h = Harness::new(widgets::live_editor, 2.0);
    let wave = sine(4096, 110.0, 0.8);
    h.output(&[&wave, &wave]).advance(16.0);
    assert_snapshot!(h, "scope_sine@2x");
}

#[test]
fn with_no_telemetry_the_scope_is_flat_and_the_meter_empty() {
    let mut h = Harness::with_link(widgets::live_editor, Box::new(NullLink), 2.0);
    h.advance(16.0);
    assert_snapshot!(h, "live_idle@2x");
}

#[test]
fn the_meter_holds_its_peak_then_falls() {
    let mut h = Harness::new(widgets::live_editor, 2.0);
    // Left at full scale, right at −12 dB.
    let (left, right) = (vec![1.0_f32; 256], vec![0.25_f32; 256]);
    h.output(&[&left, &right]).advance(16.0);
    assert_snapshot!(h, "meter_peak@2x");
    // A second of silence: the bars have fallen 24 dB, the holds stay.
    let quiet = [0.0_f32; 256];
    for _ in 0..60 {
        h.output(&[&quiet, &quiet]).advance(1000.0 / 60.0);
    }
    assert_snapshot!(h, "meter_hold@2x");
}

#[test]
fn nothing_new_repaints_nothing() {
    let mut h = Harness::new(widgets::live_editor, 1.0);
    let wave = sine(4096, 220.0, 0.5);
    h.output(&[&wave, &wave]).advance(16.0);
    h.snapshot();
    assert!(!h.last_damage().is_empty(), "the scope and meter repainted");
    // Five seconds on, the meter has fallen off the scale and settled:
    // its hold falls only once its 1.5 s hold is over.
    h.advance(5000.0);
    h.snapshot();
    h.advance(16.0);
    assert!(!h.editor().needs_render(), "nothing new");
    // Samples under the meter's floor (−60 dB) repaint the scope only.
    let faint = sine(4096, 220.0, 0.001);
    h.output(&[&faint, &faint]).advance(16.0);
    h.snapshot();
    let scope = widgets::live::LAYOUT
        .control(ids::WAVE)
        .rect
        .to_physical(1.0);
    assert!(!h.last_damage().is_empty());
    assert!(
        h.last_damage().iter().all(|d| scope.union(d) == scope),
        "{:?}",
        h.last_damage()
    );
}

#[test]
fn the_pointer_passes_through_display_controls() {
    let mut h = Harness::new(widgets::live_editor, 1.0);
    let centre = h.centre(ids::WAVE);
    h.move_to(centre).advance(2000.0);
    assert_eq!(h.editor().view.recognizer().hover(), None);
    assert_eq!(h.editor().tip(), None);
    h.click_at(Point::new(centre.x, centre.y));
    assert!(h.engine_events().is_empty());
}

/// A meter pointed at an engine meter shows it, not the output.
#[test]
fn a_meter_reads_an_engine_meter() {
    use silkscreen::MeterSource;
    let mut h = Harness::new(widgets::live_editor, 2.0);
    h.editor_mut()
        .state
        .set_meter_source(ids::LEVEL, MeterSource::Engine(1));
    // The output is at full scale; engine meter 1 at −6 and −18 dB.
    let loud = vec![1.0_f32; 256];
    h.output(&[&loud, &loud])
        .engine_meter(1, [0.5, 0.125])
        .engine_meter(0, [1.0, 1.0])
        .advance(16.0);
    assert_snapshot!(h, "meter_engine@2x");
}

#[test]
#[should_panic(expected = "is not a meter")]
fn only_meters_take_a_source() {
    let mut h = Harness::new(widgets::live_editor, 1.0);
    h.editor_mut()
        .state
        .set_meter_source(ids::WAVE, silkscreen::MeterSource::Engine(0));
}
