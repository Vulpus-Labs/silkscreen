//! Panels take input before the controls beneath them.

use std::cell::Cell;
use std::rc::Rc;

use silkscreen::core::tiny_skia::{Color, Paint, PixmapMut, Rect as SkRect, Transform};
use silkscreen::core::{Key, Modifiers, Point, RawKind, Rect};
use silkscreen::{EditorState, Panel, Reply};
use silkscreen_harness::Harness;
use widgets::{CUTOFF, faceplate::ids};

/// A grey block over the CUTOFF fader that counts presses, closing on a
/// press in its lower half.
struct Probe {
    rect: Rect,
    presses: Rc<Cell<u32>>,
}

impl Panel for Probe {
    fn rect(&self) -> Rect {
        self.rect
    }
    fn draw(&self, canvas: &mut PixmapMut<'_>, transform: Transform) {
        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba8(90, 90, 96, 255));
        let r = self.rect;
        canvas.fill_rect(
            SkRect::from_xywh(r.x, r.y, r.w, r.h).unwrap(),
            &paint,
            transform,
            None,
        );
    }
    fn input(&mut self, event: &RawKind, _: &mut EditorState) -> Reply {
        match event {
            RawKind::PointerDown { pos, .. } => {
                self.presses.set(self.presses.get() + 1);
                if pos.y > self.rect.center().y {
                    Reply::Close
                } else {
                    Reply::Handled
                }
            }
            _ => Reply::Ignored,
        }
    }
}

fn open(h: &mut Harness) -> Rc<Cell<u32>> {
    let presses = Rc::new(Cell::new(0));
    let rect = faceplate_rect(ids::CUTOFF);
    h.editor_mut().open_panel(Box::new(Probe {
        rect: Rect::new(rect.x - 10.0, rect.y, rect.w + 20.0, rect.h),
        presses: Rc::clone(&presses),
    }));
    presses
}

fn faceplate_rect(id: silkscreen::ControlId) -> Rect {
    widgets::faceplate::LAYOUT.control(id).rect
}

fn key(h: &mut Harness, key: Key) {
    h.raw(RawKind::Key {
        key,
        mods: Modifiers::NONE,
        pressed: true,
    });
}

#[test]
fn a_panel_takes_clicks_instead_of_the_control_beneath() {
    let mut h = Harness::new(widgets::editor, 1.0);
    let presses = open(&mut h);
    let top = faceplate_rect(ids::CUTOFF);
    // The upper part of the fader: would set CUTOFF high without the panel.
    h.click_at(Point::new(top.center().x, top.y + 10.0));
    assert_eq!(presses.get(), 1);
    assert_eq!(h.model().get(CUTOFF), 0.5, "the fader saw nothing");
    assert_eq!(h.editor().panels(), 1);
    // A press in its lower half asks it to close.
    h.click_at(Point::new(top.center().x, top.bottom() - 10.0));
    assert_eq!(h.editor().panels(), 0);
    assert_eq!(h.model().get(CUTOFF), 0.5);
}

#[test]
fn escape_or_a_press_outside_closes_without_reaching_the_controls() {
    let mut h = Harness::new(widgets::editor, 1.0);
    open(&mut h);
    key(&mut h, Key::Escape);
    assert_eq!(h.editor().panels(), 0);

    let presses = open(&mut h);
    // Outside: on the STOP toggle, which must not flip.
    h.click(ids::STOP);
    assert_eq!(h.editor().panels(), 0);
    assert_eq!(presses.get(), 0);
    assert_eq!(h.model().get(widgets::STOP), 0.0, "the toggle saw nothing");
    // Closed, the controls work again.
    h.advance(500.0).click(ids::STOP);
    assert_eq!(h.model().get(widgets::STOP), 1.0);
}

#[test]
fn opening_and_closing_repaint_only_the_panel() {
    let mut h = Harness::new(widgets::editor, 2.0);
    h.snapshot();
    open(&mut h);
    h.snapshot();
    let panel = faceplate_rect(ids::CUTOFF);
    let area = Rect::new(panel.x - 10.0, panel.y, panel.w + 20.0, panel.h).to_physical(2.0);
    assert_eq!(h.last_damage(), [area]);
    let grey = h
        .snapshot()
        .pixel(area.x as u32 + 2, area.y as u32 + 2)
        .unwrap();
    assert_eq!(grey.red(), 90, "drawn above the fader");
    h.editor_mut().close_panel();
    h.snapshot();
    assert_eq!(h.last_damage(), [area]);
}

#[test]
fn no_tooltips_while_a_panel_is_open() {
    let mut h = Harness::new(widgets::editor, 1.0);
    open(&mut h);
    let c = h.centre(ids::STOP);
    h.move_to(c).advance(800.0);
    assert_eq!(h.editor().tip(), None);
}

/// Counts its updates, which each sync runs.
struct Updates(Rect, Rc<Cell<u32>>);

impl Panel for Updates {
    fn rect(&self) -> Rect {
        self.0
    }
    fn draw(&self, _: &mut PixmapMut<'_>, _: Transform) {}
    fn input(&mut self, _: &RawKind, _: &mut EditorState) -> Reply {
        Reply::Ignored
    }
    fn update(&mut self, _: &EditorState) -> silkscreen::Update {
        self.1.set(self.1.get() + 1);
        silkscreen::Update::Unchanged
    }
}

#[test]
fn a_frame_syncs_once() {
    let mut h = Harness::new(widgets::editor, 1.0);
    let updates = Rc::new(Cell::new(0));
    h.editor_mut().open_panel(Box::new(Updates(
        Rect::new(10.0, 10.0, 40.0, 40.0),
        Rc::clone(&updates),
    )));
    h.advance(16.0);
    h.snapshot();
    updates.set(0);
    // A tick, then a render with nothing between: one sync.
    h.advance(16.0);
    h.snapshot();
    assert_eq!(updates.get(), 1);
    // A param changed between them: the render syncs again.
    h.advance(16.0);
    h.host_load(&[(CUTOFF, 0.25)]);
    h.snapshot();
    assert_eq!(updates.get(), 3);
}
