//! Small modal panels for the preset browser: a confirmation
//! ("Delete “Lead A”?") and the Save As dialog (a name and a folder).
//! Enter confirms or saves; Esc (the panel stack) cancels.

use std::cell::RefCell;
use std::rc::Rc;

use silkscreen_core::tiny_skia::{
    FillRule, LineCap, Paint, PathBuilder, PixmapMut, Stroke, Transform,
};
use silkscreen_core::{Key, MouseCursor, Point, RawKind, Rect, Size};
use silkscreen_text::Style;

use crate::EditorState;
use crate::list::{List, ROW_H as LIST_ROW_H};
use crate::panel::{Panel, Reply};
use crate::presets::UNCATEGORISED;
use crate::textfield::TextField;
use crate::theme::{RADIUS, Rgba, TEXT_SIZE, color, rounded, theme};
use crate::tooltip::Lettering;

const PAD: f32 = 10.0;
const BUTTON_W: f32 = 72.0;
const BUTTON_H: f32 = 22.0;
const FIELD_H: f32 = 20.0;

/// Fill `r`, rounded, in `rgba`.
pub(crate) fn fill_rounded(canvas: &mut PixmapMut<'_>, r: Rect, rgba: Rgba, transform: Transform) {
    if let Some(path) = rounded(r, RADIUS) {
        let mut paint = Paint {
            anti_alias: true,
            ..Paint::default()
        };
        paint.set_color(color(rgba));
        canvas.fill_path(&path, &paint, FillRule::Winding, transform, None);
    }
}

/// Stroke `r`'s rounded outline in the border colour, a pixel wide.
pub(crate) fn outline(canvas: &mut PixmapMut<'_>, r: Rect, transform: Transform) {
    outline_in(canvas, r, theme().border, 1.0 / transform.sx, transform);
}

/// Stroke `r`'s rounded outline, inside it, in `rgba` at `width`.
pub(crate) fn outline_in(
    canvas: &mut PixmapMut<'_>,
    r: Rect,
    rgba: Rgba,
    width: f32,
    transform: Transform,
) {
    let inset = width / 2.0;
    let r = Rect::new(
        r.x + inset,
        r.y + inset,
        r.w - 2.0 * inset,
        r.h - 2.0 * inset,
    );
    if let Some(path) = rounded(r, RADIUS) {
        let mut paint = Paint {
            anti_alias: true,
            ..Paint::default()
        };
        paint.set_color(color(rgba));
        let stroke = Stroke {
            width,
            ..Stroke::default()
        };
        canvas.stroke_path(&path, &paint, &stroke, transform, None);
    }
}

/// A small downward chevron centred at `(cx, cy)`, as a drop-down's.
pub(crate) fn chevron(
    canvas: &mut PixmapMut<'_>,
    cx: f32,
    cy: f32,
    rgba: Rgba,
    transform: Transform,
) {
    let half = 3.0;
    let mut pb = PathBuilder::new();
    pb.move_to(cx - half, cy - half / 2.0);
    pb.line_to(cx, cy + half / 2.0);
    pb.line_to(cx + half, cy - half / 2.0);
    if let Some(path) = pb.finish() {
        let mut paint = Paint {
            anti_alias: true,
            ..Paint::default()
        };
        paint.set_color(color(rgba));
        let stroke = Stroke {
            width: 1.2,
            line_cap: LineCap::Round,
            ..Stroke::default()
        };
        canvas.stroke_path(&path, &paint, &stroke, transform, None);
    }
}

/// Three dots in a row centred at `(cx, cy)`: a row's menu.
pub(crate) fn dots(canvas: &mut PixmapMut<'_>, cx: f32, cy: f32, rgba: Rgba, transform: Transform) {
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    paint.set_color(color(rgba));
    for dx in [-4.0, 0.0, 4.0] {
        if let Some(path) = PathBuilder::from_circle(cx + dx, cy, 1.2) {
            canvas.fill_path(&path, &paint, FillRule::Winding, transform, None);
        }
    }
}

/// A panel's background and border.
fn frame(canvas: &mut PixmapMut<'_>, r: Rect, transform: Transform) {
    fill_rounded(canvas, r, theme().background, transform);
    outline(canvas, r, transform);
}

