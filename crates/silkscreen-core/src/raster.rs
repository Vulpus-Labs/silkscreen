//! Faceplate pixels at any scale ([design/rendering.md]).
//!
//! [design/rendering.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/rendering.md

use std::cell::RefCell;
use std::rc::Rc;

use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

use crate::geom::Size;

/// Supplies the faceplate bitmap at a given effective scale.
pub trait RasterSource {
    /// Logical size of the faceplate.
    fn size(&self) -> Size;

    /// The faceplate rendered at `scale`, exactly
    /// [`Size::to_physical`]`(scale)` pixels. Implementations cache.
    fn faceplate(&mut self, scale: f32) -> &Pixmap;
}

/// Pre-rendered faceplate PNGs, one per scale, as emitted by
/// `silkscreen-build`: embedded in the binary ([`new`](Self::new)), or
/// named by URL and fetched at run time ([`fetched`](Self::fetched),
/// [`crate::assets`]). Immutable `const` data; decoding happens in
/// [`EmbeddedRaster`].
#[derive(Debug)]
pub struct EmbeddedPngs {
    size: Size,
    pngs: Pngs,
}

#[derive(Debug, Clone, Copy)]
enum Pngs {
    Embedded(&'static [(f32, &'static [u8])]),
    Fetched(&'static [(f32, &'static str)]),
}

/// Where a scale's PNG bytes are.
enum Source {
    Bytes(&'static [u8]),
    Url(&'static str),
}

/// A scale's PNG bytes, in hand.
enum Bytes {
    Static(&'static [u8]),
    Fetched(Rc<[u8]>),
}

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        match self {
            Bytes::Static(b) => b,
            Bytes::Fetched(b) => b,
        }
    }
}

impl EmbeddedPngs {
    /// `pngs` holds `(scale, png bytes)` pairs, each image exactly
    /// `size.to_physical(scale)` pixels.
    pub const fn new(size: Size, pngs: &'static [(f32, &'static [u8])]) -> Self {
        Self {
            size,
            pngs: Pngs::Embedded(pngs),
        }
    }

    /// `urls` holds `(scale, url)` pairs; the host fetches each URL when a
    /// raster first needs it ([`crate::assets`]).
    pub const fn fetched(size: Size, urls: &'static [(f32, &'static str)]) -> Self {
        Self {
            size,
            pngs: Pngs::Fetched(urls),
        }
    }

    pub fn size(&self) -> Size {
        self.size
    }

    /// The scales, in the order given.
    pub fn scales(&self) -> impl Iterator<Item = f32> + '_ {
        let (embedded, fetched) = match self.pngs {
            Pngs::Embedded(p) => (p, &[][..]),
            Pngs::Fetched(u) => (&[][..], u),
        };
        embedded
            .iter()
            .map(|(s, _)| *s)
            .chain(fetched.iter().map(|(s, _)| *s))
    }

    fn sources(&self) -> Vec<(f32, Source)> {
        match self.pngs {
            Pngs::Embedded(p) => p.iter().map(|&(s, b)| (s, Source::Bytes(b))).collect(),
            Pngs::Fetched(u) => u.iter().map(|&(s, url)| (s, Source::Url(url))).collect(),
        }
    }
}

/// The best of `scales` for `scale`: the smallest at least as large (so
/// only ever downsampling), or the largest if none is.
fn best_for<T>(candidates: impl Iterator<Item = (f32, T)>, scale: f32) -> Option<(f32, T)> {
    let mut at_least: Option<(f32, T)> = None;
    let mut largest: Option<(f32, T)> = None;
    for (s, t) in candidates {
        if s >= scale {
            if at_least.as_ref().is_none_or(|(a, _)| s < *a) {
                at_least = Some((s, t));
            }
        } else if largest.as_ref().is_none_or(|(l, _)| s > *l) {
            largest = Some((s, t));
        }
    }
    at_least.or(largest)
}

