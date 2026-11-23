//! Live displays: a triggered scope and a peak meter ([design/engine-bridge.md]).
//!
//! These widgets draw data the editor feeds them on `tick`, shared through
//! an `Rc`, not [`WidgetState`]. Where the data comes from (an engine's
//! output) is the editor's business; this module only searches for the
//! trigger, keeps the meter's ballistics and draws.
//!
//! [design/engine-bridge.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/engine-bridge.md

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use tiny_skia::{Color, LineCap, LineJoin, Paint, PathBuilder, PixmapMut, Stroke, Transform};

use crate::geom::Rect;
use crate::widget::{Widget, WidgetState};

/// Samples a scope draws across its rect: about 21 ms at 48 kHz, a period
/// and a half of C2.
pub const SCOPE_WINDOW: usize = 1024;

/// Where to start drawing `samples` (oldest first): the latest rising zero
/// crossing that leaves a whole `window` after it, armed only once the
/// signal has gone below `-hysteresis`, so noise doesn't trigger. With no
/// such crossing (silence, a slow sweep), the latest window.
pub fn trigger(samples: &[f32], window: usize, hysteresis: f32) -> usize {
    let latest = samples.len().saturating_sub(window);
    let mut armed = false;
    let mut found = None;
    for i in 1..=latest {
        if samples[i - 1] < -hysteresis {
            armed = true;
        }
        if armed && samples[i - 1] < 0.0 && samples[i] >= 0.0 {
            found = Some(i);
            armed = false;
        }
    }
    found.unwrap_or(latest)
}

/// The window a [`Scope`] draws, shared with whoever feeds it.
pub type ScopeTrace = Rc<RefCell<Vec<f32>>>;

/// How a [`Scope`] looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScopeStyle {
    /// The value that reaches the rect's top (and its negative the
    /// bottom). Larger values are clipped.
    pub full_scale: f32,
    /// A rising edge must come up from below minus this to trigger.
    pub hysteresis: f32,
    pub wave: Color,
    /// Logical units.
    pub wave_width: f32,
    /// A line across the middle, drawn over the wave so a wave at rest
    /// passes behind it: (colour, logical width).
    pub axis: Option<(Color, f32)>,
    /// The least time between redraws: 0 redraws on every tick that
    /// brings new samples. A slower refresh reads steadier.
    pub refresh_ms: f64,
}

impl Default for ScopeStyle {
    fn default() -> Self {
        Self {
            full_scale: 1.0,
            hysteresis: 0.01,
            wave: Color::from_rgba8(0xe2, 0x26, 0x2e, 0xff),
            wave_width: 1.0,
            axis: None,
            refresh_ms: 0.0,
        }
    }
}

/// A triggered waveform display.
#[derive(Debug)]
pub struct Scope {
    pub style: ScopeStyle,
    trace: ScopeTrace,
}

impl Scope {
    pub fn new(style: ScopeStyle, trace: ScopeTrace) -> Self {
        Self { style, trace }
    }
}

impl Default for Scope {
    /// A flat line, fed by nothing.
    fn default() -> Self {
        Self::new(ScopeStyle::default(), ScopeTrace::default())
    }
}

impl Widget for Scope {
    fn draw(&self, canvas: &mut PixmapMut<'_>, rect: Rect, transform: Transform, _: &WidgetState) {
        let s = &self.style;
        let mid = rect.center().y;
        let reach = (rect.h / 2.0 - s.wave_width).max(0.0);
        let per_unit = reach / s.full_scale;
        let trace = self.trace.borrow();
        let n = trace.len().max(2);
        let dx = rect.w / (n - 1) as f32;
        let mut pb = PathBuilder::new();
        let y = |v: f32| mid - v.clamp(-s.full_scale, s.full_scale) * per_unit;
        if trace.is_empty() {
            pb.move_to(rect.x, mid);
            pb.line_to(rect.right(), mid);
        } else {
            pb.move_to(rect.x, y(trace[0]));
            for (i, &v) in trace.iter().enumerate().skip(1) {
                pb.line_to(rect.x + i as f32 * dx, y(v));
            }
        }
        let mut paint = Paint {
            anti_alias: true,
            ..Paint::default()
        };
        if let Some(path) = pb.finish() {
            paint.set_color(s.wave);
            let stroke = Stroke {
                width: s.wave_width,
                line_cap: LineCap::Round,
                line_join: LineJoin::Round,
                ..Stroke::default()
            };
            canvas.stroke_path(&path, &paint, &stroke, transform, None);
        }
        if let Some((color, width)) = s.axis {
            let mut pb = PathBuilder::new();
            pb.move_to(rect.x, mid);
            pb.line_to(rect.right(), mid);
            if let Some(path) = pb.finish() {
                paint.set_color(color);
                let stroke = Stroke {
                    width,
                    ..Stroke::default()
                };
                canvas.stroke_path(&path, &paint, &stroke, transform, None);
            }
        }
    }
}

