//! Widgets: how controls are drawn ([design/rendering.md]).
//!
//! A widget is pure drawing: given its logical rect and a [`WidgetState`],
//! it paints. Interaction logic lives in bindings (`silkscreen-model`).
//!
//! [design/rendering.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/rendering.md

use tiny_skia::{
    Color, FillRule, FilterQuality, LineCap, Paint, PathBuilder, PixmapMut, PixmapPaint, Stroke,
    Transform,
};

use crate::geom::{Point, Rect};
use crate::layout::{ControlKind, ControlSpec};
use crate::raster::{SharedSkin, SkinCache};

/// Everything a widget needs to draw itself: the displayed value plus
/// interaction state. Bindings supply `value`; the view fills in the flags
/// from the gesture recognizer.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct WidgetState {
    /// Normalized `[0, 1]`.
    pub value: f32,
    /// Number of discrete positions (a stepped param's label count), or 0
    /// for a continuous value. Widgets draw detents from it.
    pub steps: u16,
    /// Normalized value a value arc grows from: 0 for most params, ½ for
    /// bipolar ones.
    pub origin: f32,
    /// The pointer is over the control (and no button is held elsewhere).
    pub hover: bool,
    /// The control is being pressed or dragged.
    pub active: bool,
    /// The control has keyboard focus.
    pub focused: bool,
    /// A tab whose page is shown ([design/faceplates.md]). The view sets it.
    ///
    /// [design/faceplates.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/faceplates.md
    pub selected: bool,
    /// The control does nothing in the current patch. It
    /// still works; the compositor draws it at [`DIM_OPACITY`] over the
    /// faceplate, so widgets need not handle it, unless they draw it
    /// themselves ([`Widget::draws_dimmed`]).
    ///
    /// [`DIM_OPACITY`]: crate::compositor::DIM_OPACITY
    pub dimmed: bool,
}

impl WidgetState {
    /// A value with `steps` discrete positions.
    pub const fn stepped(value: f32, steps: u16) -> Self {
        let mut s = Self::value(value);
        s.steps = steps;
        s
    }

    /// A value whose arc grows from `origin` (e.g. ½ for bipolar).
    pub const fn from_origin(value: f32, origin: f32) -> Self {
        let mut s = Self::value(value);
        s.origin = origin;
        s
    }

    pub const fn value(value: f32) -> Self {
        Self {
            value,
            steps: 0,
            origin: 0.0,
            hover: false,
            active: false,
            focused: false,
            selected: false,
            dimmed: false,
        }
    }
}

/// Draws one control.
pub trait Widget {
    /// Paint the control occupying logical `rect`. `transform` maps logical
    /// units to `canvas` pixels (scale and offset are already applied); draw
    /// with it. Stay inside `rect`: the compositor repaints only the
    /// rects it damages, so ink outside one (a stroke's half-width, a
    /// miter's point) is left behind when the control changes.
    fn draw(
        &self,
        canvas: &mut PixmapMut<'_>,
        rect: Rect,
        transform: Transform,
        state: &WidgetState,
    );

    /// The widget shows [`WidgetState::dimmed`] itself, so the compositor
    /// draws it at full opacity: a sprite fader with a dark cap, say.
    fn draws_dimmed(&self) -> bool {
        false
    }
}

/// Draws nothing: the faceplate shows through. A canvas control's
/// stand-in where nothing paints it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Blank;

