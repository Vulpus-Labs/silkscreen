//! The engine's end of a bridge, whichever transport carries it
//! ([design/engine-bridge.md]): native ([`EngineEnd`](crate::EngineEnd)) or a shared
//! buffer ([`SabEngine`](crate::sab::SabEngine)). Hosts write the steps
//! every engine host takes once, here. Audio thread only; nothing here
//! allocates or blocks.
//!
//! [design/engine-bridge.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/engine-bridge.md

use crate::telemetry::TelemetryTap;
use crate::words::SharedWords;
use crate::{Engine, EngineEvent, ParamIndex};

/// What an engine host needs from its end of the bridge.
pub trait EngineSide {
    /// The words its telemetry is written through.
    type Words: SharedWords;

    /// Deliver pending UI events in order; true after an overflow
    /// resync (see [`EngineEnd::drain`](crate::EngineEnd::drain)).
    fn drain(&mut self, deliver: impl FnMut(EngineEvent)) -> bool;

    /// Apply a pending state load, if there is one; true if one was.
    fn take_load(&mut self, apply: impl FnMut(ParamIndex, f32)) -> bool;

    /// The value to seed a fresh engine with.
    fn seed(&self, index: ParamIndex) -> f32;

    fn param_count(&self) -> usize;

    /// Where each block's output and the engine's meters go.
    fn telemetry(&mut self) -> &mut TelemetryTap<Self::Words>;

    /// Seed `engine` with every value (a pending load's, if one is
    /// waiting), then reset it, so it starts at them rather than
    /// gliding there: what a host does at activation.
    fn seed_engine<E: Engine + ?Sized>(&self, engine: &mut E) {
        for i in 0..self.param_count() {
            let index = ParamIndex(i as u32);
            engine.set_param(index, self.seed(index));
        }
        engine.reset();
    }

    /// Hand `engine` a pending load, marked as a patch load, then the
    /// UI's pending events, dropping gesture brackets: for hosts with no
    /// automation to record (standalone, the browser).
    fn apply_ui<E: Engine + ?Sized>(&mut self, engine: &mut E) {
        self.take_load(patch_loaded(|event| deliver(engine, event)));
        self.drain(|event| deliver(engine, event));
    }
}

/// `apply` for [`EngineSide::take_load`] that sends
/// [`EngineEvent::PatchLoad`] to `to_engine` ahead of the load's first
/// value, then each value as a [`EngineEvent::ParamValue`].
pub fn patch_loaded(mut to_engine: impl FnMut(EngineEvent)) -> impl FnMut(ParamIndex, f32) {
    let mut first = true;
    move |index, value| {
        if std::mem::take(&mut first) {
            to_engine(EngineEvent::PatchLoad);
        }
        to_engine(EngineEvent::ParamValue { index, value });
    }
}

/// Hand one UI event to `engine`, dropping gesture brackets.
fn deliver<E: Engine + ?Sized>(engine: &mut E, event: EngineEvent) {
    match event {
        EngineEvent::ParamValue { index, value } => engine.set_param(index, value),
        EngineEvent::Shown { group, page } => engine.shown(group, page),
        EngineEvent::PatchLoad => engine.patch_load(),
        EngineEvent::GestureBegin { .. } | EngineEvent::GestureEnd { .. } => {}
    }
}

impl<W: SharedWords> TelemetryTap<W> {
    /// Record `engine`'s own meters (`Engine::meters`), values
    /// (`Engine::values`) and frame (`Engine::frame`) for the editor.
    pub fn write_engine<E: Engine + ?Sized>(&mut self, engine: &mut E) {
        engine.meters(&mut |meter, peaks| self.write_engine_peak(meter, peaks));
        engine.values(&mut |n, value| self.write_engine_value(n, value));
        engine.frame(&mut |frame| self.write_frame(frame));
    }

    /// Record a rendered block: the scope from `engine` if it supplies one
    /// (`Engine::scope`, design/engine-bridge.md), else from the output; the output's
    /// peaks; the engine's meters, values and frame. What every audio host calls
    /// once per block, after `process`.
    pub fn write_block<E: Engine + ?Sized, C: AsRef<[f32]>>(
        &mut self,
        engine: &mut E,
        channels: &[C],
    ) {
        let mut own = false;
        engine.scope(&mut |samples| {
            own = true;
            self.write_scope(samples);
        });
        self.write_output(channels, !own);
        self.write_engine(engine);
    }
}

impl EngineSide for crate::EngineEnd {
    type Words = std::sync::Arc<[core::sync::atomic::AtomicU32]>;

    fn drain(&mut self, deliver: impl FnMut(EngineEvent)) -> bool {
        crate::EngineEnd::drain(self, deliver)
    }

