//! A page group shows one page's art and controls at a time, picked by
//! its tabs, whose lamps follow their pages' ON switches (design/faceplates.md).

use silkscreen::EngineEvent;
use silkscreen::core::ControlKind;
use silkscreen_harness::{Harness, assert_snapshot};
use widgets::pages::{self, ids, tabs};
use widgets::{AMOUNT, SECOND_ON, TIME};

fn harness() -> Harness {
    Harness::new(widgets::pages_editor, 2.0)
}

#[test]
fn the_build_finds_the_group_its_tabs_and_pages() {
    let layout = &pages::LAYOUT;
    assert_eq!(layout.pages.len(), 1);
    let group = &layout.pages[usize::from(pages::page_groups::FX)];
    assert_eq!(group.name, "fx");
    assert_eq!(group.pages.len(), 3);
    assert_eq!(
        (
            group.region.x,
            group.region.y,
            group.region.w,
            group.region.h
        ),
        (12.0, 40.0, 216.0, 108.0)
    );
    assert_eq!(tabs::FX, [ids::FX_0, ids::FX_1, ids::FX_2]);
    assert!(matches!(
        layout.control(ids::FX_1).kind,
        ControlKind::Tab {
            group: "fx",
            position: 1
        }
    ));
    let page = |id| layout.control(id).page.map(|p| (p.group, p.page));
    assert_eq!(page(ids::AMOUNT), Some((0, 0)));
    assert_eq!(page(ids::TIME), Some((0, 1)));
    assert_eq!(page(ids::THIRD_ON), Some((0, 2)));
    assert_eq!(page(ids::FX_0), None, "tabs are on no page");
}

#[test]
fn each_tab_shows_its_page() {
    let mut h = harness();
    assert_eq!(h.editor().state().shown_pages(), &[0]);
    assert_snapshot!(h, "pages_0@2x");
    h.click(ids::FX_1);
    assert_eq!(h.editor().state().shown_pages(), &[1]);
    assert_snapshot!(h, "pages_1@2x");
    h.click(ids::FX_2);
    assert_snapshot!(h, "pages_2@2x");
}

#[test]
fn hidden_controls_take_no_input() {
    let mut h = harness();
    // The fader is on page 1; page 0 shows nothing there.
    let fader = h.centre(ids::TIME);
    h.drag_from(fader, -40.0);
    assert_eq!(h.model().get(TIME), 0.25, "untouched while hidden");
    h.click(ids::FX_1);
    h.drag_from(fader, -40.0);
    assert!(h.model().get(TIME) > 0.25, "moves once shown");
    // And the knob on page 0 is now hidden.
    let knob = h.centre(ids::AMOUNT);
    h.drag_from(knob, -40.0);
    assert_eq!(h.model().get(AMOUNT), 0.5);
}

#[test]
fn switching_pages_is_not_an_edit() {
    let mut h = harness();
    h.click(ids::FX_2);
    // The engine hears which page shows (design/engine-bridge.md): at the start and on
    // the switch. No param moves.
    assert_eq!(
        h.engine_events(),
        [
            EngineEvent::Shown { group: 0, page: 0 },
            EngineEvent::Shown { group: 0, page: 2 }
        ],
        "only the shown page is sent to the engine"
    );
    assert!(!h.editor_mut().undo(), "nothing to undo");
    assert_eq!(h.editor().state().shown_pages(), &[2]);
}

#[test]
fn a_lamp_follows_automation_on_a_hidden_page() {
    let mut h = harness();
    h.snapshot();
    // SECOND_ON is on page 1, hidden; the host turns it on.
    h.host_change(SECOND_ON, 1.0).advance(16.0);
    h.snapshot();
    let tab = pages::LAYOUT.control(ids::FX_1).rect.to_physical(2.0);
    assert!(
        h.last_damage().iter().any(|d| d.intersects(&tab)),
        "the tab repainted"
    );
    let page = pages::LAYOUT.pages[0].region.to_physical(2.0);
    let switch = pages::LAYOUT.control(ids::SECOND_ON).rect.to_physical(2.0);
    assert!(
        h.last_damage()
            .iter()
            .all(|d| d.intersects(&tab) || d.intersects(&switch)),
        "nothing else: {:?} (page {page:?})",
        h.last_damage()
    );
    assert_snapshot!(h, "pages_lamp@2x");
}

#[test]
fn the_shown_page_is_saved_and_restored_as_ui_state() {
    let mut h = harness();
    assert_eq!(
        h.editor().state().ui_state(),
        [("page.fx".to_owned(), "0".to_owned())]
    );
    h.click(ids::FX_2);
    let saved = h.editor().state().ui_state();
    assert_eq!(saved, [("page.fx".to_owned(), "2".to_owned())]);

    let mut restored = harness();
    restored.editor_mut().state.restore_ui_state(&saved);
    restored.advance(16.0);
    assert_eq!(restored.editor().state().shown_pages(), &[2]);
    assert_snapshot!(restored, "pages_2@2x");
}

#[test]
fn unknown_or_bad_ui_state_is_ignored() {
    let mut h = harness();
    let ui = |k: &str, v: &str| (k.to_owned(), v.to_owned());
    h.editor_mut().state.restore_ui_state(&[
        ui("page.fx", "7"),
        ui("page.nope", "1"),
        ui("page.fx", "two"),
        ui("zoom", "2"),
    ]);
    assert_eq!(h.editor().state().shown_pages(), &[0]);
}
