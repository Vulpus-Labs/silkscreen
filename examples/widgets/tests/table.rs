//! The table panel of editable rows, as a miniature mod matrix
//! over the example's params.

use silkscreen::core::{Key, Modifiers, MouseButton, Point, RawKind, Rect};
use silkscreen::table::ROW_H;
use silkscreen::{Cell, Column, Row, Table};
use silkscreen_harness::{Harness, assert_snapshot};
use widgets::{CUTOFF, FINE, MODE, SELECT, STOP, SYNC, TUNING, WAVE};

const PANEL: Rect = Rect::new(20.0, 20.0, 360.0, 150.0);
const WIDTHS: [f32; 6] = [24.0, 22.0, 80.0, 80.0, 120.0, 30.0];

fn open(h: &mut Harness) {
    let columns = ["On", "#", "Source", "Dest", "Amount", ""]
        .iter()
        .zip(WIDTHS)
        .map(|(t, w)| Column::new(t, w))
        .collect();
    let mut rows = vec![
        Row::new(vec![
            Cell::Toggle(STOP),
            Cell::Label("1".into()),
            Cell::Choice(MODE),
            Cell::Choice(WAVE),
            Cell::Value(FINE),
            Cell::Action {
                label: "×".into(),
                run: Box::new(|state| {
                    state.edit(|e| {
                        e.reset(MODE);
                        e.reset(FINE);
                    });
                }),
            },
        ])
        .dim_when(|m| m.get(STOP) < 0.5),
        Row::new(vec![
            Cell::Toggle(SYNC),
            Cell::Label("2".into()),
            Cell::Choice(SELECT),
            Cell::Choice(TUNING),
            Cell::Value(CUTOFF),
            Cell::Label(String::new()),
        ])
        .dim_when(|m| m.get(SYNC) < 0.5),
    ];
    for i in 3..=12 {
        rows.push(Row::new(vec![
            Cell::Label(String::new()),
            Cell::Label(i.to_string()),
        ]));
    }
    let table = Table::new(PANEL, columns, rows, h.editor().state());
    h.editor_mut().open_panel(Box::new(table));
}

/// The centre of cell `(row, column)` with no scroll.
fn cell(row: usize, c: usize) -> Point {
    let x = PANEL.x + 1.0 + WIDTHS[..c].iter().sum::<f32>() + WIDTHS[c] / 2.0;
    let y = PANEL.y + 1.0 + ROW_H + row as f32 * ROW_H + ROW_H / 2.0;
    Point::new(x, y)
}

fn key(h: &mut Harness, key: Key) {
    h.raw(RawKind::Key {
        key,
        mods: Modifiers::NONE,
        pressed: true,
    });
}

#[test]
fn toggles_choices_and_actions_edit_params() {
    let mut h = Harness::new(widgets::editor, 1.0);
    open(&mut h);
    h.click_at(cell(0, 0));
    assert_eq!(h.model().get(STOP), 1.0, "the checkbox flips its param");

    // A choice opens its list over the table; choosing closes only it.
    h.click_at(cell(0, 2));
    assert_eq!(h.editor().panels(), 2);
    key(&mut h, Key::ArrowDown);
    key(&mut h, Key::Enter);
    assert_eq!(h.editor().panels(), 1, "back to the table");
    assert_eq!(h.model().get(MODE), 1.0);

    h.click_at(cell(0, 5));
    assert_eq!(h.model().get(MODE), 0.0, "the action ran");
}

#[test]
fn value_cells_drag_horizontally_as_one_gesture() {
    let mut h = Harness::new(widgets::editor, 1.0);
    open(&mut h);
    let start = cell(0, 4);
    h.move_to(start).raw(RawKind::PointerDown {
        pos: start,
        button: MouseButton::Left,
        mods: Modifiers::NONE,
    });
    let end = Point::new(start.x + 60.0, start.y);
    h.raw(RawKind::PointerMove {
        pos: end,
        mods: Modifiers::NONE,
    })
    .raw(RawKind::PointerUp {
        pos: end,
        button: MouseButton::Left,
        mods: Modifiers::NONE,
    });
    // 60 of 300 units across a range of 100: +20.
    assert!(
        (h.model().get(FINE) - 20.0).abs() < 0.01,
        "{}",
        h.model().get(FINE)
    );
    assert!(h.editor_mut().undo(), "one undoable gesture");
    assert_eq!(h.model().get(FINE), 0.0);
}

#[test]
fn host_changes_repaint_the_table() {
    let mut h = Harness::new(widgets::editor, 1.0);
    open(&mut h);
    h.snapshot();
    h.host_change(CUTOFF, 0.9).advance(16.0);
    h.snapshot();
    let table = PANEL.to_physical(1.0);
    assert!(
        h.last_damage()
            .iter()
            .any(|d| d.intersect(&table).is_some()),
        "{:?}",
        h.last_damage()
    );
}

