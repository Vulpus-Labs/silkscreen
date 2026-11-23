//! The compositor: faceplate + widgets → frame buffer, by damage
//! ([design/rendering.md]).
//!
//! For each damaged rect it copies the faceplate pixels under it into a
//! scratch pixmap, draws every widget intersecting it (offset into the
//! scratch), and copies the result back. Drawing is therefore clipped to the
//! damage without full-frame masks, and overlapping controls compose
//! correctly.
//!
//! Page groups ([design/faceplates.md]) restore their region from the shown page's
//! faceplate instead, and only controls off pages or on shown pages draw.
//!
//! [design/faceplates.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/faceplates.md
//!
//! [design/rendering.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/rendering.md

use tiny_skia::{Pixmap, PixmapMut, Transform};

use crate::geom::{IRect, Rect};
use crate::layout::{ControlId, ControlSpec, Layout};
use crate::raster::{EmbeddedRaster, RasterSource, SkinCache};
use crate::widget::{Widget, WidgetState, widget_for};

/// Something drawn above the controls ([design/overlays-and-text.md]): a tooltip, a value
/// readout, later a menu or a panel. Overlays paint after every widget, in
/// order, and take part in damage like widgets do.
///
/// [design/overlays-and-text.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/overlays-and-text.md
pub trait Overlay {
    /// Logical bounds. Everything the overlay draws must stay inside, so
    /// that showing, changing or hiding it repaints exactly what it covers.
    fn rect(&self) -> Rect;

    /// Paint, through `transform` (logical → canvas pixels), like
    /// [`Widget::draw`].
    fn draw(&self, canvas: &mut PixmapMut<'_>, transform: Transform);

    /// A logical rect the overlay paints over completely, if any (a
    /// panel's body inside its rounded corners). Controls wholly behind
    /// it aren't repainted while it shows: nothing of them would show.
    fn opaque(&self) -> Option<Rect> {
        None
    }
}

/// A page cropped to its group's region, and the asset generation it
/// was made under if it stands in for a page not yet fetched.
#[derive(Clone)]
struct Crop {
    pixels: Pixmap,
    stand_in: Option<u64>,
}

pub struct Compositor {
    layout: &'static Layout,
    widgets: Vec<Box<dyn Widget>>,
    scale: f32,
    frame: (u32, u32),
    /// Damage accumulated since the last render.
    pending: Vec<IRect>,
    /// Damage painted by the last render.
    painted: Vec<IRect>,
    /// RGBA8 bytes for the scratch pixmap, kept at the largest damage
    /// rect yet, so each rect reuses it.
    scratch: Vec<u8>,
    /// What lay under a dimmed control before its widget drew, kept at
    /// the largest yet like `scratch`.
    under: Vec<u8>,
    /// Diagnostic label drawn over the bottom-left corner.
    badge: Option<String>,
    /// Drawn above everything, in order.
    overlays: Vec<Box<dyn Overlay>>,
    /// Drawn above the overlays, and replaced on its own: a tooltip.
    top: Option<Box<dyn Overlay>>,
    /// Each page group's shown page.
    shown: Vec<u16>,
    /// Each page group's region on the frame at the current scale.
    regions: Vec<Option<IRect>>,
    /// Each group's pages, cropped to its region at the current scale,
    /// as they are first shown: a switch back to a page doesn't decode
    /// it again.
    page_crops: Vec<Vec<Option<Crop>>>,
}

impl std::fmt::Debug for Compositor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Compositor")
            .field("scale", &self.scale)
            .field("frame", &self.frame)
            .field("pending", &self.pending)
            .finish_non_exhaustive()
    }
}

impl Compositor {
    /// A compositor using each control's built-in widget (from its skin,
    /// or its kind's vector widget), at scale 1.
    pub fn new(layout: &'static Layout) -> Self {
        let mut skins = SkinCache::default();
        let widgets = layout
            .controls
            .iter()
            .map(|spec| widget_for(spec, &mut skins))
            .collect();
        Self::with_widgets(layout, widgets)
    }

    /// `widgets[i]` draws `ControlId(i)`.
    pub fn with_widgets(layout: &'static Layout, widgets: Vec<Box<dyn Widget>>) -> Self {
        assert_eq!(
            widgets.len(),
            layout.controls.len(),
            "one widget per control is required"
        );
        let mut c = Self {
            layout,
            widgets,
            scale: 1.0,
            frame: layout.size.to_physical(1.0),
            pending: Vec::new(),
            painted: Vec::new(),
            scratch: Vec::new(),
            under: Vec::new(),
            badge: None,
            overlays: Vec::new(),
            top: None,
            shown: vec![0; layout.pages.len()],
            regions: Vec::new(),
            page_crops: layout
                .pages
                .iter()
                .map(|g| vec![None; g.pages.len()])
                .collect(),
        };
        c.set_regions();
        c.damage_all();
        c
    }

