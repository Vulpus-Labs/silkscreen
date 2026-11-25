//! Headless host for silkscreen editors: the first implementation of the
//! host contract ([design/overview.md]).
//!
//! Scripts say *what* to do to which control; the harness works out *where*
//! from the layout and synthesizes the raw events, advancing a virtual clock
//! by [`Harness::STEP_MS`] per event.
//!
//! ```ignore
//! let mut h = Harness::new(|link| my_plugin::editor(link), 2.0);
//! h.drag(ids::GAIN, -40.0);
//! assert_snapshot!(h, "gain_after_drag");
//! ```
//!
//! [design/overview.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/overview.md

mod snapshot;

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use silkscreen::bridge::{
    EngineEvent, EngineLink, ParamIndex, RecordingLink, TelemetryReader, TelemetryTap,
    telemetry_with_frame,
};
use silkscreen::core::tiny_skia::Pixmap;
use silkscreen::core::{ControlId, Modifiers, MouseButton, Point, RawEvent, RawKind, Vec2};
use silkscreen::{Editor, HostEvent, IRect};

pub use snapshot::{SnapshotError, check_snapshot, check_snapshot_with};

/// Panic, listing every problem, unless factory `bank` is sound against
/// `specs`: each preset parses, is named as its entry, sits in its
/// category, names only params the table has, keeps values in range, has
/// a name no other preset has, and survives a round trip (design/presets.md).
#[track_caller]
pub fn assert_factory_bank(bank: &[silkscreen::FactoryPreset], specs: &[silkscreen::ParamSpec]) {
    let problems = silkscreen::model::preset::check_factory(bank, specs);
    assert!(
        problems.is_empty(),
        "the factory bank has {} problem(s):\n  {}",
        problems.len(),
        problems.join("\n  ")
    );
}

/// Assert that the harness frame matches `tests/snapshots/{name}.png` in the
/// calling crate. Set `SILKSCREEN_BLESS=1` to (re)write snapshots.
#[macro_export]
macro_rules! assert_snapshot {
    ($harness:expr, $name:expr) => {{
        let dir = ::std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
        if let Err(e) = $crate::check_snapshot($harness.snapshot(), &dir, $name) {
            panic!("{e}");
        }
    }};
}

/// An [`EngineLink`] whose recording the harness can still read after the
/// editor has taken ownership of the box.
#[derive(Clone, Debug, Default)]
pub struct SharedRecorder(Rc<RefCell<RecordingLink>>);

impl EngineLink for SharedRecorder {
    fn send(&mut self, event: EngineEvent) {
        self.0.borrow_mut().send(event);
    }

    fn poll_host(&mut self, report: &mut dyn FnMut(ParamIndex, f32)) {
        // Take the queue first so `report` can't observe a held borrow.
        let queued: Vec<_> = std::mem::take(&mut self.0.borrow_mut().host_changes);
        for (p, v) in queued {
            report(p, v);
        }
    }

    fn load(&mut self, values: &[f32]) {
        self.0.borrow_mut().load(values);
    }
}

/// The recorder, plus the reading end of telemetry the harness writes
/// ([`Harness::output`]).
#[derive(Debug)]
struct Telemetered {
    recorder: SharedRecorder,
    reader: TelemetryReader,
}

impl EngineLink for Telemetered {
    fn send(&mut self, event: EngineEvent) {
        self.recorder.send(event);
    }

    fn poll_host(&mut self, report: &mut dyn FnMut(ParamIndex, f32)) {
        self.recorder.poll_host(report);
    }

    fn load(&mut self, values: &[f32]) {
        self.recorder.load(values);
    }

    fn telemetry(&mut self) -> Option<&mut TelemetryReader> {
        Some(&mut self.reader)
    }
}

#[derive(Debug)]
pub struct Harness {
    editor: Editor,
    recorder: SharedRecorder,
    /// Where [`output`](Self::output) writes, unless the caller supplied
    /// the link.
    tap: Option<TelemetryTap>,
    now: f64,
    frame: Pixmap,
    last_damage: Vec<IRect>,
}

impl Harness {
    /// Virtual time between synthesized events.
    pub const STEP_MS: f64 = 16.0;
    /// Moves per scripted drag.
    pub const DRAG_STEPS: u32 = 4;

    /// The engine frame a harness's telemetry holds, in words: room for
    /// any plugin's [`engine_frame`](Self::engine_frame).
    pub const FRAME: usize = 4096;

    /// Build an editor around a recording engine link, at `scale`.
    pub fn new(make: impl FnOnce(Box<dyn EngineLink>) -> Editor, scale: f32) -> Self {
        let recorder = SharedRecorder::default();
        let (tap, reader) = telemetry_with_frame(Self::FRAME);
        let link = Telemetered {
            recorder: recorder.clone(),
            reader,
        };
        let mut h = Self::build(make(Box::new(link)), recorder, scale);
        h.tap = Some(tap);
        h
    }