/// `text` in `ink`, left-aligned at `x` and centred in `r` vertically.
fn label(
    lettering: &Lettering,
    canvas: &mut PixmapMut<'_>,
    text: &str,
    x: f32,
    r: Rect,
    ink: Rgba,
    transform: Transform,
) {
    let face = &lettering.face;
    let (ascent, descent) = face.line_metrics(TEXT_SIZE);
    let style = Style {
        face,
        size: TEXT_SIZE,
        color: color(ink),
    };
    lettering.text.borrow_mut().draw_clipped(
        canvas,
        text,
        (x, r.center().y + f32::midpoint(ascent, descent)),
        &style,
        transform,
        Some([r.x, r.y, r.right(), r.bottom()]),
    );
}

/// `text` centred in `r`.
fn centred(
    lettering: &Lettering,
    canvas: &mut PixmapMut<'_>,
    text: &str,
    r: Rect,
    ink: Rgba,
    transform: Transform,
) {
    let w = lettering
        .text
        .borrow_mut()
        .width(&lettering.face, text, TEXT_SIZE, transform.sx);
    label(
        lettering,
        canvas,
        text,
        r.center().x - w / 2.0,
        r,
        ink,
        transform,
    );
}

/// A button: a rounded box, hover-washed, the default one filled.
fn button(
    lettering: &Lettering,
    canvas: &mut PixmapMut<'_>,
    text: &str,
    r: Rect,
    hover: bool,
    default: bool,
    transform: Transform,
) {
    if default {
        fill_rounded(canvas, r, theme().selected, transform);
    }
    if hover {
        fill_rounded(canvas, r, theme().hover, transform);
    }
    outline(canvas, r, transform);
    centred(lettering, canvas, text, r, theme().ink, transform);
}

/// `w` × `h` centred over `over`, kept inside `window`.
pub(crate) fn centred_over(over: Rect, w: f32, h: f32, window: Size) -> Rect {
    let x = (over.center().x - w / 2.0).clamp(2.0, (window.w - 2.0 - w).max(2.0));
    let y = (over.center().y - h / 2.0).clamp(2.0, (window.h - 2.0 - h).max(2.0));
    Rect::new(x.round(), y.round(), w, h)
}

type Act = Box<dyn FnMut(&mut EditorState) -> Reply>;

/// "Delete “Lead A”?" with Cancel and a confirming button.
pub struct Confirm {
    rect: Rect,
    message: String,
    action: String,
    act: Act,
    hover: Option<usize>,
    lettering: Lettering,
}

impl std::fmt::Debug for Confirm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Confirm")
            .field("message", &self.message)
            .finish_non_exhaustive()
    }
}

impl Confirm {
    /// The size [`new`](Self::new)'s rect should have.
    pub const SIZE: (f32, f32) = (260.0, 74.0);

    /// Ask `message`; the `action` button (or Enter) runs `on_confirm`.
    pub fn new(
        rect: Rect,
        message: &str,
        action: &str,
        on_confirm: impl FnMut(&mut EditorState) -> Reply + 'static,
    ) -> Self {
        Self {
            rect,
            message: message.to_owned(),
            action: action.to_owned(),
            act: Box::new(on_confirm),
            hover: None,
            lettering: Lettering::shared(),
        }
    }

    /// Cancel's rect (0) and the action's (1).
    pub fn button_rect(&self, i: usize) -> Rect {
        let r = self.rect;
        let x = r.right() - PAD - BUTTON_W * (2 - i) as f32 - 6.0 * (1 - i) as f32;
        Rect::new(x, r.bottom() - PAD - BUTTON_H, BUTTON_W, BUTTON_H)
    }

    fn button_at(&self, pos: Point) -> Option<usize> {
        (0..2).find(|&i| self.button_rect(i).contains(pos))
    }
}

impl Panel for Confirm {
    fn cursor(&self, pos: Point) -> MouseCursor {
        if self.button_at(pos).is_some() {
            MouseCursor::Hand
        } else {
            MouseCursor::Default
        }
    }

    fn rect(&self) -> Rect {
        self.rect
    }

    fn opaque(&self) -> Option<Rect> {
        crate::panel::body(self.rect)
    }

