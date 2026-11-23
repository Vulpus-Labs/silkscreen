//! A table panel of editable rows: the mod matrix's and the
//! preset browser's base. Columns with a header; rows of cells bound to
//! params; rows may dim by a predicate on the model; scrolling by wheel;
//! optionally a close button at the header's right.

use silkscreen_core::tiny_skia::{
    FillRule, LineCap, Paint, PathBuilder, PixmapMut, Rect as SkRect, Stroke, Transform,
};
use silkscreen_core::{MouseCursor, Point, RawKind, Rect, Size};
use std::rc::Rc;

use silkscreen_model::{Edit, Model, ParamIndex, ParamSpec};
use silkscreen_text::Style;

use crate::EditorState;
use crate::list::{Glyph, List, ROW_H as LIST_ROW_H, glyph_size};
use crate::panel::{Panel, Reply, Update};
use crate::theme::{RADIUS, Rgba, TEXT_SIZE, color, draw_close, rounded, theme};
use crate::tooltip::Lettering;

/// Row and header height, logical units.
pub const ROW_H: f32 = 20.0;
const PAD: f32 = 6.0;
/// A value cell's full range is this many logical units of drag.
const DRAG_SPAN: f32 = 300.0;
/// Shift drags this much finer.
const FINE: f32 = 0.1;
const HEADER_INK: Rgba = [141, 141, 146, 255];
const VALUE_TRACK: Rgba = [255, 255, 255, 22];
/// Rows a choice cell's list shows before it scrolls.
const MENU_ROWS: usize = 10;

/// A column: its header and width.
#[derive(Clone, Debug)]
pub struct Column {
    pub title: String,
    pub width: f32,
}

impl Column {
    pub fn new(title: &str, width: f32) -> Self {
        Self {
            title: title.to_owned(),
            width,
        }
    }
}

/// What a cell shows and edits.
pub enum Cell {
    /// Fixed text.
    Label(String),
    /// A toggle param, as a checkbox; a click flips it.
    Toggle(ParamIndex),
    /// A stepped param, showing its label; a click opens a list of its
    /// choices over the table, and choosing one sets it.
    Choice(ParamIndex),
    /// A choice cell whose commit is the caller's: `choose` gets the
    /// chosen index and makes the edits (e.g. setting the param and
    /// enabling its row), all one undo step.
    ChoiceWith { param: ParamIndex, choose: Choose },
    /// A choice cell whose choices each have a picture (a set of curve
    /// shapes, say): the cell and its list draw `glyph` before the label.
    GlyphChoice { param: ParamIndex, glyph: Glyph },
    /// A continuous param, as a horizontal fader with its value; drag to
    /// change it (Shift for fine), Cmd-click to reset, wheel to nudge.
    /// Bipolar params fill from the centre.
    Value(ParamIndex),
    /// A button: a click runs it.
    Action {
        label: String,
        run: Box<dyn Fn(&mut EditorState)>,
    },
    /// What [`Row::flag_when`] says of the row, blank while it says nothing.
    Flag,
}

impl std::fmt::Debug for Cell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Cell::Label(l) => write!(f, "Label({l:?})"),
            Cell::Toggle(p) => write!(f, "Toggle({p:?})"),
            Cell::Choice(p) | Cell::ChoiceWith { param: p, .. } => write!(f, "Choice({p:?})"),
            Cell::GlyphChoice { param, .. } => write!(f, "GlyphChoice({param:?})"),
            Cell::Value(p) => write!(f, "Value({p:?})"),
            Cell::Action { label, .. } => write!(f, "Action({label:?})"),
            Cell::Flag => f.write_str("Flag"),
        }
    }
}

impl Cell {
    /// The param the cell shows, if any.
    fn param(&self) -> Option<ParamIndex> {
        match self {
            Cell::Toggle(p)
            | Cell::Choice(p)
            | Cell::ChoiceWith { param: p, .. }
            | Cell::GlyphChoice { param: p, .. }
            | Cell::Value(p) => Some(*p),
            Cell::Label(_) | Cell::Action { .. } | Cell::Flag => None,
        }
    }
}