    /// Build an editor around a caller-supplied link (e.g. a real bridge).
    /// [`engine_events`](Self::engine_events) and
    /// [`host_change`](Self::host_change) then do nothing useful; observe
    /// the link's other end instead.
    pub fn with_link(
        make: impl FnOnce(Box<dyn EngineLink>) -> Editor,
        link: Box<dyn EngineLink>,
        scale: f32,
    ) -> Self {
        Self::build(make(link), SharedRecorder::default(), scale)
    }

    fn build(editor: Editor, recorder: SharedRecorder, scale: f32) -> Self {
        let mut editor = editor;
        editor.set_scale(scale);
        let (w, h) = editor.physical_size();
        let mut h = Self {
            editor,
            recorder,
            tap: None,
            now: 0.0,
            frame: Pixmap::new(w, h).expect("non-empty editor"),
            last_damage: Vec::new(),
        };
        h.render();
        h
    }

    pub fn editor(&self) -> &Editor {
        &self.editor
    }

    pub fn editor_mut(&mut self) -> &mut Editor {
        &mut self.editor
    }

    pub fn model(&self) -> &silkscreen::Model {
        self.editor.model()
    }

    pub fn now(&self) -> f64 {
        self.now
    }

    /// Centre of a control, in logical units.
    pub fn centre(&self, id: ControlId) -> Point {
        self.editor.state.layout().control(id).rect.center()
    }

    /// Feed a raw event at the current virtual time, then step the clock.
    /// The mouse cursor a host would show now.
    pub fn cursor(&self) -> silkscreen::core::MouseCursor {
        self.editor.cursor()
    }

    pub fn raw(&mut self, kind: RawKind) -> &mut Self {
        self.editor.handle(&RawEvent::new(self.now, kind));
        self.now += Self::STEP_MS;
        self
    }

    pub fn move_to(&mut self, pos: Point) -> &mut Self {
        self.raw(RawKind::PointerMove {
            pos,
            mods: Modifiers::NONE,
        })
    }

    /// Click the centre of `id`. Two clicks in quick succession form a
    /// double-click, as they would for a user.
    pub fn click(&mut self, id: ControlId) -> &mut Self {
        self.click_with(id, Modifiers::NONE)
    }

    pub fn click_with(&mut self, id: ControlId, mods: Modifiers) -> &mut Self {
        let pos = self.centre(id);
        self.move_to(pos);
        self.raw(RawKind::PointerDown {
            pos,
            button: MouseButton::Left,
            mods,
        });
        self.raw(RawKind::PointerUp {
            pos,
            button: MouseButton::Left,
            mods,
        })
    }

    /// Click at `pos`, in logical units.
    pub fn click_at(&mut self, pos: Point) -> &mut Self {
        let mods = Modifiers::NONE;
        self.move_to(pos);
        self.raw(RawKind::PointerDown {
            pos,
            button: MouseButton::Left,
            mods,
        });
        self.raw(RawKind::PointerUp {
            pos,
            button: MouseButton::Left,
            mods,
        })
    }

    /// Drag vertically from `start` by `dy` logical units, in
    /// [`DRAG_STEPS`](Self::DRAG_STEPS) moves.
    pub fn drag_from(&mut self, start: Point, dy: f32) -> &mut Self {
        let mods = Modifiers::NONE;
        self.move_to(start);
        self.raw(RawKind::PointerDown {
            pos: start,
            button: MouseButton::Left,
            mods,
        });
        let mut pos = start;
        for i in 1..=Self::DRAG_STEPS {
            pos = start + Vec2::new(0.0, dy * i as f32 / Self::DRAG_STEPS as f32);
            self.raw(RawKind::PointerMove { pos, mods });
        }
        self.raw(RawKind::PointerUp {
            pos,
            button: MouseButton::Left,
            mods,
        })
    }

    pub fn double_click(&mut self, id: ControlId) -> &mut Self {
        self.click(id).click(id)
    }

    /// Drag vertically from the centre of `id` by `dy` logical units
    /// (negative is up) in [`DRAG_STEPS`](Self::DRAG_STEPS) moves.
    pub fn drag(&mut self, id: ControlId, dy: f32) -> &mut Self {
        self.drag_with(id, dy, Modifiers::NONE)
    }