    /// Work out each page group's region at the current scale.
    fn set_regions(&mut self) {
        let frame = IRect::frame(self.frame.0, self.frame.1);
        self.regions.clear();
        self.regions.extend(
            self.layout
                .pages
                .iter()
                .map(|g| g.region.to_physical(self.scale).intersect(&frame)),
        );
    }

    /// Each page group's shown page.
    pub fn shown_pages(&self) -> &[u16] {
        &self.shown
    }

    /// Show `page` of page group `group`, damaging the group's region.
    ///
    /// # Panics
    /// If the group or page doesn't exist.
    pub fn show_page(&mut self, group: u16, page: u16) {
        let g = &self.layout.pages[usize::from(group)];
        assert!(
            usize::from(page) < g.pages.len(),
            "page group {} has no page {page}",
            g.name
        );
        if self.shown[usize::from(group)] != page {
            self.shown[usize::from(group)] = page;
            self.damage(g.region.to_physical(self.scale));
        }
    }

    /// Crop each group's shown page to its region, if not already.
    ///
    /// Fetched pages are asked for together: the first crop at a scale
    /// asks for every page of its group, so a tab's first switch needn't
    /// wait on the network.
    fn crop_pages(&mut self) {
        for (i, group) in self.layout.pages.iter().enumerate() {
            let page = usize::from(self.shown[i]);
            if self.page_crops[i][page].is_some() {
                continue;
            }
            for other in group.pages {
                EmbeddedRaster::new(other).want(self.scale);
            }
            let mut raster = EmbeddedRaster::new(&group.pages[page]);
            let r = group
                .region
                .to_physical(self.scale)
                .intersect(&IRect::frame(self.frame.0, self.frame.1))
                .expect("a page group's region is on the faceplate");
            let mut crop = Pixmap::new(r.w, r.h).expect("a page group's region is not empty");
            let full = raster.faceplate(self.scale);
            copy_rect(full.data(), full.width(), r, crop.data_mut(), r.w, 0, 0);
            let stand_in = raster
                .is_stand_in(self.scale)
                .then(crate::assets::generation);
            self.page_crops[i][page] = Some(Crop {
                pixels: crop,
                stand_in,
            });
        }
    }

    /// Drop the page crops that stood in for pages since fetched, and
    /// repaint the shown ones' regions, as rasters remake their own
    /// stand-ins.
    fn refresh_stand_ins(&mut self) {
        let generation = crate::assets::generation();
        for (i, group) in self.layout.pages.iter().enumerate() {
            for (page, slot) in self.page_crops[i].iter_mut().enumerate() {
                if slot
                    .as_ref()
                    .is_some_and(|c| c.stand_in.is_some_and(|g| g != generation))
                {
                    *slot = None;
                    if usize::from(self.shown[i]) == page {
                        let r = group.region.to_physical(self.scale);
                        if let Some(r) = r.intersect(&IRect::frame(self.frame.0, self.frame.1)) {
                            self.pending.push(r);
                        }
                    }
                }
            }
        }
    }

    pub fn layout(&self) -> &'static Layout {
        self.layout
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Physical frame size at the current scale. Targets passed to
    /// [`render`](Self::render) must be exactly this size.
    pub fn frame_size(&self) -> (u32, u32) {
        self.frame
    }

    /// Change scale; everything is damaged.
    pub fn set_scale(&mut self, scale: f32) {
        self.scale = scale;
        self.page_crops.iter_mut().flatten().for_each(|c| *c = None);
        self.frame = self.layout.size.to_physical(scale);
        self.set_regions();
        self.damage_all();
    }

    pub fn damage_all(&mut self) {
        self.pending.clear();
        self.pending.push(IRect::frame(self.frame.0, self.frame.1));
    }

    pub fn damage_control(&mut self, id: ControlId) {
        let r = self.layout.control(id).rect.to_physical(self.scale);
        // Hidden behind an opaque overlay: nothing to show for it.
        let hidden = self.overlays.iter().chain(&self.top).any(|o| {
            o.opaque()
                .and_then(|q| q.to_physical(self.scale).shrunk_inside())
                .is_some_and(|q| q.intersect(&r) == Some(r))
        });
        if !hidden {
            self.damage(r);
        }
    }

    /// Show `overlay` above the others (and below the top one), damaging
    /// only what it covers.
    pub fn push_overlay(&mut self, overlay: Box<dyn Overlay>) {
        let r = overlay.rect().to_physical(self.scale);
        self.overlays.push(overlay);
        self.damage(r);
    }

    /// Hide the uppermost of the overlays, damaging only what it covered.
    pub fn pop_overlay(&mut self) {
        if let Some(o) = self.overlays.pop() {
            self.damage(o.rect().to_physical(self.scale));
        }
    }

    /// Damage an arbitrary physical rect (clipped to the frame).
    pub fn damage(&mut self, rect: IRect) {
        let Some(r) = rect.intersect(&IRect::frame(self.frame.0, self.frame.1)) else {
            return;
        };
        // Skip rects already covered by pending damage.
        if self.pending.iter().any(|p| p.intersect(&r) == Some(r)) {
            return;
        }
        self.pending.retain(|p| r.intersect(p) != Some(*p));
        self.pending.push(r);
    }