impl Widget for Blank {
    fn draw(&self, _: &mut PixmapMut<'_>, _: Rect, _: Transform, _: &WidgetState) {}
}

/// The built-in vector widget for a control kind.
pub fn default_widget(kind: ControlKind) -> Box<dyn Widget> {
    match kind {
        ControlKind::Knob => Box::new(Knob::default()),
        ControlKind::Fader => Box::new(Fader::default()),
        ControlKind::Selector => Box::new(Selector::default()),
        ControlKind::Toggle => Box::new(Bat::default()),
        ControlKind::Button | ControlKind::Radio { .. } => Box::new(Lamp::default()),
        ControlKind::Tab { .. } => Box::new(Tab::default()),
        ControlKind::DropDown | ControlKind::Preset => Box::new(Field::default()),
        ControlKind::Action => Box::new(Push::default()),
        ControlKind::Scope => Box::new(crate::telemetry::Scope::default()),
        ControlKind::Meter => Box::new(crate::telemetry::Meter::default()),
        ControlKind::Canvas => Box::new(Blank),
    }
}

/// The built-in widget for a control: drawn from its skin if it has one
/// ([design/faceplates.md]), its frames shared through `skins`, otherwise the
/// kind's vector widget.
///
/// [design/faceplates.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/faceplates.md
pub fn widget_for(spec: &ControlSpec, skins: &mut SkinCache) -> Box<dyn Widget> {
    let mut frames = |skin| skins.get(skin);
    match (spec.kind, spec.skin) {
        (ControlKind::Toggle | ControlKind::Button | ControlKind::Radio { .. }, Some(skin)) => {
            Box::new(SpriteSwitch::new(frames(skin)))
        }
        (ControlKind::Tab { .. }, Some(skin)) => Box::new(SpriteTab::new(frames(skin))),
        (ControlKind::Knob, Some(skin)) => Box::new(SpriteKnob::new(frames(skin))),
        (ControlKind::Fader, Some(skin)) => Box::new(SpriteFader::fader(frames(skin))),
        (ControlKind::Selector, Some(skin)) => Box::new(SpriteFader::selector(frames(skin))),
        (kind, _) => default_widget(kind),
    }
}

/// Draw `frame`, `scale` physical pixels per logical unit, filling logical
/// `rect`.
fn draw_frame(
    canvas: &mut PixmapMut<'_>,
    frame: &tiny_skia::Pixmap,
    rect: Rect,
    transform: Transform,
    scale: f32,
) {
    let paint = PixmapPaint {
        quality: FilterQuality::Bilinear,
        ..PixmapPaint::default()
    };
    let t = transform
        .pre_translate(rect.x, rect.y)
        .pre_scale(1.0 / scale, 1.0 / scale);
    canvas.draw_pixmap(0, 0, frame.as_ref(), &paint, t, None);
}

/// Draw the part of `frame` (laid out as by [`draw_frame`] over `rect`)
/// that falls within logical `part`.
fn draw_frame_part(
    canvas: &mut PixmapMut<'_>,
    frame: &tiny_skia::Pixmap,
    rect: Rect,
    part: Rect,
    transform: Transform,
    scale: f32,
) {
    let Some(part) = tiny_skia::Rect::from_xywh(part.x, part.y, part.w, part.h) else {
        return;
    };
    let paint = Paint {
        shader: tiny_skia::Pattern::new(
            frame.as_ref(),
            tiny_skia::SpreadMode::Pad,
            FilterQuality::Bilinear,
            1.0,
            Transform::from_translate(rect.x, rect.y).pre_scale(1.0 / scale, 1.0 / scale),
        ),
        anti_alias: true,
        ..Paint::default()
    };
    canvas.fill_rect(part, &paint, transform, None);
}

/// The body frame, of `steps` spread evenly across one `period_deg`, for
/// a knob turned `angle_deg` clockwise from 12 o'clock.
pub fn cycle_frame(angle_deg: f32, period_deg: f32, steps: usize) -> usize {
    if steps == 0 || period_deg <= 0.0 {
        return 0;
    }
    let phase = angle_deg.rem_euclid(period_deg) / period_deg;
    (phase * steps as f32).round() as usize % steps
}

/// The filmstrip frame for normalized `value`, of `frames`.
pub fn filmstrip_frame(value: f32, frames: usize) -> usize {
    let last = frames.saturating_sub(1);
    ((value.clamp(0.0, 1.0) * last as f32).round() as usize).min(last)
}

/// Rotation for a knob top at normalized `value`, about `centre`: the
/// top's drawn orientation is the middle of a `sweep_deg` range, and
/// positive angles turn clockwise.
pub fn knob_rotation(centre: Point, value: f32, sweep_deg: f32) -> Transform {
    let deg = sweep_deg * (value.clamp(0.0, 1.0) - 0.5);
    Transform::from_rotate_at(deg, centre.x, centre.y)
}

/// A sprite knob. With two frames, frame 0 is a static base and frame 1 a
/// top rotated by the value over the skin's sweep. With more, the frames
/// are a filmstrip, one per angle with the lighting baked in: the widget
/// shows the one nearest the value.
#[derive(Debug)]
pub struct SpriteKnob {
    frames: SharedSkin,
    sweep_deg: f32,
    cycle_deg: f32,
}

impl SpriteKnob {
    pub fn new(frames: SharedSkin) -> Self {
        let skin = frames.borrow().skin();
        Self {
            frames,
            sweep_deg: skin.sweep_deg(),
            cycle_deg: skin.cycle_deg(),
        }
    }
}

impl Widget for SpriteKnob {
    fn draw(
        &self,
        canvas: &mut PixmapMut<'_>,
        rect: Rect,
        transform: Transform,
        state: &WidgetState,
    ) {
        let scale = transform.get_scale().0;
        let mut frames = self.frames.borrow_mut();
        if self.cycle_deg > 0.0 {
            let angle = self.sweep_deg * (state.value.clamp(0.0, 1.0) - 0.5);
            let body = 1 + cycle_frame(angle, self.cycle_deg, frames.len() - 1);
            draw_frame(canvas, frames.frame(body, scale), rect, transform, scale);
            let turn = knob_rotation(rect.center(), state.value, self.sweep_deg);
            draw_frame(
                canvas,
                frames.frame(0, scale),
                rect,
                transform.pre_concat(turn),
                scale,
            );
            return;
        }
        if frames.len() > 2 {
            let i = filmstrip_frame(state.value, frames.len());
            draw_frame(canvas, frames.frame(i, scale), rect, transform, scale);
            return;
        }
        draw_frame(canvas, frames.frame(0, scale), rect, transform, scale);
        let turn = knob_rotation(rect.center(), state.value, self.sweep_deg);
        draw_frame(
            canvas,
            frames.frame(1, scale),
            rect,
            transform.pre_concat(turn),
            scale,
        );
    }
}

/// A sprite switch: frame 0 when off, frame 1 when on (value at or above
/// ½). For toggles, latching buttons and radio buttons. A three-way
/// toggle's skin has three frames, down, middle and up, one per position.
#[derive(Debug)]
pub struct SpriteSwitch {
    frames: SharedSkin,
}

impl SpriteSwitch {
    pub fn new(frames: SharedSkin) -> Self {
        Self { frames }
    }
}

impl Widget for SpriteSwitch {
    fn draw(
        &self,
        canvas: &mut PixmapMut<'_>,
        rect: Rect,
        transform: Transform,
        state: &WidgetState,
    ) {
        let scale = transform.get_scale().0;
        let mut frames = self.frames.borrow_mut();
        let last = frames.len().saturating_sub(1);
        let index = if last > 1 {
            (state.value.clamp(0.0, 1.0) * last as f32).round() as usize
        } else {
            usize::from(state.value >= 0.5)
        };
        let frame = frames.frame(index, scale);
        draw_frame(canvas, frame, rect, transform, scale);
    }
}

/// A tab from its skin ([design/faceplates.md]): two frames (unselected, selected),
/// or four (unselected and selected, each with its lamp off then lit;
/// the lamp is lit while `value` ≥ ½).
///
/// [design/faceplates.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/faceplates.md
#[derive(Debug)]
pub struct SpriteTab {
    frames: SharedSkin,
}

impl SpriteTab {
    pub fn new(frames: SharedSkin) -> Self {
        Self { frames }
    }
}

impl Widget for SpriteTab {
    fn draw(
        &self,
        canvas: &mut PixmapMut<'_>,
        rect: Rect,
        transform: Transform,
        state: &WidgetState,
    ) {
        let scale = transform.get_scale().0;
        let mut frames = self.frames.borrow_mut();
        let selected = usize::from(state.selected);
        let index = if frames.len() >= 4 {
            2 * selected + usize::from(state.value >= 0.5)
        } else {
            selected
        };
        let frame = frames.frame(index, scale);
        draw_frame(canvas, frame, rect, transform, scale);
    }
}

/// An action's push area: nothing at rest (the faceplate's art is the
/// button), a light wash on hover and a stronger one while pressed.
#[derive(Clone, Copy, Debug)]
pub struct Push {
    pub hover: Color,
    pub active: Color,
}

impl Default for Push {
    fn default() -> Self {
        Self {
            hover: Color::from_rgba8(255, 255, 255, 22),
            active: Color::from_rgba8(255, 255, 255, 48),
        }
    }
}

impl Widget for Push {
    fn draw(
        &self,
        canvas: &mut PixmapMut<'_>,
        rect: Rect,
        transform: Transform,
        state: &WidgetState,
    ) {
        let wash = if state.active {
            self.active
        } else if state.hover {
            self.hover
        } else {
            return;
        };
        if let Some(path) = rounded_rect(rect, rect.h.min(rect.w) * 0.2) {
            let mut paint = Paint {
                anti_alias: true,
                ..Paint::default()
            };
            paint.set_color(wash);
            canvas.fill_path(&path, &paint, FillRule::Winding, transform, None);
        }
    }
}

/// A vector tab: a raised body when selected, a lamp at its right lit
/// while `value` ≥ ½.
#[derive(Clone, Debug)]
pub struct Tab {
    pub body: Color,
    pub selected: Color,
    pub border: Color,
    pub lamp_off: Color,
    pub lamp_on: Color,
}

impl Default for Tab {
    fn default() -> Self {
        Self {
            body: Color::from_rgba8(24, 24, 27, 255),
            selected: Color::from_rgba8(58, 58, 64, 255),
            border: Color::from_rgba8(220, 220, 220, 90),
            lamp_off: Lamp::default().off,
            lamp_on: Lamp::default().on,
        }
    }
}

impl Widget for Tab {
    fn draw(
        &self,
        canvas: &mut PixmapMut<'_>,
        rect: Rect,
        transform: Transform,
        state: &WidgetState,
    ) {
        let mut paint = Paint {
            anti_alias: true,
            ..Paint::default()
        };
        if let Some(path) = rounded_rect(rect, rect.h * 0.2) {
            paint.set_color(if state.selected {
                self.selected
            } else {
                self.body
            });
            canvas.fill_path(&path, &paint, FillRule::Winding, transform, None);
            paint.set_color(self.border);
            let stroke = Stroke {
                width: 1.0,
                ..Stroke::default()
            };
            canvas.stroke_path(&path, &paint, &stroke, transform, None);
        }
        let d = rect.h * 0.3;
        let lamp = Rect::new(rect.right() - d * 1.8, rect.center().y - d / 2.0, d, d);
        if let Some(path) = rounded_rect(lamp, d / 2.0) {
            paint.set_color(if state.value >= 0.5 {
                self.lamp_on
            } else {
                self.lamp_off
            });
            canvas.fill_path(&path, &paint, FillRule::Winding, transform, None);
        }
    }
}

/// A vector bat-handle toggle: a lever pointing up (on) or down (off).
/// With three steps the middle position is the lever upright, seen
/// end-on: a dot over the pivot.
#[derive(Clone, Debug)]
pub struct Bat {
    pub ring: Color,
    pub lever: Color,
    pub accent: Color,
}

impl Default for Bat {
    fn default() -> Self {
        let knob = Knob::default();
        Self {
            ring: knob.track,
            lever: knob.pointer,
            accent: knob.accent,
        }
    }
}

impl Widget for Bat {
    fn draw(
        &self,
        canvas: &mut PixmapMut<'_>,
        rect: Rect,
        transform: Transform,
        state: &WidgetState,
    ) {
        let c = rect.center();
        let half = rect.w.min(rect.h) * 0.5;
        let mut paint = Paint {
            anti_alias: true,
            ..Paint::default()
        };
        if let Some(ring) = PathBuilder::from_circle(c.x, c.y, half * 0.45) {
            paint.set_color(self.ring);
            canvas.fill_path(&ring, &paint, FillRule::Winding, transform, None);
        }
        paint.set_color(if state.active {
            self.accent
        } else {
            self.lever
        });
        let width = half * 0.22;
        if state.steps == 3 && (state.value - 0.5).abs() < 0.25 {
            if let Some(end) = PathBuilder::from_circle(c.x, c.y, width * 0.8) {
                canvas.fill_path(&end, &paint, FillRule::Winding, transform, None);
            }
            return;
        }
        let on = state.value >= 0.5;
        let tip_y = if on {
            c.y - half * 0.85
        } else {
            c.y + half * 0.85
        };
        let mut pb = PathBuilder::new();
        pb.move_to(c.x, c.y);
        pb.line_to(c.x, tip_y);
        if let Some(lever) = pb.finish() {
            let stroke = Stroke {
                width,
                line_cap: LineCap::Round,
                ..Stroke::default()
            };
            canvas.stroke_path(&lever, &paint, &stroke, transform, None);
        }
    }
}

/// A vector LED button: lit (accent) when on, dark when off.
#[derive(Clone, Debug)]
pub struct Lamp {
    pub off: Color,
    pub off_hover: Color,
    pub on: Color,
}

impl Default for Lamp {
    fn default() -> Self {
        Self {
            off: Color::from_rgba8(70, 70, 76, 255),
            off_hover: Color::from_rgba8(88, 88, 96, 255),
            on: Knob::default().accent,
        }
    }
}

impl Widget for Lamp {
    fn draw(
        &self,
        canvas: &mut PixmapMut<'_>,
        rect: Rect,
        transform: Transform,
        state: &WidgetState,
    ) {
        let inset = rect.w.min(rect.h) * 0.12;
        let r = Rect::new(
            rect.x + inset,
            rect.y + inset,
            rect.w - 2.0 * inset,
            rect.h - 2.0 * inset,
        );
        let color = if state.value >= 0.5 {
            self.on
        } else if state.hover || state.active {
            self.off_hover
        } else {
            self.off
        };
        let mut paint = Paint {
            anti_alias: true,
            ..Paint::default()
        };
        paint.set_color(color);
        if let Some(path) = rounded_rect(r, r.w.min(r.h) * 0.2) {
            canvas.fill_path(&path, &paint, FillRule::Winding, transform, None);
        }
    }
}

/// A vertical fader's moving parts within its control rect. The widget
/// draws with it and the binding hit-tests with it, so they agree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaderGeometry {
    rect: Rect,
    cap_h: f32,
}

