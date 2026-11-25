//! The soft lock ([design/licensing-hook.md]): a plugin that sets
//! [`SilkscreenPlugin::LICENCE`] checks its saved code when it is created,
//! with its own [`CheckCode`]: this crate knows nothing of the code format,
//! and until a code checks out its engine runs behind a [`Gated`] wrapper
//! that inserts a silence gap every minute.
//!
//! The unlocked flag is process-wide, one per plugin id, so unlocking one
//! instance unlocks every instance in the host. The audio thread reads it
//! with a relaxed load and nothing else.
//!
//! [design/licensing-hook.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/licensing-hook.md

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use silkscreen::bridge::{Engine, NoteEvent, ParamIndex, Transport};

use crate::SilkscreenPlugin;

/// Checks a licence code for a plugin: `Ok` unlocks it, `Err` says why
/// not, as a sentence for the unlock dialog. The code arrives as typed,
/// pasted or read from the saved file, spaces and newlines included.
/// Called on the main thread only, so it may allocate.
pub type CheckCode = fn(&str) -> Result<(), String>;

/// The saved code's file name, in the plugin's user folder.
pub const LICENCE_FILE: &str = "licence.txt";

/// Seconds of audio between gaps.
pub const GAP_PERIOD_S: f64 = 60.0;
/// Seconds of each fade, out and back in.
pub const GAP_FADE_S: f64 = 0.010;
/// Seconds of silence between the fades.
pub const GAP_HOLD_S: f64 = 1.5;

/// The unlocked flag for plugin id `id`, made on first use. Main thread
/// only: it takes a lock.
fn flag(id: &'static str) -> &'static AtomicBool {
    static FLAGS: Mutex<Vec<(&'static str, &'static AtomicBool)>> = Mutex::new(Vec::new());
    let mut flags = FLAGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(&(_, f)) = flags.iter().find(|(i, _)| *i == id) {
        return f;
    }
    let f: &'static AtomicBool = Box::leak(Box::new(AtomicBool::new(false)));
    flags.push((id, f));
    f
}

/// `P`'s unlocked flag, after checking its saved code if it isn't set yet;
/// `None` if `P` isn't licensed. Called on the main thread when an
/// instance is created. A missing, unreadable or invalid code leaves the
/// plugin locked: it never stops the plugin loading.
pub(crate) fn load<P: SilkscreenPlugin>() -> Option<&'static AtomicBool> {
    let check = P::LICENCE?;
    let unlocked = flag(P::ID);
    if !unlocked.load(Ordering::Relaxed)
        && let Some(path) = P::licence_file()
        && let Ok(code) = std::fs::read_to_string(&path)
    {
        match check(&code) {
            Ok(()) => unlocked.store(true, Ordering::Relaxed),
            Err(e) => eprintln!("{}: {}: {e}", P::ID, path.display()),
        }
    }
    Some(unlocked)
}

/// Whether `P` is unlocked in this process. Always `true` for a plugin
/// without [`LICENCE`](SilkscreenPlugin::LICENCE).
pub fn is_unlocked<P: SilkscreenPlugin>() -> bool {
    P::LICENCE.is_none() || flag(P::ID).load(Ordering::Relaxed)
}

/// Why [`try_unlock`] did not unlock for good.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnlockError {
    /// The code was refused, for the reason given. The plugin stays
    /// locked.
    Invalid(String),
    /// The code is good and the plugin is unlocked, but the code could not
    /// be saved, so it will be locked again next time.
    NotSaved(String),
    /// The plugin has no [`LICENCE`](SilkscreenPlugin::LICENCE).
    NotLicensed,
}

impl fmt::Display for UnlockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(e) => e.fmt(f),
            Self::NotSaved(why) => write!(
                f,
                "Unlocked, but the code could not be saved ({why}). Enter it again next time."
            ),
            Self::NotLicensed => f.write_str("This plugin needs no licence code."),
        }
    }
}