/// A [`RasterSource`] over [`EmbeddedPngs`]: decodes the best scale and
/// resamples it (bicubic) when the requested scale isn't one of them.
/// Results are cached for the two scales most recently asked for: the
/// current one and the one before (a zoom step back is instant, and a
/// resize doesn't keep a bitmap per step it passed).
///
/// For fetched PNGs, the best scale may not have arrived. Then it is asked
/// for, and the best scale that has arrived stands in (or blank, if none
/// has) until [`assets::generation`](crate::assets::generation) moves on.
#[derive(Debug)]
pub struct EmbeddedRaster {
    pngs: &'static EmbeddedPngs,
    /// `(scale, pixmap, stand-in)`: a stand-in records the asset
    /// generation it was made under.
    cache: Vec<(f32, Pixmap, Option<u64>)>,
    /// What stands in while nothing has arrived.
    blank: tiny_skia::Color,
}

impl EmbeddedRaster {
    /// A faceplate or page: while nothing has arrived, an opaque blank.
    pub fn new(pngs: &'static EmbeddedPngs) -> Self {
        Self {
            pngs,
            cache: Vec::new(),
            blank: tiny_skia::Color::from_rgba8(0x20, 0x20, 0x24, 0xff),
        }
    }

    /// A skin frame: while nothing has arrived, transparent, so the
    /// faceplate shows through rather than a dark square.
    pub fn skin_frame(pngs: &'static EmbeddedPngs) -> Self {
        Self {
            blank: tiny_skia::Color::TRANSPARENT,
            ..Self::new(pngs)
        }
    }

    /// Whether the bitmap cached for `scale` stands in for a fetched PNG
    /// that hadn't arrived when it was made.
    pub fn is_stand_in(&self, scale: f32) -> bool {
        self.cache
            .iter()
            .any(|(s, _, stand_in)| s.to_bits() == scale.to_bits() && stand_in.is_some())
    }

    /// Drop all cached bitmaps.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Ask for the PNG `scale` would use, if it is fetched and hasn't
    /// arrived, without decoding anything.
    pub fn want(&self, scale: f32) {
        let sources = self.pngs.sources();
        if let Some((_, Source::Url(url))) =
            best_for(sources.iter().map(|(s, src)| (*s, src)), scale)
            && crate::assets::get(url).is_none()
        {
            crate::assets::want(url);
        }
    }

    /// The bitmap for `scale`, and whether it is a stand-in.
    fn produce(&self, scale: f32) -> (Pixmap, bool) {
        let (w, h) = self.pngs.size.to_physical(scale);
        let sources = self.pngs.sources();
        let ideal = best_for(sources.iter().map(|(s, src)| (*s, src)), scale);
        let mut stand_in = false;
        if let Some((_, Source::Url(url))) = ideal
            && crate::assets::get(url).is_none()
        {
            crate::assets::want(url);
            stand_in = true;
        }
        let available = sources.iter().filter_map(|(s, src)| match src {
            Source::Bytes(b) => Some((*s, Bytes::Static(b), "embedded")),
            Source::Url(url) => crate::assets::get(url).map(|b| (*s, Bytes::Fetched(b), *url)),
        });
        let Some((src_scale, bytes, origin)) =
            best_for(available.map(|(s, b, o)| (s, (b, o))), scale).map(|(s, (b, o))| (s, b, o))
        else {
            // Nothing has arrived yet: a blank.
            let mut blank = Pixmap::new(w, h).expect("faceplate has non-zero size");
            blank.fill(self.blank);
            return (blank, true);
        };
        let decoded = Pixmap::decode_png(bytes.as_ref()).unwrap_or_else(|e| {
            panic!("faceplate PNG for scale {src_scale} ({origin}) is corrupt: {e}")
        });
        if decoded.width() == w && decoded.height() == h {
            return (decoded, stand_in);
        }
        let mut out = Pixmap::new(w, h).expect("faceplate has non-zero size");
        let paint = PixmapPaint {
            quality: FilterQuality::Bicubic,
            blend_mode: tiny_skia::BlendMode::Source,
            ..PixmapPaint::default()
        };
        let transform = Transform::from_scale(
            w as f32 / decoded.width() as f32,
            h as f32 / decoded.height() as f32,
        );
        out.draw_pixmap(0, 0, decoded.as_ref(), &paint, transform, None);
        (out, stand_in)
    }
}

