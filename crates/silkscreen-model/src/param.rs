//! Parameter descriptions.

pub use silkscreen_bridge::ParamIndex;

/// How the normalized `[0, 1]` range maps onto the plain range of a
/// continuous param.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Taper {
    Linear,
    /// Linear, centred: the default is the middle of the range and widgets
    /// draw from there (e.g. a ±50 fine-tune).
    Bipolar,
    /// Exponential, pinned so the middle of travel reads `mid` and the top
    /// `max`: geometric between `min` and `mid` and between `mid` and
    /// `max` when `0 < min < mid < max`, otherwise one exponential through
    /// `(0, 0)`, `(0.5, mid)` and `(1, max)` (a floor of zero). A fader
    /// taper (design/model.md), for cutoffs and times.
    Exp {
        mid: f32,
    },
    /// [`Exp`](Self::Exp) mirrored about the centre of a symmetric range
    /// `-max..=max`: the centre reads 0, half travel each way `±mid`, the
    /// ends `±max`. Widgets draw it from the centre. `mid` must be below
    /// `max / 2`; otherwise it maps linearly.
    BipolarExp {
        mid: f32,
    },
}

/// What values a param takes ([design/model.md]).
///
/// [design/model.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/model.md
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParamKind {
    /// Any value in `min..=max`, mapped by the spec's [`Taper`].
    Continuous,
    /// Whole positions `0..=n − 1`, one per label (`min = 0`, `max = n − 1`).
    Stepped { labels: &'static [&'static str] },
    /// Off (`0`) or on (`1`). Shown as "Off"/"On" unless labelled.
    Toggle {
        labels: Option<&'static [&'static str; 2]>,
    },
}

impl ParamKind {
    /// Whether values are whole positions.
    pub const fn is_discrete(&self) -> bool {
        !matches!(self, ParamKind::Continuous)
    }
}

/// How a continuous param's plain value is shown: mapped into display
/// units, such as a slider position into hertz, and back for text typed
/// in. The two must be inverses over the param's range.
#[derive(Clone, Copy, Debug)]
pub struct Display {
    /// Plain value to the number shown.
    pub to_display: fn(f32) -> f32,
    /// A number typed in back to the plain value.
    pub from_display: fn(f32) -> f32,
}

impl PartialEq for Display {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::fn_addr_eq(self.to_display, other.to_display)
            && std::ptr::fn_addr_eq(self.from_display, other.from_display)
    }
}

/// One parameter: what the user sees and how the value maps to a control.
///
/// Values held by the model are always **plain** (in the param's own units).
/// Bindings and widgets work in normalized `[0, 1]` via [`normalize`] and
/// [`denormalize`].
///
/// [`normalize`]: ParamSpec::normalize
/// [`denormalize`]: ParamSpec::denormalize
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParamSpec {
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub taper: Taper,
    pub kind: ParamKind,
    /// Unit suffix for display, e.g. `"dB"`. Empty for none.
    pub unit: &'static str,
    /// Decimal places for display.
    pub decimals: u8,
    /// The mapping into display units, for a continuous param shown in
    /// other units than it holds; none to show the plain value.
    pub display: Option<Display>,
    /// Stable host-facing id (e.g. the CLAP param id), independent of the
    /// param's position in the table. [`ParamSpec::NO_ID`] until set with
    /// [`id`](Self::id). Hosts key saved projects and automation on it, so
    /// never change or reuse one once released.
    pub id: u32,
    /// Offered to the host for automation. False for patch state (e.g. a
    /// mod matrix's routing): saved, restored, undone and sent to the
    /// engine like any param, but hidden from the host's automation
    /// lists. See [`patch_state`](Self::patch_state).
    pub automatable: bool,
}

impl ParamSpec {
    /// The `id` of a spec that hasn't been given one.
    pub const NO_ID: u32 = u32::MAX;