    /// Replace the overlays. What the old ones covered and what the new
    /// ones cover is damaged, so set them only when they change.
    pub fn set_overlays(&mut self, overlays: Vec<Box<dyn Overlay>>) {
        let old = std::mem::replace(&mut self.overlays, overlays);
        let rects: Vec<Rect> = old.iter().chain(&self.overlays).map(|o| o.rect()).collect();
        for r in rects {
            self.damage(r.to_physical(self.scale));
        }
    }

    /// The overlays shown, in drawing order.
    pub fn overlays(&self) -> &[Box<dyn Overlay>] {
        &self.overlays
    }

    /// Replace the overlay drawn above all the others (a tooltip),
    /// leaving them be. What the old one covered and the new one covers
    /// is damaged.
    pub fn set_top(&mut self, top: Option<Box<dyn Overlay>>) {
        let old = std::mem::replace(&mut self.top, top);
        let rects: Vec<Rect> = old.iter().chain(&self.top).map(|o| o.rect()).collect();
        for r in rects {
            self.damage(r.to_physical(self.scale));
        }
    }

    /// Show (or clear) a small diagnostic label in the bottom-left corner,
    /// e.g. a build id. Repaints only when the text changes.
    pub fn set_badge(&mut self, text: Option<String>) {
        if text == self.badge {
            return;
        }
        if let Some(r) = self.badge_rect() {
            self.damage(r.to_physical(self.scale));
        }
        self.badge = text;
        if let Some(r) = self.badge_rect() {
            self.damage(r.to_physical(self.scale));
        }
    }

    /// Logical rect of the badge, if one is shown.
    fn badge_rect(&self) -> Option<Rect> {
        let text = self.badge.as_deref()?;
        let w = crate::text::width(text, BADGE_DOT) + 2.0 * BADGE_PAD;
        let h = crate::text::HEIGHT * BADGE_DOT + 2.0 * BADGE_PAD;
        Some(Rect::new(
            BADGE_MARGIN,
            self.layout.size.h - BADGE_MARGIN - h,
            w,
            h,
        ))
    }

    /// Whether the next [`render`](Self::render) draws control `id`: it
    /// is shown and meets pending damage. Callers needn't work out the
    /// state of controls it won't draw.
    pub fn will_draw(&self, id: ControlId) -> bool {
        let spec = self.layout.control(id);
        if !spec.is_shown(&self.shown) {
            return false;
        }
        let r = spec.rect.to_physical(self.scale);
        self.pending.iter().any(|d| d.intersects(&r))
    }

    /// Settle this render's damage: remake stale stand-in page crops and
    /// merge the rects that meet each overlay. [`render`](Self::render)
    /// does this first; callers that build widget states from
    /// [`will_draw`](Self::will_draw) call it before them, so the states
    /// cover exactly what is drawn. Idempotent.
    pub fn prepare(&mut self) {
        self.refresh_stand_ins();
        self.coalesce_over_overlays();
    }

    /// Merge the pending rects that meet each overlay into their union,
    /// so the overlay draws once this render rather than once a rect: a
    /// panel paints whole whatever part is damaged. A union may meet
    /// another overlay, so this repeats until nothing merges. Each merge
    /// leaves fewer rects, so it ends. Allocation-free.
    fn coalesce_over_overlays(&mut self) {
        let frame = IRect::frame(self.frame.0, self.frame.1);
        let count = self.overlays.len() + usize::from(self.top.is_some());
        let mut merged = true;
        while merged && self.pending.len() >= 2 {
            merged = false;
            for k in 0..count {
                let o = match self.overlays.get(k) {
                    Some(o) => o.rect(),
                    None => self.top.as_ref().expect("the top overlay").rect(),
                }
                .to_physical(self.scale);
                let mut over = self.pending.iter().filter(|d| d.intersects(&o));
                let Some(&first) = over.next() else {
                    continue;
                };
                let mut union = first;
                let mut n = 1;
                for d in over {
                    union = union.union(d);
                    n += 1;
                }
                if n < 2 {
                    continue;
                }
                self.pending.retain(|d| !d.intersects(&o));
                if let Some(u) = union.intersect(&frame) {
                    self.damage(u);
                }
                merged = true;
            }
        }
    }

    /// A fingerprint of the pending damage, for checking that none
    /// arrived between two points (callers that build widget states from
    /// [`will_draw`](Self::will_draw) before rendering). Allocation-free.
    pub fn damage_mark(&self) -> (usize, i64) {
        let sum = self.pending.iter().fold(0i64, |a, d| {
            a.wrapping_mul(31).wrapping_add(
                i64::from(d.x)
                    ^ (i64::from(d.y) << 16)
                    ^ (i64::from(d.w) << 32)
                    ^ (i64::from(d.h) << 48),
            )
        });
        (self.pending.len(), sum)
    }