impl RasterSource for EmbeddedRaster {
    fn size(&self) -> Size {
        self.pngs.size
    }

    fn faceplate(&mut self, scale: f32) -> &Pixmap {
        let generation = crate::assets::generation();
        // Exact-bits match: a cache key, not a numeric comparison. A
        // stand-in made before the latest delivery is made again.
        let found = self.cache.iter().position(|(s, _, stand_in)| {
            s.to_bits() == scale.to_bits() && stand_in.is_none_or(|g| g == generation)
        });
        if let Some(i) = found {
            // Most recently used last.
            let hit = self.cache.remove(i);
            self.cache.push(hit);
        } else {
            self.cache
                .retain(|(s, _, _)| s.to_bits() != scale.to_bits());
            let (pixmap, stand_in) = self.produce(scale);
            self.cache
                .push((scale, pixmap, stand_in.then_some(generation)));
            if self.cache.len() > CACHED_SCALES {
                self.cache.remove(0);
            }
        }
        &self.cache.last().expect("just cached").1
    }
}

/// How many scales a raster keeps decoded.
const CACHED_SCALES: usize = 2;

/// A control's sprite frames ([design/faceplates.md]), as emitted by
/// `silkscreen-build`: each frame pre-rendered at every scale, at the
/// control's logical size. What the frames mean depends on the control
/// kind: a knob has a static base and a top that rotates; a toggle,
/// button or radio button has an off frame and an on frame.
///
/// [design/faceplates.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/faceplates.md
#[derive(Debug)]
pub struct Skin {
    name: &'static str,
    frames: &'static [EmbeddedPngs],
    sweep_deg: f32,
    cycle_deg: f32,
}

impl Skin {
    /// Knob sweep unless a skin sets its own: 270°, centred on 12 o'clock.
    pub const DEFAULT_SWEEP_DEG: f32 = 270.0;

    pub const fn new(name: &'static str, frames: &'static [EmbeddedPngs]) -> Self {
        Self {
            name,
            frames,
            sweep_deg: Self::DEFAULT_SWEEP_DEG,
            cycle_deg: 0.0,
        }
    }

    /// For knob skins whose body repeats every `period_deg` (e.g. a knurl):
    /// frame 0 is the pointer alone, drawn at 12 o'clock and rotated to the
    /// exact angle; frames 1.. are the body turned evenly across one period,
    /// and the widget shows the one for the angle modulo the period.
    pub const fn with_cycle(mut self, period_deg: f32) -> Self {
        self.cycle_deg = period_deg;
        self
    }

    /// The body period of a cycled knob skin, or 0.
    pub fn cycle_deg(&self) -> f32 {
        self.cycle_deg
    }

    /// For knob skins: the top's total rotation from minimum to maximum,
    /// centred on its drawn (12 o'clock) orientation.
    pub const fn with_sweep(mut self, degrees: f32) -> Self {
        self.sweep_deg = degrees;
        self
    }

    pub fn sweep_deg(&self) -> f32 {
        self.sweep_deg
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn frames(&self) -> &'static [EmbeddedPngs] {
        self.frames
    }
}

/// Skins are static data: two are equal only if they are the same one.
impl PartialEq for Skin {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

/// Decoded, scaled frames of a [`Skin`], cached per scale like
/// [`EmbeddedRaster`]. One per skin per view, shared by every widget
/// drawn with it ([`SkinCache`]).
///
/// Fetched frames are asked for together: the first frame drawn at a
/// scale asks for every frame at that scale. A knob turning would
/// otherwise reach angles not yet fetched, and blink out while each one
/// arrives.
#[derive(Debug)]
pub struct SkinRaster {
    skin: &'static Skin,
    frames: Vec<EmbeddedRaster>,
    /// The scale (as bits) whose frames have all been asked for.
    asked: Option<u32>,
}

impl SkinRaster {
    pub fn new(skin: &'static Skin) -> Self {
        Self {
            skin,
            frames: skin.frames.iter().map(EmbeddedRaster::skin_frame).collect(),
            asked: None,
        }
    }

