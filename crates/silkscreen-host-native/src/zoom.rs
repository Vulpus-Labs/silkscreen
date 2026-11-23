//! Discrete zoom steps ([design/rendering.md]).
//!
//! [design/rendering.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/rendering.md

use silkscreen::core::Size;

/// The zoom steps editors offer, smallest first.
pub const ZOOM_STEPS: [f32; 5] = [0.75, 1.0, 1.25, 1.5, 2.0];

/// The step nearest `zoom`.
pub fn nearest_zoom(zoom: f32) -> f32 {
    ZOOM_STEPS
        .into_iter()
        .min_by(|a, b| (a - zoom).abs().total_cmp(&(b - zoom).abs()))
        .expect("non-empty")
}

/// The step for a window of `w` × `h` logical units showing a faceplate
/// of logical size `base`. The two axes' ratios are combined by geometric
/// mean: exact when the aspect is kept, and still responsive when a host
/// drags only one edge.
pub fn zoom_for_size(base: Size, w: f64, h: f64) -> f32 {
    let rw = w / f64::from(base.w);
    let rh = h / f64::from(base.h);
    let fit = (rw * rh).sqrt();
    nearest_zoom(if fit.is_finite() { fit as f32 } else { 1.0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snaps_to_nearest_step() {
        assert_eq!(nearest_zoom(1.0), 1.0);
        assert_eq!(nearest_zoom(1.1), 1.0);
        assert_eq!(nearest_zoom(1.2), 1.25);
        assert_eq!(nearest_zoom(0.1), 0.75);
        assert_eq!(nearest_zoom(9.0), 2.0);
    }

    #[test]
    fn sizes_snap_by_both_axes() {
        let base = Size::new(200.0, 100.0);
        assert_eq!(zoom_for_size(base, 200.0, 100.0), 1.0);
        assert_eq!(zoom_for_size(base, 300.0, 150.0), 1.5);
        assert_eq!(zoom_for_size(base, 390.0, 195.0), 2.0);
        assert_eq!(zoom_for_size(base, 10.0, 5.0), 0.75);
        // One edge dragged: 1.6 × 1.0 → mean ≈ 1.26.
        assert_eq!(zoom_for_size(base, 320.0, 100.0), 1.25);
        assert_eq!(zoom_for_size(base, 0.0, 0.0), 0.75);
        assert_eq!(zoom_for_size(base, f64::NAN, 1.0), 1.0);
    }
}
