//! Drop-downs: a field showing a stepped param's choice, and the
//! list of choices it opens.

use silkscreen_core::tiny_skia::{PixmapMut, Transform};
use silkscreen_core::{ControlId, Field, Rect, Widget, WidgetState};
use silkscreen_model::ParamIndex;
use silkscreen_text::Style;

use crate::EditorState;
use crate::list::{List, ROW_H};
use crate::panel::Reply;
use crate::theme::{TEXT_SIZE, color, over, theme};
use crate::tooltip::Lettering;

const PAD_X: f32 = 6.0;
/// Between the field and its list.
const GAP: f32 = 2.0;
/// The list's padding and border, above and below its rows.
const LIST_CHROME: f32 = 6.0;
/// Rows a list shows at most before it scrolls.
const MAX_ROWS: usize = 12;

/// The field: [`Field`]'s box and chevron, with the current choice's
/// label in Jost.
pub(crate) struct DropDownField {
    labels: &'static [&'static str],
    lettering: Lettering,
}

impl DropDownField {
    pub fn new(labels: &'static [&'static str]) -> Self {
        Self {
            labels,
            lettering: Lettering::shared(),
        }
    }
}

/// The choice a stepped display state shows.
fn index(state: &WidgetState) -> usize {
    let last = f32::from(state.steps.max(1) - 1);
    (state.value.clamp(0.0, 1.0) * last).round() as usize
}

impl Widget for DropDownField {
    fn draw(
        &self,
        canvas: &mut PixmapMut<'_>,
        rect: Rect,
        transform: Transform,
        state: &WidgetState,
    ) {
        let t = theme();
        let field = Field {
            body: color(t.background),
            body_hover: color(over(t.hover, t.background)),
            border: color(t.border),
            chevron: color(t.ink),
        };
        field.draw(canvas, rect, transform, state);
        let Some(label) = self.labels.get(index(state)) else {
            return;
        };
        let face = &self.lettering.face;
        let (ascent, descent) = face.line_metrics(TEXT_SIZE);
        let style = Style {
            face,
            size: TEXT_SIZE,
            color: color(theme().ink),
        };
        let right = rect.right() - Field::chevron_w(rect);
        self.lettering.text.borrow_mut().draw_clipped(
            canvas,
            label,
            (
                rect.x + PAD_X,
                rect.center().y + f32::midpoint(ascent, descent),
            ),
            &style,
            transform,
            Some([rect.x, rect.y, right, rect.bottom()]),
        );
    }
}

/// The list for a menu request on `control` over `param`: below the
/// field, as wide as it or its widest label, or above it when there's no
/// room; choosing commits the choice as one gesture and closes.
pub(crate) fn menu(state: &EditorState, control: ControlId, param: ParamIndex, scale: f32) -> List {
    let field = state.layout().control(control).rect;
    let window = state.layout().size;
    let spec = state.model().spec(param);
    let labels: Vec<String> = spec
        .position_labels()
        .iter()
        .map(|l| (*l).to_owned())
        .collect();
    let lettering = Lettering::shared();
    let widest = labels
        .iter()
        .map(|l| {
            lettering
                .text
                .borrow_mut()
                .width(&lettering.face, l, TEXT_SIZE, scale)
        })
        .fold(0.0_f32, f32::max);
    let w = field.w.max(widest + 2.0 * 8.0 + 10.0).min(window.w - 4.0);
    let rows = labels.len().clamp(1, MAX_ROWS) as f32;
    let below = window.h - 2.0 - (field.bottom() + GAP);
    let above = field.y - GAP - 2.0;
    let fit = |space: f32| ((space - LIST_CHROME) / ROW_H).floor().clamp(1.0, rows);
    let (h, y) = if fit(below) >= rows.min(4.0) || below >= above {
        let h = fit(below) * ROW_H + LIST_CHROME;
        (h, field.bottom() + GAP)
    } else {
        let h = fit(above) * ROW_H + LIST_CHROME;
        (h, field.y - GAP - h)
    };
    let x = field.x.min(window.w - 2.0 - w).max(2.0);
    let current = state.model().get(param).round() as usize;
    List::new(Rect::new(x, y, w, h), labels, move |i, state| {
        state.edit(|e| {
            e.begin(param);
            e.set(param, i as f32);
            e.end(param);
        });
        Reply::Close
    })
    .with_selected(current)
}
