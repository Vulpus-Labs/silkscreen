/// Dense index of a parameter in the editor's `ParamSpec` table. Mapping to
/// stable host ids (e.g. CLAP param ids) happens at the host boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ParamIndex(pub u32);

impl ParamIndex {
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// What the model tells the engine, in order.
///
/// Values are plain (not normalized) `f32`. Gesture brackets let the engine
/// forward begin/end to the host for automation recording.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EngineEvent {
    ParamValue {
        index: ParamIndex,
        value: f32,
    },
    GestureBegin {
        index: ParamIndex,
    },
    GestureEnd {
        index: ParamIndex,
    },
    /// The editor shows `page` of page group `group` ([design/engine-bridge.md]): view
    /// state, not a param, so never saved, automated or undone. An engine
    /// whose telemetry follows the page (a scope per layer) reads it in
    /// `Engine::shown`. Sent when an editor is made and on every change. A
    /// resync after an overflow doesn't repeat it: the engine keeps the
    /// last page it heard of until the next change.
    ///
    /// [design/engine-bridge.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/engine-bridge.md
    Shown {
        group: u16,
        page: u16,
    },
    /// A whole patch replaces the current one ([design/engine-bridge.md]): the values
    /// that follow, a preset's, belong to it. Sent ahead of a preset load's
    /// gestures, and handed to the engine ahead of a state load's values.
    /// Engines that fade the swap read it in `Engine::patch_load`;
    /// the host never hears of it. Lost, with the rest of the queue, in an
    /// overflow resync, when the values arrive unmarked.
    ///
    /// [design/engine-bridge.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/engine-bridge.md
    PatchLoad,
}

impl EngineEvent {
    /// The param the event is about; none for [`EngineEvent::Shown`] and
    /// [`EngineEvent::PatchLoad`].
    pub fn index(&self) -> Option<ParamIndex> {
        match *self {
            EngineEvent::ParamValue { index, .. }
            | EngineEvent::GestureBegin { index }
            | EngineEvent::GestureEnd { index } => Some(index),
            EngineEvent::Shown { .. } | EngineEvent::PatchLoad => None,
        }
    }
}
