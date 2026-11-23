//! The audio-thread half of the plugin.

use std::ops::Bound;
use std::sync::atomic::Ordering;

use clack_plugin::prelude::*;
use silkscreen::bridge::{DenormalGuard, Engine, EngineEvent, EngineSide};

use crate::licence::Gated;
use crate::link::Link;
use crate::{MainThread, Shared, SilkscreenPlugin, notes, params, transport};

pub struct Processor<'a, P: SilkscreenPlugin> {
    /// The plugin's engine, behind the soft lock if it is licensed.
    engine: Gated<P::Engine>,
    link: Option<Link>,
    shared: &'a Shared<P>,
}

impl<P: SilkscreenPlugin> Processor<'_, P> {
    /// Apply everything the UI has sent and report it to the host.
    fn drain_ui(&mut self, output: &mut OutputEvents) {
        let engine = &mut self.engine;
        if let Some(link) = self.link.as_mut() {
            link.ui_to_host(P::UI.specs, output, |event| match event {
                EngineEvent::ParamValue { index, value } => engine.set_param(index, value),
                EngineEvent::Shown { group, page } => engine.shown(group, page),
                EngineEvent::PatchLoad => engine.patch_load(),
                EngineEvent::GestureBegin { .. } | EngineEvent::GestureEnd { .. } => {}
            });
        }
    }

    /// Apply a host input event: param values go to the engine and are
    /// published back for the model (design/engine-bridge.md); notes go to the engine
    /// only.
    fn host_event(&mut self, event: &UnknownEvent) {
        if P::NOTE_INPUT
            && let Some(note) = notes::note_event(event)
        {
            self.engine.note(note);
            return;
        }
        if let Some((index, value)) = params::param_value(event, &self.shared.ids, P::UI.specs) {
            self.engine.set_param(index, value);
            if let Some(link) = self.link.as_mut() {
                link.end.publish_host(index, value);
            }
        }
    }

    pub(crate) fn flush(&mut self, input: &InputEvents, output: &mut OutputEvents) {
        self.drain_ui(output);
        for event in input {
            self.host_event(event);
        }
    }
}

/// A batch's sample range, clamped to the block.
fn batch_range((start, end): (Bound<usize>, Bound<usize>), frames: usize) -> (usize, usize) {
    let s = match start {
        Bound::Included(n) => n,
        Bound::Excluded(n) => n + 1,
        Bound::Unbounded => 0,
    };
    let e = match end {
        Bound::Included(n) => n + 1,
        Bound::Excluded(n) => n,
        Bound::Unbounded => frames,
    };
    (s.min(frames), e.min(frames))
}

impl<'a, P: SilkscreenPlugin> PluginAudioProcessor<'a, Shared<P>, MainThread<'a, P>>
    for Processor<'a, P>
{
    fn activate(
        _host: HostAudioProcessorHandle<'a>,
        main_thread: &mut MainThread<'a, P>,
        shared: &'a Shared<P>,
        config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        let link = main_thread
            .link
            .take()
            .ok_or(PluginError::Message("activated twice"))?;
        let mut engine = Gated::new(
            P::engine(config.sample_rate as f32, config.max_frames_count),
            shared.unlocked,
            config.sample_rate as f32,
        );
        // Seed the engine with every current value before the first block,
        // including a state load not yet taken (a project opening).
        // Start *at* those values: smoothers snap rather than glide from
        // whatever the constructor chose.
        link.end.seed_engine(&mut engine);
        link.replay_shown(|group, page| engine.shown(group, page));
        Ok(Self {
            engine,
            link: Some(link),
            shared,
        })
    }

    fn deactivate(mut self, main_thread: &mut MainThread<'a, P>) {
        main_thread.link = self.link.take();
    }

    fn process(
        &mut self,
        process: Process,
        mut audio: Audio,
        events: Events,
    ) -> Result<ProcessStatus, PluginError> {
        // Subnormals flush to zero for the whole call, engine included
        // (design/engine-bridge.md); the host's mode comes back on return.
        let _fp = DenormalGuard::new();
        self.engine.begin_block();
        self.drain_ui(events.output);
        let transport = transport::from_clap(process.transport);
        self.engine.transport(&transport);
        if P::DIAGNOSTICS {
            self.shared
                .tempo
                .store(transport.tempo.to_bits(), Ordering::Relaxed);
            self.shared
                .playing
                .store(transport.playing, Ordering::Relaxed);
        }

        let Some(mut pair) = audio.port_pair(0) else {
            for event in events.input {
                self.host_event(event);
            }
            return Ok(ProcessStatus::Continue);
        };
        let mut channels = pair
            .channels()?
            .into_f32()
            .ok_or(PluginError::Message("expected f32 audio"))?;

        // Gather up to two output channels, pre-filled with the input (or
        // silence), without allocating.
        let mut slots: [&mut [f32]; 2] = [&mut [], &mut []];
        let mut n = 0;
        for channel in channels.iter_mut() {
            if n == slots.len() {
                break;
            }
            let out: &mut [f32] = match channel {
                ChannelPair::InputOnly(_) => continue,
                ChannelPair::OutputOnly(o) => {
                    o.fill(0.0);
                    o
                }
                ChannelPair::InPlace(b) => b,
                ChannelPair::InputOutput(i, o) => {
                    o.copy_from_slice(i);
                    o
                }
            };
            slots[n] = out;
            n += 1;
        }
        let frames = slots[..n].iter().map(|c| c.len()).min().unwrap_or(0);

        // Sample-accurate: run the engine up to each event time, then apply
        // the events there.
        for batch in events.input.batch() {
            for event in batch.events() {
                self.host_event(event);
            }
            let (start, end) = batch_range(batch.sample_bounds(), frames);
            if start < end {
                let mut sub: [&mut [f32]; 2] = [&mut [], &mut []];
                for (dst, src) in sub.iter_mut().zip(slots[..n].iter_mut()) {
                    *dst = &mut src[start..end];
                }
                self.engine.process(&mut sub[..n]);
            }
        }
        // The block's output, for the editor's scope and meters (design/engine-bridge.md).
        if let Some(link) = self.link.as_mut() {
            link.end
                .telemetry()
                .write_block(&mut self.engine, &slots[..n]);
        }

        Ok(if P::AUDIO_INPUT {
            ProcessStatus::ContinueIfNotQuiet
        } else {
            ProcessStatus::Continue
        })
    }

    fn reset(&mut self) {
        self.engine.reset();
    }

    fn start_processing(&mut self) -> Result<(), PluginError> {
        self.shared.processing.store(true, Ordering::Release);
        Ok(())
    }

    fn stop_processing(&mut self) {
        self.shared.processing.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_ranges_clamp_to_the_block() {
        use Bound::*;
        assert_eq!(batch_range((Included(0), Excluded(10)), 64), (0, 10));
        assert_eq!(batch_range((Included(10), Unbounded), 64), (10, 64));
        assert_eq!(batch_range((Unbounded, Included(9)), 64), (0, 10));
        assert_eq!(batch_range((Included(60), Excluded(100)), 64), (60, 64));
    }
}