    pub fn has_damage(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Repaint all pending damage into `target` and return the rects
    /// painted. With no damage, touches nothing and returns an empty slice.
    ///
    /// `states[i]` is what `ControlId(i)`'s widget draws.
    pub fn render(
        &mut self,
        target: &mut PixmapMut<'_>,
        raster: &mut dyn RasterSource,
        states: &[WidgetState],
    ) -> &[IRect] {
        assert_eq!(
            (target.width(), target.height()),
            self.frame,
            "render target must match Compositor::frame_size()"
        );
        assert_eq!(states.len(), self.widgets.len(), "one state per control");
        self.painted.clear();
        self.prepare();
        if self.pending.is_empty() {
            return &self.painted;
        }
        self.crop_pages();
        let faceplate = raster.faceplate(self.scale);
        debug_assert_eq!((faceplate.width(), faceplate.height()), self.frame);
        let frame = IRect::frame(self.frame.0, self.frame.1);
        let badge_rect = self.badge_rect();
        // The pending rects become the painted ones, and `pending` keeps
        // the old buffer for the next frame's damage.
        std::mem::swap(&mut self.pending, &mut self.painted);
        for i in 0..self.painted.len() {
            let d = self.painted[i];
            // Drawn with a pixel's margin, then only `d` copied out: a
            // shape that starts just outside a pixmap's edge anti-aliases
            // that edge's pixels short, so a partial repaint would differ
            // from a full one along the rect's sides.
            let s = IRect::new(d.x - 1, d.y - 1, d.w + 2, d.h + 2)
                .intersect(&frame)
                .expect("damage is on the frame");
            let bytes = s.w as usize * s.h as usize * 4;
            if self.scratch.len() < bytes {
                self.scratch.resize(bytes, 0);
            }
            let scratch = &mut self.scratch[..bytes];
            copy_rect(faceplate.data(), faceplate.width(), s, scratch, s.w, 0, 0);
            for ((region, crops), &page) in
                self.regions.iter().zip(&self.page_crops).zip(&self.shown)
            {
                let (Some(region), Some(Crop { pixels: crop, .. })) =
                    (region, &crops[usize::from(page)])
                else {
                    continue;
                };
                let Some(over) = region.intersect(&s) else {
                    continue;
                };
                copy_rect(
                    crop.data(),
                    crop.width(),
                    IRect::new(over.x - region.x, over.y - region.y, over.w, over.h),
                    scratch,
                    s.w,
                    over.x - s.x,
                    over.y - s.y,
                );
            }
            let to_scratch = Transform::from_row(
                self.scale,
                0.0,
                0.0,
                self.scale,
                -(s.x as f32),
                -(s.y as f32),
            );
            {
                WidgetPass {
                    controls: self.layout.controls,
                    widgets: &self.widgets,
                    shown: &self.shown,
                    scale: self.scale,
                    states,
                }
                .draw(scratch, s, to_scratch, &mut self.under);
                let mut canvas =
                    PixmapMut::from_bytes(scratch, s.w, s.h).expect("damage rects are non-empty");
                if let (Some(r), Some(text)) = (badge_rect, self.badge.as_deref())
                    && r.to_physical(self.scale).intersects(&s)
                {
                    draw_badge(&mut canvas, r, text, to_scratch);
                }
                for overlay in self.overlays.iter().chain(&self.top) {
                    if overlay.rect().to_physical(self.scale).intersects(&s) {
                        overlay.draw(&mut canvas, to_scratch);
                    }
                }
            }
            let tw = target.width();
            copy_rect(
                scratch,
                s.w,
                IRect::new(d.x - s.x, d.y - s.y, d.w, d.h),
                target.data_mut(),
                tw,
                d.x,
                d.y,
            );
        }
        &self.painted
    }
}

/// What one damage rect's widget pass reads.
struct WidgetPass<'a> {
    controls: &'a [ControlSpec],
    widgets: &'a [Box<dyn Widget>],
    shown: &'a [u16],
    scale: f32,
    states: &'a [WidgetState],
}

impl WidgetPass<'_> {
    /// Draw every shown widget meeting `s` into `scratch` (`s`-sized),
    /// fading dimmed ones; `under` keeps what a dimmed one covered.
    fn draw(&self, scratch: &mut [u8], s: IRect, to_scratch: Transform, under: &mut Vec<u8>) {
        for (spec, widget) in self.controls.iter().zip(self.widgets) {
            if !spec.is_shown(self.shown) {
                continue;
            }
            let Some(r) = spec.rect.to_physical(self.scale).intersect(&s) else {
                continue;
            };
            let state = &self.states[spec.id.index()];
            // A dimmed widget is drawn, then faded back toward what it
            // covered, unless it draws its dimming itself.
            let local = IRect::new(r.x - s.x, r.y - s.y, r.w, r.h);
            let fade = state.dimmed && !widget.draws_dimmed();
            if fade {
                let n = r.w as usize * r.h as usize * 4;
                if under.len() < n {
                    under.resize(n, 0);
                }
                copy_rect(scratch, s.w, local, under, r.w, 0, 0);
            }
            let mut canvas =
                PixmapMut::from_bytes(scratch, s.w, s.h).expect("damage rects are non-empty");
            widget.draw(&mut canvas, spec.rect, to_scratch, state);
            if fade {
                fade_toward(scratch, s.w, local, under, DIM_OPACITY);
            }
        }
    }
}