    pub fn skin(&self) -> &'static Skin {
        self.skin
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Frame `index` at `scale`: exactly the control's logical size times
    /// `scale`, in pixels. Panics if `index` is out of range.
    pub fn frame(&mut self, index: usize, scale: f32) -> &Pixmap {
        if self.asked != Some(scale.to_bits()) {
            self.asked = Some(scale.to_bits());
            self.frames.iter().for_each(|f| f.want(scale));
        }
        self.frames[index].faceplate(scale)
    }
}

/// A [`SkinRaster`] shared by the widgets drawn with its skin.
pub type SharedSkin = Rc<RefCell<SkinRaster>>;

/// One [`SkinRaster`] per skin, for a view's widgets: knobs on the same
/// skin decode each frame once between them.
#[derive(Debug, Default)]
pub struct SkinCache {
    skins: Vec<SharedSkin>,
}

impl SkinCache {
    /// `skin`'s frames, shared with every other widget that asked.
    pub fn get(&mut self, skin: &'static Skin) -> SharedSkin {
        if let Some(found) = self.skins.iter().find(|r| r.borrow().skin() == skin) {
            return Rc::clone(found);
        }
        let raster = Rc::new(RefCell::new(SkinRaster::new(skin)));
        self.skins.push(Rc::clone(&raster));
        raster
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_skia::Color;

    const SIZE: Size = Size::new(10.0, 5.0);

    /// Solid-colour PNG at `scale`, leaked to get `'static` bytes.
    fn png(scale: f32, color: Color) -> &'static [u8] {
        let (w, h) = SIZE.to_physical(scale);
        let mut p = Pixmap::new(w, h).unwrap();
        p.fill(color);
        Box::leak(p.encode_png().unwrap().into_boxed_slice())
    }

