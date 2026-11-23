//! The model: parameter values, settled; every change logged.

use crate::log::{EventLog, ModelEvent, Origin};
use crate::param::{ParamIndex, ParamSpec};
use crate::undo::{UndoEntry, UndoStack};

/// An open gesture: who opened it, and the value when it opened.
#[derive(Clone, Copy, Debug)]
struct Gesture {
    origin: Origin,
    before: f32,
}

/// Something a binding asks the view for, beyond changing values: the
/// model has no UI, so it queues these for the editor to act on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UiRequest {
    /// Open a menu of `param`'s choices (a stepped param) under `control`.
    Menu {
        control: silkscreen_core::ControlId,
        param: ParamIndex,
    },
    /// Open the preset browser under `control`.
    Presets { control: silkscreen_core::ControlId },
    /// Open the panel the plugin registered for `control` (e.g. a mod
    /// matrix behind a button).
    Panel { control: silkscreen_core::ControlId },
    /// Show the page that tab `control` stands for ([design/faceplates.md]).
    ///
    /// [design/faceplates.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/faceplates.md
    ShowPage { control: silkscreen_core::ControlId },
    /// Load the next (or previous) preset, factory then user, wrapping
    /// ([design/presets.md]).
    ///
    /// [design/presets.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/presets.md
    PresetStep { forward: bool },
    /// Save the current preset over itself if it's the user's, else ask
    /// for a name (Save As).
    PresetSave,
    /// Ask for a name and folder, and save the current sound there.
    PresetSaveAs,
    /// Every param to its default, as one undo step.
    PresetInit,
    /// A random sound, as one undo step.
    PresetRandomise,
    /// Set `param`, a linear gain, so the peak engine meter `meter` last
    /// reported (measured before that gain) lands at `target_db` dBFS:
    /// level keys ([`LevelCommand`](crate::LevelCommand)).
    MatchLevel {
        meter: usize,
        param: ParamIndex,
        target_db: f32,
    },
}

/// Holds every parameter's plain value. All mutation goes through
/// [`Model::edit`], which appends to the [`EventLog`].
#[derive(Debug)]
pub struct Model {
    specs: &'static [ParamSpec],
    values: Vec<f32>,
    gestures: Vec<Option<Gesture>>,
    log: EventLog,
    undo: UndoStack,
    requests: Vec<UiRequest>,
}

impl Model {
    /// A model with every param at its default.
    pub fn new(specs: &'static [ParamSpec]) -> Self {
        Self {
            specs,
            values: specs.iter().map(|s| s.clamp(s.default)).collect(),
            gestures: vec![None; specs.len()],
            log: EventLog::new(),
            undo: UndoStack::new(),
            requests: Vec::new(),
        }
    }

    /// Requests bindings made since the last call, oldest first.
    pub fn take_requests(&mut self) -> Vec<UiRequest> {
        std::mem::take(&mut self.requests)
    }

    pub fn specs(&self) -> &'static [ParamSpec] {
        self.specs
    }

    pub fn spec(&self, p: ParamIndex) -> &'static ParamSpec {
        &self.specs[p.index()]
    }

    /// Plain value.
    pub fn get(&self, p: ParamIndex) -> f32 {
        self.values[p.index()]
    }

    pub fn get_normalized(&self, p: ParamIndex) -> f32 {
        self.spec(p).normalize(self.get(p))
    }

    pub fn in_gesture(&self, p: ParamIndex) -> bool {
        self.gestures[p.index()].is_some()
    }

    fn any_gesture_open(&self) -> bool {
        self.gestures.iter().any(Option::is_some)
    }

    /// Revert the most recent user gesture, as an `Origin::Undo` gesture.
    /// Does nothing (returns `false`) with nothing to undo, or while any
    /// gesture is in progress.
    pub fn undo(&mut self) -> bool {
        if self.any_gesture_open() {
            return false;
        }
        let Some(step) = self.undo.pop_undo() else {
            return false;
        };
        for entry in step.iter().rev() {
            self.replay(entry.param, entry.before);
        }
        true
    }

    /// Re-apply the most recently undone gesture. Same conditions as
    /// [`undo`](Self::undo).
    pub fn redo(&mut self) -> bool {
        if self.any_gesture_open() {
            return false;
        }
        let Some(step) = self.undo.pop_redo() else {
            return false;
        };
        for entry in &step {
            self.replay(entry.param, entry.after);
        }
        true
    }

    pub fn can_undo(&self) -> bool {
        self.undo.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.undo.can_redo()
    }

    /// Forget undo and redo history, as after a state load: the entries
    /// describe edits to a state that no longer exists. A user gesture
    /// still open now counts from the current value, so ending it records
    /// only what it changes from here.
    pub fn clear_history(&mut self) {
        self.undo.clear();
        for (gesture, value) in self.gestures.iter_mut().zip(&self.values) {
            if let Some(g) = gesture {
                g.before = *value;
            }
        }
    }

    /// Maximum undo entries kept. Default 100.
    pub fn set_undo_depth(&mut self, depth: usize) {
        self.undo.set_depth(depth);
    }

    fn replay(&mut self, p: ParamIndex, value: f32) {
        let mut edit = self.edit(Origin::Undo);
        edit.begin(p);
        edit.set(p, value);
        edit.end(p);
    }

    pub fn log(&self) -> &EventLog {
        &self.log
    }

    pub fn log_mut(&mut self) -> &mut EventLog {
        &mut self.log
    }

    /// Start a batch of changes attributed to `origin`.
    pub fn edit(&mut self, origin: Origin) -> Edit<'_> {
        Edit {
            model: self,
            origin,
        }
    }
}