    fn draw(&self, canvas: &mut PixmapMut<'_>, transform: Transform) {
        frame(canvas, self.rect, transform);
        let r = self.rect;
        let line = Rect::new(r.x + PAD, r.y + PAD, r.w - 2.0 * PAD, 18.0);
        label(
            &self.lettering,
            canvas,
            &self.message,
            line.x,
            line,
            theme().ink,
            transform,
        );
        for (i, text) in ["Cancel", self.action.as_str()].iter().enumerate() {
            button(
                &self.lettering,
                canvas,
                text,
                self.button_rect(i),
                self.hover == Some(i),
                i == 1,
                transform,
            );
        }
    }

    fn input(&mut self, event: &RawKind, state: &mut EditorState) -> Reply {
        match *event {
            RawKind::Key {
                key: Key::Enter,
                pressed: true,
                ..
            } => (self.act)(state),
            RawKind::PointerMove { pos, .. } => {
                let hover = self.button_at(pos);
                if hover == self.hover {
                    return Reply::Ignored;
                }
                let rects = [self.hover, hover]
                    .into_iter()
                    .flatten()
                    .map(|i| self.button_rect(i))
                    .collect();
                self.hover = hover;
                Reply::repaint(rects)
            }
            RawKind::PointerDown { pos, .. } => match self.button_at(pos) {
                Some(0) => Reply::Close,
                Some(_) => (self.act)(state),
                None => Reply::Handled,
            },
            _ => Reply::Ignored,
        }
    }
}

type Save = Box<dyn FnMut(&str, Option<&str>, &mut EditorState) -> Result<Reply, String>>;

/// Save As: a name field, a folder chooser and Save / Cancel.
pub struct SaveAs {
    rect: Rect,
    name: TextField,
    folders: Vec<String>,
    /// The chosen folder (`None`: Uncategorised), shared with its list.
    folder: Rc<RefCell<Option<String>>>,
    window: Size,
    /// Why the last save didn't happen.
    message: Option<String>,
    on_save: Save,
    hover: Option<usize>,
    lettering: Lettering,
    /// The host's prompt for the name has been opened (it opens once by
    /// itself; a click on the name opens it again).
    asked: bool,
}

impl std::fmt::Debug for SaveAs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SaveAs")
            .field("name", &self.name.text())
            .field("folder", &self.folder.borrow())
            .finish_non_exhaustive()
    }
}

impl SaveAs {
    /// The size [`new`](Self::new)'s rect should have.
    pub const SIZE: (f32, f32) = (280.0, 130.0);

    /// A dialog over `rect` saving `name` into `folder` (one of
    /// `folders`, or `None` for Uncategorised). Save (or Enter) calls
    /// `on_save` with the name and folder: its reply is the dialog's, or
    /// its error shows in the dialog, which stays open.
    pub fn new(
        rect: Rect,
        name: &str,
        folder: Option<String>,
        folders: Vec<String>,
        window: Size,
        on_save: impl FnMut(&str, Option<&str>, &mut EditorState) -> Result<Reply, String> + 'static,
    ) -> Self {
        let mut d = Self {
            rect,
            name: TextField::new(Rect::new(0.0, 0.0, 1.0, 1.0), name, |_, _| Reply::Ignored),
            folders,
            folder: Rc::new(RefCell::new(folder)),
            window,
            message: None,
            on_save: Box::new(on_save),
            hover: None,
            lettering: Lettering::shared(),
            asked: false,
        };
        d.name =
            TextField::new(d.name_rect(), name, |_, _| Reply::Ignored).with_title("Save Preset As");
        d
    }

    /// Why the last save didn't happen, if it didn't.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    const LABEL_W: f32 = 52.0;

    pub fn name_rect(&self) -> Rect {
        let r = self.rect;
        Rect::new(
            r.x + PAD + Self::LABEL_W,
            r.y + PAD,
            r.w - 2.0 * PAD - Self::LABEL_W,
            FIELD_H,
        )
    }

    pub fn folder_rect(&self) -> Rect {
        let n = self.name_rect();
        Rect::new(n.x, n.bottom() + 8.0, n.w, FIELD_H)
    }