/// Channels a [`Meter`] draws.
pub const METER_CHANNELS: usize = 2;

/// How a [`Meter`] looks and moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeterStyle {
    /// The scale, in dBFS: the foot and the top.
    pub floor_db: f32,
    pub ceiling_db: f32,
    /// Segments per bar, and the gap between them and between bars
    /// (logical units).
    pub segments: u16,
    pub gap: f32,
    /// A lit segment's colour by its level, (dBFS, colour) in rising
    /// order: the first below the first stop, the last above the last,
    /// blended between.
    pub stops: [(f32, Color); 3],
    /// What an unlit segment's colour is dimmed towards, and how much of
    /// its lit colour shows (0..1).
    pub unlit: Color,
    pub unlit_tint: f32,
    /// How long the peak-hold tick stays before it falls.
    pub hold_ms: f64,
    /// How fast the bar falls.
    pub fall_db_per_s: f32,
    /// How fast the hold tick falls once its hold is over: faster than
    /// the bar rejoins a sustained level (40 dB/s against 20, say).
    pub hold_fall_db_per_s: f32,
}

impl MeterStyle {
    /// A lit segment's colour at `db`.
    pub fn colour_at(&self, db: f32) -> Color {
        let (first, last) = (self.stops[0], self.stops[self.stops.len() - 1]);
        if db <= first.0 {
            return first.1;
        }
        for pair in self.stops.windows(2) {
            let ((a_db, a), (b_db, b)) = (pair[0], pair[1]);
            if db <= b_db {
                return mix(a, b, (db - a_db) / (b_db - a_db));
            }
        }
        last.1
    }

    /// An unlit segment's colour at `db`.
    pub fn unlit_at(&self, db: f32) -> Color {
        mix(self.unlit, self.colour_at(db), self.unlit_tint)
    }
}

/// `a` blended `t` of the way to `b`.
fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let f = |x: f32, y: f32| x + (y - x) * t;
    Color::from_rgba(
        f(a.red(), b.red()),
        f(a.green(), b.green()),
        f(a.blue(), b.blue()),
        f(a.alpha(), b.alpha()),
    )
    .unwrap_or(a)
}

impl Default for MeterStyle {
    /// −48 to +6 dBFS in 27 segments of 2 dB: green to −18, through
    /// amber at −6 to red at 0 and over.
    fn default() -> Self {
        Self {
            floor_db: -48.0,
            ceiling_db: 6.0,
            segments: 27,
            gap: 1.0,
            stops: [
                (-18.0, Color::from_rgba8(0x2f, 0xc4, 0x4b, 0xff)),
                (-6.0, Color::from_rgba8(0xf2, 0xa5, 0x1a, 0xff)),
                (0.0, Color::from_rgba8(0xe2, 0x26, 0x2e, 0xff)),
            ],
            unlit: Color::from_rgba8(0x14, 0x12, 0x13, 0xff),
            unlit_tint: 0.18,
            hold_ms: 1500.0,
            fall_db_per_s: 24.0,
            hold_fall_db_per_s: 24.0,
        }
    }
}

/// What a [`Meter`] shows, per channel, in dBFS.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeterReading {
    /// Bars drawn: 0 before any output.
    pub channels: usize,
    pub level_db: [f32; METER_CHANNELS],
    pub hold_db: [f32; METER_CHANNELS],
}

impl Default for MeterReading {
    fn default() -> Self {
        Self {
            channels: 0,
            level_db: [f32::NEG_INFINITY; METER_CHANNELS],
            hold_db: [f32::NEG_INFINITY; METER_CHANNELS],
        }
    }
}

/// Turns peaks into a [`MeterReading`] over time: the bar jumps up to a
/// peak and falls; the hold tick stays at the highest peak for
/// `hold_ms`, then falls.
#[derive(Clone, Debug, Default)]
pub struct Ballistics {
    reading: MeterReading,
    /// When each channel's hold was last raised.
    held_at: [f64; METER_CHANNELS],
    last_ms: Option<f64>,
}