    pub fn drag_with(&mut self, id: ControlId, dy: f32, mods: Modifiers) -> &mut Self {
        let start = self.centre(id);
        self.move_to(start);
        self.raw(RawKind::PointerDown {
            pos: start,
            button: MouseButton::Left,
            mods,
        });
        let mut pos = start;
        for i in 1..=Self::DRAG_STEPS {
            pos = start + Vec2::new(0.0, dy * i as f32 / Self::DRAG_STEPS as f32);
            self.raw(RawKind::PointerMove { pos, mods });
        }
        self.raw(RawKind::PointerUp {
            pos,
            button: MouseButton::Left,
            mods,
        })
    }

    /// Scroll `lines` over `id` (positive is up).
    pub fn wheel(&mut self, id: ControlId, lines: f32) -> &mut Self {
        let pos = self.centre(id);
        self.move_to(pos);
        self.raw(RawKind::Wheel {
            pos,
            delta: Vec2::new(0.0, lines),
            mods: Modifiers::NONE,
        })
    }

    /// Write one block of engine output, one slice per channel, as the
    /// audio host would; scopes and meters see it on the next tick.
    ///
    /// # Panics
    /// If the harness was built [`with_link`](Self::with_link).
    pub fn output(&mut self, channels: &[&[f32]]) -> &mut Self {
        self.tap
            .as_mut()
            .expect("output needs Harness::new's link")
            .write(channels);
        self
    }

    /// Report engine meter `n`'s peaks for a block, as the audio host
    /// does from `Engine::meters`.
    ///
    /// # Panics
    /// If the harness was built [`with_link`](Self::with_link).
    pub fn engine_meter(&mut self, n: usize, levels: [f32; 2]) -> &mut Self {
        self.tap
            .as_mut()
            .expect("engine_meter needs Harness::new's link")
            .write_engine_peak(n, levels);
        self
    }

    /// Set engine value `n` (design/engine-bridge.md), as a host does from
    /// `Engine::values`.
    ///
    /// # Panics
    /// If the harness was built [`with_link`](Self::with_link).
    pub fn engine_value(&mut self, n: usize, value: f32) -> &mut Self {
        self.tap
            .as_mut()
            .expect("engine_value needs Harness::new's link")
            .write_engine_value(n, value);
        self
    }

    /// Publish an engine frame (design/engine-bridge.md), as a host does
    /// from `Engine::frame`: the editor takes it on the next tick.
    ///
    /// # Panics
    /// If the harness was built [`with_link`](Self::with_link), or `frame`
    /// is over [`FRAME`](Self::FRAME) words.
    pub fn engine_frame(&mut self, frame: &[f32]) -> &mut Self {
        assert!(frame.len() <= Self::FRAME, "a frame over Harness::FRAME");
        self.tap
            .as_mut()
            .expect("engine_frame needs Harness::new's link")
            .write_frame(frame);
        self
    }

    /// Advance the virtual clock by `ms` and tick the editor.
    pub fn advance(&mut self, ms: f64) -> &mut Self {
        self.now += ms;
        self.editor.tick(self.now);
        self
    }

    /// Queue a change as if the host had automated `param`; it arrives on
    /// the next tick.
    pub fn host_change(&mut self, param: ParamIndex, value: f32) -> &mut Self {
        self.recorder
            .0
            .borrow_mut()
            .host_changes
            .push((param, value));
        self
    }

    /// Load state as the host would: every listed param takes its value,
    /// every other returns to its default.
    pub fn host_load(&mut self, values: &[(ParamIndex, f32)]) -> &mut Self {
        self.editor
            .state
            .handle_host(HostEvent::StateLoaded(values.to_vec()));
        self
    }

    /// State loads the engine was asked to apply since the last call.
    pub fn engine_loads(&mut self) -> Vec<Vec<f32>> {
        std::mem::take(&mut self.recorder.0.borrow_mut().loads)
    }

    /// Everything sent to the engine since the last call.
    pub fn engine_events(&mut self) -> Vec<EngineEvent> {
        std::mem::take(&mut self.recorder.0.borrow_mut().sent)
    }

    /// Render pending damage and return the up-to-date frame.
    pub fn snapshot(&mut self) -> &Pixmap {
        self.render();
        &self.frame
    }

    /// Rects repainted by the most recent render.
    pub fn last_damage(&self) -> &[IRect] {
        &self.last_damage
    }

    pub fn save_png(&mut self, path: impl AsRef<Path>) {
        self.snapshot().save_png(path).expect("write PNG");
    }

    fn render(&mut self) {
        let (w, h) = self.editor.physical_size();
        if (self.frame.width(), self.frame.height()) != (w, h) {
            self.frame = Pixmap::new(w, h).expect("non-empty editor");
        }
        let painted = self.editor.render(&mut self.frame.as_mut()).to_vec();
        self.last_damage = painted;
    }
}