/// A [`Cell::ChoiceWith`] commit.
pub type Choose = Rc<dyn Fn(&mut Edit<'_>, usize)>;

type Dim = Box<dyn Fn(&Model) -> bool>;
type DimChoice = Box<dyn Fn(&Model, ParamIndex, usize) -> bool>;
type Flag = Box<dyn Fn(&Model) -> Option<&'static str>>;

/// A row of cells, one per column.
pub struct Row {
    pub cells: Vec<Cell>,
    /// When it returns true, the row draws dimmed (it still edits).
    pub dim: Option<Dim>,
    /// When it returns true for a choice cell's param and a choice, that
    /// choice draws dimmed in the cell's list (it can still be chosen).
    pub dim_choices: Option<DimChoice>,
    /// When it returns a note, the row draws in [`Theme::alert`](crate::Theme)
    /// ink, over any dimming, and its [`Cell::Flag`] shows the note (it still
    /// edits).
    pub flag: Option<Flag>,
}

impl std::fmt::Debug for Row {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Row")
            .field("cells", &self.cells)
            .finish_non_exhaustive()
    }
}

impl Row {
    pub fn new(cells: Vec<Cell>) -> Self {
        Self {
            cells,
            dim: None,
            dim_choices: None,
            flag: None,
        }
    }

    #[must_use]
    pub fn dim_when(mut self, dim: impl Fn(&Model) -> bool + 'static) -> Self {
        self.dim = Some(Box::new(dim));
        self
    }

    /// Dims the choices `dim` picks in this row's choice cells' lists:
    /// those the row's other fields rule out, say.
    #[must_use]
    pub fn dim_choices_when(
        mut self,
        dim: impl Fn(&Model, ParamIndex, usize) -> bool + 'static,
    ) -> Self {
        self.dim_choices = Some(Box::new(dim));
        self
    }

    /// Flags the row while `flag` returns a note: something wrong with what
    /// its fields add up to, which only reads params its cells show.
    #[must_use]
    pub fn flag_when(mut self, flag: impl Fn(&Model) -> Option<&'static str> + 'static) -> Self {
        self.flag = Some(Box::new(flag));
        self
    }
}

/// A value drag in progress: the param, its normalized value and the
/// pointer's x when the drag began.
struct Drag {
    param: ParamIndex,
    start: f32,
    x0: f32,
}

pub struct Table {
    rect: Rect,
    columns: Vec<Column>,
    rows: Vec<Row>,
    specs: &'static [ParamSpec],
    /// The params the cells show, each once.
    watched: Vec<ParamIndex>,
    /// Each param's value as of the last update (only `watched` are
    /// kept current), by param index.
    values: Vec<f32>,
    dimmed: Vec<bool>,
    /// Each row's note, as last read.
    flags: Vec<Option<&'static str>>,
    scroll: f32,
    hover: Option<(usize, usize)>,
    drag: Option<Drag>,
    /// Whether the header has a close button, and whether it's hovered.
    close: bool,
    close_hover: bool,
    lettering: Lettering,
}

impl std::fmt::Debug for Table {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Table")
            .field("rect", &self.rect)
            .field("rows", &self.rows.len())
            .field("scroll", &self.scroll)
            .finish_non_exhaustive()
    }
}

impl Table {
    pub fn new(rect: Rect, columns: Vec<Column>, rows: Vec<Row>, state: &EditorState) -> Self {
        let model = state.model();
        let mut t = Self {
            rect,
            columns,
            rows,
            specs: model.specs(),
            watched: Vec::new(),
            values: Vec::new(),
            dimmed: Vec::new(),
            flags: Vec::new(),
            scroll: 0.0,
            hover: None,
            drag: None,
            close: false,
            close_hover: false,
            lettering: Lettering::shared(),
        };
        t.refresh(model);
        t
    }

    /// With a close button (an X) at the header's right; a click on it
    /// closes the table. Leave the last column's title empty to make room.
    #[must_use]
    pub fn with_close(mut self) -> Self {
        self.close = true;
        self
    }