/// How opaque a dimmed control's widget is drawn ([`WidgetState::dimmed`]).
///
/// [`WidgetState::dimmed`]: crate::WidgetState::dimmed
pub const DIM_OPACITY: f32 = 0.4;

const BADGE_DOT: f32 = 1.5;
const BADGE_PAD: f32 = 3.0;
const BADGE_MARGIN: f32 = 4.0;

fn draw_badge(canvas: &mut PixmapMut<'_>, r: Rect, text: &str, transform: Transform) {
    let mut paint = tiny_skia::Paint::default();
    paint.set_color(tiny_skia::Color::from_rgba8(0, 0, 0, 200));
    if let Some(bg) = tiny_skia::Rect::from_xywh(r.x, r.y, r.w, r.h) {
        canvas.fill_rect(bg, &paint, transform, None);
    }
    crate::text::draw(
        canvas,
        text,
        r.x + BADGE_PAD,
        r.y + BADGE_PAD,
        BADGE_DOT,
        tiny_skia::Color::from_rgba8(230, 230, 230, 255),
        transform,
    );
}

/// Copy `src_rect` of an RGBA8 buffer `src_w` pixels wide into `dst` (width
/// `dst_w`) at `(dx, dy)`.
fn copy_rect(
    src: &[u8],
    src_w: u32,
    src_rect: IRect,
    dst: &mut [u8],
    dst_w: u32,
    dx: i32,
    dy: i32,
) {
    let row_bytes = src_rect.w as usize * 4;
    for row in 0..src_rect.h as usize {
        let s = ((src_rect.y as usize + row) * src_w as usize + src_rect.x as usize) * 4;
        let d = ((dy as usize + row) * dst_w as usize + dx as usize) * 4;
        dst[d..d + row_bytes].copy_from_slice(&src[s..s + row_bytes]);
    }
}

