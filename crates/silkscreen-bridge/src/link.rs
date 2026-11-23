use crate::{EngineEvent, ParamIndex};

/// The editor's end of the bridge ([design/engine-bridge.md]). Called on the main/UI
/// thread only.
///
/// [design/engine-bridge.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/engine-bridge.md
pub trait EngineLink {
    /// Send one event to the engine. Never blocks; a transport that runs out
    /// of room must degrade to a resync, not drop state.
    fn send(&mut self, event: EngineEvent);

    /// Report each parameter the *host* has changed since the last poll
    /// (automation, the host's generic UI), with its latest value. Never
    /// reports values that came from [`send`](Self::send).
    fn poll_host(&mut self, report: &mut dyn FnMut(ParamIndex, f32));

    /// Replace every engine value with `values` (indexed by `ParamIndex`),
    /// for a state load. Anything sent before it and not yet delivered is
    /// superseded. The engine applies the values without reporting them to
    /// the host as edits: the host asked for this state.
    fn load(&mut self, values: &[f32]);

    /// The engine's output as telemetry, if this transport carries it
    /// ([design/engine-bridge.md]).
    ///
    /// [design/engine-bridge.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/engine-bridge.md
    #[cfg(feature = "std")]
    fn telemetry(&mut self) -> Option<&mut crate::TelemetryReader> {
        None
    }
}

/// A link to nowhere: for editors with no engine attached.
#[derive(Clone, Copy, Debug, Default)]
pub struct NullLink;

impl EngineLink for NullLink {
    fn send(&mut self, _event: EngineEvent) {}
    fn poll_host(&mut self, _report: &mut dyn FnMut(ParamIndex, f32)) {}
    fn load(&mut self, _values: &[f32]) {}
}

/// Records sent events, and replays queued host changes on poll. For tests.
#[cfg(feature = "std")]
#[derive(Clone, Debug, Default)]
pub struct RecordingLink {
    pub sent: Vec<EngineEvent>,
    pub host_changes: Vec<(ParamIndex, f32)>,
    pub loads: Vec<Vec<f32>>,
}

#[cfg(feature = "std")]
impl EngineLink for RecordingLink {
    fn send(&mut self, event: EngineEvent) {
        self.sent.push(event);
    }

    fn poll_host(&mut self, report: &mut dyn FnMut(ParamIndex, f32)) {
        for (index, value) in self.host_changes.drain(..) {
            report(index, value);
        }
    }

    fn load(&mut self, values: &[f32]) {
        self.loads.push(values.to_vec());
    }
}