impl FaderGeometry {
    pub fn new(rect: Rect) -> Self {
        Self {
            rect,
            cap_h: (rect.w * 0.6).min(rect.h * 0.25),
        }
    }

    /// Distance the cap's centre travels from bottom (0) to top (1).
    pub fn travel(&self) -> f32 {
        (self.rect.h - self.cap_h).max(1.0)
    }

    /// The cap at normalized `value`.
    pub fn cap(&self, value: f32) -> Rect {
        let top = self.rect.y + (1.0 - value.clamp(0.0, 1.0)) * self.travel();
        Rect::new(self.rect.x, top, self.rect.w, self.cap_h)
    }

    /// The value that centres the cap on `y`, clamped.
    pub fn value_at(&self, y: f32) -> f32 {
        let top = self.rect.y + self.cap_h * 0.5;
        (1.0 - (y - top) / self.travel()).clamp(0.0, 1.0)
    }
}

/// A vector fader: a slot, a value fill rising from the bottom, and a cap.
#[derive(Clone, Debug)]
pub struct Fader {
    pub track: Color,
    pub accent: Color,
    pub body: Color,
    pub body_hover: Color,
    pub line: Color,
}

impl Default for Fader {
    fn default() -> Self {
        let knob = Knob::default();
        Self {
            track: knob.track,
            accent: knob.accent,
            body: Color::from_rgba8(58, 58, 64, 255),
            body_hover: Color::from_rgba8(74, 74, 82, 255),
            line: knob.pointer,
        }
    }
}