impl std::error::Error for UnlockError {}

/// Checks `code` for `P`. A good code unlocks every instance of `P` at
/// once and is saved to [`SilkscreenPlugin::licence_file`]. Main thread
/// only (the unlock dialog).
pub fn try_unlock<P: SilkscreenPlugin>(code: &str) -> Result<(), UnlockError> {
    let check = P::LICENCE.ok_or(UnlockError::NotLicensed)?;
    check(code).map_err(UnlockError::Invalid)?;
    flag(P::ID).store(true, Ordering::Relaxed);
    let path = P::licence_file().ok_or_else(|| UnlockError::NotSaved("no user folder".into()))?;
    save(&path, code.trim()).map_err(UnlockError::NotSaved)
}

/// Writes `code` to `path` through a temporary file, so a failed write
/// never leaves half a code behind.
fn save(path: &Path, code: &str) -> Result<(), String> {
    let describe = |e: std::io::Error, p: &Path| format!("{}: {e}", p.display());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| describe(e, dir))?;
    }
    let tmp = path.with_extension("txt.tmp");
    std::fs::write(&tmp, format!("{code}\n")).map_err(|e| describe(e, &tmp))?;
    std::fs::rename(&tmp, path).map_err(|e| describe(e, path))
}

/// `P`'s lock as its editors see it: the strip shows while it is locked,
/// and the unlock dialog unlocks through it.
pub(crate) struct Lock<P> {
    unlocked: &'static AtomicBool,
    _plugin: std::marker::PhantomData<fn() -> P>,
}

impl<P: SilkscreenPlugin> Default for Lock<P> {
    fn default() -> Self {
        Self {
            unlocked: flag(P::ID),
            _plugin: std::marker::PhantomData,
        }
    }
}

impl<P: SilkscreenPlugin> silkscreen::Licence for Lock<P> {
    /// Asked each tick: the flag itself, no lookup.
    fn locked(&self) -> bool {
        !self.unlocked.load(Ordering::Relaxed)
    }

    fn unlock(&self, code: &str) -> Result<(), String> {
        try_unlock::<P>(code).map_err(|e| e.to_string())
    }
}

/// The default [`SilkscreenPlugin::licence_file`]: `licence.txt` in the
/// plugin's user-preset folder (design/presets.md).
pub(crate) fn default_file(vendor: &str, product: &str) -> Option<PathBuf> {
    silkscreen_host_native::DirStore::default_for(vendor, product)
        .map(|s| s.dir().join(LICENCE_FILE))
}

/// The gap's shape in samples at one sample rate.
#[derive(Debug, Clone, Copy)]
struct Gap {
    period: u64,
    fade: u64,
    hold: u64,
}

impl Gap {
    fn new(sample_rate: f32) -> Self {
        let samples = |s: f64| (s * f64::from(sample_rate)).round().max(1.0) as u64;
        Self {
            period: samples(GAP_PERIOD_S),
            fade: samples(GAP_FADE_S),
            hold: samples(GAP_HOLD_S),
        }
    }

    fn len(self) -> u64 {
        2 * self.fade + self.hold
    }

    /// Where `pos` falls in its gap, if it is in one. The first gap starts
    /// after a full period.
    fn offset(self, pos: u64) -> Option<u64> {
        let q = pos.checked_sub(self.period)? % self.period;
        (q < self.len()).then_some(q)
    }

    /// The gain at `pos`: 1 outside a gap, raised-cosine fades, 0 between.
    fn gain(self, pos: u64) -> f32 {
        let Some(q) = self.offset(pos) else {
            return 1.0;
        };
        let ramp = |x: u64| 0.5 - 0.5 * (std::f32::consts::PI * x as f32 / self.fade as f32).cos();
        if q < self.fade {
            1.0 - ramp(q)
        } else if q < self.fade + self.hold {
            0.0
        } else {
            ramp(q - self.fade - self.hold)
        }
    }