#[test]
fn snapshot() {
    let mut h = Harness::new(widgets::editor, 2.0);
    open(&mut h);
    h.click_at(cell(0, 0));
    h.move_to(cell(1, 4));
    assert_snapshot!(h, "table@2x");
    h.click_at(cell(1, 3));
    assert_snapshot!(h, "table_choice@2x");
}

#[test]
fn hover_repaints_only_the_cells_it_crosses() {
    let mut h = Harness::new(widgets::editor, 1.0);
    open(&mut h);
    h.move_to(cell(0, 2));
    h.snapshot();
    h.move_to(cell(1, 2));
    h.snapshot();
    let painted: u64 = h
        .last_damage()
        .iter()
        .map(|d| u64::from(d.w) * u64::from(d.h))
        .sum();
    let cell_area = (WIDTHS[2] + 2.0) * (ROW_H + 2.0);
    assert!(
        painted as f32 <= 2.0 * cell_area,
        "two cells, not the table: {:?}",
        h.last_damage()
    );
    // And the frame matches a full repaint's.
    let partial = h.snapshot().clone();
    h.editor_mut().set_scale(1.0);
    let full = h.snapshot().clone();
    let w = full.width() as usize;
    let diffs: Vec<(usize, usize)> = full
        .data()
        .chunks(4)
        .zip(partial.data().chunks(4))
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| (i % w, i / w))
        .collect();
    assert!(
        diffs.is_empty(),
        "{} differ, first {:?} last {:?}",
        diffs.len(),
        diffs.first(),
        diffs.last()
    );
}

/// The frame as drawn, then as a full repaint draws it: they must match.
fn matches_full_repaint(h: &mut Harness) {
    let partial = h.snapshot().clone();
    h.editor_mut().set_scale(1.0);
    let full = h.snapshot().clone();
    let w = full.width() as usize;
    let diffs: Vec<(usize, usize, [u8; 4], [u8; 4])> = full
        .data()
        .chunks(4)
        .zip(partial.data().chunks(4))
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, (a, b))| (i % w, i / w, a.try_into().unwrap(), b.try_into().unwrap()))
        .collect();
    assert!(
        diffs.is_empty(),
        "{} pixels differ from a full repaint, first {:?}, last {:?}",
        diffs.len(),
        diffs.first(),
        diffs.last()
    );
}

#[test]
fn a_control_wholly_behind_the_table_doesnt_repaint() {
    let mut h = Harness::new(widgets::editor, 1.0);
    open(&mut h);
    h.snapshot();
    // STOP's switch sits wholly behind the table's body; its row shows
    // STOP too, so the row repaints, but not the switch beneath.
    let stop = widgets::faceplate::LAYOUT
        .control(widgets::faceplate::ids::STOP)
        .rect;
    assert!(
        stop.x > PANEL.x + 3.0
            && stop.right() < PANEL.right() - 3.0
            && stop.y > PANEL.y + 3.0
            && stop.bottom() < PANEL.bottom() - 3.0,
        "the test needs a control behind the table: {stop:?}"
    );
    h.host_change(STOP, 1.0).advance(16.0);
    h.snapshot();
    let stop = stop.to_physical(1.0);
    assert!(!h.last_damage().is_empty(), "the row repaints");
    assert!(
        h.last_damage()
            .iter()
            .all(|d| d.intersect(&stop) != Some(stop)),
        "{:?}",
        h.last_damage()
    );
    matches_full_repaint(&mut h);
}

#[test]
fn a_value_drag_repaints_its_row_not_the_table() {
    let mut h = Harness::new(widgets::editor, 1.0);
    open(&mut h);
    let start = cell(0, 4);
    h.move_to(start).raw(RawKind::PointerDown {
        pos: start,
        button: MouseButton::Left,
        mods: Modifiers::NONE,
    });
    h.snapshot();
    h.raw(RawKind::PointerMove {
        pos: Point::new(start.x + 30.0, start.y),
        mods: Modifiers::NONE,
    });
    h.snapshot();
    // The row, and FINE's knob under the table, but not the whole
    // table: the header stays as it was.
    let header_bottom = (PANEL.y + 1.0 + ROW_H) as i32;
    assert!(
        h.last_damage().iter().all(|d| d.y >= header_bottom),
        "{:?}",
        h.last_damage()
    );
    matches_full_repaint(&mut h);
}

#[test]
fn automation_the_table_doesnt_show_repaints_none_of_it() {
    let mut h = Harness::new(widgets::editor, 1.0);
    open(&mut h);
    h.snapshot();
    h.host_change(widgets::LEVEL, 0.1).advance(16.0);
    h.snapshot();
    // LEVEL's knob, partly under the table, repaints; nothing else does.
    let knob = widgets::faceplate::LAYOUT
        .control(widgets::faceplate::ids::LEVEL)
        .rect
        .to_physical(1.0);
    assert!(
        h.last_damage()
            .iter()
            .all(|d| d.intersect(&knob) == Some(*d)),
        "{:?}",
        h.last_damage()
    );
    matches_full_repaint(&mut h);
}
