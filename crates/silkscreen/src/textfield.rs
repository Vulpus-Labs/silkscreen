//! Single-line text entry: a preset's name, a search. A panel, so
//! it takes the keyboard while open; Enter commits, Esc (handled by the
//! panel stack) cancels.
//!
//! Where the host gives a [`TextPrompt`](crate::TextPrompt) (a plugin, in
//! a DAW that takes keys), the field asks it for the text instead, and
//! takes no keys: the prompt's Enter commits and its cancel closes the
//! field.

use std::cell::Cell;

use silkscreen_core::tiny_skia::{FillRule, Paint, PixmapMut, Rect as SkRect, Stroke, Transform};
use silkscreen_core::{Key, Modifiers, MouseCursor, Point, RawKind, Rect};
use silkscreen_text::Style;

use crate::EditorState;
use crate::panel::{Panel, Reply};
use crate::prompt::{Answer, Prompt};
use crate::theme::{RADIUS, TEXT_SIZE, color, rounded, theme};
use crate::tooltip::Lettering;

const PAD_X: f32 = 6.0;

type Commit = Box<dyn FnMut(&str, &mut EditorState) -> Reply>;

/// A one-line text field. Positions are char indices into the text.
pub struct TextField {
    rect: Rect,
    text: String,
    /// Where typing goes, and the selection's moving end.
    caret: usize,
    /// The selection's fixed end; equal to `caret` when nothing is
    /// selected.
    anchor: usize,
    max_chars: usize,
    /// Horizontal scroll keeping the caret in view, logical units.
    scroll: Cell<f32>,
    /// The scale last drawn at, for measuring during input.
    scale: Cell<f32>,
    /// A drag selecting text is in progress.
    selecting: bool,
    lettering: Lettering,
    on_commit: Commit,
    /// What a host prompt for the text is titled.
    title: String,
    /// The host prompts for the text (it has a `TextPrompt`).
    hosted: bool,
    /// The host's prompt, while one is open.
    prompt: Option<Prompt>,
}

impl std::fmt::Debug for TextField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextField")
            .field("text", &self.text)
            .field("caret", &self.caret)
            .field("anchor", &self.anchor)
            .finish_non_exhaustive()
    }
}

impl TextField {
    /// A field over `rect` holding `text`, all of it selected (so typing
    /// replaces it). Enter calls `on_commit` with the text.
    pub fn new(
        rect: Rect,
        text: &str,
        on_commit: impl FnMut(&str, &mut EditorState) -> Reply + 'static,
    ) -> Self {
        let n = text.chars().count();
        Self {
            rect,
            text: text.to_owned(),
            caret: n,
            anchor: 0,
            max_chars: 64,
            scroll: Cell::new(0.0),
            scale: Cell::new(1.0),
            selecting: false,
            lettering: Lettering::shared(),
            on_commit: Box::new(on_commit),
            title: "Name".to_owned(),
            hosted: false,
            prompt: None,
        }
    }

    /// Title a host's prompt for the text with `title` (default "Name").
    #[must_use]
    pub fn with_title(mut self, title: &str) -> Self {
        title.clone_into(&mut self.title);
        self
    }

    /// Replace the text, all of it selected.
    pub fn set_text(&mut self, text: &str) {
        let text: String = text.chars().take(self.max_chars).collect();
        self.caret = text.chars().count();
        self.anchor = 0;
        self.text = text;
        self.scroll.set(0.0);
    }

    /// Whether the host prompts for the text, as far as the field has
    /// seen (once it has polled with the editor state).
    pub fn hosted(&self) -> bool {
        self.hosted
    }

    /// If the host prompts for text, open its prompt (unless one is
    /// open). False if it doesn't: the field takes keys.
    pub(crate) fn ask(&mut self, state: &EditorState) -> bool {
        let Some(host) = state.text_prompt() else {
            return false;
        };
        self.hosted = true;
        if self.prompt.is_none() {
            self.prompt = Some(host.prompt(&self.title, &self.text));
        }
        true
    }

    /// The open prompt's answer, once it has come; text it gives becomes
    /// the field's.
    pub(crate) fn answered(&mut self) -> Option<Answer> {
        let answer = self.prompt.as_mut()?.answer()?;
        self.prompt = None;
        if let Some(text) = &answer {
            self.set_text(text);
        }
        Some(answer)
    }