    /// The close button's rect: the header's rightmost square.
    fn close_rect(&self) -> Option<Rect> {
        self.close.then(|| {
            Rect::new(
                self.rect.right() - 1.0 - ROW_H,
                self.rect.y + 1.0,
                ROW_H,
                ROW_H,
            )
        })
    }

    fn on_close(&self, pos: Point) -> bool {
        self.close_rect().is_some_and(|r| r.contains(pos))
    }

    /// Re-read the cells' values and the rows' dimming; the rows whose
    /// look changed.
    fn refresh(&mut self, model: &Model) -> Vec<usize> {
        if self.values.is_empty() {
            self.values = vec![f32::NAN; self.specs.len()];
            for p in self
                .rows
                .iter()
                .flat_map(|r| r.cells.iter().filter_map(Cell::param))
            {
                if !self.watched.contains(&p) {
                    self.watched.push(p);
                }
            }
        }
        self.dimmed.resize(self.rows.len(), false);
        self.flags.resize(self.rows.len(), None);
        let mut moved = Vec::new();
        for &p in &self.watched {
            let now = model.get(p);
            if now.to_bits() != self.values[p.index()].to_bits() {
                self.values[p.index()] = now;
                moved.push(p);
            }
        }
        let mut rows = Vec::new();
        for (i, (row, dim)) in self.rows.iter().zip(&mut self.dimmed).enumerate() {
            let now = row.dim.as_ref().is_some_and(|d| d(model));
            let flag = row.flag.as_ref().and_then(|f| f(model));
            let shows_moved = row
                .cells
                .iter()
                .filter_map(Cell::param)
                .any(|p| moved.contains(&p));
            if now != *dim || flag != self.flags[i] || shows_moved {
                rows.push(i);
            }
            *dim = now;
            self.flags[i] = flag;
        }
        rows
    }

    /// Row `i`'s band, cut to the viewport; `None` if scrolled out.
    fn row_rect(&self, i: usize) -> Option<Rect> {
        let view = self.view();
        let top = view.y + i as f32 * ROW_H - self.scroll;
        Rect::new(view.x, top, view.w, ROW_H).intersect(&view)
    }

    fn value(&self, p: ParamIndex) -> f32 {
        self.values[p.index()]
    }

    fn normalized(&self, p: ParamIndex) -> f32 {
        self.specs[p.index()].normalize(self.value(p))
    }

    /// The rows' viewport, below the header.
    fn view(&self) -> Rect {
        let r = self.rect;
        Rect::new(r.x + 1.0, r.y + 1.0 + ROW_H, r.w - 2.0, r.h - 2.0 - ROW_H)
    }

    fn max_scroll(&self) -> f32 {
        (self.rows.len() as f32 * ROW_H - self.view().h).max(0.0)
    }

    /// Left edge of column `c`.
    fn column_x(&self, c: usize) -> f32 {
        self.rect.x + 1.0 + self.columns[..c].iter().map(|col| col.width).sum::<f32>()
    }

    /// Cell `(row, column)`'s rect, scrolled (it may lie outside the view).
    fn cell_rect(&self, row: usize, c: usize) -> Rect {
        let view = self.view();
        Rect::new(
            self.column_x(c),
            view.y + row as f32 * ROW_H - self.scroll,
            self.columns[c].width,
            ROW_H,
        )
    }

    fn cell_at(&self, pos: Point) -> Option<(usize, usize)> {
        let view = self.view();
        if !view.contains(pos) {
            return None;
        }
        let row = ((pos.y - view.y + self.scroll) / ROW_H).floor() as usize;
        if row >= self.rows.len() {
            return None;
        }
        let mut x = self.rect.x + 1.0;
        for (c, col) in self.columns.iter().enumerate() {
            if pos.x >= x && pos.x < x + col.width {
                return Some((row, c));
            }
            x += col.width;
        }
        None
    }

