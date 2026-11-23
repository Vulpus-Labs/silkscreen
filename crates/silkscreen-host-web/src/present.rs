//! Frame → surface, over damage rects. Target-independent.

use silkscreen::IRect;
use silkscreen::core::tiny_skia::Pixmap;

/// Somewhere a frame can be shown a rect at a time: a canvas on the web.
pub trait Surface {
    type Error;

    /// Whether the surface's view of `frame`'s pixels is still valid. A
    /// view over wasm memory goes stale when the memory grows or the frame
    /// is reallocated.
    fn is_bound(&self, frame: &Pixmap) -> bool;

    /// (Re)create the surface's view of `frame`'s pixels.
    fn bind(&mut self, frame: &Pixmap) -> Result<(), Self::Error>;

    /// Show `rect` (in frame pixels, within the frame) from the bound view.
    fn put(&mut self, rect: IRect) -> Result<(), Self::Error>;
}

/// Presents a frame to a [`Surface`]: the whole frame after
/// [`invalidate`](Self::invalidate), otherwise only the damage.
#[derive(Debug)]
pub struct Presenter<S> {
    surface: S,
    full: bool,
}

impl<S: Surface> Presenter<S> {
    pub fn new(surface: S) -> Self {
        Self {
            surface,
            full: true,
        }
    }

    pub fn surface(&self) -> &S {
        &self.surface
    }

    /// The next present covers the whole frame: after a resize, or when the
    /// surface's own pixels were lost.
    pub fn invalidate(&mut self) {
        self.full = true;
    }

    /// Show `damage` (or the whole frame, if invalidated), rebinding the
    /// surface first if its view went stale. Returns the number of rects
    /// put; an idle frame puts none.
    pub fn present(&mut self, frame: &Pixmap, damage: &[IRect]) -> Result<usize, S::Error> {
        if !self.full && damage.is_empty() {
            return Ok(0);
        }
        if !self.surface.is_bound(frame) {
            self.surface.bind(frame)?;
        }
        let bounds = IRect::frame(frame.width(), frame.height());
        if self.full {
            self.surface.put(bounds)?;
            self.full = false;
            return Ok(1);
        }
        let mut puts = 0;
        for rect in damage {
            if let Some(rect) = rect.intersect(&bounds) {
                self.surface.put(rect)?;
                puts += 1;
            }
        }
        Ok(puts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records puts. Its view of a frame goes stale when `generation`
    /// moves on, as a view over wasm memory does when the memory grows.
    #[derive(Debug, Default)]
    struct Fake {
        generation: u32,
        bound: Option<(u32, (u32, u32))>,
        binds: usize,
        puts: Vec<IRect>,
    }

    impl Surface for Fake {
        type Error = ();

        fn is_bound(&self, frame: &Pixmap) -> bool {
            self.bound == Some((self.generation, (frame.width(), frame.height())))
        }

        fn bind(&mut self, frame: &Pixmap) -> Result<(), ()> {
            self.bound = Some((self.generation, (frame.width(), frame.height())));
            self.binds += 1;
            Ok(())
        }

        fn put(&mut self, rect: IRect) -> Result<(), ()> {
            self.puts.push(rect);
            Ok(())
        }
    }

    #[test]
    fn first_present_is_the_whole_frame_then_only_damage() {
        let frame = Pixmap::new(40, 30).unwrap();
        let mut p = Presenter::new(Fake::default());
        assert_eq!(p.present(&frame, &[IRect::new(1, 1, 2, 2)]), Ok(1));
        assert_eq!(p.surface().puts, [IRect::new(0, 0, 40, 30)]);
        let damage = [IRect::new(1, 2, 3, 4), IRect::new(10, 10, 5, 5)];
        assert_eq!(p.present(&frame, &damage), Ok(2));
        assert_eq!(p.surface().puts[1..], damage);
        assert_eq!(p.surface().binds, 1);
    }

    #[test]
    fn idle_frames_put_nothing() {
        let frame = Pixmap::new(8, 8).unwrap();
        let mut p = Presenter::new(Fake::default());
        p.present(&frame, &[]).unwrap();
        for _ in 0..10 {
            assert_eq!(p.present(&frame, &[]), Ok(0));
        }
        assert_eq!(p.surface().puts.len(), 1);
    }

    #[test]
    fn damage_is_clipped_to_the_frame() {
        let frame = Pixmap::new(10, 10).unwrap();
        let mut p = Presenter::new(Fake::default());
        p.present(&frame, &[]).unwrap();
        let damage = [IRect::new(-2, 8, 5, 5), IRect::new(20, 0, 4, 4)];
        assert_eq!(p.present(&frame, &damage), Ok(1));
        assert_eq!(p.surface().puts[1], IRect::new(0, 8, 3, 2));
    }

    #[test]
    fn a_stale_view_is_rebound_before_putting() {
        let frame = Pixmap::new(8, 8).unwrap();
        let mut p = Presenter::new(Fake::default());
        p.present(&frame, &[]).unwrap();
        // Memory grew: the view is stale, but the canvas still holds the
        // last frame, so only the damage is put.
        p.surface.generation += 1;
        assert_eq!(p.present(&frame, &[IRect::new(0, 0, 1, 1)]), Ok(1));
        assert_eq!(p.surface().binds, 2);
        assert_eq!(p.surface().puts[1], IRect::new(0, 0, 1, 1));
    }

    #[test]
    fn a_resized_frame_is_rebound_and_presented_whole() {
        let mut p = Presenter::new(Fake::default());
        p.present(&Pixmap::new(8, 8).unwrap(), &[]).unwrap();
        let bigger = Pixmap::new(16, 16).unwrap();
        p.invalidate();
        assert_eq!(p.present(&bigger, &[]), Ok(1));
        assert_eq!(p.surface().binds, 2);
        assert_eq!(p.surface().puts[1], IRect::new(0, 0, 16, 16));
    }
}