impl Ballistics {
    /// Take the linear peaks since the last update, at `now_ms`. Returns
    /// whether the reading changed enough to redraw.
    pub fn update(
        &mut self,
        channels: usize,
        peaks: [f32; METER_CHANNELS],
        now_ms: f64,
        style: &MeterStyle,
    ) -> bool {
        let dt = self.last_ms.map_or(0.0, |t| ((now_ms - t) / 1000.0) as f32);
        self.last_ms = Some(now_ms);
        let before = self.reading;
        let fall = style.fall_db_per_s * dt;
        let r = &mut self.reading;
        r.channels = r.channels.max(channels.min(METER_CHANNELS));
        for (c, linear) in peaks.iter().enumerate().take(r.channels) {
            let peak = 20.0 * linear.log10();
            r.level_db[c] = peak.max(r.level_db[c] - fall);
            if peak >= r.hold_db[c] {
                r.hold_db[c] = peak;
                self.held_at[c] = now_ms;
            } else if now_ms - self.held_at[c] > style.hold_ms {
                // Only for the time past the hold's end, however the
                // updates fall.
                let past = ((now_ms - self.held_at[c] - style.hold_ms) / 1000.0) as f32;
                let fall = style.hold_fall_db_per_s * past.min(dt);
                r.hold_db[c] = (r.hold_db[c] - fall).max(r.level_db[c]);
            }
            // Below the floor is all the same: off the scale.
            for v in [&mut r.level_db[c], &mut r.hold_db[c]] {
                if *v < style.floor_db - 1.0 {
                    *v = f32::NEG_INFINITY;
                }
            }
        }
        // Both off the scale (−∞ − −∞) is NaN: not moved.
        let moved = |a: f32, b: f32| (a - b).abs() >= 0.05;
        r.channels != before.channels
            || (0..METER_CHANNELS).any(|c| {
                moved(r.level_db[c], before.level_db[c]) || moved(r.hold_db[c], before.hold_db[c])
            })
    }

    pub fn reading(&self) -> MeterReading {
        self.reading
    }
}

/// A segmented peak meter: one vertical bar per channel, side by side.
#[derive(Debug)]
pub struct Meter {
    pub style: MeterStyle,
    reading: Rc<Cell<MeterReading>>,
}

impl Meter {
    pub fn new(style: MeterStyle, reading: Rc<Cell<MeterReading>>) -> Self {
        Self { style, reading }
    }
}

impl Default for Meter {
    /// Empty, fed by nothing.
    fn default() -> Self {
        Self::new(MeterStyle::default(), Rc::default())
    }
}

