//! The params extension, through clack-host.

use clack_extensions::params::{ParamInfoBuffer, PluginParams};
use clack_host::events::event_types::ParamValueEvent;
use clack_host::prelude::*;
use clack_host::utils::Cookie;
use gain_plugin::GainPlugin;
use silkscreen_clap::test_host::{self, Host, HostAudio};

const GAIN_ID: u32 = 1;
const FRAMES: usize = 4800;

fn load() -> (PluginEntry, PluginInstance<Host>) {
    test_host::load::<GainPlugin>()
}

fn gain_event(db: f64) -> ParamValueEvent {
    gain_event_at(0, db)
}

fn gain_event_at(time: u32, db: f64) -> ParamValueEvent {
    ParamValueEvent::new(
        time,
        ClapId::new(GAIN_ID),
        Pckn::match_all(),
        db,
        Cookie::empty(),
    )
}

/// One activate/process/deactivate cycle; returns the last output sample of
/// the left channel (input is all 1.0).
fn run(plugin: &mut PluginInstance<Host>, events: &EventBuffer) -> f32 {
    run_full(plugin, events)[FRAMES - 1]
}

/// As [`run`], returning the whole left channel.
fn run_full(plugin: &mut PluginInstance<Host>, events: &EventBuffer) -> Vec<f32> {
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
    let mut output_events = EventBuffer::new();
    processor
        .process(
            &ins,
            &mut outs,
            &events.as_input(),
            &mut output_events.as_output(),
            None,
            None,
        )
        .unwrap();
    plugin.deactivate(processor.stop_processing());
    let [left, _] = out_bufs;
    left
}

#[test]
fn info_text_and_value() {
    let (_entry, mut plugin) = load();
    let mut handle = plugin.plugin_handle();
    let params = handle.get_extension::<PluginParams>().unwrap();
    assert_eq!(params.count(&mut handle), 1);

    let mut buf = ParamInfoBuffer::new();
    let info = params.get_info(&mut handle, 0, &mut buf).unwrap();
    assert_eq!(info.id, ClapId::new(GAIN_ID));
    assert_eq!(info.name, b"Gain");
    assert_eq!(
        (info.min_value, info.max_value, info.default_value),
        (-60.0, 12.0, 0.0)
    );

    assert_eq!(
        params.get_value(&mut handle, ClapId::new(GAIN_ID)),
        Some(0.0)
    );
    assert_eq!(params.get_value(&mut handle, ClapId::new(99)), None);

    let mut text = [0u8; 64];
    let shown = params
        .value_to_text(&mut handle, ClapId::new(GAIN_ID), -6.0, &mut text)
        .unwrap();
    assert_eq!(shown, b"-6.0 dB");
    assert_eq!(
        params.text_to_value(&mut handle, ClapId::new(GAIN_ID), c"-12.5 dB"),
        Some(-12.5)
    );
    assert_eq!(
        params.text_to_value(&mut handle, ClapId::new(GAIN_ID), c"loud"),
        None
    );
}

#[test]
fn automation_reaches_audio_and_the_model() {
    let (_entry, mut plugin) = load();
    let mut events = EventBuffer::new();
    events.push(&gain_event(-6.0));
    let last = run(&mut plugin, &events);
    // An automation step glides: after 100 ms of 20 ms smoothing, within
    // e^-5 of the 1.0 → 0.501 jump.
    let target = 10f32.powf(-6.0 / 20.0);
    assert!(
        (last - target).abs() < (1.0 - target) * (-4.9f32).exp(),
        "{last}"
    );

    let mut handle = plugin.plugin_handle();
    let params = handle.get_extension::<PluginParams>().unwrap();
    assert_eq!(
        params.get_value(&mut handle, ClapId::new(GAIN_ID)),
        Some(-6.0)
    );

    // The next activation starts *at* it (no glide), with no new events.
    let last = run(&mut plugin, &EventBuffer::new());
    assert!((last - 10f32.powf(-6.0 / 20.0)).abs() < 1e-3, "{last}");
}

#[test]
fn main_thread_flush_while_inactive() {
    let (_entry, mut plugin) = load();
    let mut events = EventBuffer::new();
    events.push(&gain_event(6.0));
    {
        let mut inactive = plugin.inactive_plugin_handle().expect("not active");
        let params = inactive.get_extension::<PluginParams>().unwrap();
        let mut out = EventBuffer::new();
        params.flush(&mut inactive, &events.as_input(), &mut out.as_output());
    }
    {
        let mut handle = plugin.plugin_handle();
        let params = handle.get_extension::<PluginParams>().unwrap();
        assert_eq!(
            params.get_value(&mut handle, ClapId::new(GAIN_ID)),
            Some(6.0)
        );
    }
    let last = run(&mut plugin, &EventBuffer::new());
    assert!((last - 10f32.powf(6.0 / 20.0)).abs() < 1e-3, "{last}");
}

#[test]
fn out_of_range_values_are_clamped() {
    let (_entry, mut plugin) = load();
    let mut events = EventBuffer::new();
    events.push(&gain_event(100.0));
    run(&mut plugin, &events);
    let mut handle = plugin.plugin_handle();
    let params = handle.get_extension::<PluginParams>().unwrap();
    assert_eq!(
        params.get_value(&mut handle, ClapId::new(GAIN_ID)),
        Some(12.0)
    );
}

#[test]
fn host_events_are_sample_accurate() {
    let (_entry, mut plugin) = load();
    let mut events = EventBuffer::new();
    events.push(&gain_event_at(2400, -60.0));
    let out = run_full(&mut plugin, &events);
    assert!(
        out[..2400].iter().all(|&s| s == 1.0),
        "unity before the event"
    );
    assert!(out[2400] < 1.0, "the glide starts exactly at the event");
    assert!(out[FRAMES - 1] < 0.1);
}
