use crate::{NoteEvent, ParamIndex, Transport};

/// The audio-thread side of a plugin: the seam every audio host drives (a
/// CLAP processor, a web `AudioWorklet`, a cpal callback).
///
/// Implementations must be real-time safe: no allocation, locking or I/O
/// in any method except `new`-style constructors.
pub trait Engine: Send + 'static {
    /// A parameter has a new plain value, from the UI or the host.
    fn set_param(&mut self, index: ParamIndex, value: f32);

    /// A note-input event from the host, delivered at its sample offset:
    /// hosts split `process` calls there. Instruments override this.
    fn note(&mut self, _event: NoteEvent) {}

    /// The host transport, once per block before any of the block's audio
    /// is rendered. Tempo-synced engines override this.
    fn transport(&mut self, _transport: &Transport) {}

    /// Render one block in place. `channels` holds the output channels,
    /// already filled with the input signal for effects, or zeros for
    /// instruments. All have the same length.
    fn process(&mut self, channels: &mut [&mut [f32]]);

    /// Report each of the engine's meters ([design/engine-bridge.md]): `report(n,
    /// [left, right])` with meter `n`'s peak magnitude over the block just
    /// rendered. Hosts call it once per block, after `process`. Engines
    /// with no meters of their own report nothing.
    ///
    /// [design/engine-bridge.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/engine-bridge.md
    fn meters(&mut self, _report: &mut dyn FnMut(usize, [f32; 2])) {}

    /// Report values for the editor to show ([design/engine-bridge.md]): `report(n,
    /// value)` for each of up to `ENGINE_VALUES` (an envelope's level, an
    /// LFO's phase, voices sounding). Hosts call it once per block, after
    /// `process`; the editor reads the latest. Most engines report none.
    ///
    /// [design/engine-bridge.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/engine-bridge.md
    fn values(&mut self, _report: &mut dyn FnMut(usize, f32)) {}

    /// Publish a frame for the editor ([design/engine-bridge.md]): `write(frame)`
    /// with a snapshot of many values that belong together, such as every
    /// voice's position, which the editor reads all or nothing of. Up to
    /// the frame size the plugin declares (`PluginUi::frame`); words past
    /// it are dropped. Hosts call it once per block, after `process`; the
    /// editor reads the latest. An engine with nothing new to show writes
    /// nothing. Most engines publish none.
    ///
    /// [design/engine-bridge.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/engine-bridge.md
    fn frame(&mut self, _write: &mut dyn FnMut(&[f32])) {}

    /// The editor now shows `page` of page group `group`
    /// ([`EngineEvent::Shown`](crate::EngineEvent::Shown)): for telemetry
    /// that follows the page, as a scope of the shown layer. Most engines
    /// ignore it.
    fn shown(&mut self, _group: u16, _page: u16) {}

    /// A whole patch replaces the current one
    /// ([`EngineEvent::PatchLoad`](crate::EngineEvent::PatchLoad)): a preset
    /// load or a host state load. Called before the patch's values arrive
    /// by `set_param`, all of them before the next `process`. An engine that
    /// would retune tails under the swap fades it here; most
    /// ignore it.
    fn patch_load(&mut self) {}

    /// Supply the scope's samples for the block just rendered ([design/engine-bridge.md]),
    /// in place of the output's: `write(samples)`, mono, at the host's
    /// rate, in one or more runs. Hosts call it once per block, after
    /// `process`. An engine that writes nothing leaves the scope on the
    /// output, the default.
    ///
    /// [design/engine-bridge.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/engine-bridge.md
    fn scope(&mut self, _write: &mut dyn FnMut(&[f32])) {}

    /// Clear all processing state (voices, filters), and snap smoothed
    /// parameters to their targets. Hosts call this after seeding a new
    /// engine with `set_param`, so it starts at those values.
    fn reset(&mut self) {}
}
