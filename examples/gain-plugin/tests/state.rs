//! The state extension, through clack-host: save, mutate, load. Includes
//! the editor zoom set through the gui extension.

use clack_extensions::gui::{GuiSize, PluginGui};
use clack_extensions::params::{ParamRescanFlags, PluginParams};
use clack_extensions::state::PluginState;
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
    ParamValueEvent::new(
        0,
        ClapId::new(GAIN_ID),
        Pckn::match_all(),
        db,
        Cookie::empty(),
    )
}

fn amp(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

fn activate(plugin: &mut PluginInstance<Host>) -> StartedPluginAudioProcessor<Host> {
    let config = PluginAudioConfiguration {
        sample_rate: 48_000.0,
        min_frames_count: 1,
        max_frames_count: FRAMES as u32,
    };
    let processor = plugin.activate(|_, _| HostAudio, config).unwrap();
    processor.start_processing().unwrap()
}

/// One block of all-1.0 input: the left output channel and the output
/// events.
fn process(
    processor: &mut StartedPluginAudioProcessor<Host>,
    events: &EventBuffer,
) -> (Vec<f32>, EventBuffer) {
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
    let mut output = EventBuffer::new();
    processor
        .process(
            &ins,
            &mut outs,
            &events.as_input(),
            &mut output.as_output(),
            None,
            None,
        )
        .unwrap();
    let [left, _] = out_bufs;
    (left, output)
}

/// One activate/process/deactivate cycle; the last output sample.
fn run(plugin: &mut PluginInstance<Host>, events: &EventBuffer) -> f32 {
    let mut processor = activate(plugin);
    let (left, _) = process(&mut processor, events);
    plugin.deactivate(processor.stop_processing());
    left[FRAMES - 1]
}

fn automate(plugin: &mut PluginInstance<Host>, db: f64) {
    let mut events = EventBuffer::new();
    events.push(&gain_event(db));
    run(plugin, &events);
}

fn save(plugin: &mut PluginInstance<Host>) -> Vec<u8> {
    let mut handle = plugin.plugin_handle();
    let state = handle.get_extension::<PluginState>().unwrap();
    let mut bytes = Vec::new();
    state.save(&mut handle, &mut bytes).unwrap();
    bytes
}

fn restore(plugin: &mut PluginInstance<Host>, bytes: &[u8]) -> bool {
    let mut handle = plugin.plugin_handle();
    let state = handle.get_extension::<PluginState>().unwrap();
    state.load(&mut handle, &mut &bytes[..]).is_ok()
}

fn gain(plugin: &mut PluginInstance<Host>) -> f64 {
    let mut handle = plugin.plugin_handle();
    let params = handle.get_extension::<PluginParams>().unwrap();
    params.get_value(&mut handle, ClapId::new(GAIN_ID)).unwrap()
}

#[test]
fn save_mutate_load_restores_model_and_audio() {
    let (_entry, mut plugin) = load();
    automate(&mut plugin, -6.0);
    let saved = save(&mut plugin);

    automate(&mut plugin, 6.0);
    assert_eq!(gain(&mut plugin), 6.0);

    assert!(restore(&mut plugin, &saved));
    assert_eq!(gain(&mut plugin), -6.0);
    // Loaded while inactive: the next activation starts at it, no glide.
    let last = run(&mut plugin, &EventBuffer::new());
    assert!((last - amp(-6.0)).abs() < 1e-4, "{last}");
}

#[test]
fn a_fresh_instance_loads_another_ones_state() {
    let (_entry, mut a) = load();
    automate(&mut a, -12.0);
    let saved = save(&mut a);

    let (_entry, mut b) = load();
    assert!(restore(&mut b, &saved));
    assert_eq!(gain(&mut b), -12.0);
    // The host re-reads the values the load changed (clap-validator's
    // state-reproducibility tests).
    assert_eq!(test_host::rescans(&b), [ParamRescanFlags::VALUES]);
    let last = run(&mut b, &EventBuffer::new());
    assert!((last - amp(-12.0)).abs() < 1e-4, "{last}");
}

#[test]
fn loading_while_active_reaches_the_engine_without_echo() {
    let (_entry, mut plugin) = load();
    let saved = save(&mut plugin); // the default, 0 dB
    automate(&mut plugin, -20.0);

    let mut processor = activate(&mut plugin);
    let (left, _) = process(&mut processor, &EventBuffer::new());
    assert!((left[FRAMES - 1] - amp(-20.0)).abs() < 1e-4);

    assert!(restore(&mut plugin, &saved));
    assert_eq!(gain(&mut plugin), 0.0);
    let (left, output) = process(&mut processor, &EventBuffer::new());
    // Applied like any change: a 20 ms glide, settled within the block.
    assert!(
        (left[FRAMES - 1] - 1.0).abs() < 1e-2,
        "{}",
        left[FRAMES - 1]
    );
    assert_eq!(
        output.len(),
        0,
        "the host chose this state; it isn't told of it as edits"
    );
    plugin.deactivate(processor.stop_processing());
}

#[test]
fn malformed_state_is_refused_and_changes_nothing() {
    let (_entry, mut plugin) = load();
    automate(&mut plugin, -6.0);
    let mut saved = save(&mut plugin);
    assert!(!restore(&mut plugin, b"not ours"));
    assert!(!restore(&mut plugin, &saved[..saved.len() - 2]));
    saved[4] = 99; // a version from the future
    assert!(!restore(&mut plugin, &saved));
    assert_eq!(gain(&mut plugin), -6.0);
    assert!(test_host::rescans(&plugin).is_empty());
}

fn gui_size(plugin: &mut PluginInstance<Host>) -> GuiSize {
    let mut handle = plugin.plugin_handle();
    let gui = handle.get_extension::<PluginGui>().unwrap();
    gui.get_size(&mut handle).unwrap()
}

#[test]
fn zoom_is_resizable_snapped_and_saved() {
    let (_entry, mut a) = load();
    let base = gui_size(&mut a);
    let scaled = |z: f32| GuiSize {
        width: (base.width as f32 * z).round() as u32,
        height: (base.height as f32 * z).round() as u32,
    };
    {
        let mut handle = a.plugin_handle();
        let gui = handle.get_extension::<PluginGui>().unwrap();
        assert!(gui.can_resize(&mut handle));
        // A drag to roughly 140% snaps to 150%.
        let adjusted = gui.adjust_size(&mut handle, scaled(1.4)).unwrap();
        assert_eq!(adjusted, scaled(1.5));
        gui.set_size(&mut handle, adjusted).unwrap();
    }
    assert_eq!(gui_size(&mut a), scaled(1.5));
    let saved = save(&mut a);

    let (_entry, mut b) = load();
    assert_eq!(gui_size(&mut b), base);
    assert!(restore(&mut b, &saved));
    assert_eq!(gui_size(&mut b), scaled(1.5), "zoom survives save/load");
}