    /// A linear param with no unit, shown to two decimal places.
    pub const fn new(name: &'static str, min: f32, max: f32, default: f32) -> Self {
        Self {
            name,
            min,
            max,
            default,
            taper: Taper::Linear,
            kind: ParamKind::Continuous,
            unit: "",
            decimals: 2,
            display: None,
            id: Self::NO_ID,
            automatable: true,
        }
    }

    /// A param with one position per label, starting at `default`.
    pub const fn stepped(
        name: &'static str,
        labels: &'static [&'static str],
        default: usize,
    ) -> Self {
        assert!(
            !labels.is_empty(),
            "a stepped param needs at least one label"
        );
        assert!(default < labels.len(), "default position out of range");
        let mut spec = Self::new(name, 0.0, (labels.len() - 1) as f32, default as f32);
        spec.kind = ParamKind::Stepped { labels };
        spec.decimals = 0;
        spec
    }

    /// An off/on param.
    pub const fn toggle(name: &'static str, default_on: bool) -> Self {
        let mut spec = Self::new(name, 0.0, 1.0, if default_on { 1.0 } else { 0.0 });
        spec.kind = ParamKind::Toggle { labels: None };
        spec.decimals = 0;
        spec
    }

    /// Name a toggle's two states, off first: `.labels(&["Down", "Up"])`.
    pub const fn labels(mut self, labels: &'static [&'static str; 2]) -> Self {
        assert!(
            matches!(self.kind, ParamKind::Toggle { .. }),
            "labels() is for toggles; stepped params take labels in `stepped`"
        );
        self.kind = ParamKind::Toggle {
            labels: Some(labels),
        };
        self
    }

    /// A continuous param centred on the middle of its range, which
    /// becomes the default.
    pub const fn bipolar(mut self) -> Self {
        self.taper = Taper::Bipolar;
        self.default = (self.min + self.max) * 0.5;
        self
    }

    /// Patch state rather than a performance control: hidden from host
    /// automation, otherwise an ordinary param (design/model.md).
    pub const fn patch_state(mut self) -> Self {
        self.automatable = false;
        self
    }

    /// Whether widgets draw it from its centre (see [`bipolar`](Self::bipolar)
    /// and [`Taper::BipolarExp`]).
    pub const fn is_bipolar(&self) -> bool {
        matches!(self.taper, Taper::Bipolar | Taper::BipolarExp { .. })
    }

    pub const fn id(mut self, id: u32) -> Self {
        self.id = id;
        self
    }

    pub const fn unit(mut self, unit: &'static str) -> Self {
        self.unit = unit;
        self
    }

    pub const fn decimals(mut self, decimals: u8) -> Self {
        self.decimals = decimals;
        self
    }

    pub const fn taper(mut self, taper: Taper) -> Self {
        self.taper = taper;
        self
    }

    /// Show the value through `to_display`, in [`unit`](Self::unit), and
    /// read typed text back through `from_display`.
    pub const fn display(
        mut self,
        to_display: fn(f32) -> f32,
        from_display: fn(f32) -> f32,
    ) -> Self {
        self.display = Some(Display {
            to_display,
            from_display,
        });
        self
    }

    /// Into range, and onto the nearest position for discrete kinds. Every
    /// value the model accepts passes through here.
    pub fn clamp(&self, plain: f32) -> f32 {
        let v = plain.clamp(self.min, self.max);
        if self.kind.is_discrete() {
            v.round()
        } else {
            v
        }
    }

    /// Plain → `[0, 1]`, clamped (and snapped, for discrete kinds).
    pub fn normalize(&self, plain: f32) -> f32 {
        let span = self.max - self.min;
        if span <= 0.0 {
            return 0.0;
        }
        let linear = |v: f32| ((v - self.min) / span).clamp(0.0, 1.0);
        let v = self.clamp(plain);
        match self.taper {
            Taper::Linear | Taper::Bipolar => linear(v),
            Taper::Exp { mid } => {
                if self.min > 0.0 && mid > self.min && self.max > mid {
                    if v <= mid {
                        0.5 * (v / self.min).ln() / (mid / self.min).ln()
                    } else {
                        0.5 + 0.5 * (v / mid).ln() / (self.max / mid).ln()
                    }
                } else {
                    let (a, k) = exp_from_zero(mid, self.max);
                    ((v / a + 1.0).ln() / k).clamp(0.0, 1.0)
                }
            }
            Taper::BipolarExp { mid } => match bipolar_exp(mid, self.max) {
                Some((a, k)) => {
                    let t = ((v.abs() / a + 1.0).ln() / k).clamp(0.0, 1.0);
                    if v < 0.0 {
                        0.5 - 0.5 * t
                    } else {
                        0.5 + 0.5 * t
                    }
                }
                None => linear(v),
            },
        }
    }

    /// `[0, 1]` → plain, clamped (and snapped, for discrete kinds).
    pub fn denormalize(&self, normalized: f32) -> f32 {
        let n = normalized.clamp(0.0, 1.0);
        let linear = self.min + n * (self.max - self.min);
        let plain = match self.taper {
            Taper::Linear | Taper::Bipolar => linear,
            Taper::Exp { mid } => {
                if self.min > 0.0 && mid > self.min && self.max > mid {
                    if n <= 0.5 {
                        self.min * (mid / self.min).powf(2.0 * n)
                    } else {
                        mid * (self.max / mid).powf(2.0 * n - 1.0)
                    }
                } else {
                    let (a, k) = exp_from_zero(mid, self.max);
                    a * ((k * n).exp() - 1.0)
                }
            }
            Taper::BipolarExp { mid } => match bipolar_exp(mid, self.max) {
                Some((a, k)) => {
                    let magnitude = (a * ((k * (2.0 * n - 1.0).abs()).exp() - 1.0)).min(self.max);
                    if n < 0.5 { -magnitude } else { magnitude }
                }
                None => linear,
            },
        };
        self.clamp(plain)
    }

    /// The label of each position, for discrete kinds; empty for
    /// continuous params.
    pub fn position_labels(&self) -> &'static [&'static str] {
        const OFF_ON: [&str; 2] = ["Off", "On"];
        match &self.kind {
            ParamKind::Continuous => &[],
            ParamKind::Stepped { labels } => labels,
            ParamKind::Toggle { labels: None } => &OFF_ON,
            ParamKind::Toggle { labels: Some(l) } => *l,
        }
    }

    /// Parse text such as `"-6"`, `"-6.0 dB"` or `" +3dB"` to a plain
    /// value, clamped. Leading whitespace, a sign, a decimal number and
    /// any trailing unit are accepted.
    ///
    /// Discrete params also accept a position's label, ignoring case.
    pub fn parse(&self, text: &str) -> Option<f32> {
        let trimmed = text.trim();
        if let Some(i) = self
            .position_labels()
            .iter()
            .position(|l| l.eq_ignore_ascii_case(trimmed))
        {
            return Some(i as f32);
        }
        let t = text.trim_start();
        let end = t
            .char_indices()
            .find(|&(i, c)| !(c.is_ascii_digit() || c == '.' || (i == 0 && (c == '-' || c == '+'))))
            .map_or(t.len(), |(i, _)| i);
        let number = match t[..end].parse::<f32>() {
            Ok(v) if v.is_finite() => v,
            // `format` shows a display's infinite ends as `inf` and `-inf`
            // (DECAY at SINGING, GAIN at 0); they must parse back.
            _ if self.display.is_some() => infinity(&t[..end], &t[end..])?,
            _ => return None,
        };
        let plain = match self.display {
            Some(d) if !self.kind.is_discrete() => (d.from_display)(number),
            _ => number,
        };
        plain.is_finite().then(|| self.clamp(plain))
    }

    /// The value as text, e.g. `"-6.00 dB"`, or a discrete param's label.
    pub fn format(&self, plain: f32) -> String {
        if self.kind.is_discrete()
            && let Some(label) = self.position_labels().get(self.clamp(plain) as usize)
        {
            return (*label).to_owned();
        }
        let shown = match self.display {
            Some(d) => (d.to_display)(self.clamp(plain)),
            None => plain,
        };
        let d = usize::from(self.decimals);
        if self.unit.is_empty() {
            format!("{shown:.d$}")
        } else {
            format!("{shown:.d$} {}", self.unit)
        }
    }
}