/// Fill `r` with rounded corners.
fn fill_rounded(canvas: &mut PixmapMut<'_>, color: Color, r: Rect, transform: Transform) {
    let paint = Paint {
        shader: tiny_skia::Shader::SolidColor(color),
        anti_alias: true,
        ..Paint::default()
    };
    if let Some(path) = rounded_rect(r, (r.w.min(r.h) * 0.3).min(4.0)) {
        canvas.fill_path(&path, &paint, FillRule::Winding, transform, None);
    }
}

impl Fader {
    /// The slot, and the value fill rising from the bottom to `value`.
    fn draw_slot(&self, canvas: &mut PixmapMut<'_>, rect: Rect, transform: Transform, value: f32) {
        let g = FaderGeometry::new(rect);
        let cx = rect.center().x;
        let slot_w = (rect.w * 0.18).max(1.0);
        let slot_top = rect.y + g.cap_h * 0.5;
        let slot_bottom = rect.bottom() - g.cap_h * 0.5;
        fill_rounded(
            canvas,
            self.track,
            Rect::new(cx - slot_w / 2.0, slot_top, slot_w, slot_bottom - slot_top),
            transform,
        );
        let level = g.cap(value).center().y;
        if level < slot_bottom {
            fill_rounded(
                canvas,
                self.accent,
                Rect::new(cx - slot_w / 2.0, level, slot_w, slot_bottom - level),
                transform,
            );
        }
    }