impl Widget for Meter {
    fn draw(&self, canvas: &mut PixmapMut<'_>, rect: Rect, transform: Transform, _: &WidgetState) {
        let style = &self.style;
        let reading = self.reading.get();
        let bars = METER_CHANNELS;
        let bar_w = (rect.w - style.gap * (bars - 1) as f32) / bars as f32;
        let segments = style.segments.max(1);
        let seg_h = (rect.h - style.gap * f32::from(segments - 1)) / f32::from(segments);
        let step = (style.ceiling_db - style.floor_db) / f32::from(segments);
        let mut paint = Paint {
            anti_alias: true,
            ..Paint::default()
        };
        for b in 0..bars {
            // A mono output lights both bars alike.
            let c = if reading.channels == 1 { 0 } else { b };
            let shown = c < reading.channels;
            let x = rect.x + b as f32 * (bar_w + style.gap);
            for i in 0..segments {
                let low = style.floor_db + f32::from(i) * step;
                let high = low + step;
                let lit = shown
                    && (reading.level_db[c] > low
                        || (reading.hold_db[c] > low && reading.hold_db[c] <= high));
                let middle = f32::midpoint(low, high);
                let color = if lit {
                    style.colour_at(middle)
                } else {
                    style.unlit_at(middle)
                };
                paint.set_color(color);
                let y = rect.bottom() - f32::from(i + 1) * seg_h - f32::from(i) * style.gap;
                if let Some(seg) = tiny_skia::Rect::from_xywh(x, y, bar_w, seg_h) {
                    canvas.fill_rect(seg, &paint, transform, None);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(len: usize, period: f64, phase: f64, amp: f32) -> Vec<f32> {
        (0..len)
            .map(|i| amp * (2.0 * std::f64::consts::PI * (i as f64 + phase) / period).sin() as f32)
            .collect()
    }

    #[test]
    fn a_steady_note_starts_on_a_rising_zero_crossing() {
        let s = sine(4096, 48_000.0 / 220.0, 0.3, 0.8);
        let start = trigger(&s, SCOPE_WINDOW, 0.01);
        assert!(start <= 4096 - SCOPE_WINDOW, "a whole window after it");
        assert!(
            s[start - 1] < 0.0 && s[start] >= 0.0,
            "on the way up through zero"
        );
    }

    #[test]
    fn the_start_is_the_latest_crossing_that_leaves_a_window() {
        let period = 48_000.0 / 110.0;
        let s = sine(4096, period, 0.0, 1.0);
        let start = trigger(&s, SCOPE_WINDOW, 0.01);
        let latest = 4096 - SCOPE_WINDOW;
        assert!(start as f64 > latest as f64 - period - 1.0, "{start}");
    }

    #[test]
    fn silence_free_runs() {
        let s = vec![0.0; 4096];
        assert_eq!(trigger(&s, SCOPE_WINDOW, 0.01), 4096 - SCOPE_WINDOW);
    }

    #[test]
    fn noise_under_the_hysteresis_does_not_trigger() {
        let s: Vec<f32> = (0..4096)
            .map(|i| if i % 2 == 0 { 0.001 } else { -0.001 })
            .collect();
        assert_eq!(trigger(&s, SCOPE_WINDOW, 0.01), 4096 - SCOPE_WINDOW);
    }

    #[test]
    fn segments_run_green_through_amber_to_red() {
        let style = MeterStyle::default();
        let green = style.stops[0].1;
        assert_eq!(style.colour_at(-40.0), green);
        let amber = style.colour_at(-6.0);
        assert!(amber.red() > 0.9 && amber.green() > 0.6 && amber.blue() < 0.2);
        let between = style.colour_at(-12.0);
        assert!(between.green() > amber.green() && between.red() < amber.red());
        assert_eq!(style.colour_at(3.0), style.stops[2].1);
        let unlit = style.unlit_at(-40.0);
        assert!(
            unlit.green() > unlit.red() && unlit.green() < 0.3,
            "dim green"
        );
    }

    #[test]
    fn the_bar_jumps_to_a_peak_and_falls() {
        let style = MeterStyle::default();
        let mut b = Ballistics::default();
        assert!(b.update(2, [1.0, 0.5], 0.0, &style));
        let r = b.reading();
        assert_eq!(r.channels, 2);
        assert!((r.level_db[0] - 0.0).abs() < 1e-4);
        assert!((r.level_db[1] + 6.0206).abs() < 1e-3);
        // Half a second of silence: down 12 dB.
        assert!(b.update(2, [0.0, 0.0], 500.0, &style));
        assert!((b.reading().level_db[0] + 12.0).abs() < 1e-3);
    }

    #[test]
    fn the_hold_stays_then_falls() {
        let style = MeterStyle::default();
        let mut b = Ballistics::default();
        b.update(1, [1.0, 0.0], 0.0, &style);
        b.update(1, [0.0, 0.0], 1000.0, &style);
        assert_eq!(b.reading().hold_db[0], 0.0, "held");
        b.update(1, [0.0, 0.0], 1600.0, &style);
        let hold = b.reading().hold_db[0];
        assert!(
            hold < 0.0 && hold >= b.reading().level_db[0],
            "falling: {hold}"
        );
    }

    #[test]
    fn a_released_hold_falls_at_its_own_rate() {
        let style = MeterStyle {
            floor_db: -60.0,
            hold_ms: 1000.0,
            fall_db_per_s: 20.0,
            hold_fall_db_per_s: 40.0,
            ..MeterStyle::default()
        };
        let mut b = Ballistics::default();
        b.update(1, [1.0, 0.0], 0.0, &style);
        b.update(1, [0.0, 0.0], 1000.0, &style);
        // 0.25 s past the hold: the bar is 25 dB down, the hold 10.
        b.update(1, [0.0, 0.0], 1250.0, &style);
        let r = b.reading();
        assert!((r.level_db[0] + 25.0).abs() < 1e-3, "{r:?}");
        assert!((r.hold_db[0] + 10.0).abs() < 1e-3, "{r:?}");
    }

    #[test]
    fn silence_settles_and_stops_redrawing() {
        let style = MeterStyle::default();
        let mut b = Ballistics::default();
        b.update(2, [0.5, 0.5], 0.0, &style);
        let mut t = 0.0;
        while b.update(2, [0.0, 0.0], t + 50.0, &style) {
            t += 50.0;
            assert!(t < 10_000.0, "never settled");
        }
        assert_eq!(b.reading().level_db, [f32::NEG_INFINITY; 2]);
        assert_eq!(b.reading().hold_db, [f32::NEG_INFINITY; 2]);
    }
}