    /// Embedded 1× red, 1.5× green, 2× blue, so the source used for any
    /// request is visible in the output colour.
    fn fixture() -> &'static EmbeddedPngs {
        let pngs: &'static [(f32, &'static [u8])] = Box::leak(Box::new([
            (1.0, png(1.0, Color::from_rgba8(255, 0, 0, 255))),
            (1.5, png(1.5, Color::from_rgba8(0, 255, 0, 255))),
            (2.0, png(2.0, Color::from_rgba8(0, 0, 255, 255))),
        ]));
        Box::leak(Box::new(EmbeddedPngs::new(SIZE, pngs)))
    }

    fn centre_rgb(p: &Pixmap) -> (u8, u8, u8) {
        let c = p.pixel(p.width() / 2, p.height() / 2).unwrap();
        (c.red(), c.green(), c.blue())
    }

    #[test]
    fn exact_scale_is_used_directly() {
        let mut r = EmbeddedRaster::new(fixture());
        let p = r.faceplate(1.5);
        assert_eq!((p.width(), p.height()), (15, 8));
        assert_eq!(centre_rgb(p), (0, 255, 0));
    }

    #[test]
    fn in_between_scale_downsamples_from_next_larger() {
        let mut r = EmbeddedRaster::new(fixture());
        let p = r.faceplate(1.25);
        // 12.5 → 13, 6.25 → 6.
        assert_eq!((p.width(), p.height()), (13, 6));
        assert_eq!(centre_rgb(p), (0, 255, 0));
    }

    #[test]
    fn scale_below_smallest_downsamples_from_smallest() {
        let mut r = EmbeddedRaster::new(fixture());
        let p = r.faceplate(0.75);
        assert_eq!((p.width(), p.height()), (8, 4));
        assert_eq!(centre_rgb(p), (255, 0, 0));
    }

    #[test]
    fn scale_above_largest_upscales_largest() {
        let mut r = EmbeddedRaster::new(fixture());
        let p = r.faceplate(3.0);
        assert_eq!((p.width(), p.height()), (30, 15));
        assert_eq!(centre_rgb(p), (0, 0, 255));
    }

    #[test]
    fn resampled_output_stays_opaque_to_the_edges() {
        let mut r = EmbeddedRaster::new(fixture());
        for scale in [0.75, 1.25, 1.75, 3.0] {
            let p = r.faceplate(scale);
            assert!(
                p.pixels().iter().all(|c| c.alpha() == 255),
                "translucent pixels at {scale}x"
            );
        }
    }

    #[test]
    fn skin_frames_are_looked_up_by_index_and_scaled() {
        let pngs: &'static [EmbeddedPngs] = Box::leak(Box::new([
            EmbeddedPngs::new(
                SIZE,
                Box::leak(Box::new([(
                    1.0,
                    png(1.0, Color::from_rgba8(255, 0, 0, 255)),
                )])),
            ),
            EmbeddedPngs::new(
                SIZE,
                Box::leak(Box::new([(
                    2.0,
                    png(2.0, Color::from_rgba8(0, 0, 255, 255)),
                )])),
            ),
        ]));
        let skin: &'static Skin = Box::leak(Box::new(Skin::new("test", pngs)));
        let mut r = SkinRaster::new(skin);
        assert_eq!(r.len(), 2);
        let off = r.frame(0, 1.0);
        assert_eq!((off.width(), off.height()), (10, 5));
        assert_eq!(centre_rgb(off), (255, 0, 0));
        let on = r.frame(1, 1.5);
        assert_eq!((on.width(), on.height()), (15, 8), "downsampled from 2x");
        assert_eq!(centre_rgb(on), (0, 0, 255));
        assert_eq!(*skin, *skin);
    }

    /// Fetched 1× and 2× PNGs, under URLs unique to `tag` (the asset store
    /// is per thread, and tests share threads).
    fn fetched_fixture(tag: &str) -> (&'static EmbeddedPngs, [&'static str; 2]) {
        let urls: [&'static str; 2] = [
            Box::leak(format!("{tag}@1x.png").into_boxed_str()),
            Box::leak(format!("{tag}@2x.png").into_boxed_str()),
        ];
        let pairs: &'static [(f32, &'static str)] =
            Box::leak(Box::new([(1.0, urls[0]), (2.0, urls[1])]));
        (
            Box::leak(Box::new(EmbeddedPngs::fetched(SIZE, pairs))),
            urls,
        )
    }

    #[test]
    fn a_fetched_png_is_asked_for_once_and_blank_stands_in() {
        let (pngs, urls) = fetched_fixture("blank");
        let mut r = EmbeddedRaster::new(pngs);
        let p = r.faceplate(1.0);
        assert_eq!((p.width(), p.height()), (10, 5));
        assert!(p.pixels().iter().all(|c| c.alpha() == 255), "opaque");
        assert!(crate::assets::take_wanted().contains(&urls[0]));
        r.faceplate(1.0);
        r.clear_cache();
        r.faceplate(1.0);
        assert!(!crate::assets::take_wanted().contains(&urls[0]), "once");
    }

    #[test]
    fn a_stand_in_is_replaced_when_the_right_png_arrives() {
        let (pngs, urls) = fetched_fixture("standin");
        let red = png(2.0, Color::from_rgba8(255, 0, 0, 255)).to_vec();
        let green = png(1.0, Color::from_rgba8(0, 255, 0, 255)).to_vec();
        crate::assets::provide(urls[1], red);
        let mut r = EmbeddedRaster::new(pngs);
        // 1× wanted, 2× in hand: downsampled 2× stands in.
        assert_eq!(centre_rgb(r.faceplate(1.0)), (255, 0, 0));
        assert!(crate::assets::take_wanted().contains(&urls[0]));
        assert_eq!(centre_rgb(r.faceplate(1.0)), (255, 0, 0), "cached");
        crate::assets::provide(urls[0], green);
        assert_eq!(centre_rgb(r.faceplate(1.0)), (0, 255, 0), "replaced");
        // 2× was exact from the start: not asked for.
        assert_eq!(centre_rgb(r.faceplate(2.0)), (255, 0, 0));
        assert!(!crate::assets::take_wanted().contains(&urls[1]));
    }

    #[test]
    fn a_skin_frame_not_yet_fetched_is_transparent_then_drawn() {
        let (pngs, urls) = fetched_fixture("skin");
        let skin: &'static Skin =
            Box::leak(Box::new(Skin::new("fetched", std::slice::from_ref(pngs))));
        let mut r = SkinRaster::new(skin);
        let p = r.frame(0, 2.0);
        assert!(p.pixels().iter().all(|c| c.alpha() == 0), "transparent");
        assert!(crate::assets::take_wanted().contains(&urls[1]));
        crate::assets::provide(
            urls[1],
            png(2.0, Color::from_rgba8(0, 0, 255, 255)).to_vec(),
        );
        assert_eq!(centre_rgb(r.frame(0, 2.0)), (0, 0, 255));
    }

    #[test]
    fn drawing_one_skin_frame_asks_for_every_frame_at_that_scale() {
        let urls: Vec<[&'static str; 2]> = (0..3)
            .map(|n| {
                [
                    &*Box::leak(format!("together{n}@1x.png").into_boxed_str()),
                    &*Box::leak(format!("together{n}@2x.png").into_boxed_str()),
                ]
            })
            .collect();
        let pngs: &'static [EmbeddedPngs] = Box::leak(
            urls.iter()
                .map(|u| {
                    let pairs: &'static [(f32, &'static str)] =
                        Box::leak(Box::new([(1.0, u[0]), (2.0, u[1])]));
                    EmbeddedPngs::fetched(SIZE, pairs)
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        let skin: &'static Skin = Box::leak(Box::new(Skin::new("together", pngs)));
        let mut r = SkinRaster::new(skin);
        r.frame(1, 2.0);
        let wanted = crate::assets::take_wanted();
        for urls in &urls {
            assert!(wanted.contains(&urls[1]), "{} in {wanted:?}", urls[1]);
            assert!(!wanted.contains(&urls[0]), "only the 2x scale: {wanted:?}");
        }
    }

    #[test]
    fn results_are_cached_per_scale() {
        let mut r = EmbeddedRaster::new(fixture());
        let first = r.faceplate(1.25).data().as_ptr();
        r.faceplate(2.0);
        assert_eq!(r.faceplate(1.25).data().as_ptr(), first);
        r.clear_cache();
        assert_eq!(r.cache.len(), 0);
    }

    #[test]
    fn a_third_scale_evicts_the_least_recently_used() {
        let mut r = EmbeddedRaster::new(fixture());
        let one = r.faceplate(1.0).data().as_ptr();
        r.faceplate(2.0);
        // 1x again: now the most recent.
        assert_eq!(r.faceplate(1.0).data().as_ptr(), one);
        r.faceplate(1.5);
        let scales: Vec<f32> = r.cache.iter().map(|(s, _, _)| *s).collect();
        assert_eq!(scales, [1.0, 1.5], "2x went, the oldest");
    }

    #[test]
    fn widgets_on_one_skin_share_its_frames() {
        let skin: &'static Skin = Box::leak(Box::new(Skin::new(
            "shared",
            std::slice::from_ref(fixture()),
        )));
        let other: &'static Skin = Box::leak(Box::new(Skin::new(
            "other",
            std::slice::from_ref(fixture()),
        )));
        let mut cache = SkinCache::default();
        let (a, b, c) = (cache.get(skin), cache.get(skin), cache.get(other));
        assert!(Rc::ptr_eq(&a, &b));
        assert!(!Rc::ptr_eq(&a, &c));
        // A frame one widget decoded is the other's too.
        let drawn = a.borrow_mut().frame(0, 1.0).data().as_ptr();
        assert_eq!(b.borrow_mut().frame(0, 1.0).data().as_ptr(), drawn);
    }
}