    /// The vector cap: a body with a centre line.
    fn draw_cap(
        &self,
        canvas: &mut PixmapMut<'_>,
        cap: Rect,
        transform: Transform,
        state: &WidgetState,
    ) {
        let body = if state.hover || state.active {
            self.body_hover
        } else {
            self.body
        };
        fill_rounded(canvas, body, cap, transform);
        let line_h = (cap.h * 0.12).max(1.0);
        fill_rounded(
            canvas,
            if state.active { self.accent } else { self.line },
            Rect::new(
                cap.x + cap.w * 0.15,
                cap.center().y - line_h / 2.0,
                cap.w * 0.7,
                line_h,
            ),
            transform,
        );
    }
}

impl Widget for Fader {
    fn draw(
        &self,
        canvas: &mut PixmapMut<'_>,
        rect: Rect,
        transform: Transform,
        state: &WidgetState,
    ) {
        self.draw_slot(canvas, rect, transform, state.value);
        self.draw_cap(
            canvas,
            FaderGeometry::new(rect).cap(state.value),
            transform,
            state,
        );
    }
}

/// A vector selector: a slot with a detent mark per position, and a cap
/// that sits on the selected one. Labels belong to the faceplate art.
#[derive(Clone, Debug)]
pub struct Selector {
    pub fader: Fader,
    pub detent: Color,
}

impl Default for Selector {
    fn default() -> Self {
        Self {
            fader: Fader::default(),
            detent: Color::from_rgba8(150, 150, 158, 255),
        }
    }
}

impl Selector {
    /// The slot, with a detent mark either side of it at each of `steps`
    /// positions.
    fn draw_slot(&self, canvas: &mut PixmapMut<'_>, rect: Rect, transform: Transform, steps: u16) {
        let g = FaderGeometry::new(rect);
        let cx = rect.center().x;
        let slot_w = (rect.w * 0.18).max(1.0);
        let top = g.cap(1.0).center().y;
        let bottom = g.cap(0.0).center().y;
        fill_rounded(
            canvas,
            self.fader.track,
            Rect::new(cx - slot_w / 2.0, top, slot_w, bottom - top),
            transform,
        );
        let mark_h = (rect.h * 0.012).max(1.0);
        let n = steps.max(2);
        for i in 0..n {
            let y = g.cap(f32::from(i) / f32::from(n - 1)).center().y - mark_h / 2.0;
            for x in [rect.x, cx + slot_w] {
                fill_rounded(
                    canvas,
                    self.detent,
                    Rect::new(x, y, cx - slot_w - rect.x, mark_h),
                    transform,
                );
            }
        }
    }
}

impl Widget for Selector {
    fn draw(
        &self,
        canvas: &mut PixmapMut<'_>,
        rect: Rect,
        transform: Transform,
        state: &WidgetState,
    ) {
        self.draw_slot(canvas, rect, transform, state.steps);
        let cap = FaderGeometry::new(rect).cap(state.value);
        self.fader.draw_cap(canvas, cap, transform, state);
    }
}

/// A sprite fader or selector. Its last frame is the cap, rendered at the
/// cap's size ([`FaderGeometry::cap`]) and drawn where the value puts it.
/// Before it, at the control's size:
///
/// - nothing: the slot, value fill and detents stay vector;
/// - a fader's empty track, then its filled track, shown from the cap's
///   centre down;
/// - a selector's track, detents included.
///
/// A fader may instead end with two caps, lit then dark (four frames). A
/// dimmed one is then drawn so, by the widget: its empty track and the
/// dark cap, at full opacity ([`Widget::draws_dimmed`]).
#[derive(Debug)]
pub struct SpriteFader {
    frames: SharedSkin,
    selector: Selector,
    detents: bool,
}

impl SpriteFader {
    /// A fader: slot and value fill under the sprite cap.
    pub fn fader(frames: SharedSkin) -> Self {
        Self::new(frames, false)
    }

    /// A selector: slot and detent marks under the sprite cap.
    pub fn selector(frames: SharedSkin) -> Self {
        Self::new(frames, true)
    }

    fn new(frames: SharedSkin, detents: bool) -> Self {
        Self {
            frames,
            selector: Selector::default(),
            detents,
        }
    }
}

impl Widget for SpriteFader {
    fn draw(
        &self,
        canvas: &mut PixmapMut<'_>,
        rect: Rect,
        transform: Transform,
        state: &WidgetState,
    ) {
        let scale = transform.get_scale().0;
        let mut frames = self.frames.borrow_mut();
        let cap = FaderGeometry::new(rect).cap(state.value);
        let n = frames.len();
        let dark = n == 4 && state.dimmed;
        match n {
            1 if self.detents => self
                .selector
                .draw_slot(canvas, rect, transform, state.steps),
            1 => self
                .selector
                .fader
                .draw_slot(canvas, rect, transform, state.value),
            _ => {
                draw_frame(canvas, frames.frame(0, scale), rect, transform, scale);
                if n >= 3 && !dark {
                    let lit = Rect::new(
                        rect.x,
                        cap.center().y,
                        rect.w,
                        rect.bottom() - cap.center().y,
                    );
                    draw_frame_part(canvas, frames.frame(1, scale), rect, lit, transform, scale);
                }
            }
        }
        let cap_frame = if n == 4 && !dark { 2 } else { n - 1 };
        draw_frame(
            canvas,
            frames.frame(cap_frame, scale),
            cap,
            transform,
            scale,
        );
    }

    fn draws_dimmed(&self) -> bool {
        !self.detents && self.frames.borrow().len() == 4
    }
}