    /// A list of `p`'s choices under (or over) cell `rect`, kept inside a
    /// `window` of that size (scrolling if its rows don't all fit); choosing
    /// runs `choose`, or else sets `p`, as one undo step.
    fn menu(
        &self,
        p: ParamIndex,
        rect: Rect,
        window: Size,
        choose: Option<Choose>,
        dimmed: Vec<bool>,
        glyph: Option<Glyph>,
    ) -> List {
        let spec = &self.specs[p.index()];
        let labels: Vec<String> = spec
            .position_labels()
            .iter()
            .map(|l| (*l).to_owned())
            .collect();
        let rows = labels.len().clamp(1, MENU_ROWS) as f32;
        let width = rect.w.max(if glyph.is_some() { 130.0 } else { 100.0 });
        // Rows that fit in `space`, at least one.
        let fit = |space: f32| ((space - 6.0) / LIST_ROW_H).floor().clamp(1.0, rows);
        let below = window.h - 2.0 - (rect.bottom() + 1.0);
        let above = rect.y - 1.0 - 2.0;
        let list = if fit(below) >= rows || below >= above {
            let height = fit(below) * LIST_ROW_H + 6.0;
            Rect::new(0.0, rect.bottom() + 1.0, width, height)
        } else {
            let height = fit(above) * LIST_ROW_H + 6.0;
            Rect::new(0.0, rect.y - 1.0 - height, width, height)
        };
        let current = self.value(p).round() as usize;
        let x = rect.x.min(window.w - 2.0 - width).max(2.0);
        let list = List::new(Rect { x, ..list }, labels, move |i, state| {
            state.edit(|e| {
                e.group(|e| {
                    if let Some(choose) = &choose {
                        choose(e, i);
                    } else {
                        e.begin(p);
                        e.set(p, i as f32);
                        e.end(p);
                    }
                });
            });
            Reply::Close
        })
        .with_selected(current)
        .with_dimmed(dimmed);
        match glyph {
            Some(glyph) => list.with_glyph(glyph),
            None => list,
        }
    }