    /// Whether `pos..pos + len` touches no gap.
    fn clear(self, pos: u64, len: u64) -> bool {
        if self.offset(pos).is_some() {
            return false;
        }
        // The next gap after `pos`.
        let next = if pos < self.period {
            self.period
        } else {
            (pos / self.period + 1) * self.period
        };
        pos + len <= next
    }
}

/// An engine behind the soft lock. While `P` is locked, every
/// [`GAP_PERIOD_S`] of processed audio it fades the output out, holds
/// silence and fades back in. The engine always runs underneath. Its own
/// scope and meters are scaled to match, so the editor shows the gated
/// output. Unlocking takes effect once any gap in progress has ended.
///
/// Without a flag (a plugin without a licence) every call goes straight
/// through, so the output is bit for bit the plain engine's.
pub struct Gated<E> {
    engine: E,
    unlocked: Option<&'static AtomicBool>,
    /// Where the audio thread first saw the flag set. Gaps that start
    /// from here on are skipped.
    unlocked_at: Option<u64>,
    gap: Gap,
    /// Samples processed since activation.
    pos: u64,
    /// Where the current host block began.
    block_start: u64,
}

impl<E> Gated<E> {
    pub(crate) fn new(engine: E, unlocked: Option<&'static AtomicBool>, sample_rate: f32) -> Self {
        Self {
            engine,
            unlocked,
            unlocked_at: None,
            gap: Gap::new(sample_rate),
            pos: 0,
            block_start: 0,
        }
    }

    /// Marks the start of a host block, for [`scope`](Engine::scope) and
    /// [`meters`](Engine::meters).
    pub(crate) fn begin_block(&mut self) {
        self.block_start = self.pos;
    }

    /// The gain at `pos`. Once unlocked, a gap already under way finishes
    /// so the sound never jumps back, but no new one starts.
    fn gain(&self, pos: u64) -> f32 {
        match (self.unlocked, self.unlocked_at, self.gap.offset(pos)) {
            (None, ..) | (_, _, None) => 1.0,
            (_, Some(at), Some(q)) if pos - q >= at => 1.0,
            _ => self.gap.gain(pos),
        }
    }

    /// Whether every sample in `pos..pos + len` passes untouched.
    fn open(&self, pos: u64, len: u64) -> bool {
        match (self.unlocked, self.unlocked_at) {
            (None, _) => true,
            _ if self.gap.clear(pos, len) => true,
            // Any gap starting inside the range starts after the unlock.
            (_, Some(_)) => self.gain(pos) == 1.0 && self.gap.offset(pos).is_none(),
            (_, None) => false,
        }
    }

    /// The highest gain over `pos..pos + len`.
    fn peak_gain(&self, pos: u64, len: u64) -> f32 {
        (pos..pos + len).map(|p| self.gain(p)).fold(0.0, f32::max)
    }
}

impl<E: Engine> Engine for Gated<E> {
    fn set_param(&mut self, index: ParamIndex, value: f32) {
        self.engine.set_param(index, value);
    }

    fn note(&mut self, event: NoteEvent) {
        self.engine.note(event);
    }

    fn transport(&mut self, transport: &Transport) {
        self.engine.transport(transport);
    }

    fn process(&mut self, channels: &mut [&mut [f32]]) {
        if self.unlocked_at.is_none()
            && let Some(u) = self.unlocked
            && u.load(Ordering::Relaxed)
        {
            self.unlocked_at = Some(self.pos);
        }
        self.engine.process(channels);
        let len = channels.iter().map(|c| c.len()).min().unwrap_or(0) as u64;
        if len == 0 {
            return;
        }
        if !self.open(self.pos, len) {
            for channel in channels.iter_mut() {
                for (i, s) in channel.iter_mut().enumerate() {
                    *s *= self.gain(self.pos + i as u64);
                }
            }
        }
        self.pos += len;
    }

