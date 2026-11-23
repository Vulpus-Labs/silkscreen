//! The plugin's process call must not allocate (design/engine-bridge.md): the bridge
//! drain, host events, output events and the engine, all through clack-host.

use silkscreen_bridge::testing::{CountingAllocator, allocations};
use silkscreen_clap::test_host::{self, HostAudio};

use clack_host::events::event_types::ParamValueEvent;
use clack_host::prelude::*;
use clack_host::utils::Cookie;
use gain_plugin::GainPlugin;

#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator;

const FRAMES: usize = 256;

#[test]
fn process_never_allocates() {
    let (_entry, mut plugin) = test_host::load::<GainPlugin>();
    let config = PluginAudioConfiguration {
        sample_rate: 48_000.0,
        min_frames_count: 1,
        max_frames_count: FRAMES as u32,
    };
    let processor = plugin.activate(|_, _| HostAudio, config).unwrap();
    let mut processor = processor.start_processing().unwrap();

    let mut in_bufs = [vec![1f32; FRAMES], vec![1f32; FRAMES]];
    let mut out_bufs = [vec![0f32; FRAMES], vec![0f32; FRAMES]];
    let mut in_ports = AudioPorts::with_capacity(2, 1);
    let mut out_ports = AudioPorts::with_capacity(2, 1);
    let mut input_events = EventBuffer::with_capacity(16);
    for (t, db) in [(0, -3.0), (64, -6.0), (200, 3.0)] {
        input_events.push(&ParamValueEvent::new(
            t,
            ClapId::new(1),
            Pckn::match_all(),
            db,
            Cookie::empty(),
        ));
    }
    let mut output_events = EventBuffer::with_capacity(16);

    let ins = in_ports.with_input_buffers([AudioPortBuffer {
        channels: AudioPortBufferType::f32_input_only(
            in_bufs.iter_mut().map(InputChannel::variable),
        ),
        latency: 0,
    }]);
    let mut outs = out_ports.with_output_buffers([AudioPortBuffer {
        channels: AudioPortBufferType::f32_output_only(out_bufs.iter_mut().map(Vec::as_mut_slice)),
        latency: 0,
    }]);

    // Warm up once (host-side buffers may grow on first use), then count.
    let input = input_events.as_input();
    processor
        .process(
            &ins,
            &mut outs,
            &input,
            &mut output_events.as_output(),
            None,
            None,
        )
        .unwrap();
    output_events.clear();

    let n = allocations(|| {
        processor
            .process(
                &ins,
                &mut outs,
                &input,
                &mut output_events.as_output(),
                None,
                None,
            )
            .unwrap();
    });
    assert_eq!(n, 0, "process allocated {n} times");

    plugin.deactivate(processor.stop_processing());
}

#[test]
fn the_counter_sees_allocations() {
    let n = allocations(|| drop(std::hint::black_box(vec![1u8; 16])));
    assert_eq!(n, 1);
}

/// A licensed copy of the gain plugin that is never unlocked.
struct Locked;

impl silkscreen_clap::SilkscreenPlugin for Locked {
    const ID: &'static str = "test.licence.no-alloc";
    const NAME: &'static str = "locked gain";
    const VERSION: &'static str = gain::UI.version;
    const FEATURES: &'static [&'static std::ffi::CStr] = &[silkscreen_clap::features::AUDIO_EFFECT];
    const AUDIO_INPUT: bool = true;
    const UI: silkscreen::PluginUi = gain::UI;
    const LICENCE: Option<silkscreen_clap::CheckCode> = Some(|_| Err("never".into()));

    type Engine = gain::engine::GainEngine;

    fn licence_file() -> Option<std::path::PathBuf> {
        None
    }

    fn engine(sample_rate: f32, _max_frames: u32) -> gain::engine::GainEngine {
        gain::engine::GainEngine::new(sample_rate)
    }
}

/// The soft lock's gate (design/licensing-hook.md) doesn't allocate either, in or out
/// of a gap.
#[test]
fn a_locked_plugin_never_allocates_through_a_gap() {
    const RATE: usize = 8000;
    const BLOCK: usize = 800;
    let (_entry, mut plugin) = test_host::load::<Locked>();
    let config = PluginAudioConfiguration {
        sample_rate: RATE as f64,
        min_frames_count: 1,
        max_frames_count: BLOCK as u32,
    };
    let processor = plugin.activate(|_, _| HostAudio, config).unwrap();
    let mut processor = processor.start_processing().unwrap();

    let mut in_bufs = [vec![1f32; BLOCK], vec![1f32; BLOCK]];
    let mut out_bufs = [vec![0f32; BLOCK], vec![0f32; BLOCK]];
    let mut in_ports = AudioPorts::with_capacity(2, 1);
    let mut out_ports = AudioPorts::with_capacity(2, 1);
    let input_events = EventBuffer::with_capacity(16);
    let mut output_events = EventBuffer::with_capacity(16);
    let ins = in_ports.with_input_buffers([AudioPortBuffer {
        channels: AudioPortBufferType::f32_input_only(
            in_bufs.iter_mut().map(InputChannel::variable),
        ),
        latency: 0,
    }]);
    let mut outs = out_ports.with_output_buffers([AudioPortBuffer {
        channels: AudioPortBufferType::f32_output_only(out_bufs.iter_mut().map(Vec::as_mut_slice)),
        latency: 0,
    }]);
    let input = input_events.as_input();

    // 62 s: through the first gap (60.00–61.52 s), counting every block.
    let blocks = 62 * RATE / BLOCK;
    let mut total = 0;
    for i in 0..blocks {
        let n = allocations(|| {
            processor
                .process(
                    &ins,
                    &mut outs,
                    &input,
                    &mut output_events.as_output(),
                    None,
                    None,
                )
                .unwrap();
        });
        // The first block may warm host-side buffers.
        if i > 0 {
            total += n;
        }
        output_events.clear();
    }
    assert_eq!(total, 0, "process allocated {total} times");
    plugin.deactivate(processor.stop_processing());
}
