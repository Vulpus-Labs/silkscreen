//! Geometry in two coordinate spaces.
//!
//! **Logical** units (`Point`, `Vec2`, `Size`, `Rect`, all `f32`) are SVG user
//! units: layout, hit testing and input all happen here. **Physical** pixels
//! (`IRect`) are logical × effective scale ([design/rendering.md]); rendering and damage
//! happen here.
//!
//! [design/rendering.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/rendering.md

use core::ops::{Add, Sub};

/// A position in logical units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Euclidean distance to `other`.
    pub fn distance(self, other: Point) -> f32 {
        (self - other).length()
    }
}

/// A displacement in logical units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2::new(0.0, 0.0);

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn length(self) -> f32 {
        self.x.hypot(self.y)
    }
}

impl Sub for Point {
    type Output = Vec2;
    fn sub(self, rhs: Point) -> Vec2 {
        Vec2::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl Add<Vec2> for Point {
    type Output = Point;
    fn add(self, rhs: Vec2) -> Point {
        Point::new(self.x + rhs.x, self.y + rhs.y)
    }
}

/// A size in logical units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub w: f32,
    pub h: f32,
}

impl Size {
    pub const fn new(w: f32, h: f32) -> Self {
        Self { w, h }
    }

    /// Physical pixel dimensions at `scale`, rounded to nearest.
    ///
    /// This is the size of the frame buffer a host allocates, and the size
    /// `silkscreen-build` renders faceplate PNGs at, so the two always agree.
    pub fn to_physical(self, scale: f32) -> (u32, u32) {
        (
            (self.w * scale).round().max(1.0) as u32,
            (self.h * scale).round().max(1.0) as u32,
        )
    }
}

/// An axis-aligned rectangle in logical units. Contains points on its
/// left/top edges but not its right/bottom edges.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn center(&self) -> Point {
        Point::new(self.x + self.w * 0.5, self.y + self.h * 0.5)
    }

    pub fn size(&self) -> Size {
        Size::new(self.w, self.h)
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    /// The overlap of two rects, if they overlap.
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let r = self.right().min(other.right());
        let b = self.bottom().min(other.bottom());
        (r > x && b > y).then(|| Rect::new(x, y, r - x, b - y))
    }

    /// The physical-pixel rect covering this rect at `scale`, rounded
    /// *outward* so damage always covers every pixel a widget may touch.
    pub fn to_physical(&self, scale: f32) -> IRect {
        let x0 = (self.x * scale).floor() as i32;
        let y0 = (self.y * scale).floor() as i32;
        let x1 = (self.right() * scale).ceil() as i32;
        let y1 = (self.bottom() * scale).ceil() as i32;
        IRect::new(x0, y0, (x1 - x0).max(0) as u32, (y1 - y0).max(0) as u32)
    }
}

/// An axis-aligned rectangle in physical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct IRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl IRect {
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    /// The rect covering a whole `w`×`h` frame.
    pub const fn frame(w: u32, h: u32) -> Self {
        Self::new(0, 0, w, h)
    }

    pub fn right(&self) -> i32 {
        self.x + self.w as i32
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h as i32
    }

    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }

    /// The overlap of two rects, or `None` if they don't overlap.
    pub fn intersect(&self, other: &IRect) -> Option<IRect> {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = self.right().min(other.right());
        let y1 = self.bottom().min(other.bottom());
        (x1 > x0 && y1 > y0).then(|| IRect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32))
    }

    /// This rect one pixel in from each side: the pixels a logical rect
    /// rounded outward (`Rect::to_physical`) surely covers whole. `None`
    /// if nothing is left.
    pub fn shrunk_inside(&self) -> Option<IRect> {
        (self.w > 2 && self.h > 2)
            .then(|| IRect::new(self.x + 1, self.y + 1, self.w - 2, self.h - 2))
    }

    pub fn intersects(&self, other: &IRect) -> bool {
        self.intersect(other).is_some()
    }

    /// The smallest rect containing both.
    pub fn union(&self, other: &IRect) -> IRect {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = self.right().max(other.right());
        let y1 = self.bottom().max(other.bottom());
        IRect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_physical_rounds_outward() {
        let r = Rect::new(10.3, 20.6, 30.0, 40.2);
        // x: 10.3..40.3, y: 20.6..60.8
        assert_eq!(r.to_physical(1.0), IRect::new(10, 20, 31, 41));
        // x: 15.45..60.45, y: 30.9..91.2
        assert_eq!(r.to_physical(1.5), IRect::new(15, 30, 46, 62));
        // x: 20.6..80.6, y: 41.2..121.6
        assert_eq!(r.to_physical(2.0), IRect::new(20, 41, 61, 81));
    }

    #[test]
    fn integral_rects_map_exactly() {
        let r = Rect::new(10.0, 20.0, 30.0, 40.0);
        assert_eq!(r.to_physical(1.0), IRect::new(10, 20, 30, 40));
        assert_eq!(r.to_physical(1.5), IRect::new(15, 30, 45, 60));
        assert_eq!(r.to_physical(2.0), IRect::new(20, 40, 60, 80));
    }

    #[test]
    fn size_to_physical_rounds_to_nearest() {
        assert_eq!(Size::new(301.0, 99.0).to_physical(1.5), (452, 149));
        assert_eq!(Size::new(300.0, 100.0).to_physical(2.0), (600, 200));
    }

    #[test]
    fn contains_is_half_open() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(r.contains(Point::new(0.0, 0.0)));
        assert!(r.contains(Point::new(9.99, 9.99)));
        assert!(!r.contains(Point::new(10.0, 5.0)));
        assert!(!r.contains(Point::new(5.0, 10.0)));
    }

    #[test]
    fn irect_intersect_and_union() {
        let a = IRect::new(0, 0, 10, 10);
        let b = IRect::new(5, 5, 10, 10);
        assert_eq!(a.intersect(&b), Some(IRect::new(5, 5, 5, 5)));
        assert_eq!(a.union(&b), IRect::new(0, 0, 15, 15));
        assert_eq!(a.intersect(&IRect::new(10, 0, 5, 5)), None);
        assert_eq!(IRect::default().union(&b), b);
    }
}
