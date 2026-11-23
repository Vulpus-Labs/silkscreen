//! Panels: overlays that take input before the controls beneath them
//!: menus, drop-down lists, the mod matrix, the preset browser.
//!
//! The view keeps a stack of open panels. While one is open, every
//! pointer event goes to the top panel when it falls inside it; a press
//! outside closes it and goes no further, and Esc closes it. The controls
//! beneath see nothing (they lose hover when the first panel opens).
//! Panels draw through the compositor's overlay layer, above the
//! controls and below any tooltip.

use std::cell::RefCell;
use std::rc::Rc;

use silkscreen_core::tiny_skia::{PixmapMut, Transform};
use silkscreen_core::{MouseCursor, Overlay, Point, RawKind, Rect};

use crate::EditorState;

/// What a panel did with an input event.
pub enum Reply {
    /// Nothing changed.
    Ignored,
    /// Handled; the panel's rect is repainted.
    Handled,
    /// Handled; only these logical rects changed (a hover moving from
    /// row to row), so only they repaint.
    Repaint(Vec<Rect>),
    /// Close this panel.
    Close,
    /// Open another panel above this one (e.g. a cell's drop-down list),
    /// which takes input until it closes.
    Open(Box<dyn Panel>),
    /// Close this panel and open another in its place (a menu item that
    /// opens a submenu, a confirmation or a name field).
    Replace(Box<dyn Panel>),
}

impl std::fmt::Debug for Reply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Reply::Ignored => "Ignored",
            Reply::Handled => "Handled",
            Reply::Repaint(_) => "Repaint(..)",
            Reply::Close => "Close",
            Reply::Open(_) => "Open(..)",
            Reply::Replace(_) => "Replace(..)",
        })
    }
}

impl Reply {
    /// The reply for one event two parts of a panel each handled: what
    /// both changed. Opening, replacing and closing win, the first
    /// first; two repaints merge; any whole-panel repaint makes it whole.
    #[must_use]
    pub fn and(self, other: Reply) -> Reply {
        match (self, other) {
            (r @ (Reply::Close | Reply::Open(_) | Reply::Replace(_)), _)
            | (Reply::Ignored, r)
            | (r, Reply::Ignored)
            | (_, r @ (Reply::Close | Reply::Open(_) | Reply::Replace(_))) => r,
            (Reply::Repaint(mut a), Reply::Repaint(b)) => {
                a.extend(b);
                Reply::Repaint(a)
            }
            _ => Reply::Handled,
        }
    }

    /// A repaint of `rects`, or nothing if there are none.
    pub fn repaint(rects: Vec<Rect>) -> Reply {
        if rects.is_empty() {
            Reply::Ignored
        } else {
            Reply::Repaint(rects)
        }
    }
}

/// An overlay that takes input. Positions are logical, like the
/// faceplate's.
pub trait Panel {
    /// Logical bounds: input inside goes to the panel, and everything it
    /// draws stays inside.
    fn rect(&self) -> Rect;

    /// Paint through `transform` (logical → canvas pixels).
    fn draw(&self, canvas: &mut PixmapMut<'_>, transform: Transform);

    /// The mouse cursor at `pos`, inside the panel.
    fn cursor(&self, pos: Point) -> MouseCursor {
        let _ = pos;
        MouseCursor::Default
    }

    /// Handle pointer input inside the panel (and moves anywhere, so hover
    /// can follow the pointer out), keys, and the clock. A panel edits
    /// params through `state`, as bindings do, so changes reach the host
    /// and the engine.
    fn input(&mut self, event: &RawKind, state: &mut EditorState) -> Reply;

    /// A logical rect the panel paints over completely, if any: controls
    /// wholly behind it aren't repainted while it shows. Panels with a
    /// filled body give it, inset from their rounded corners.
    fn opaque(&self) -> Option<Rect> {
        None
    }

    /// Whether the panel takes typed text (a name or search field), and
    /// so wants every key. In a plugin, keys go to the host's window
    /// unless the top panel takes text: a DAW's shortcuts keep working
    /// while the editor is open.
    fn takes_text(&self) -> bool {
        false
    }

    /// Called on the top panel each tick, for news from outside its
    /// input: a host prompt's answer ([`crate::prompt`]). Replies as
    /// [`input`](Self::input) does.
    fn poll(&mut self, state: &mut EditorState) -> Reply {
        let _ = state;
        Reply::Ignored
    }

    /// Catch up with the model (e.g. host automation moved a value the
    /// panel shows). Called whenever the view syncs; say what to
    /// repaint. Panels that show no params needn't override it.
    fn update(&mut self, state: &EditorState) -> Update {
        let _ = state;
        Update::Unchanged
    }
}

/// What a panel's catch-up with the model ([`Panel::update`]) changed.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    Unchanged,
    /// Only these logical rects (the rows whose values moved).
    Rects(Vec<Rect>),
    /// The whole panel.
    Whole,
}

impl From<bool> for Update {
    /// True for the whole panel.
    fn from(changed: bool) -> Self {
        if changed {
            Update::Whole
        } else {
            Update::Unchanged
        }
    }
}

/// A panel shared between the view (input) and the compositor (drawing).
pub(crate) type Shared = Rc<RefCell<Box<dyn Panel>>>;

/// Draws a shared panel as an overlay; `opaque` says whether the
/// editor's theme fills panels' bodies (an opaque background), so its
/// [`Panel::opaque`] can be trusted.
pub(crate) struct Drawn(pub Shared, pub bool);

impl Overlay for Drawn {
    fn rect(&self) -> Rect {
        self.0.borrow().rect()
    }

    fn draw(&self, canvas: &mut PixmapMut<'_>, transform: Transform) {
        self.0.borrow().draw(canvas, transform);
    }

    fn opaque(&self) -> Option<Rect> {
        if self.1 {
            self.0.borrow().opaque()
        } else {
            None
        }
    }
}

/// `r` inside its rounded corners: the body a filled panel surely
/// covers.
pub(crate) fn body(r: Rect) -> Option<Rect> {
    let inset = crate::theme::RADIUS;
    (r.w > 2.0 * inset && r.h > 2.0 * inset).then(|| {
        Rect::new(
            r.x + inset,
            r.y + inset,
            r.w - 2.0 * inset,
            r.h - 2.0 * inset,
        )
    })
}

/// Where an event lands with panels open.
pub(crate) enum Route {
    /// To the top panel.
    Panel,
    /// Close the top panel and drop the event.
    CloseTop,
    /// Drop it: the controls beneath are covered.
    Swallow,
}

/// Route `event` given the top panel's `rect`.
pub(crate) fn route(event: &RawKind, rect: Rect) -> Route {
    let inside = |p: silkscreen_core::Point| rect.contains(p);
    match event {
        RawKind::Key {
            key: silkscreen_core::Key::Escape,
            pressed: true,
            ..
        } => Route::CloseTop,
        RawKind::Key { .. } => Route::Panel,
        RawKind::PointerDown { pos, .. } if !inside(*pos) => Route::CloseTop,
        RawKind::PointerDown { .. } | RawKind::PointerUp { .. } | RawKind::PointerMove { .. } => {
            Route::Panel
        }
        RawKind::Wheel { pos, .. } if inside(*pos) => Route::Panel,
        _ => Route::Swallow,
    }
}