/// `(a, k)` of [`Taper::Exp`]'s curve from zero, `a·(exp(k·n) − 1)`,
/// through `(0.5, mid)` and `(1, max)`.
fn exp_from_zero(mid: f32, max: f32) -> (f32, f32) {
    let r = max / mid - 1.0;
    (mid / (r - 1.0), 2.0 * r.ln())
}

/// `(a, k)` of [`Taper::BipolarExp`]'s magnitude `a·(exp(k·t) − 1)` over
/// travel `t` from the centre, through `(0.5, mid)` and `(1, max)`; none
/// unless `0 < mid < max / 2`, where the pinning has a rising solution.
fn bipolar_exp(mid: f32, max: f32) -> Option<(f32, f32)> {
    if !(mid > 0.0 && max > 0.0 && mid < max * 0.5) {
        return None;
    }
    let r = max / mid - 1.0;
    Some((mid / (r - 1.0), 2.0 * r.ln()))
}

/// `±inf` from a parsed sign (`""`, `"+"` or `"-"`) and the text after it,
/// if that text starts with `inf` (any case).
fn infinity(sign: &str, rest: &str) -> Option<f32> {
    let inf = rest.get(..3)?.eq_ignore_ascii_case("inf");
    match (sign, inf) {
        ("" | "+", true) => Some(f32::INFINITY),
        ("-", true) => Some(f32::NEG_INFINITY),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GAIN: ParamSpec = ParamSpec::new("Gain", -60.0, 12.0, 0.0)
        .unit("dB")
        .decimals(1);

    #[test]
    fn normalize_round_trips() {
        for plain in [-60.0, -24.0, 0.0, 6.0, 12.0] {
            let n = GAIN.normalize(plain);
            assert!((GAIN.denormalize(n) - plain).abs() < 1e-4, "{plain}");
        }
        assert_eq!(GAIN.normalize(-60.0), 0.0);
        assert_eq!(GAIN.normalize(12.0), 1.0);
        assert!((GAIN.normalize(-24.0) - 0.5).abs() < 1e-6);
    }

    const EXP_CUTOFF: ParamSpec =
        ParamSpec::new("Cutoff", 16.3516, 16000.0, 1000.0).taper(Taper::Exp { mid: 800.0 });
    const GLIDE: ParamSpec = ParamSpec::new("Glide", 0.0, 0.5, 0.0).taper(Taper::Exp { mid: 0.1 });
    const DETUNE: ParamSpec =
        ParamSpec::new("Detune", -50.0, 50.0, 0.0).taper(Taper::BipolarExp { mid: 20.0 });

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= 1e-4 * b.abs().max(1.0)
    }

    #[test]
    fn exp_taper_is_pinned_at_its_ends_and_middle() {
        for (spec, mid) in [(EXP_CUTOFF, 800.0), (GLIDE, 0.1)] {
            assert!(close(spec.normalize(spec.min), 0.0), "{}", spec.name);
            assert!(close(spec.normalize(mid), 0.5), "{}", spec.name);
            assert!(close(spec.normalize(spec.max), 1.0), "{}", spec.name);
            assert!(close(spec.denormalize(0.5), mid), "{}", spec.name);
            assert!(close(spec.denormalize(1.0), spec.max), "{}", spec.name);
        }
    }

    #[test]
    fn bipolar_exp_taper_is_centred_and_pinned_each_way() {
        assert!(DETUNE.is_bipolar());
        assert_eq!(DETUNE.normalize(0.0), 0.5);
        assert!(close(DETUNE.normalize(20.0), 0.75));
        assert!(close(DETUNE.normalize(-20.0), 0.25));
        assert!(close(DETUNE.denormalize(1.0), 50.0));
        assert!(close(DETUNE.denormalize(0.0), -50.0));
    }

    #[test]
    fn exp_tapers_round_trip() {
        for spec in [EXP_CUTOFF, GLIDE, DETUNE] {
            for i in 0..=100 {
                let n = i as f32 / 100.0;
                let back = spec.normalize(spec.denormalize(n));
                assert!((back - n).abs() < 1e-4, "{} at {n}: {back}", spec.name);
            }
        }
    }

    #[test]
    fn bipolar_exp_without_a_solution_is_linear() {
        let spec = ParamSpec::new("Wide", -10.0, 10.0, 0.0).taper(Taper::BipolarExp { mid: 6.0 });
        assert!(close(spec.normalize(5.0), 0.75));
        assert!(close(spec.denormalize(0.25), -5.0));
    }

    #[test]
    fn out_of_range_clamps() {
        assert_eq!(GAIN.normalize(-100.0), 0.0);
        assert_eq!(GAIN.normalize(100.0), 1.0);
        assert_eq!(GAIN.denormalize(-1.0), -60.0);
        assert_eq!(GAIN.denormalize(2.0), 12.0);
    }

    #[test]
    fn degenerate_range_does_not_divide_by_zero() {
        let p = ParamSpec::new("Fixed", 1.0, 1.0, 1.0);
        assert_eq!(p.normalize(1.0), 0.0);
        assert_eq!(p.denormalize(0.5), 1.0);
    }

    /// A slider position shown in hertz, exponential 20 Hz to 20 kHz.
    const CUTOFF: ParamSpec = ParamSpec::new("Cutoff", 0.0, 1.0, 1.0)
        .unit("Hz")
        .decimals(0)
        .display(|p| 20.0 * 1000f32.powf(p), |hz| (hz / 20.0).log10() / 3.0);

    #[test]
    fn a_display_mapping_formats_and_parses_in_its_units() {
        assert_eq!(CUTOFF.format(0.0), "20 Hz");
        assert_eq!(CUTOFF.format(1.0), "20000 Hz");
        assert_eq!(CUTOFF.format(2.0), "20000 Hz", "clamped before mapping");
        let half = CUTOFF.parse("632 Hz").unwrap();
        assert!((half - 0.5).abs() < 1e-3, "{half}");
        assert_eq!(
            CUTOFF.parse("1e9"),
            Some(0.0),
            "a stray number: 1 Hz, clamped"
        );
        assert_eq!(CUTOFF.parse("-5"), None, "no plain value maps from it");
        assert_eq!(CUTOFF, CUTOFF);
    }

    /// A linear level shown in decibels: −∞ at 0.
    const LEVEL: ParamSpec = ParamSpec::new("Level", 0.0, 2.0, 1.0)
        .unit("dB")
        .decimals(1)
        .display(|g| 20.0 * g.log10(), |db| 10f32.powf(db / 20.0));

    #[test]
    fn a_displays_infinite_ends_round_trip() {
        assert_eq!(LEVEL.format(0.0), "-inf dB");
        assert_eq!(LEVEL.parse("-inf dB"), Some(0.0));
        assert_eq!(LEVEL.parse(" -INF"), Some(0.0));
        assert_eq!(LEVEL.parse("inf"), None, "+inf dB is no level");
        assert_eq!(LEVEL.parse("-info"), Some(0.0), "only the prefix counts");
        assert_eq!(LEVEL.parse("-in"), None);
        assert_eq!(CUTOFF.parse("-inf"), None, "no plain value maps from it");
        assert_eq!(GAIN.parse("inf"), None, "no display: no infinite value");
    }

    #[test]
    fn parses_numbers_with_or_without_units() {
        assert_eq!(GAIN.parse("-6"), Some(-6.0));
        assert_eq!(GAIN.parse("  -6.5 dB"), Some(-6.5));
        assert_eq!(GAIN.parse("+3dB"), Some(3.0));
        assert_eq!(GAIN.parse("100"), Some(12.0), "clamped");
        assert_eq!(GAIN.parse("loud"), None);
        assert_eq!(GAIN.parse(""), None);
    }

    #[test]
    fn ids_default_to_unset() {
        assert_eq!(GAIN.id, ParamSpec::NO_ID);
        assert_eq!(GAIN.id(7).id, 7);
    }

    const WAVE: ParamSpec =
        ParamSpec::stepped("Wave", &["Saw", "Square", "Pulse", "Tri", "Sine"], 1);
    const STOP: ParamSpec = ParamSpec::toggle("Stop", false);
    const LONG: ParamSpec = ParamSpec::toggle("Long", true).labels(&["Short", "Long"]);
    const FINE: ParamSpec = ParamSpec::new("Fine", -50.0, 50.0, 10.0).bipolar();

    #[test]
    fn stepped_params_snap_everywhere() {
        assert_eq!((WAVE.min, WAVE.max, WAVE.default), (0.0, 4.0, 1.0));
        assert_eq!(WAVE.clamp(2.4), 2.0);
        assert_eq!(WAVE.clamp(2.6), 3.0);
        assert_eq!(WAVE.clamp(9.0), 4.0);
        assert_eq!(WAVE.denormalize(0.6), 2.0);
        assert_eq!(WAVE.denormalize(0.65), 3.0);
        assert_eq!(WAVE.normalize(2.4), 0.5);
        assert_eq!(WAVE.normalize(3.0), 0.75);
    }

    #[test]
    fn stepped_params_format_and_parse_labels() {
        assert_eq!(WAVE.format(3.0), "Tri");
        assert_eq!(WAVE.format(0.2), "Saw");
        assert_eq!(WAVE.parse("pulse"), Some(2.0));
        assert_eq!(WAVE.parse(" SINE "), Some(4.0));
        assert_eq!(WAVE.parse("1"), Some(1.0));
        assert_eq!(WAVE.parse("2.7"), Some(3.0), "numbers snap too");
        assert_eq!(WAVE.parse("noise"), None);
    }

    #[test]
    fn toggles() {
        assert_eq!((STOP.min, STOP.max, STOP.default), (0.0, 1.0, 0.0));
        assert_eq!(LONG.default, 1.0);
        assert_eq!(STOP.clamp(0.4), 0.0);
        assert_eq!(STOP.clamp(0.6), 1.0);
        assert_eq!(
            (STOP.format(0.0), STOP.format(1.0)),
            ("Off".into(), "On".into())
        );
        assert_eq!(
            (LONG.format(0.0), LONG.format(1.0)),
            ("Short".into(), "Long".into())
        );
        assert_eq!(STOP.parse("on"), Some(1.0));
        assert_eq!(LONG.parse("short"), Some(0.0));
        assert!(STOP.kind.is_discrete() && !GAIN.kind.is_discrete());
    }

    #[test]
    fn bipolar_defaults_to_its_centre() {
        assert_eq!(FINE.default, 0.0);
        assert_eq!(FINE.taper, Taper::Bipolar);
        assert_eq!(FINE.normalize(0.0), 0.5);
        assert_eq!(FINE.denormalize(0.75), 25.0);
        assert_eq!(FINE.clamp(12.3), 12.3, "continuous: no snapping");
    }

    #[test]
    fn continuous_is_still_the_default_kind() {
        assert_eq!(GAIN.kind, ParamKind::Continuous);
        assert!(GAIN.position_labels().is_empty());
    }

    #[test]
    fn formats_with_unit_and_decimals() {
        assert_eq!(GAIN.format(-6.0), "-6.0 dB");
        assert_eq!(ParamSpec::new("Mix", 0.0, 1.0, 0.5).format(0.25), "0.25");
    }
}