    /// Cancel's rect (0) and Save's (1).
    pub fn button_rect(&self, i: usize) -> Rect {
        let r = self.rect;
        let x = r.right() - PAD - BUTTON_W * (2 - i) as f32 - 6.0 * (1 - i) as f32;
        Rect::new(x, r.bottom() - PAD - BUTTON_H, BUTTON_W, BUTTON_H)
    }

    fn button_at(&self, pos: Point) -> Option<usize> {
        (0..2).find(|&i| self.button_rect(i).contains(pos))
    }

    fn save(&mut self, state: &mut EditorState) -> Reply {
        let name = self.name.text().to_owned();
        let folder = self.folder.borrow().clone();
        match (self.on_save)(&name, folder.as_deref(), state) {
            Ok(reply) => reply,
            Err(e) => {
                self.message = Some(e);
                Reply::Handled
            }
        }
    }

    /// The folder list, under (or over) the chooser.
    fn folder_list(&self) -> List {
        let mut labels = vec![UNCATEGORISED.to_owned()];
        labels.extend(self.folders.iter().cloned());
        let rows = labels.len().min(8) as f32;
        let h = rows * LIST_ROW_H + 6.0;
        let f = self.folder_rect();
        let y = if f.bottom() + 1.0 + h <= self.window.h - 2.0 {
            f.bottom() + 1.0
        } else {
            (f.y - 1.0 - h).max(2.0)
        };
        let current = self
            .folder
            .borrow()
            .as_ref()
            .and_then(|c| self.folders.iter().position(|f| f == c))
            .map_or(0, |i| i + 1);
        let folders = self.folders.clone();
        let chosen = Rc::clone(&self.folder);
        List::new(Rect::new(f.x, y, f.w, h), labels, move |i, _| {
            *chosen.borrow_mut() = i.checked_sub(1).map(|i| folders[i].clone());
            Reply::Close
        })
        .with_selected(current)
    }
}

impl Panel for SaveAs {
    fn takes_text(&self) -> bool {
        !self.name.hosted()
    }

    /// With a host prompt for the name: open it at once; its Enter saves,
    /// its cancel leaves the dialog open (a folder can still be chosen,
    /// the name clicked to ask again).
    fn poll(&mut self, state: &mut EditorState) -> Reply {
        if !self.asked {
            if !self.name.ask(state) {
                return Reply::Ignored;
            }
            self.asked = true;
        }
        match self.name.answered() {
            Some(Some(_)) => self.save(state),
            _ => Reply::Ignored,
        }
    }

    fn cursor(&self, pos: Point) -> MouseCursor {
        if self.button_at(pos).is_some() {
            MouseCursor::Hand
        } else {
            MouseCursor::Default
        }
    }

    fn rect(&self) -> Rect {
        self.rect
    }

    fn opaque(&self) -> Option<Rect> {
        crate::panel::body(self.rect)
    }

    fn draw(&self, canvas: &mut PixmapMut<'_>, transform: Transform) {
        frame(canvas, self.rect, transform);
        let x = self.rect.x + PAD;
        // The labels' column, left of the fields.
        let beside = |r: Rect| Rect::new(x, r.y, r.x - x, r.h);
        let n = self.name_rect();
        label(
            &self.lettering,
            canvas,
            "Name",
            x,
            beside(n),
            theme().dim_ink,
            transform,
        );
        self.name.draw(canvas, transform);
        let f = self.folder_rect();
        label(
            &self.lettering,
            canvas,
            "Folder",
            x,
            beside(f),
            theme().dim_ink,
            transform,
        );
        outline(canvas, f, transform);
        let shown = self
            .folder
            .borrow()
            .clone()
            .unwrap_or_else(|| UNCATEGORISED.to_owned());
        label(
            &self.lettering,
            canvas,
            &shown,
            f.x + 6.0,
            f,
            theme().ink,
            transform,
        );
        chevron(
            canvas,
            f.right() - 10.0,
            f.center().y,
            theme().dim_ink,
            transform,
        );
        if let Some(m) = self.message.as_deref() {
            let b = self.button_rect(0);
            let line = Rect::new(x, b.y, b.x - x - 4.0, b.h);
            label(&self.lettering, canvas, m, x, line, theme().ink, transform);
        }
        for (i, text) in ["Cancel", "Save"].iter().enumerate() {
            button(
                &self.lettering,
                canvas,
                text,
                self.button_rect(i),
                self.hover == Some(i),
                i == 1,
                transform,
            );
        }
    }

