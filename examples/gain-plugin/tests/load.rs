//! Load the plugin in-process through clack-host, as a DAW would.

use clack_extensions::audio_ports::{AudioPortInfoBuffer, PluginAudioPorts};
use clack_host::factory::plugin::PluginFactory;
use clack_host::prelude::*;
use clack_plugin::entry::SinglePluginEntry;
use gain_plugin::GainPlugin;
use silkscreen_clap::Clap;
use silkscreen_clap::test_host::{self, Host, HostAudio};

const FRAMES: usize = 64;

fn load() -> (PluginEntry, PluginInstance<Host>) {
    test_host::load::<GainPlugin>()
}

/// Activate, run one block of `input` through, deactivate; returns the
/// output.
fn run_block(plugin: &mut PluginInstance<Host>, input: &[[f32; FRAMES]; 2]) -> [[f32; FRAMES]; 2] {
    let config = PluginAudioConfiguration {
        sample_rate: 48_000.0,
        min_frames_count: FRAMES as u32,
        max_frames_count: FRAMES as u32,
    };
    let processor = plugin.activate(|_, _| HostAudio, config).unwrap();
    let mut processor = processor.start_processing().unwrap();

    let mut in_bufs = *input;
    let mut out_bufs = [[0f32; FRAMES]; 2];
    let mut in_ports = AudioPorts::with_capacity(2, 1);
    let mut out_ports = AudioPorts::with_capacity(2, 1);
    let ins = in_ports.with_input_buffers([AudioPortBuffer {
        channels: AudioPortBufferType::f32_input_only(
            in_bufs.iter_mut().map(InputChannel::variable),
        ),
        latency: 0,
    }]);
    let mut outs = out_ports.with_output_buffers([AudioPortBuffer {
        channels: AudioPortBufferType::f32_output_only(
            out_bufs.iter_mut().map(<[f32; 64]>::as_mut_slice),
        ),
        latency: 0,
    }]);
    let input_events = EventBuffer::new();
    let mut output_events = EventBuffer::new();
    processor
        .process(
            &ins,
            &mut outs,
            &input_events.as_input(),
            &mut output_events.as_output(),
            None,
            None,
        )
        .unwrap();
    plugin.deactivate(processor.stop_processing());
    out_bufs
}

#[test]
fn descriptor_and_ports() {
    let entry = PluginEntry::load_from_clack::<SinglePluginEntry<Clap<GainPlugin>>>(c"").unwrap();
    let descriptor = entry
        .get_factory::<PluginFactory>()
        .unwrap()
        .plugin_descriptor(0)
        .unwrap();
    assert_eq!(
        descriptor.id().unwrap().to_bytes(),
        b"com.example.silkscreen.gain"
    );
    assert_eq!(descriptor.name().unwrap().to_bytes(), b"silkscreen Gain");
    assert_eq!(descriptor.vendor().unwrap().to_bytes(), b"Example Audio");
    let features: Vec<_> = descriptor
        .features()
        .map(std::ffi::CStr::to_bytes)
        .collect();
    assert_eq!(features, [&b"audio-effect"[..], &b"stereo"[..]]);

    let (_entry, mut plugin) = load();
    let mut handle = plugin.plugin_handle();
    let ports = handle.get_extension::<PluginAudioPorts>().unwrap();
    assert_eq!(ports.count(&mut handle, true), 1);
    assert_eq!(ports.count(&mut handle, false), 1);
    let mut buf = AudioPortInfoBuffer::new();
    let out = ports.get(&mut handle, 0, false, &mut buf).unwrap();
    assert_eq!(out.channel_count, 2);
}

#[test]
fn passes_audio_at_unity_by_default() {
    let (_entry, mut plugin) = load();
    let mut input = [[0f32; FRAMES]; 2];
    for (i, s) in input[0].iter_mut().enumerate() {
        *s = (i as f32 * 0.1).sin();
    }
    input[1] = input[0].map(|s| -s);
    let output = run_block(&mut plugin, &input);
    assert_eq!(output, input);
}

#[test]
fn survives_repeated_activation() {
    // The bridge's engine end moves into the processor on activate and back
    // on deactivate; a second activation must find it again.
    let (_entry, mut plugin) = load();
    let input = [[0.5f32; FRAMES]; 2];
    for _ in 0..3 {
        assert_eq!(run_block(&mut plugin, &input), input);
    }
}