/// Blend `rect` of `dst` (a `dst_w`-wide RGBA8 premultiplied buffer)
/// toward `under` (`rect`-sized, packed), leaving `opacity` of `dst`.
fn fade_toward(dst: &mut [u8], dst_w: u32, rect: IRect, under: &[u8], opacity: f32) {
    // Fixed point: k/256 of dst, the rest of under.
    let k = (opacity.clamp(0.0, 1.0) * 256.0).round() as u32;
    let row_bytes = rect.w as usize * 4;
    for row in 0..rect.h as usize {
        let d = ((rect.y as usize + row) * dst_w as usize + rect.x as usize) * 4;
        let u = row * row_bytes;
        for (a, &b) in dst[d..d + row_bytes]
            .iter_mut()
            .zip(&under[u..u + row_bytes])
        {
            *a = ((u32::from(*a) * k + u32::from(b) * (256 - k) + 128) >> 8) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{Rect, Size};
    use crate::layout::{ControlKind, ControlSpec};
    use tiny_skia::Color;

    static CONTROLS: [ControlSpec; 2] = [
        ControlSpec::new(
            ControlId(0),
            "a",
            ControlKind::Knob,
            Rect::new(10.0, 10.0, 40.0, 40.0),
        ),
        ControlSpec::new(
            ControlId(1),
            "b",
            ControlKind::Knob,
            Rect::new(60.5, 10.25, 30.0, 30.0),
        ),
    ];
    static LAYOUT: Layout = Layout::new(Size::new(100.0, 60.0), &CONTROLS);

    /// A flat faceplate with a gradient so misplaced copies show up.
    struct TestRaster(Vec<(f32, Pixmap)>);

    impl RasterSource for TestRaster {
        fn size(&self) -> Size {
            LAYOUT.size
        }
        fn faceplate(&mut self, scale: f32) -> &Pixmap {
            if !self.0.iter().any(|(s, _)| *s == scale) {
                let (w, h) = LAYOUT.size.to_physical(scale);
                let mut p = Pixmap::new(w, h).unwrap();
                for y in 0..h {
                    for x in 0..w {
                        let c = Color::from_rgba8((x % 256) as u8, (y % 256) as u8, 40, 255)
                            .premultiply()
                            .to_color_u8();
                        p.pixels_mut()[(y * w + x) as usize] = c;
                    }
                }
                self.0.push((scale, p));
            }
            &self.0.iter().find(|(s, _)| *s == scale).unwrap().1
        }
    }

    fn states(a: f32, b: f32) -> [WidgetState; 2] {
        [WidgetState::value(a), WidgetState::value(b)]
    }

    fn setup(scale: f32) -> (Compositor, TestRaster, Pixmap) {
        let mut c = Compositor::new(&LAYOUT);
        c.set_scale(scale);
        let (w, h) = c.frame_size();
        (c, TestRaster(Vec::new()), Pixmap::new(w, h).unwrap())
    }

    /// Pixels that differ between two frames, as (x, y).
    fn diff(a: &Pixmap, b: &Pixmap) -> Vec<(u32, u32)> {
        let w = a.width();
        a.pixels()
            .iter()
            .zip(b.pixels())
            .enumerate()
            .filter(|(_, (p, q))| p != q)
            .map(|(i, _)| (i as u32 % w, i as u32 / w))
            .collect()
    }

    fn inside(r: IRect, (x, y): (u32, u32)) -> bool {
        (x as i32) >= r.x && (x as i32) < r.right() && (y as i32) >= r.y && (y as i32) < r.bottom()
    }

    /// A solid block.
    struct Block(Rect);

    impl Overlay for Block {
        fn rect(&self) -> Rect {
            self.0
        }
        fn draw(&self, canvas: &mut PixmapMut<'_>, transform: Transform) {
            let mut paint = tiny_skia::Paint::default();
            paint.set_color(Color::from_rgba8(255, 255, 255, 255));
            let r = tiny_skia::Rect::from_xywh(self.0.x, self.0.y, self.0.w, self.0.h).unwrap();
            canvas.fill_rect(r, &paint, transform, None);
        }
    }

    /// Counts its draws.
    struct Counting(Rect, std::rc::Rc<std::cell::Cell<usize>>);

    impl Overlay for Counting {
        fn rect(&self) -> Rect {
            self.0
        }
        fn draw(&self, _: &mut PixmapMut<'_>, _: Transform) {
            self.1.set(self.1.get() + 1);
        }
    }

    #[test]
    fn an_overlay_draws_once_however_many_rects_meet_it() {
        let (mut c, mut raster, mut target) = setup(1.0);
        c.render(&mut target.as_mut(), &mut raster, &states(0.5, 0.5));
        let draws = std::rc::Rc::new(std::cell::Cell::new(0));
        c.set_overlays(vec![Box::new(Counting(
            Rect::new(5.0, 5.0, 80.0, 40.0),
            std::rc::Rc::clone(&draws),
        ))]);
        c.render(&mut target.as_mut(), &mut raster, &states(0.5, 0.5));
        draws.set(0);
        // Two rows of the panel and something outside it.
        c.damage(IRect::new(10, 10, 20, 4));
        c.damage(IRect::new(10, 30, 20, 4));
        c.damage(IRect::new(90, 50, 5, 5));
        let painted = c
            .render(&mut target.as_mut(), &mut raster, &states(0.5, 0.5))
            .len();
        assert_eq!(draws.get(), 1);
        assert_eq!(painted, 2, "the rows' union and the rect outside");
    }

    #[test]
    fn merging_repeats_until_nothing_changes() {
        let (mut c, mut raster, mut target) = setup(1.0);
        c.render(&mut target.as_mut(), &mut raster, &states(0.5, 0.5));
        // A strip, then a band across it: neither nests in the other.
        let strip = Rect::new(40.0, 0.0, 20.0, 60.0);
        let band = Rect::new(0.0, 0.0, 100.0, 20.0);
        c.set_overlays(vec![Box::new(Block(strip)), Box::new(Block(band))]);
        c.render(&mut target.as_mut(), &mut raster, &states(0.5, 0.5));
        // Two in the band beside the strip, one in the strip below it:
        // the band's union then meets the strip's rect.
        c.damage(IRect::new(5, 5, 2, 2));
        c.damage(IRect::new(90, 5, 2, 2));
        c.damage(IRect::new(45, 50, 2, 2));
        c.prepare();
        assert_eq!(c.pending, [IRect::new(5, 5, 87, 47)]);
        c.prepare();
        assert_eq!(c.pending, [IRect::new(5, 5, 87, 47)], "idempotent");
    }

    #[test]
    fn overlays_draw_above_widgets_and_damage_where_they_were() {
        let (mut c, mut raster, mut target) = setup(2.0);
        c.render(&mut target.as_mut(), &mut raster, &states(0.5, 0.5));
        let before = target.clone();

        // Over knob a: drawn on top of it, and only its rect repainted.
        let block = Rect::new(20.0, 20.0, 10.0, 5.0);
        c.set_overlays(vec![Box::new(Block(block))]);
        let painted = c
            .render(&mut target.as_mut(), &mut raster, &states(0.5, 0.5))
            .to_vec();
        assert_eq!(painted, [block.to_physical(2.0)]);
        let white = target.pixel(50, 45).unwrap();
        assert_eq!((white.red(), white.alpha()), (255, 255), "on top");

        // Removed: its rect repaints back to the knob beneath.
        c.set_overlays(Vec::new());
        let painted = c
            .render(&mut target.as_mut(), &mut raster, &states(0.5, 0.5))
            .to_vec();
        assert_eq!(painted, [block.to_physical(2.0)]);
        let worst = before
            .data()
            .iter()
            .zip(target.data())
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        // As with the badge: a partial repaint of knob a can move its
        // anti-aliasing by one level at a clipped edge.
        assert!(worst <= 1, "worst channel diff {worst}");
    }

    #[test]
    fn a_fetched_page_stands_in_until_it_arrives_and_the_host_rescales() {
        use crate::layout::PageGroup;
        use crate::raster::EmbeddedPngs;

        // A page group over the right half, its two pages fetched at 1×.
        static URLS: [[(f32, &str); 1]; 2] = [
            [(1.0, "compositor-test/page.0@1x.png")],
            [(1.0, "compositor-test/page.1@1x.png")],
        ];
        static PAGES: [EmbeddedPngs; 2] = [
            EmbeddedPngs::fetched(Size::new(100.0, 60.0), &URLS[0]),
            EmbeddedPngs::fetched(Size::new(100.0, 60.0), &URLS[1]),
        ];
        static GROUPS: [PageGroup; 1] = [PageGroup::new(
            "fx",
            Rect::new(50.0, 0.0, 50.0, 60.0),
            &PAGES,
        )];
        static PAGED: Layout = Layout::new(Size::new(100.0, 60.0), &[]).with_pages(&GROUPS);

        let mut c = Compositor::new(&PAGED);
        let mut raster = TestRaster(Vec::new());
        let mut target = Pixmap::new(100, 60).unwrap();
        let rgb = |t: &Pixmap| {
            let p = t.pixel(75, 30).unwrap();
            (p.red(), p.green(), p.blue())
        };

        // Every page is asked for together; blank stands in.
        c.render(&mut target.as_mut(), &mut raster, &[]);
        assert_eq!(rgb(&target), (0x20, 0x20, 0x24));
        let wanted = crate::assets::take_wanted();
        assert!(wanted.contains(&URLS[0][0].1), "{wanted:?}");
        assert!(wanted.contains(&URLS[1][0].1), "{wanted:?}");

        // It arrives; the web host calls set_scale, which re-crops.
        let mut page = Pixmap::new(100, 60).unwrap();
        page.fill(Color::from_rgba8(0, 200, 0, 255));
        crate::assets::provide(URLS[0][0].1, page.encode_png().unwrap());
        c.set_scale(1.0);
        c.render(&mut target.as_mut(), &mut raster, &[]);
        assert_eq!(rgb(&target), (0, 200, 0));
        // Off the region, the base faceplate.
        assert_ne!(target.pixel(25, 30).unwrap().green(), 200);
    }

    #[test]
    fn a_stand_in_page_is_remade_when_it_arrives_without_a_rescale() {
        use crate::layout::PageGroup;
        use crate::raster::EmbeddedPngs;

        static URLS: [[(f32, &str); 1]; 1] = [[(1.0, "compositor-test/arrives.0@1x.png")]];
        static PAGES: [EmbeddedPngs; 1] = [EmbeddedPngs::fetched(Size::new(100.0, 60.0), &URLS[0])];
        static GROUPS: [PageGroup; 1] = [PageGroup::new(
            "fx",
            Rect::new(50.0, 0.0, 50.0, 60.0),
            &PAGES,
        )];
        static PAGED: Layout = Layout::new(Size::new(100.0, 60.0), &[]).with_pages(&GROUPS);

        let mut c = Compositor::new(&PAGED);
        let mut raster = TestRaster(Vec::new());
        let mut target = Pixmap::new(100, 60).unwrap();
        c.render(&mut target.as_mut(), &mut raster, &[]);
        assert_eq!(
            target.pixel(75, 30).unwrap().green(),
            0x20,
            "blank stands in"
        );

        let mut page = Pixmap::new(100, 60).unwrap();
        page.fill(Color::from_rgba8(0, 200, 0, 255));
        crate::assets::provide(URLS[0][0].1, page.encode_png().unwrap());
        // No rescale: the next render notices and repaints the region.
        let painted = c.render(&mut target.as_mut(), &mut raster, &[]).to_vec();
        assert_eq!(painted, [IRect::new(50, 0, 50, 60)]);
        assert_eq!(target.pixel(75, 30).unwrap().green(), 200);
    }

    #[test]
    fn first_render_paints_everything_then_nothing() {
        let (mut c, mut raster, mut target) = setup(1.5);
        let painted = c
            .render(&mut target.as_mut(), &mut raster, &states(0.5, 0.5))
            .to_vec();
        assert_eq!(painted, [IRect::frame(150, 90)]);

        let before = target.clone();
        // Even with different states: no damage means no drawing.
        let painted = c.render(&mut target.as_mut(), &mut raster, &states(1.0, 0.0));
        assert!(painted.is_empty());
        assert_eq!(target, before);
    }

    #[test]
    fn set_scale_damages_everything() {
        let (mut c, mut raster, mut target) = setup(1.0);
        c.render(&mut target.as_mut(), &mut raster, &states(0.5, 0.5));
        c.set_scale(2.0);
        assert_eq!(c.frame_size(), (200, 120));
        let mut target = Pixmap::new(200, 120).unwrap();
        let painted = c.render(&mut target.as_mut(), &mut raster, &states(0.5, 0.5));
        assert_eq!(painted, [IRect::frame(200, 120)]);
    }

    #[test]
    fn widgets_draw_only_inside_their_rect() {
        for scale in [1.0, 1.5, 2.0] {
            let (mut c, mut raster, mut target) = setup(scale);
            c.render(&mut target.as_mut(), &mut raster, &states(1.0, 1.0));
            let faceplate = raster.faceplate(scale).clone();
            let rects: Vec<IRect> = CONTROLS.iter().map(|s| s.rect.to_physical(scale)).collect();
            let changed = diff(&faceplate, &target);
            assert!(!changed.is_empty(), "knobs drew nothing at {scale}x");
            for px in changed {
                assert!(
                    rects.iter().any(|r| inside(*r, px)),
                    "{px:?} outside every knob at {scale}x"
                );
            }
        }
    }

    #[test]
    fn control_damage_repaints_only_that_control() {
        let (mut c, mut raster, mut target) = setup(1.5);
        c.render(&mut target.as_mut(), &mut raster, &states(0.2, 0.2));
        let before = target.clone();

        c.damage_control(ControlId(1));
        let painted = c
            .render(&mut target.as_mut(), &mut raster, &states(0.2, 0.9))
            .to_vec();
        let b = CONTROLS[1].rect.to_physical(1.5);
        assert_eq!(painted, [b]);
        let changed = diff(&before, &target);
        assert!(!changed.is_empty());
        assert!(changed.iter().all(|px| inside(b, *px)));
    }

    #[test]
    fn repaint_matches_a_fresh_full_render() {
        // Incremental damage must produce the same pixels as starting over.
        let (mut c, mut raster, mut target) = setup(1.5);
        c.render(&mut target.as_mut(), &mut raster, &states(0.2, 0.2));
        c.damage_control(ControlId(0));
        c.render(&mut target.as_mut(), &mut raster, &states(0.8, 0.2));

        let (mut fresh, mut raster2, mut expected) = setup(1.5);
        fresh.render(&mut expected.as_mut(), &mut raster2, &states(0.8, 0.2));
        assert_eq!(target, expected);
    }

    #[test]
    fn rendering_is_deterministic() {
        let render = || {
            let (mut c, mut raster, mut target) = setup(2.0);
            c.render(&mut target.as_mut(), &mut raster, &states(0.37, 0.81));
            target
        };
        assert_eq!(render().data(), render().data());
    }

    #[test]
    fn badge_draws_bottom_left_and_repaints_only_on_change() {
        let (mut c, mut raster, mut target) = setup(1.0);
        c.render(&mut target.as_mut(), &mut raster, &states(0.5, 0.5));
        let before = target.clone();

        c.set_badge(Some("v1.2".into()));
        let painted = c
            .render(&mut target.as_mut(), &mut raster, &states(0.5, 0.5))
            .to_vec();
        assert_eq!(painted.len(), 1);
        let r = painted[0];
        assert_eq!(r.x, 4);
        assert_eq!(r.bottom(), 60 - 4, "sits on the bottom margin");
        let changed = diff(&before, &target);
        assert!(!changed.is_empty());
        assert!(changed.iter().all(|px| inside(r, *px)));

        c.set_badge(Some("v1.2".into()));
        assert!(!c.has_damage(), "same text: no repaint");

        c.set_badge(None);
        c.render(&mut target.as_mut(), &mut raster, &states(0.5, 0.5));
        let worst = before
            .data()
            .iter()
            .zip(target.data())
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        // The badge overlapped knob a, which is repainted through a partial
        // damage rect; tiny-skia's anti-aliasing coverage can move by a
        // level or two with the scratch's origin (design/rendering.md).
        assert!(
            worst <= 2,
            "clearing restores the faceplate (worst channel diff {worst})"
        );
    }

    #[test]
    fn damage_coalesces_contained_rects() {
        let (mut c, mut raster, mut target) = setup(1.0);
        c.render(&mut target.as_mut(), &mut raster, &states(0.5, 0.5));
        c.damage(IRect::new(10, 10, 5, 5));
        c.damage(IRect::new(0, 0, 50, 50)); // swallows the first
        c.damage(IRect::new(20, 20, 5, 5)); // already covered
        c.damage(IRect::new(500, 500, 5, 5)); // off-frame
        let painted = c.render(&mut target.as_mut(), &mut raster, &states(0.5, 0.5));
        assert_eq!(painted, [IRect::new(0, 0, 50, 50)]);
    }
}