    fn fill(canvas: &mut PixmapMut<'_>, r: Rect, rgba: Rgba, transform: Transform) {
        if let Some(rect) = SkRect::from_xywh(r.x, r.y, r.w, r.h) {
            let mut paint = Paint::default();
            paint.set_color(color(rgba));
            canvas.fill_rect(rect, &paint, transform, None);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn text(
        &self,
        canvas: &mut PixmapMut<'_>,
        label: &str,
        x: f32,
        rect: Rect,
        ink: Rgba,
        clip: [f32; 4],
        transform: Transform,
    ) {
        let face = &self.lettering.face;
        let (ascent, descent) = face.line_metrics(TEXT_SIZE);
        let style = Style {
            face,
            size: TEXT_SIZE,
            color: color(ink),
        };
        let clip = [
            clip[0].max(rect.x),
            clip[1].max(rect.y),
            clip[2].min(rect.right()),
            clip[3].min(rect.bottom()),
        ];
        self.lettering.text.borrow_mut().draw_clipped(
            canvas,
            label,
            (x, rect.center().y + f32::midpoint(ascent, descent)),
            &style,
            transform,
            Some(clip),
        );
    }

    fn draw_cell(
        &self,
        canvas: &mut PixmapMut<'_>,
        cell: &Cell,
        r: Rect,
        ink: Rgba,
        clip: [f32; 4],
        transform: Transform,
    ) {
        match cell {
            Cell::Label(label) => self.text(canvas, label, r.x + PAD, r, ink, clip, transform),
            Cell::Flag => {}
            Cell::Toggle(p) => {
                let size = 10.0;
                let b = Rect::new(
                    r.center().x - size / 2.0,
                    r.center().y - size / 2.0,
                    size,
                    size,
                );
                if b.y >= clip[1] && b.bottom() <= clip[3] {
                    let mut paint = Paint {
                        anti_alias: true,
                        ..Paint::default()
                    };
                    if let Some(path) = rounded(b, 2.0) {
                        if self.value(*p) >= 0.5 {
                            paint.set_color(color(theme().selected));
                            canvas.fill_path(&path, &paint, FillRule::Winding, transform, None);
                        }
                        paint.set_color(color(ink));
                        let stroke = Stroke {
                            width: 1.0,
                            ..Stroke::default()
                        };
                        canvas.stroke_path(&path, &paint, &stroke, transform, None);
                    }
                }
            }
            Cell::Choice(p)
            | Cell::ChoiceWith { param: p, .. }
            | Cell::GlyphChoice { param: p, .. } => {
                let spec = &self.specs[p.index()];
                let label = spec.format(self.value(*p));
                let mut x = r.x + PAD;
                if let Cell::GlyphChoice { glyph, .. } = cell {
                    let (gw, gh) = glyph_size(ROW_H);
                    let at = Rect::new(x, r.center().y - gh / 2.0, gw, gh);
                    if at.y >= clip[1] && at.bottom() <= clip[3] {
                        glyph(canvas, at, self.value(*p).round() as usize, ink, transform);
                    }
                    x += gw + 5.0;
                }
                self.text(canvas, &label, x, r, ink, clip, transform);
                chevron(canvas, r, ink, clip, transform);
            }
            Cell::Value(p) => {
                let spec = &self.specs[p.index()];
                let track = Rect::new(r.x + PAD, r.y + 4.0, r.w - 2.0 * PAD, r.h - 8.0);
                let cut = |t: Rect| {
                    let top = t.y.max(clip[1]);
                    let bottom = t.bottom().min(clip[3]);
                    (bottom > top).then(|| Rect::new(t.x, top, t.w, bottom - top))
                };
                if let Some(t) = cut(track) {
                    Self::fill(canvas, t, VALUE_TRACK, transform);
                }
                let n = self.normalized(*p);
                let origin: f32 = if spec.is_bipolar() { 0.5 } else { 0.0 };
                let (lo, hi) = (origin.min(n), origin.max(n));
                // On whole pixels, as the scrollbar's thumb is, so a
                // partial repaint matches a full one.
                let px = |v: f32| (v * transform.sx).round() / transform.sx;
                let (x0, x1) = (px(track.x + lo * track.w), px(track.x + hi * track.w));
                let bar = Rect::new(x0, track.y, x1 - x0, track.h);
                if let Some(shown) = cut(bar) {
                    Self::fill(canvas, shown, theme().selected, transform);
                }
                let label = spec.format(self.value(*p));
                self.text(canvas, &label, track.x + 4.0, r, ink, clip, transform);
            }
            Cell::Action { label, .. } => {
                let w = self.lettering.text.borrow_mut().width(
                    &self.lettering.face,
                    label,
                    TEXT_SIZE,
                    transform.sx,
                );
                self.text(
                    canvas,
                    label,
                    r.center().x - w / 2.0,
                    r,
                    ink,
                    clip,
                    transform,
                );
            }
        }
    }

    fn click(&mut self, row: usize, c: usize, mods_cmd: bool, state: &mut EditorState) -> Reply {
        let rect = self.cell_rect(row, c);
        // The row's dimmed choices for param `p`.
        let dimmed = |p: ParamIndex| -> Vec<bool> {
            let count = self.specs[p.index()].position_labels().len();
            let model = state.model();
            match &self.rows[row].dim_choices {
                Some(dim) => (0..count).map(|i| dim(model, p, i)).collect(),
                None => Vec::new(),
            }
        };
        match &self.rows[row].cells[c] {
            Cell::Toggle(p) => {
                let p = *p;
                let next = if self.value(p) >= 0.5 { 0.0 } else { 1.0 };
                state.edit(|e| {
                    e.begin(p);
                    e.set(p, next);
                    e.end(p);
                });
                Reply::Handled
            }
            Cell::Choice(p) => {
                let dimmed = dimmed(*p);
                Reply::Open(Box::new(self.menu(
                    *p,
                    rect,
                    state.layout().size,
                    None,
                    dimmed,
                    None,
                )))
            }
            Cell::GlyphChoice { param, glyph } => {
                let dimmed = dimmed(*param);
                Reply::Open(Box::new(self.menu(
                    *param,
                    rect,
                    state.layout().size,
                    None,
                    dimmed,
                    Some(Rc::clone(glyph)),
                )))
            }
            Cell::ChoiceWith { param, choose } => {
                let dimmed = dimmed(*param);
                Reply::Open(Box::new(self.menu(
                    *param,
                    rect,
                    state.layout().size,
                    Some(Rc::clone(choose)),
                    dimmed,
                    None,
                )))
            }
            Cell::Value(p) if mods_cmd => {
                let p = *p;
                state.edit(|e| e.reset(p));
                Reply::Handled
            }
            Cell::Action { run, .. } => {
                run(state);
                Reply::Handled
            }
            _ => Reply::Ignored,
        }
    }
}

/// A choice cell's chevron, at the right of `r`.
fn chevron(canvas: &mut PixmapMut<'_>, r: Rect, ink: Rgba, clip: [f32; 4], transform: Transform) {
    let (cx, cy, half) = (r.right() - 8.0, r.center().y, 2.5);
    if cy - half < clip[1] || cy + half > clip[3] {
        return;
    }
    let mut pb = PathBuilder::new();
    pb.move_to(cx - half, cy - half / 2.0);
    pb.line_to(cx, cy + half / 2.0);
    pb.line_to(cx + half, cy - half / 2.0);
    if let Some(path) = pb.finish() {
        let mut paint = Paint {
            anti_alias: true,
            ..Paint::default()
        };
        paint.set_color(color(ink));
        let stroke = Stroke {
            width: 1.2,
            line_cap: LineCap::Round,
            ..Stroke::default()
        };
        canvas.stroke_path(&path, &paint, &stroke, transform, None);
    }
}

impl Panel for Table {
    fn cursor(&self, pos: Point) -> MouseCursor {
        if self.on_close(pos) {
            return MouseCursor::Hand;
        }
        match self.cell_at(pos).map(|(row, c)| &self.rows[row].cells[c]) {
            Some(Cell::Value(_)) => MouseCursor::Horizontal,
            Some(Cell::Label(_) | Cell::Flag) | None => MouseCursor::Default,
            Some(_) => MouseCursor::Hand,
        }
    }

    fn rect(&self) -> Rect {
        self.rect
    }

    fn opaque(&self) -> Option<Rect> {
        crate::panel::body(self.rect)
    }

    fn draw(&self, canvas: &mut PixmapMut<'_>, transform: Transform) {
        let inset = 0.5 / transform.sx;
        let outline = Rect::new(
            self.rect.x + inset,
            self.rect.y + inset,
            self.rect.w - 2.0 * inset,
            self.rect.h - 2.0 * inset,
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

        // Header.
        let header = Rect::new(
            self.rect.x + 1.0,
            self.rect.y + 1.0,
            self.rect.w - 2.0,
            ROW_H,
        );
        let all = [header.x, header.y, header.right(), header.bottom()];
        for (c, col) in self.columns.iter().enumerate() {
            let r = Rect::new(self.column_x(c), header.y, col.width, ROW_H);
            self.text(canvas, &col.title, r.x + PAD, r, HEADER_INK, all, transform);
        }
        Self::fill(
            canvas,
            Rect::new(
                header.x,
                header.bottom() - 1.0 / transform.sx,
                header.w,
                1.0 / transform.sx,
            ),
            theme().border,
            transform,
        );
        if let Some(r) = self.close_rect() {
            let glyph = Rect::new(r.x + 2.0, r.y + 2.0, r.w - 4.0, r.h - 4.0);
            draw_close(canvas, glyph, self.close_hover, transform);
        }

        // Rows, clipped to the view.
        let view = self.view();
        let clip = [view.x, view.y, view.right(), view.bottom()];
        let first = (self.scroll / ROW_H).floor() as usize;
        for (i, row) in self.rows.iter().enumerate().skip(first) {
            let top = view.y + i as f32 * ROW_H - self.scroll;
            if top >= view.bottom() {
                break;
            }
            let ink = if self.flags[i].is_some() {
                theme().alert
            } else if self.dimmed[i] {
                theme().dim_ink
            } else {
                theme().ink
            };
            for (c, cell) in row.cells.iter().enumerate().take(self.columns.len()) {
                let r = self.cell_rect(i, c);
                if self.hover == Some((i, c)) && !matches!(cell, Cell::Label(_) | Cell::Flag) {
                    let top = r.y.max(view.y);
                    let bottom = r.bottom().min(view.bottom());
                    Self::fill(
                        canvas,
                        Rect::new(r.x, top, r.w, bottom - top),
                        theme().hover,
                        transform,
                    );
                }
                if let (Cell::Flag, Some(note)) = (cell, self.flags[i]) {
                    self.text(canvas, note, r.x + PAD, r, ink, clip, transform);
                } else {
                    self.draw_cell(canvas, cell, r, ink, clip, transform);
                }
            }
        }

        let content_h = self.rows.len() as f32 * ROW_H;
        if let Some((track, thumb)) =
            crate::list::scrollbar(view, content_h, self.scroll, transform.sy)
        {
            Self::fill(canvas, track, theme().scroll_track, transform);
            Self::fill(canvas, thumb, theme().scroll_thumb, transform);
        }

        if let Some(path) = &path {
            paint.set_color(color(theme().border));
            let stroke = Stroke {
                width: 1.0 / transform.sx,
                ..Stroke::default()
            };
            canvas.stroke_path(path, &paint, &stroke, transform, None);
        }
    }

    fn input(&mut self, event: &RawKind, state: &mut EditorState) -> Reply {
        match *event {
            RawKind::Wheel { pos, delta, mods } => {
                // Over a value cell, the wheel nudges it; elsewhere it
                // scrolls.
                if let Some((row, c)) = self.cell_at(pos)
                    && let Cell::Value(p) = self.rows[row].cells[c]
                {
                    let step = if mods.shift { 0.001 } else { 0.01 } * delta.y.signum();
                    let n = (self.normalized(p) + step).clamp(0.0, 1.0);
                    state.edit(|e| {
                        e.begin(p);
                        e.set_normalized(p, n);
                        e.end(p);
                    });
                    return Reply::Handled;
                }
                let s = (self.scroll - delta.y * ROW_H).clamp(0.0, self.max_scroll());
                if s == self.scroll {
                    Reply::Ignored
                } else {
                    self.scroll = s;
                    Reply::Handled
                }
            }
            RawKind::PointerMove { pos, mods } => {
                if let Some(d) = &self.drag {
                    let scale = if mods.shift { FINE } else { 1.0 };
                    let n = (d.start + (pos.x - d.x0) / DRAG_SPAN * scale).clamp(0.0, 1.0);
                    let p = d.param;
                    state.edit(|e| e.set_normalized(p, n));
                    // The row repaints when `update` sees the value move.
                    return Reply::Ignored;
                }
                let hover = self.cell_at(pos);
                let close_hover = self.on_close(pos);
                // Repaint the cells the hover left and entered, and the X.
                let mut rects = Vec::new();
                if hover != self.hover {
                    let view = self.view();
                    for (row, c) in [self.hover, hover].into_iter().flatten() {
                        rects.extend(self.cell_rect(row, c).intersect(&view));
                    }
                }
                if close_hover != self.close_hover {
                    rects.extend(self.close_rect());
                }
                self.hover = hover;
                self.close_hover = close_hover;
                Reply::repaint(rects)
            }
            RawKind::PointerDown { pos, mods, .. } => {
                if self.on_close(pos) {
                    return Reply::Close;
                }
                let Some((row, c)) = self.cell_at(pos) else {
                    return Reply::Ignored;
                };
                if let Cell::Value(p) = self.rows[row].cells[c]
                    && !mods.cmd
                {
                    state.edit(|e| e.begin(p));
                    self.drag = Some(Drag {
                        param: p,
                        start: self.normalized(p),
                        x0: pos.x,
                    });
                    return Reply::Handled;
                }
                self.click(row, c, mods.cmd, state)
            }
            RawKind::PointerUp { .. } => {
                if let Some(d) = self.drag.take() {
                    state.edit(|e| e.end(d.param));
                    return Reply::Handled;
                }
                Reply::Ignored
            }
            _ => Reply::Ignored,
        }
    }

    fn update(&mut self, state: &EditorState) -> Update {
        let rows = self.refresh(state.model());
        if rows.is_empty() {
            return Update::Unchanged;
        }
        Update::Rects(rows.into_iter().filter_map(|i| self.row_rect(i)).collect())
    }
}