/// A drop-down's field: a box with a chevron at its right. Its label is
/// runtime text, which this crate doesn't draw: `silkscreen`'s editor
/// draws it over the field.
#[derive(Clone, Debug)]
pub struct Field {
    pub body: Color,
    pub body_hover: Color,
    pub border: Color,
    pub chevron: Color,
}

impl Default for Field {
    fn default() -> Self {
        Self {
            body: Color::from_rgba8(24, 24, 27, 255),
            body_hover: Color::from_rgba8(36, 36, 40, 255),
            border: Color::from_rgba8(220, 220, 220, 90),
            chevron: Color::from_rgba8(237, 237, 237, 255),
        }
    }
}

impl Field {
    /// Width of the chevron's area at the field's right, in logical units.
    pub fn chevron_w(rect: Rect) -> f32 {
        rect.h.min(16.0)
    }
}

impl Widget for Field {
    fn draw(
        &self,
        canvas: &mut PixmapMut<'_>,
        rect: Rect,
        transform: Transform,
        state: &WidgetState,
    ) {
        let inset = 0.5 / transform.sx;
        let outline = Rect::new(
            rect.x + inset,
            rect.y + inset,
            rect.w - 2.0 * inset,
            rect.h - 2.0 * inset,
        );
        let mut paint = Paint {
            anti_alias: true,
            ..Paint::default()
        };
        if let Some(path) = rounded_rect(outline, 3.0) {
            paint.set_color(if state.hover || state.active {
                self.body_hover
            } else {
                self.body
            });
            canvas.fill_path(&path, &paint, FillRule::Winding, transform, None);
            paint.set_color(self.border);
            let stroke = Stroke {
                width: 1.0 / transform.sx,
                ..Stroke::default()
            };
            canvas.stroke_path(&path, &paint, &stroke, transform, None);
        }
        // A downward chevron, centred in the right-hand square.
        let w = Field::chevron_w(rect);
        let cx = rect.right() - w / 2.0;
        let cy = rect.center().y;
        let half = (w * 0.18).max(2.0);
        let mut pb = PathBuilder::new();
        pb.move_to(cx - half, cy - half / 2.0);
        pb.line_to(cx, cy + half / 2.0);
        pb.line_to(cx + half, cy - half / 2.0);
        if let Some(path) = pb.finish() {
            paint.set_color(self.chevron);
            let stroke = Stroke {
                width: 1.4,
                line_cap: LineCap::Round,
                ..Stroke::default()
            };
            canvas.stroke_path(&path, &paint, &stroke, transform, None);
        }
    }
}