    fn meters(&mut self, report: &mut dyn FnMut(usize, [f32; 2])) {
        let (start, len) = (self.block_start, self.pos - self.block_start);
        if len == 0 || self.open(start, len) {
            self.engine.meters(report);
            return;
        }
        let g = self.peak_gain(start, len);
        self.engine
            .meters(&mut |n, [l, r]| report(n, [l * g, r * g]));
    }

    fn values(&mut self, report: &mut dyn FnMut(usize, f32)) {
        self.engine.values(report);
    }

    fn frame(&mut self, write: &mut dyn FnMut(&[f32])) {
        self.engine.frame(write);
    }

    fn shown(&mut self, group: u16, page: u16) {
        self.engine.shown(group, page);
    }

    fn patch_load(&mut self) {
        self.engine.patch_load();
    }

    fn scope(&mut self, write: &mut dyn FnMut(&[f32])) {
        let (start, len) = (self.block_start, self.pos - self.block_start);
        if len == 0 || self.open(start, len) {
            self.engine.scope(write);
            return;
        }
        // Scale the engine's samples in fixed chunks: no allocation.
        let gate = Gated {
            engine: (),
            unlocked: self.unlocked,
            unlocked_at: self.unlocked_at,
            gap: self.gap,
            pos: self.pos,
            block_start: self.block_start,
        };
        let mut at = start;
        self.engine.scope(&mut |samples| {
            let mut chunk = [0f32; 64];
            for run in samples.chunks(chunk.len()) {
                for (d, &s) in chunk.iter_mut().zip(run) {
                    *d = s * gate.gain(at);
                    at += 1;
                }
                write(&chunk[..run.len()]);
            }
        });
    }

    fn reset(&mut self) {
        self.engine.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gap_falls_each_period() {
        let g = Gap::new(1000.0);
        assert_eq!((g.period, g.fade, g.hold), (60_000, 10, 1500));
        assert_eq!(g.gain(0), 1.0);
        assert_eq!(g.gain(59_999), 1.0);
        assert_eq!(g.gain(60_000), 1.0, "the fade starts at full gain");
        assert!(g.gain(60_005) < 1.0 && g.gain(60_005) > 0.0);
        assert_eq!(g.gain(60_010), 0.0);
        assert_eq!(g.gain(61_509), 0.0);
        assert!(g.gain(61_515) > 0.0 && g.gain(61_515) < 1.0);
        assert_eq!(g.gain(61_520), 1.0);
        assert_eq!(g.gain(120_010), 0.0);
        assert!(g.clear(0, 60_000));
        assert!(!g.clear(0, 60_001));
        assert!(g.clear(61_520, 58_480));
        assert!(!g.clear(61_519, 1));
    }

    #[test]
    fn fades_are_monotonic() {
        let g = Gap::new(48_000.0);
        let out: Vec<f32> = (0..g.fade).map(|i| g.gain(g.period + i)).collect();
        assert!(out.windows(2).all(|w| w[1] <= w[0]));
        let back: Vec<f32> = (0..=g.fade)
            .map(|i| g.gain(g.period + g.fade + g.hold + i))
            .collect();
        assert!(back.windows(2).all(|w| w[1] >= w[0]));
    }
}

#[cfg(test)]
mod gated_tests {
    use super::*;

    /// Reports full-scale meters and a scope of ones.
    struct Loud;

    impl Engine for Loud {
        fn set_param(&mut self, _: ParamIndex, _: f32) {}
        fn process(&mut self, channels: &mut [&mut [f32]]) {
            for c in channels {
                c.fill(1.0);
            }
        }
        fn meters(&mut self, report: &mut dyn FnMut(usize, [f32; 2])) {
            report(0, [1.0, 1.0]);
        }
        fn scope(&mut self, write: &mut dyn FnMut(&[f32])) {
            write(&[1.0; 100]);
        }
    }