    fn input(&mut self, event: &RawKind, state: &mut EditorState) -> Reply {
        match *event {
            RawKind::Key {
                key: Key::Enter,
                pressed: true,
                ..
            } => self.save(state),
            RawKind::Key { .. } | RawKind::PointerUp { .. } => self.name.input(event, state),
            RawKind::PointerMove { pos, .. } => {
                let hover = self.button_at(pos);
                let text = self.name.input(event, state);
                let rects = if hover == self.hover {
                    Vec::new()
                } else {
                    [self.hover, hover]
                        .into_iter()
                        .flatten()
                        .map(|i| self.button_rect(i))
                        .collect()
                };
                self.hover = hover;
                Reply::repaint(rects).and(text)
            }
            RawKind::PointerDown { pos, .. } => {
                if self.name_rect().contains(pos) {
                    if self.name.hosted() {
                        self.name.ask(state);
                        return Reply::Handled;
                    }
                    return self.name.input(event, state);
                }
                if self.folder_rect().contains(pos) {
                    return Reply::Open(Box::new(self.folder_list()));
                }
                match self.button_at(pos) {
                    Some(0) => Reply::Close,
                    Some(_) => self.save(state),
                    None => Reply::Handled,
                }
            }
            _ => Reply::Ignored,
        }
    }
}

/// Unlock (design/licensing-hook.md): a licence code field with Paste, and Cancel and
/// Unlock. Enter unlocks, Esc (the panel stack) cancels. A refused code
/// shows why and stays in the field; a good one closes the dialog.
///
/// With a host prompt (a plugin), a click on the field asks the prompt
/// for the code, and its Enter unlocks; Paste works either way.
pub struct Unlock {
    rect: Rect,
    code: TextField,
    /// Why the last code was refused.
    message: Option<String>,
    hover: Option<usize>,
    lettering: Lettering,
}

impl std::fmt::Debug for Unlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Unlock")
            .field("code", &self.code.text())
            .field("message", &self.message)
            .finish_non_exhaustive()
    }
}

/// The dialog's buttons, in [`Unlock::button_rect`]'s order.
const UNLOCK_BUTTONS: [&str; 3] = ["Paste", "Cancel", "Unlock"];

impl Unlock {
    /// The size [`new`](Self::new)'s rect should have.
    pub const SIZE: (f32, f32) = (380.0, 124.0);

    /// An empty dialog over `rect`.
    pub fn new(rect: Rect) -> Self {
        let mut d = Self {
            rect,
            code: TextField::new(Rect::new(0.0, 0.0, 1.0, 1.0), "", |_, _| Reply::Ignored),
            message: None,
            hover: None,
            lettering: Lettering::shared(),
        };
        d.code = TextField::new(d.code_rect(), "", |_, _| Reply::Ignored)
            .with_title("Licence Code")
            // 112 characters, a prefix and dashes, with room for spaces.
            .with_max_chars(256);
        d
    }

    /// The dialog centred on a faceplate of `size`.
    pub fn centred(size: Size) -> Self {
        let (w, h) = Self::SIZE;
        Self::new(centred_over(
            Rect::new(0.0, 0.0, size.w, size.h),
            w,
            h,
            size,
        ))
    }

    /// The code as it stands.
    pub fn code(&self) -> &str {
        self.code.text()
    }