    fn take_load(&mut self, apply: impl FnMut(ParamIndex, f32)) -> bool {
        crate::EngineEnd::take_load(self, apply)
    }

    fn seed(&self, index: ParamIndex) -> f32 {
        crate::EngineEnd::seed(self, index)
    }

    fn param_count(&self) -> usize {
        crate::EngineEnd::param_count(self)
    }

    fn telemetry(&mut self) -> &mut TelemetryTap<Self::Words> {
        crate::EngineEnd::telemetry(self)
    }
}

impl<W: SharedWords + Clone> EngineSide for crate::sab::SabEngine<W> {
    type Words = W;

    fn drain(&mut self, deliver: impl FnMut(EngineEvent)) -> bool {
        crate::sab::SabEngine::drain(self, deliver)
    }

    fn take_load(&mut self, apply: impl FnMut(ParamIndex, f32)) -> bool {
        crate::sab::SabEngine::take_load(self, apply)
    }

    fn seed(&self, index: ParamIndex) -> f32 {
        crate::sab::SabEngine::seed(self, index)
    }

    fn param_count(&self) -> usize {
        crate::sab::SabEngine::param_count(self)
    }

    fn telemetry(&mut self) -> &mut TelemetryTap<W> {
        crate::sab::SabEngine::telemetry(self)
    }
}

#[cfg(test)]
mod tests {
    use crate::telemetry::{SCOPE_CAPACITY, telemetry};
    use crate::{Engine, ParamIndex};

    /// An engine whose scope, if `own`, is a ramp of its own.
    struct Scoped {
        own: bool,
        shown: Option<(u16, u16)>,
    }

    impl Engine for Scoped {
        fn set_param(&mut self, _: ParamIndex, _: f32) {}
        fn process(&mut self, _: &mut [&mut [f32]]) {}
        fn shown(&mut self, group: u16, page: u16) {
            self.shown = Some((group, page));
        }
        fn scope(&mut self, write: &mut dyn FnMut(&[f32])) {
            if self.own {
                write(&[0.1, 0.2]);
                write(&[0.3]);
            }
        }
    }

    #[test]
    fn an_engines_scope_replaces_the_outputs_and_the_output_still_meters() {
        for own in [false, true] {
            let (mut tap, mut reader) = telemetry();
            let mut engine = Scoped { own, shown: None };
            let out = [0.5_f32, -0.5, 0.25, 0.0];
            tap.write_block(&mut engine, &[&out[..], &out[..]]);
            let mut ring = [0.0; SCOPE_CAPACITY];
            assert!(reader.scope(&mut ring));
            let tail = if own { &[0.1, 0.2, 0.3][..] } else { &out[..] };
            assert_eq!(&ring[SCOPE_CAPACITY - tail.len()..], tail, "own: {own}");
            assert_eq!(reader.take_peaks().levels[0], 0.5, "own: {own}");
        }
    }

    /// Records the engine calls that matter to a patch load.
    #[derive(Default)]
    struct Loads(Vec<&'static str>);

    impl Engine for Loads {
        fn set_param(&mut self, _: ParamIndex, _: f32) {
            self.0.push("value");
        }
        fn patch_load(&mut self) {
            self.0.push("patch");
        }
        fn process(&mut self, _: &mut [&mut [f32]]) {}
    }

    /// A state load, and a preset's mark from the UI, reach the
    /// engine as a patch load ahead of their values.
    #[test]
    fn a_patch_load_reaches_the_engine_ahead_of_its_values() {
        use crate::{EngineEvent, EngineLink, EngineSide};
        let (mut ui, mut end) = crate::bridge(&[0.0, 0.0], 8);
        let mut engine = Loads::default();
        ui.load(&[0.5, 0.25]);
        end.apply_ui(&mut engine);
        assert_eq!(engine.0, ["patch", "value", "value"]);

        engine.0.clear();
        ui.send(EngineEvent::PatchLoad);
        ui.send(EngineEvent::ParamValue {
            index: ParamIndex(1),
            value: 1.0,
        });
        end.apply_ui(&mut engine);
        assert_eq!(engine.0, ["patch", "value"]);
    }

    #[test]
    fn the_shown_page_reaches_the_engine() {
        use crate::{EngineEvent, EngineLink, EngineSide};
        let (mut ui, mut end) = crate::bridge(&[0.0], 8);
        let mut engine = Scoped {
            own: false,
            shown: None,
        };
        ui.send(EngineEvent::Shown { group: 0, page: 2 });
        end.apply_ui(&mut engine);
        assert_eq!(engine.shown, Some((0, 2)));
    }
}