    fn block(g: &mut Gated<Loud>) -> (f32, Vec<f32>) {
        g.begin_block();
        let mut buf = [0f32; 100];
        g.process(&mut [&mut buf]);
        let mut meter = -1.0;
        g.meters(&mut |_, [l, _]| meter = l);
        let mut scope = Vec::new();
        g.scope(&mut |s| scope.extend_from_slice(s));
        assert_eq!(scope, buf, "the scope matches the gated output");
        (meter, scope)
    }

    #[test]
    fn telemetry_follows_the_gate() {
        static LOCKED: AtomicBool = AtomicBool::new(false);
        // 1000 Hz: blocks of 0.1 s; the gap is 60.0–61.52 s.
        let mut g = Gated::new(Loud, Some(&LOCKED), 1000.0);
        for _ in 0..600 {
            assert_eq!(block(&mut g).0, 1.0);
        }
        let (meter, scope) = block(&mut g); // 60.0–60.1 s: the fade out
        assert_eq!(meter, 1.0, "the meter shows the block's peak");
        assert!(scope[50] == 0.0 && scope[5] > 0.0 && scope[5] < 1.0);
        let (meter, scope) = block(&mut g); // in the silence
        assert_eq!(meter, 0.0);
        assert!(scope.iter().all(|&s| s == 0.0));
    }

    /// Records which of its hooks were called.
    #[derive(Default)]
    struct Hooks(Vec<&'static str>);

    impl Engine for Hooks {
        fn set_param(&mut self, _: ParamIndex, _: f32) {
            self.0.push("set_param");
        }
        fn note(&mut self, _: NoteEvent) {
            self.0.push("note");
        }
        fn transport(&mut self, _: &Transport) {
            self.0.push("transport");
        }
        fn process(&mut self, _: &mut [&mut [f32]]) {
            self.0.push("process");
        }
        fn meters(&mut self, _: &mut dyn FnMut(usize, [f32; 2])) {
            self.0.push("meters");
        }
        fn values(&mut self, _: &mut dyn FnMut(usize, f32)) {
            self.0.push("values");
        }
        fn frame(&mut self, _: &mut dyn FnMut(&[f32])) {
            self.0.push("frame");
        }
        fn shown(&mut self, _: u16, _: u16) {
            self.0.push("shown");
        }
        fn patch_load(&mut self) {
            self.0.push("patch_load");
        }
        fn scope(&mut self, _: &mut dyn FnMut(&[f32])) {
            self.0.push("scope");
        }
        fn reset(&mut self) {
            self.0.push("reset");
        }
    }

    /// The gate stands between every host and the engine, so a hook it
    /// doesn't forward never reaches a plugin: a new `Engine` method
    /// belongs here and in the gate.
    #[test]
    fn every_hook_reaches_the_engine_through_the_gate() {
        let mut g = Gated::new(Hooks::default(), None, 1000.0);
        g.begin_block();
        g.set_param(ParamIndex(0), 0.0);
        g.note(NoteEvent::On {
            channel: 0,
            key: 60,
            velocity: 1.0,
        });
        g.transport(&Transport::default());
        g.process(&mut [&mut [0.0; 4]]);
        g.meters(&mut |_, _| {});
        g.values(&mut |_, _| {});
        g.frame(&mut |_| {});
        g.shown(0, 0);
        g.patch_load();
        g.scope(&mut |_| {});
        g.reset();
        assert_eq!(
            g.engine.0,
            [
                "set_param",
                "note",
                "transport",
                "process",
                "meters",
                "values",
                "frame",
                "shown",
                "patch_load",
                "scope",
                "reset"
            ]
        );
    }

    #[test]
    fn without_a_flag_nothing_is_touched() {
        let mut g = Gated::new(Loud, None, 1000.0);
        for _ in 0..1300 {
            let (meter, scope) = block(&mut g);
            assert_eq!(meter, 1.0);
            assert!(scope.iter().all(|&s| s == 1.0));
        }
    }
}