    /// Why the last code was refused, if it was.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    fn line_rect(&self) -> Rect {
        let r = self.rect;
        Rect::new(r.x + PAD, r.y + PAD, r.w - 2.0 * PAD, 18.0)
    }

    pub fn code_rect(&self) -> Rect {
        let l = self.line_rect();
        Rect::new(l.x, l.bottom() + 6.0, l.w - BUTTON_W - 6.0, FIELD_H)
    }

    /// Paste's rect (0), beside the field; Cancel's (1) and Unlock's (2),
    /// bottom right.
    pub fn button_rect(&self, i: usize) -> Rect {
        let r = self.rect;
        if i == 0 {
            let c = self.code_rect();
            return Rect::new(c.right() + 6.0, c.y - 1.0, BUTTON_W, BUTTON_H);
        }
        let x = r.right() - PAD - BUTTON_W * (3 - i) as f32 - 6.0 * (2 - i) as f32;
        Rect::new(x, r.bottom() - PAD - BUTTON_H, BUTTON_W, BUTTON_H)
    }

    fn button_at(&self, pos: Point) -> Option<usize> {
        (0..UNLOCK_BUTTONS.len()).find(|&i| self.button_rect(i).contains(pos))
    }

    fn paste(&mut self, state: &EditorState) -> Reply {
        match state.clipboard().and_then(|c| c.text()) {
            Some(text) => {
                self.code.set_text("");
                self.code.paste(&text);
                self.message = None;
            }
            None => self.message = Some("The clipboard holds no text.".to_owned()),
        }
        Reply::Handled
    }

    fn unlock(&mut self, state: &mut EditorState) -> Reply {
        let Some(licence) = state.licence() else {
            return Reply::Close;
        };
        match licence.unlock(self.code.text()) {
            Ok(()) => Reply::Close,
            Err(e) => {
                self.message = Some(e);
                Reply::Handled
            }
        }
    }
}

impl Panel for Unlock {
    fn takes_text(&self) -> bool {
        !self.code.hosted()
    }

    /// The host prompt's answer: Enter there unlocks.
    fn poll(&mut self, state: &mut EditorState) -> Reply {
        match self.code.answered() {
            Some(Some(_)) => self.unlock(state),
            _ => Reply::Ignored,
        }
    }

    fn cursor(&self, pos: Point) -> MouseCursor {
        if self.button_at(pos).is_some() {
            MouseCursor::Hand
        } else if self.code_rect().contains(pos) {
            MouseCursor::Text
        } else {
            MouseCursor::Default
        }
    }

    fn rect(&self) -> Rect {
        self.rect
    }

    fn opaque(&self) -> Option<Rect> {
        crate::panel::body(self.rect)
    }

    fn draw(&self, canvas: &mut PixmapMut<'_>, transform: Transform) {
        frame(canvas, self.rect, transform);
        let line = self.line_rect();
        label(
            &self.lettering,
            canvas,
            "Enter your licence code to unlock.",
            line.x,
            line,
            theme().ink,
            transform,
        );
        self.code.draw(canvas, transform);
        if let Some(m) = self.message.as_deref() {
            let c = self.code_rect();
            let line = Rect::new(line.x, c.bottom() + 6.0, line.w, 18.0);
            label(
                &self.lettering,
                canvas,
                m,
                line.x,
                line,
                theme().ink,
                transform,
            );
        }
        for (i, text) in UNLOCK_BUTTONS.iter().enumerate() {
            button(
                &self.lettering,
                canvas,
                text,
                self.button_rect(i),
                self.hover == Some(i),
                i == 2,
                transform,
            );
        }
    }

    fn input(&mut self, event: &RawKind, state: &mut EditorState) -> Reply {
        match *event {
            RawKind::Key {
                key: Key::Enter,
                pressed: true,
                ..
            } => self.unlock(state),
            RawKind::Key { .. } | RawKind::PointerUp { .. } => self.code.input(event, state),
            RawKind::PointerMove { pos, .. } => {
                let hover = self.button_at(pos);
                let text = self.code.input(event, state);
                let rects = if hover == self.hover {
                    Vec::new()
                } else {
                    [self.hover, hover]
                        .into_iter()
                        .flatten()
                        .map(|i| self.button_rect(i))
                        .collect()
                };
                self.hover = hover;
                Reply::repaint(rects).and(text)
            }
            RawKind::PointerDown { pos, .. } => {
                if self.code_rect().contains(pos) {
                    if self.code.ask(state) {
                        return Reply::Handled;
                    }
                    return self.code.input(event, state);
                }
                match self.button_at(pos) {
                    Some(0) => self.paste(state),
                    Some(1) => Reply::Close,
                    Some(_) => self.unlock(state),
                    None => Reply::Handled,
                }
            }
            _ => Reply::Ignored,
        }
    }
}