    /// Refuse input beyond `n` characters (default 64).
    #[must_use]
    pub fn with_max_chars(mut self, n: usize) -> Self {
        self.max_chars = n;
        self
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// The selection as an ordered char range; empty when there is none.
    pub fn selection(&self) -> std::ops::Range<usize> {
        self.caret.min(self.anchor)..self.caret.max(self.anchor)
    }

    pub fn caret(&self) -> usize {
        self.caret
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    /// Byte offset of char index `i`.
    fn byte(&self, i: usize) -> usize {
        self.text
            .char_indices()
            .nth(i)
            .map_or(self.text.len(), |(b, _)| b)
    }

    /// Logical x of char boundary `i`, from the text's start.
    fn x_of(&self, i: usize) -> f32 {
        let prefix = &self.text[..self.byte(i)];
        self.lettering.text.borrow_mut().width(
            &self.lettering.face,
            prefix,
            TEXT_SIZE,
            self.scale.get(),
        )
    }

    /// The char boundary nearest logical `x` (absolute).
    fn index_at(&self, x: f32) -> usize {
        let local = x - (self.rect.x + PAD_X) + self.scroll.get();
        let mut best = (0, f32::INFINITY);
        for i in 0..=self.len() {
            let d = (self.x_of(i) - local).abs();
            if d < best.1 {
                best = (i, d);
            }
        }
        best.0
    }

    /// The width text may use inside the field.
    fn inner_w(&self) -> f32 {
        self.rect.w - 2.0 * PAD_X
    }

    /// Scroll so the caret is inside the field.
    fn reveal(&self) {
        let x = self.x_of(self.caret);
        let s = self.scroll.get();
        let w = self.inner_w();
        if x < s {
            self.scroll.set(x);
        } else if x > s + w {
            self.scroll.set(x - w);
        }
        let total = self.x_of(self.len());
        self.scroll
            .set(self.scroll.get().min((total - w).max(0.0)).max(0.0));
    }

    fn delete_selection(&mut self) -> bool {
        let range = self.selection();
        if range.is_empty() {
            return false;
        }
        let (a, b) = (self.byte(range.start), self.byte(range.end));
        self.text.replace_range(a..b, "");
        self.caret = range.start;
        self.anchor = range.start;
        true
    }

    fn insert(&mut self, c: char) {
        if c.is_control() {
            return;
        }
        let replacing = !self.selection().is_empty();
        if !replacing && self.len() >= self.max_chars {
            return;
        }
        self.delete_selection();
        let at = self.byte(self.caret);
        self.text.insert(at, c);
        self.caret += 1;
        self.anchor = self.caret;
    }

    /// Insert `text` at the caret, replacing the selection, as typing it
    /// would: line breaks and other control characters become spaces,
    /// and nothing goes past the field's limit.
    pub fn paste(&mut self, text: &str) {
        self.delete_selection();
        for c in text.trim().chars() {
            let c = if c.is_control() { ' ' } else { c };
            if self.len() >= self.max_chars {
                break;
            }
            self.insert(c);
        }
        self.reveal();
    }

    /// Move the caret to `to`, extending the selection with shift.
    fn go(&mut self, to: usize, mods: Modifiers) {
        self.caret = to.min(self.len());
        if !mods.shift {
            self.anchor = self.caret;
        }
    }

    fn key(&mut self, key: Key, mods: Modifiers, state: &mut EditorState) -> Reply {
        match key {
            Key::Enter => {
                let text = self.text.clone();
                return (self.on_commit)(&text, state);
            }
            Key::Char('a' | 'A') if mods.cmd => {
                self.anchor = 0;
                self.caret = self.len();
            }
            Key::Char('v' | 'V') if mods.cmd || mods.ctrl => {
                match state.clipboard().and_then(|c| c.text()) {
                    Some(text) => self.paste(&text),
                    None => return Reply::Ignored,
                }
            }
            Key::Char(_) if mods.cmd || mods.ctrl => return Reply::Ignored,
            Key::Char(c) => self.insert(c),
            Key::Backspace => {
                if !self.delete_selection() && self.caret > 0 {
                    self.anchor = self.caret - 1;
                    self.delete_selection();
                }
            }
            Key::Delete => {
                if !self.delete_selection() && self.caret < self.len() {
                    self.anchor = self.caret + 1;
                    self.delete_selection();
                }
            }
            Key::ArrowLeft => {
                let range = self.selection();
                if !mods.shift && !range.is_empty() {
                    self.go(range.start, mods);
                } else {
                    self.go(self.caret.saturating_sub(1), mods);
                }
            }
            Key::ArrowRight => {
                let range = self.selection();
                if !mods.shift && !range.is_empty() {
                    self.go(range.end, mods);
                } else {
                    self.go(self.caret + 1, mods);
                }
            }
            Key::Home | Key::ArrowUp => self.go(0, mods),
            Key::End | Key::ArrowDown => self.go(self.len(), mods),
            _ => return Reply::Ignored,
        }
        self.reveal();
        Reply::Handled
    }
}

impl Panel for TextField {
    fn cursor(&self, _: Point) -> MouseCursor {
        MouseCursor::Text
    }

    fn takes_text(&self) -> bool {
        !self.hosted
    }

    /// With a host prompt: ask, and commit what it answers. A commit that
    /// keeps the field open (a taken name) asks again; a cancel closes.
    fn poll(&mut self, state: &mut EditorState) -> Reply {
        if !self.ask(state) {
            return Reply::Ignored;
        }
        match self.answered() {
            None => Reply::Ignored,
            Some(None) => Reply::Close,
            Some(Some(_)) => {
                let text = self.text.clone();
                match (self.on_commit)(&text, state) {
                    r @ (Reply::Close | Reply::Open(_) | Reply::Replace(_)) => r,
                    _ => Reply::Handled,
                }
            }
        }
    }

    fn rect(&self) -> Rect {
        self.rect
    }

    fn draw(&self, canvas: &mut PixmapMut<'_>, transform: Transform) {
        let scale = transform.sx;
        if (scale - self.scale.get()).abs() > f32::EPSILON {
            self.scale.set(scale);
            self.reveal();
        }
        let r = self.rect;
        let inset = 0.5 / scale;
        let outline = Rect::new(
            r.x + inset,
            r.y + inset,
            r.w - 2.0 * inset,
            r.h - 2.0 * inset,
        );
        let mut paint = Paint {
            anti_alias: true,
            ..Paint::default()
        };
        let path = rounded(outline, RADIUS);
        if let Some(path) = &path {
            paint.set_color(color(theme().background));
            canvas.fill_path(path, &paint, FillRule::Winding, transform, None);
        }

        let (ascent, descent) = self.lettering.face.line_metrics(TEXT_SIZE);
        let left = r.x + PAD_X - self.scroll.get();
        let clip = [r.x + PAD_X - 1.0, r.y, r.right() - PAD_X + 1.0, r.bottom()];
        let line_top = r.center().y - (ascent - descent) / 2.0;
        let line_h = ascent - descent;

        let range = self.selection();
        if !range.is_empty() {
            let x0 = (left + self.x_of(range.start)).max(clip[0]);
            let x1 = (left + self.x_of(range.end)).min(clip[2]);
            if let Some(band) = SkRect::from_ltrb(x0, line_top, x1, line_top + line_h) {
                let mut fill = Paint::default();
                fill.set_color(color(theme().selected));
                canvas.fill_rect(band, &fill, transform, None);
            }
        }

        let style = Style {
            face: &self.lettering.face,
            size: TEXT_SIZE,
            color: color(theme().ink),
        };
        self.lettering.text.borrow_mut().draw_clipped(
            canvas,
            &self.text,
            (left, line_top + ascent),
            &style,
            transform,
            Some(clip),
        );

        // The caret: a one-pixel bar, snapped to a whole pixel.
        let x = ((left + self.x_of(self.caret)) * scale).round() / scale;
        if x >= clip[0]
            && x <= clip[2]
            && let Some(bar) = SkRect::from_xywh(x, line_top, 1.0 / scale, line_h)
        {
            let mut fill = Paint::default();
            fill.set_color(color(theme().ink));
            canvas.fill_rect(bar, &fill, transform, None);
        }

        if let Some(path) = &path {
            paint.set_color(color(theme().border));
            let stroke = Stroke {
                width: 1.0 / scale,
                ..Stroke::default()
            };
            canvas.stroke_path(path, &paint, &stroke, transform, None);
        }
    }

    fn input(&mut self, event: &RawKind, state: &mut EditorState) -> Reply {
        match *event {
            RawKind::Key {
                key,
                mods,
                pressed: true,
            } => self.key(key, mods, state),
            RawKind::PointerDown { pos, mods, .. } => {
                let i = self.index_at(pos.x);
                self.go(i, mods);
                self.selecting = true;
                Reply::Handled
            }
            RawKind::PointerMove { pos, .. } if self.selecting => {
                self.caret = self.index_at(pos.x);
                self.reveal();
                // The field alone, wherever it sits.
                Reply::Repaint(vec![self.rect()])
            }
            RawKind::PointerUp { .. } => {
                self.selecting = false;
                Reply::Ignored
            }
            _ => Reply::Ignored,
        }
    }
}