/// `rect` with corners of radius `r` (held to half the shorter side, and
/// to 0): the one rounded-rect path faceplate widgets and overlays share.
pub fn rounded_rect(rect: Rect, r: f32) -> Option<tiny_skia::Path> {
    let r = r.min(rect.w / 2.0).min(rect.h / 2.0).max(0.0);
    let (x, y, right, bottom) = (rect.x, rect.y, rect.right(), rect.bottom());
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(right - r, y);
    pb.quad_to(right, y, right, y + r);
    pb.line_to(right, bottom - r);
    pb.quad_to(right, bottom, right - r, bottom);
    pb.line_to(x + r, bottom);
    pb.quad_to(x, bottom, x, bottom - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

/// A vector knob: a 270° track, a value arc, a body and a pointer.
#[derive(Clone, Debug)]
pub struct Knob {
    pub track: Color,
    pub accent: Color,
    pub body: Color,
    pub body_hover: Color,
    pub pointer: Color,
}

impl Default for Knob {
    fn default() -> Self {
        Self {
            track: Color::from_rgba8(70, 70, 74, 255),
            accent: Color::from_rgba8(255, 154, 46, 255),
            body: Color::from_rgba8(38, 38, 42, 255),
            body_hover: Color::from_rgba8(52, 52, 58, 255),
            pointer: Color::from_rgba8(236, 236, 236, 255),
        }
    }
}

/// Sweep of the knob's travel, in degrees, centred on 12 o'clock.
const SWEEP_DEG: f32 = 270.0;

impl Knob {
    /// Point at `radius` from `c`, `deg` clockwise from 12 o'clock.
    fn polar(c: Point, radius: f32, deg: f32) -> (f32, f32) {
        let (s, co) = deg.to_radians().sin_cos();
        (c.x + radius * s, c.y - radius * co)
    }

    /// Polyline arc from `from` to `to` degrees. `scale` sets the segment
    /// count so curves stay smooth at any size.
    fn arc(c: Point, radius: f32, from: f32, to: f32, scale: f32) -> Option<tiny_skia::Path> {
        let length_px = radius * scale * (to - from).abs().to_radians();
        let segments = (length_px / 3.0).ceil().clamp(2.0, 512.0) as usize;
        let mut pb = PathBuilder::new();
        let (x, y) = Self::polar(c, radius, from);
        pb.move_to(x, y);
        for i in 1..=segments {
            let deg = from + (to - from) * (i as f32 / segments as f32);
            let (x, y) = Self::polar(c, radius, deg);
            pb.line_to(x, y);
        }
        pb.finish()
    }
}

impl Widget for Knob {
    fn draw(
        &self,
        canvas: &mut PixmapMut<'_>,
        rect: Rect,
        transform: Transform,
        state: &WidgetState,
    ) {
        let c = rect.center();
        let half = rect.w.min(rect.h) * 0.5;
        let ring = half * 0.88;
        let width = half * 0.12;
        let scale = transform.get_scale().0;
        let start = -SWEEP_DEG / 2.0;
        let value_deg = start + SWEEP_DEG * state.value.clamp(0.0, 1.0);
        let origin_deg = start + SWEEP_DEG * state.origin.clamp(0.0, 1.0);

        let mut paint = Paint {
            anti_alias: true,
            ..Paint::default()
        };
        let stroke = Stroke {
            width,
            line_cap: LineCap::Round,
            ..Stroke::default()
        };

        if let Some(track) = Self::arc(c, ring, start, -start, scale) {
            paint.set_color(self.track);
            canvas.stroke_path(&track, &paint, &stroke, transform, None);
        }
        let (from, to) = if value_deg < origin_deg {
            (value_deg, origin_deg)
        } else {
            (origin_deg, value_deg)
        };
        if to - from > 0.01
            && let Some(value) = Self::arc(c, ring, from, to, scale)
        {
            paint.set_color(self.accent);
            canvas.stroke_path(&value, &paint, &stroke, transform, None);
        }

        if let Some(body) = PathBuilder::from_circle(c.x, c.y, half * 0.66) {
            paint.set_color(if state.hover || state.active {
                self.body_hover
            } else {
                self.body
            });
            canvas.fill_path(&body, &paint, FillRule::Winding, transform, None);
        }

        let mut pb = PathBuilder::new();
        let (x0, y0) = Self::polar(c, half * 0.18, value_deg);
        let (x1, y1) = Self::polar(c, half * 0.56, value_deg);
        pb.move_to(x0, y0);
        pb.line_to(x1, y1);
        if let Some(pointer) = pb.finish() {
            paint.set_color(if state.active {
                self.accent
            } else {
                self.pointer
            });
            let stroke = Stroke {
                width: half * 0.09,
                line_cap: LineCap::Round,
                ..Stroke::default()
            };
            canvas.stroke_path(&pointer, &paint, &stroke, transform, None);
        }
    }
}

#[cfg(test)]
mod fader_tests {
    use super::*;

    #[test]
    fn cap_and_value_agree() {
        let g = FaderGeometry::new(Rect::new(10.0, 20.0, 20.0, 120.0));
        // Cap height: min(20 * 0.6, 120 * 0.25) = 12; travel 108.
        assert_eq!(g.travel(), 108.0);
        assert_eq!(g.cap(1.0), Rect::new(10.0, 20.0, 20.0, 12.0));
        assert_eq!(g.cap(0.0), Rect::new(10.0, 128.0, 20.0, 12.0));
        for v in [0.0, 0.25, 0.5, 1.0] {
            let y = g.cap(v).center().y;
            assert!((g.value_at(y) - v).abs() < 1e-6, "{v}");
        }
        assert_eq!(g.value_at(0.0), 1.0, "above the track clamps");
        assert_eq!(g.value_at(500.0), 0.0);
    }

    /// A skin of solid frames, each `(width, height, colour)`, at 1×.
    fn solid_skin(frames: &[(f32, f32, Color)]) -> SharedSkin {
        use crate::geom::Size;
        use crate::raster::{EmbeddedPngs, Skin, SkinRaster};
        let pngs: Vec<EmbeddedPngs> = frames
            .iter()
            .map(|&(w, h, color)| {
                let mut p = tiny_skia::Pixmap::new(w as u32, h as u32).unwrap();
                p.fill(color);
                let png: &'static [u8] = Box::leak(p.encode_png().unwrap().into_boxed_slice());
                EmbeddedPngs::new(Size::new(w, h), Box::leak(Box::new([(1.0, png)])))
            })
            .collect();
        let skin: &'static Skin = Box::leak(Box::new(Skin::new("test", pngs.leak())));
        std::rc::Rc::new(std::cell::RefCell::new(SkinRaster::new(skin)))
    }

    fn rgb_at(fader: &SpriteFader, state: &WidgetState, x: u32, y: u32) -> (u8, u8, u8) {
        let mut p = tiny_skia::Pixmap::new(20, 100).unwrap();
        let rect = Rect::new(0.0, 0.0, 20.0, 100.0);
        fader.draw(&mut p.as_mut(), rect, Transform::identity(), state);
        let c = p.pixel(x, y).unwrap();
        (c.red(), c.green(), c.blue())
    }

    #[test]
    fn a_dimmed_four_frame_fader_shows_its_dark_cap_and_no_strip() {
        let (track, strip) = (
            Color::from_rgba8(40, 40, 40, 255),
            Color::from_rgba8(0, 255, 0, 255),
        );
        let (lit, dark) = (
            Color::from_rgba8(0, 0, 255, 255),
            Color::from_rgba8(255, 0, 0, 255),
        );
        let fader = SpriteFader::fader(solid_skin(&[
            (20.0, 100.0, track),
            (20.0, 100.0, strip),
            (20.0, 12.0, lit),
            (20.0, 12.0, dark),
        ]));
        assert!(fader.draws_dimmed());
        // At ½ the cap spans y 44..56; the strip shows below its centre.
        let mut state = WidgetState::value(0.5);
        assert_eq!(rgb_at(&fader, &state, 10, 50), (0, 0, 255), "lit cap");
        assert_eq!(rgb_at(&fader, &state, 10, 90), (0, 255, 0), "lit strip");
        state.dimmed = true;
        assert_eq!(rgb_at(&fader, &state, 10, 50), (255, 0, 0), "dark cap");
        assert_eq!(rgb_at(&fader, &state, 10, 90), (40, 40, 40), "no strip");
    }

    #[test]
    fn a_three_frame_fader_leaves_dimming_to_the_compositor() {
        let grey = Color::from_rgba8(40, 40, 40, 255);
        let fader = SpriteFader::fader(solid_skin(&[
            (20.0, 100.0, grey),
            (20.0, 100.0, grey),
            (20.0, 12.0, grey),
        ]));
        assert!(!fader.draws_dimmed());
    }
}

#[cfg(test)]
mod switch_tests {
    use super::*;
    use tiny_skia::Pixmap;

    fn draw(widget: &dyn Widget, value: f32) -> Pixmap {
        let mut p = Pixmap::new(40, 40).unwrap();
        widget.draw(
            &mut p.as_mut(),
            Rect::new(0.0, 0.0, 40.0, 40.0),
            Transform::identity(),
            &WidgetState::stepped(value, 2),
        );
        p
    }

    #[test]
    fn the_lamp_lights_when_on() {
        let lamp = Lamp::default();
        let off = draw(&lamp, 0.0).pixel(20, 20).unwrap();
        let on = draw(&lamp, 1.0).pixel(20, 20).unwrap();
        assert_eq!((off.red(), off.green()), (70, 70));
        assert_eq!((on.red(), on.green()), (255, 154));
    }

    #[test]
    fn the_bat_lever_points_up_when_on() {
        let bat = Bat::default();
        let lever = |p: &Pixmap, y| p.pixel(20, y).unwrap().red() > 200;
        let off = draw(&bat, 0.0);
        let on = draw(&bat, 1.0);
        assert!(lever(&on, 5) && !lever(&on, 35));
        assert!(lever(&off, 35) && !lever(&off, 5));
    }
}

#[cfg(test)]
mod knob_tests {
    use super::*;
    use tiny_skia::Pixmap;

    fn turned(value: f32) -> (f32, f32) {
        // Where the top's 12 o'clock mark, 10 units above the centre, lands.
        let mut p = [tiny_skia::Point::from_xy(0.0, -10.0)];
        knob_rotation(Point::new(0.0, 0.0), value, 270.0).map_points(&mut p);
        (p[0].x, p[0].y)
    }

    #[test]
    fn cycle_frames_wrap_with_the_period() {
        // 6° period, 8 frames: 0.75° per frame.
        assert_eq!(cycle_frame(0.0, 6.0, 8), 0);
        assert_eq!(cycle_frame(0.75, 6.0, 8), 1);
        assert_eq!(cycle_frame(6.0, 6.0, 8), 0, "a full period looks the same");
        assert_eq!(cycle_frame(5.8, 6.0, 8), 0, "nearest wraps round");
        assert_eq!(cycle_frame(-0.75, 6.0, 8), 7, "anticlockwise of 12");
        assert_eq!(cycle_frame(-135.0, 6.0, 8), 4, "-135 ≡ 3 (mod 6)");
        assert_eq!(cycle_frame(1.0, 0.0, 8), 0);
    }

    #[test]
    fn filmstrip_frames_are_nearest_to_the_value() {
        assert_eq!(filmstrip_frame(0.0, 128), 0);
        assert_eq!(filmstrip_frame(1.0, 128), 127);
        assert_eq!(filmstrip_frame(0.5, 128), 64, "63.5 rounds up");
        assert_eq!(filmstrip_frame(0.25, 5), 1);
        assert_eq!(filmstrip_frame(0.4, 5), 2);
        assert_eq!(filmstrip_frame(-1.0, 5), 0);
        assert_eq!(filmstrip_frame(2.0, 5), 4);
    }

    #[test]
    fn rotation_centres_on_the_drawn_orientation() {
        let near =
            |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4;
        assert!(near(turned(0.5), (0.0, -10.0)), "middle: as drawn");
        let d = 10.0 * std::f32::consts::FRAC_1_SQRT_2;
        assert!(
            near(turned(1.0), (d, d)),
            "max: 135° clockwise, {:?}",
            turned(1.0)
        );
        assert!(
            near(turned(0.0), (-d, d)),
            "min: 135° anticlockwise, {:?}",
            turned(0.0)
        );
    }

    fn accent_pixels(origin: f32, value: f32) -> (usize, usize) {
        let mut p = Pixmap::new(100, 100).unwrap();
        Knob::default().draw(
            &mut p.as_mut(),
            Rect::new(0.0, 0.0, 100.0, 100.0),
            Transform::identity(),
            &WidgetState::from_origin(value, origin),
        );
        let accent = |x: u32, y: u32| {
            let c = p.pixel(x, y).unwrap();
            c.red() > 200 && c.green() > 100 && c.green() < 200
        };
        let side = |xs: std::ops::Range<u32>| {
            xs.flat_map(|x| (0..100).map(move |y| (x, y)))
                .filter(|&(x, y)| accent(x, y))
                .count()
        };
        // Halves clear of the 12 o'clock line: the arc's round cap pokes
        // a few pixels past it.
        (side(0..45), side(55..100))
    }

    #[test]
    fn a_bipolar_arc_grows_from_the_centre() {
        assert_eq!(accent_pixels(0.5, 0.5), (0, 0), "nothing at the centre");
        let (left, right) = accent_pixels(0.5, 0.8);
        assert!(left == 0 && right > 0, "{left} {right}");
        let (left, right) = accent_pixels(0.5, 0.2);
        assert!(left > 0 && right == 0, "{left} {right}");
        let (left, right) = accent_pixels(0.0, 0.8);
        assert!(left > 0 && right > 0, "unipolar sweeps from the start");
    }
}