/// Mutating access to a [`Model`], attributed to one [`Origin`]. Bindings
/// receive one of these; it is the only way to change state.
#[derive(Debug)]
pub struct Edit<'a> {
    model: &'a mut Model,
    origin: Origin,
}

impl Edit<'_> {
    pub fn origin(&self) -> Origin {
        self.origin
    }

    pub fn model(&self) -> &Model {
        self.model
    }

    pub fn get(&self, p: ParamIndex) -> f32 {
        self.model.get(p)
    }

    pub fn get_normalized(&self, p: ParamIndex) -> f32 {
        self.model.get_normalized(p)
    }

    /// Open a gesture on `p`. Opening one that is already open is a no-op.
    pub fn begin(&mut self, p: ParamIndex) {
        if self.model.in_gesture(p) {
            return;
        }
        self.model.gestures[p.index()] = Some(Gesture {
            origin: self.origin,
            before: self.model.get(p),
        });
        self.model.log.push(ModelEvent::GestureBegin {
            param: p,
            origin: self.origin,
        });
    }

    /// Close a gesture on `p`. Closing one that isn't open is a bug in the
    /// caller: it asserts in debug builds and is ignored in release.
    /// A completed user (`Origin::Ui`) gesture that changed the value
    /// becomes one undo entry.
    pub fn end(&mut self, p: ParamIndex) {
        let gesture = self.model.gestures[p.index()].take();
        debug_assert!(gesture.is_some(), "end({p:?}) without begin");
        let Some(gesture) = gesture else {
            return;
        };
        self.model.log.push(ModelEvent::GestureEnd {
            param: p,
            origin: self.origin,
        });
        let after = self.model.get(p);
        if gesture.origin == Origin::Ui && after.to_bits() != gesture.before.to_bits() {
            self.model.undo.record(UndoEntry {
                param: p,
                before: gesture.before,
                after,
            });
        }
    }

    /// Set a plain value, clamped to range. Setting the current value logs
    /// nothing.
    pub fn set(&mut self, p: ParamIndex, plain: f32) {
        let value = self.model.spec(p).clamp(plain);
        let slot = &mut self.model.values[p.index()];
        if slot.to_bits() == value.to_bits() {
            return;
        }
        *slot = value;
        self.model.log.push(ModelEvent::ParamChanged {
            param: p,
            value,
            origin: self.origin,
        });
    }

    pub fn set_normalized(&mut self, p: ParamIndex, normalized: f32) {
        let plain = self.model.spec(p).denormalize(normalized);
        self.set(p, plain);
    }

    /// Make every gesture `f` completes one undo step, e.g. loading a
    /// preset.
    pub fn group(&mut self, f: impl FnOnce(&mut Self)) {
        self.model.undo.begin_group();
        f(self);
        self.model.undo.end_group();
    }

    /// Ask the view for something (see [`UiRequest`]).
    pub fn request(&mut self, request: UiRequest) {
        self.model.requests.push(request);
    }

    /// Return `p` to its default as one complete gesture.
    pub fn reset(&mut self, p: ParamIndex) {
        let default = self.model.spec(p).default;
        self.begin(p);
        self.set(p, default);
        self.end(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::Cursor;

    static SPECS: [ParamSpec; 2] = [
        ParamSpec::new("Gain", -60.0, 12.0, 0.0),
        ParamSpec::new("Mix", 0.0, 1.0, 0.5),
    ];
    const GAIN: ParamIndex = ParamIndex(0);
    const MIX: ParamIndex = ParamIndex(1);

    fn drain(m: &Model, c: &mut Cursor) -> Vec<ModelEvent> {
        m.log().read(c).copied().collect()
    }

    #[test]
    fn starts_at_defaults() {
        let m = Model::new(&SPECS);
        assert_eq!(m.get(GAIN), 0.0);
        assert_eq!(m.get(MIX), 0.5);
        assert!(m.log().is_empty());
    }

    #[test]
    fn set_clamps_and_logs() {
        let mut m = Model::new(&SPECS);
        let mut c = m.log().cursor();
        m.edit(Origin::Ui).set(GAIN, 100.0);
        assert_eq!(m.get(GAIN), 12.0);
        assert_eq!(
            drain(&m, &mut c),
            [ModelEvent::ParamChanged {
                param: GAIN,
                value: 12.0,
                origin: Origin::Ui
            }]
        );
    }

    #[test]
    fn setting_current_value_logs_nothing() {
        let mut m = Model::new(&SPECS);
        let mut c = m.log().cursor();
        m.edit(Origin::Ui).set(MIX, 0.5);
        // Clamped to the current value: also nothing.
        m.edit(Origin::Ui).set(GAIN, 12.0);
        m.edit(Origin::Ui).set(GAIN, 50.0);
        assert_eq!(drain(&m, &mut c).len(), 1);
    }

    #[test]
    fn set_normalized_maps_through_spec() {
        let mut m = Model::new(&SPECS);
        m.edit(Origin::Ui).set_normalized(GAIN, 0.5);
        assert!((m.get(GAIN) - -24.0).abs() < 1e-4);
        assert!((m.get_normalized(GAIN) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn reset_is_a_full_bracket() {
        let mut m = Model::new(&SPECS);
        m.edit(Origin::Ui).set(MIX, 0.9);
        let mut c = m.log().cursor();
        m.edit(Origin::Ui).reset(MIX);
        assert_eq!(
            drain(&m, &mut c),
            [
                ModelEvent::GestureBegin {
                    param: MIX,
                    origin: Origin::Ui
                },
                ModelEvent::ParamChanged {
                    param: MIX,
                    value: 0.5,
                    origin: Origin::Ui
                },
                ModelEvent::GestureEnd {
                    param: MIX,
                    origin: Origin::Ui
                },
            ]
        );
        assert!(!m.in_gesture(MIX));
    }

    #[test]
    fn double_begin_is_idempotent() {
        let mut m = Model::new(&SPECS);
        let mut c = m.log().cursor();
        let mut e = m.edit(Origin::Ui);
        e.begin(GAIN);
        e.begin(GAIN);
        e.end(GAIN);
        assert_eq!(drain(&m, &mut c).len(), 2);
    }

    #[test]
    #[cfg_attr(debug_assertions, should_panic(expected = "without begin"))]
    fn end_without_begin_asserts_in_debug() {
        let mut m = Model::new(&SPECS);
        let mut c = m.log().cursor();
        m.edit(Origin::Ui).end(GAIN);
        assert!(drain(&m, &mut c).is_empty(), "ignored in release");
    }

    #[test]
    fn origins_are_carried_through() {
        let mut m = Model::new(&SPECS);
        let mut c = m.log().cursor();
        for (origin, v) in [(Origin::Ui, 0.1), (Origin::Host, 0.2), (Origin::Undo, 0.3)] {
            let mut e = m.edit(origin);
            e.begin(MIX);
            e.set(MIX, v);
            e.end(MIX);
        }
        let origins: Vec<Origin> = drain(&m, &mut c).iter().map(ModelEvent::origin).collect();
        let expected: Vec<Origin> = [Origin::Ui, Origin::Host, Origin::Undo]
            .into_iter()
            .flat_map(|o| [o; 3])
            .collect();
        assert_eq!(origins, expected);
    }
}
