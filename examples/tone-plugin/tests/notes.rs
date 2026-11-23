//! Note input through clack-host: CLAP note events and raw MIDI reach the
//! engine at their sample offsets, and the note path never allocates.

use silkscreen_bridge::testing::{CountingAllocator, with_allocations};
use silkscreen_clap::test_host::{self, Host, HostAudio};

use clack_extensions::note_ports::{NoteDialects, NotePortInfoBuffer, PluginNotePorts};
use clack_host::events::Match;
use clack_host::events::event_types::{MidiEvent, NoteOffEvent, NoteOnEvent};
use clack_host::prelude::*;
use tone_plugin::TonePlugin;

#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator;

const FRAMES: usize = 4800;
const RATE: f64 = 48_000.0;

fn load() -> (PluginEntry, PluginInstance<Host>) {
    test_host::load::<TonePlugin>()
}

/// One activate/process/deactivate cycle with no audio input; the left
/// output channel and the allocations made inside `process`.
fn run(plugin: &mut PluginInstance<Host>, events: &EventBuffer) -> (Vec<f32>, usize) {
    let config = PluginAudioConfiguration {
        sample_rate: RATE,
        min_frames_count: 1,
        max_frames_count: FRAMES as u32,
    };
    let processor = plugin.activate(|_, _| HostAudio, config).unwrap();
    let mut processor = processor.start_processing().unwrap();
    let mut out_bufs = [vec![0f32; FRAMES], vec![0f32; FRAMES]];
    let mut out_ports = AudioPorts::with_capacity(2, 1);
    let ins = InputAudioBuffers::empty();
    let mut outs = out_ports.with_output_buffers([AudioPortBuffer {
        channels: AudioPortBufferType::f32_output_only(out_bufs.iter_mut().map(Vec::as_mut_slice)),
        latency: 0,
    }]);
    let mut output = EventBuffer::with_capacity(64);
    let input = events.as_input();
    let mut out = output.as_output();
    let (result, n) =
        with_allocations(|| processor.process(&ins, &mut outs, &input, &mut out, None, None));
    result.unwrap();
    plugin.deactivate(processor.stop_processing());
    let [left, _] = out_bufs;
    (left, n)
}

fn pckn(key: u16) -> Pckn {
    Pckn::new(0u16, 0u16, key, Match::All)
}

/// First sample at or after `from` whose magnitude exceeds a whisper.
fn onset(out: &[f32], from: usize) -> Option<usize> {
    (from..out.len()).find(|&i| out[i].abs() > 1e-6)
}

/// Frequency from upward zero crossings over `out[from..to]`.
fn frequency(out: &[f32], from: usize, to: usize) -> f64 {
    let ups: Vec<usize> = (from + 1..to)
        .filter(|&i| out[i - 1] < 0.0 && out[i] >= 0.0)
        .collect();
    let periods = (ups.len() - 1) as f64;
    periods * RATE / (ups[ups.len() - 1] - ups[0]) as f64
}

#[test]
fn one_note_input_port_with_clap_and_midi() {
    let (_entry, mut plugin) = load();
    let mut handle = plugin.plugin_handle();
    let ports = handle.get_extension::<PluginNotePorts>().unwrap();
    assert_eq!(ports.count(&mut handle, true), 1);
    assert_eq!(ports.count(&mut handle, false), 0);
    let mut buf = NotePortInfoBuffer::new();
    let info = ports.get(&mut handle, 0, true, &mut buf).unwrap();
    assert_eq!(
        info.supported_dialects,
        NoteDialects::CLAP | NoteDialects::MIDI
    );
}

#[test]
fn clap_notes_start_and_stop_at_their_sample_offsets() {
    let (_entry, mut plugin) = load();
    let mut events = EventBuffer::new();
    events.push(&NoteOnEvent::new(1000, pckn(69), 1.0));
    events.push(&NoteOffEvent::new(3000, pckn(69), 0.0));
    let (out, allocs) = run(&mut plugin, &events);
    assert_eq!(allocs, 0, "the note path allocated");

    assert!(
        out[..1000].iter().all(|&s| s == 0.0),
        "silent before the note"
    );
    assert!(onset(&out, 1000).is_some_and(|i| i < 1004));
    let f = frequency(&out, 1500, 3000);
    assert!((f - 440.0).abs() < 2.0, "{f}");
    // Released at 3000: a 5 ms decay at the same pitch, to silence.
    assert!(out[2900..3000].iter().any(|s| s.abs() > 0.1));
    let f = frequency(&out, 3000, 3300);
    assert!((f - 440.0).abs() < 5.0, "the tail keeps its pitch: {f}");
    assert!(out[4500..].iter().all(|s| s.abs() < 1e-3));
}

#[test]
fn midi_notes_and_bend_arrive_at_their_sample_offsets() {
    let (_entry, mut plugin) = load();
    let mut events = EventBuffer::new();
    events.push(&MidiEvent::new(0, 0, [0xB0, 1, 64])); // CC passes through
    events.push(&MidiEvent::new(2000, 0, [0x90, 69, 127]));
    events.push(&MidiEvent::new(3200, 0, [0xE0, 0x7F, 0x7F])); // full up
    let (out, allocs) = run(&mut plugin, &events);
    assert_eq!(allocs, 0, "the note path allocated");

    assert!(out[..2000].iter().all(|&s| s == 0.0));
    assert!(onset(&out, 2000).is_some_and(|i| i < 2004));
    let f = frequency(&out, 2400, 3200);
    assert!((f - 440.0).abs() < 2.0, "{f}");
    let bent = 440.0 * 2f64.powf(2.0 / 12.0);
    let f = frequency(&out, 3300, FRAMES);
    assert!((f - bent).abs() < 2.0, "{f} vs {bent}");
}

#[test]
fn notes_on_another_port_are_ignored() {
    let (_entry, mut plugin) = load();
    let mut events = EventBuffer::new();
    events.push(&NoteOnEvent::new(
        0,
        Pckn::new(1u16, 0u16, 60u16, Match::All),
        1.0,
    ));
    events.push(&MidiEvent::new(0, 1, [0x90, 60, 100]));
    let (out, _) = run(&mut plugin, &events);
    assert!(out.iter().all(|&s| s == 0.0));
}

#[test]
fn the_allocation_counter_counts() {
    let (v, n) = with_allocations(|| std::hint::black_box(vec![0u8; 16]));
    drop(v);
    assert_eq!(n, 1);
}
