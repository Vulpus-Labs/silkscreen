//! The soft lock (design/licensing-hook.md), through clack-host: a licensed
//! copy of the gain plugin, locked or unlocked, against the plain one.
//!
//! The unlocked flag is process-wide per plugin id and tests run in
//! parallel, so each test has its own plugin type, id and licence file.

use std::ffi::CStr;
use std::path::PathBuf;

use clack_extensions::state::PluginState;
use clack_host::prelude::*;
use gain::engine::GainEngine;
use gain_plugin::GainPlugin;
use silkscreen::PluginUi;
use silkscreen::bridge::Engine;
use silkscreen_clap::test_host::{self, Host, HostAudio};
use silkscreen_clap::{SilkscreenPlugin, UnlockError, features, is_unlocked, try_unlock};

const RATE: f32 = 8000.0;
const FRAMES: usize = 4000;
/// Enough blocks for two gaps: 125 s.
const BLOCKS: usize = 250;

const MALFORMED: &str = "That isn't a licence code.";
const WRONG_PRODUCT: &str = "That code is for a different product.";

/// The test plugins' code check, standing in for a real scheme: the
/// shell only needs a yes, or a no with a reason. `GAIN-<n>` unlocks,
/// `OTHER-<n>` is another product's.
fn check(code: &str) -> Result<(), String> {
    let code = code.trim();
    let serial = |prefix: &str| {
        code.strip_prefix(prefix)
            .is_some_and(|n| n.parse::<u32>().is_ok())
    };
    if serial("GAIN-") {
        Ok(())
    } else if serial("OTHER-") {
        Err(WRONG_PRODUCT.into())
    } else {
        Err(MALFORMED.into())
    }
}

/// A code for `product` (9 is the gain plugin's) with `serial`.
fn code(product: u8, serial: u32) -> String {
    let prefix = if product == 9 { "GAIN" } else { "OTHER" };
    format!("{prefix}-{serial}")
}

/// A folder for one test's licence file, emptied first.
fn folder(id: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("silkscreen-licence-{id}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

macro_rules! licensed {
    ($name:ident, $id:literal) => {
        struct $name;

        impl SilkscreenPlugin for $name {
            const ID: &'static str = $id;
            const NAME: &'static str = "licensed gain";
            const VERSION: &'static str = gain::UI.version;
            const FEATURES: &'static [&'static CStr] = &[features::AUDIO_EFFECT];
            const AUDIO_INPUT: bool = true;
            const UI: PluginUi = gain::UI;
            const LICENCE: Option<silkscreen_clap::CheckCode> = Some(check);

            type Engine = GainEngine;

            fn licence_file() -> Option<PathBuf> {
                Some(folder_of($id).join("licence.txt"))
            }

            fn engine(sample_rate: f32, _max_frames: u32) -> GainEngine {
                GainEngine::new(sample_rate)
            }
        }
    };
}

/// The licence folder for plugin id `id`, without emptying it.
fn folder_of(id: &str) -> PathBuf {
    std::env::temp_dir().join(format!("silkscreen-licence-{id}-{}", std::process::id()))
}

/// Writes `text` as `id`'s saved licence code.
fn save_code(id: &str, text: &str) {
    let dir = folder(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("licence.txt"), text).unwrap();
}

struct Run {
    processor: StartedPluginAudioProcessor<Host>,
}

impl Run {
    fn start(plugin: &mut PluginInstance<Host>) -> Self {
        let config = PluginAudioConfiguration {
            sample_rate: f64::from(RATE),
            min_frames_count: 1,
            max_frames_count: FRAMES as u32,
        };
        let processor = plugin.activate(|_, _| HostAudio, config).unwrap();
        Self {
            processor: processor.start_processing().unwrap(),
        }
    }

    /// `blocks` blocks of all-1.0 input; the left output.
    fn blocks(&mut self, blocks: usize) -> Vec<f32> {
        let mut all = Vec::with_capacity(blocks * FRAMES);
        for _ in 0..blocks {
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
                channels: AudioPortBufferType::f32_output_only(
                    out_bufs.iter_mut().map(Vec::as_mut_slice),
                ),
                latency: 0,
            }]);
            let events = EventBuffer::new();
            let mut output = EventBuffer::new();
            self.processor
                .process(
                    &ins,
                    &mut outs,
                    &events.as_input(),
                    &mut output.as_output(),
                    None,
                    None,
                )
                .unwrap();
            let [left, right] = out_bufs;
            assert_eq!(left, right);
            all.extend(left);
        }
        all
    }

    fn stop(self, plugin: &mut PluginInstance<Host>) {
        plugin.deactivate(self.processor.stop_processing());
    }
}

/// `BLOCKS` blocks through a fresh activation of `plugin`.
fn render(plugin: &mut PluginInstance<Host>) -> Vec<f32> {
    let mut run = Run::start(plugin);
    let out = run.blocks(BLOCKS);
    run.stop(plugin);
    out
}

fn at(seconds: f64) -> usize {
    (seconds * f64::from(RATE)).round() as usize
}

/// Asserts `out` has the gap starting at `start` seconds: smooth fades
/// either side of 1.5 s of silence, full level just outside.
fn assert_gap(out: &[f32], start: f64) {
    let (s, fade, hold) = (at(start), at(0.010), at(1.5));
    assert_eq!(out[s - 1], 1.0, "full level before the gap at {start} s");
    let down = &out[s..=s + fade];
    assert!(
        down.windows(2).all(|w| w[1] <= w[0]),
        "fade out at {start} s"
    );
    assert!(
        down.windows(2).all(|w| w[0] - w[1] < 0.05),
        "no step in the fade out at {start} s"
    );
    assert!(
        out[s + fade..s + fade + hold].iter().all(|&x| x == 0.0),
        "silence through the gap at {start} s"
    );
    let up = &out[s + fade + hold..=s + 2 * fade + hold];
    assert!(up.windows(2).all(|w| w[1] >= w[0]), "fade in at {start} s");
    assert!(up.windows(2).all(|w| w[1] - w[0] < 0.05));
    assert_eq!(
        out[s + 2 * fade + hold],
        1.0,
        "full level after the gap at {start} s"
    );
}

/// Where the output is not at full level.
fn dips(out: &[f32]) -> Vec<usize> {
    out.iter()
        .enumerate()
        .filter(|&(_, &x)| x != 1.0)
        .map(|(i, _)| i)
        .collect()
}

licensed!(Locked, "test.licence.locked");

#[test]
fn locked_output_gaps_at_60_and_120_seconds() {
    folder(Locked::ID);
    let (_entry, mut plugin) = test_host::load::<Locked>();
    assert!(!is_unlocked::<Locked>());
    let out = render(&mut plugin);
    assert_gap(&out, 60.0);
    assert_gap(&out, 120.0);
    let gap = at(0.020) + at(1.5);
    assert_eq!(
        dips(&out).len(),
        2 * (gap - 1),
        "level is full outside the two gaps"
    );
    assert_eq!(*dips(&out).first().unwrap(), at(60.0) + 1);
}

licensed!(Unlocked, "test.licence.unlocked");

#[test]
fn unlocked_output_is_the_plain_plugins() {
    save_code(Unlocked::ID, &code(9, 1));
    let (_entry, mut plugin) = test_host::load::<Unlocked>();
    assert!(is_unlocked::<Unlocked>());
    let (_e, mut plain) = test_host::load::<GainPlugin>();
    let out = render(&mut plugin);
    assert_eq!(out, render(&mut plain));
    assert!(dips(&out).is_empty());
}

#[test]
fn without_a_licence_the_output_is_the_engines_bit_for_bit() {
    let (_entry, mut plugin) = test_host::load::<GainPlugin>();
    assert!(is_unlocked::<GainPlugin>());
    let out = render(&mut plugin);

    let mut engine = GainEngine::new(RATE);
    engine.set_param(gain::GAIN, 0.0);
    engine.reset();
    let mut expected = Vec::new();
    for _ in 0..BLOCKS {
        let (mut l, mut r) = (vec![1f32; FRAMES], vec![1f32; FRAMES]);
        engine.process(&mut [&mut l, &mut r]);
        expected.extend(l);
    }
    assert_eq!(out, expected);
}

licensed!(Saved, "test.licence.state");

#[test]
fn the_lock_leaves_saved_state_alone() {
    folder(Saved::ID);
    let save = |plugin: &mut PluginInstance<Host>| {
        let mut handle = plugin.plugin_handle();
        let state = handle.get_extension::<PluginState>().unwrap();
        let mut bytes = Vec::new();
        state.save(&mut handle, &mut bytes).unwrap();
        bytes
    };
    let (_entry, mut locked) = test_host::load::<Saved>();
    let (_e, mut plain) = test_host::load::<GainPlugin>();
    let before = save(&mut locked);
    render(&mut locked);
    assert_eq!(save(&mut locked), before, "unchanged through a gap");
    assert_eq!(
        before,
        save(&mut plain),
        "the same as an unlicensed plugin's"
    );
}

licensed!(BadFiles, "test.licence.bad");

#[test]
fn a_bad_saved_code_leaves_the_plugin_locked_but_loading() {
    for text in ["", "rubbish", &code(8, 1), &code(9, 1)[..5]] {
        save_code(BadFiles::ID, text);
        let (_entry, mut plugin) = test_host::load::<BadFiles>();
        assert!(!is_unlocked::<BadFiles>(), "{text:?}");
        let mut run = Run::start(&mut plugin);
        run.blocks(1);
        run.stop(&mut plugin);
    }
}

licensed!(Unlocking, "test.licence.unlocking");

#[test]
fn unlocking_during_a_gap_finishes_it_then_stops_gapping() {
    let dir = folder(Unlocking::ID);
    let (_entry, mut plugin) = test_host::load::<Unlocking>();
    let mut run = Run::start(&mut plugin);
    // Into the first gap's silence: 60.5 s.
    let mut out = run.blocks(121);

    assert_eq!(
        try_unlock::<Unlocking>("not a code"),
        Err(UnlockError::Invalid(MALFORMED.into()))
    );
    assert_eq!(
        try_unlock::<Unlocking>(&code(8, 2)),
        Err(UnlockError::Invalid(WRONG_PRODUCT.into()))
    );
    assert!(!is_unlocked::<Unlocking>());
    assert!(!dir.join("licence.txt").exists());

    let good = code(9, 2);
    assert_eq!(try_unlock::<Unlocking>(&format!("  {good}\n")), Ok(()));
    assert!(is_unlocked::<Unlocking>());
    let saved = std::fs::read_to_string(dir.join("licence.txt")).unwrap();
    assert_eq!(saved.trim(), good);

    out.extend(run.blocks(BLOCKS - 121));
    run.stop(&mut plugin);
    assert_gap(&out, 60.0);
    assert!(
        dips(&out).iter().all(|&i| i < at(62.0)),
        "no gap after the one under way"
    );

    // A new instance reads the saved code; the flag was already set anyway.
    let (_e, mut again) = test_host::load::<Unlocking>();
    assert!(dips(&render(&mut again)).is_empty());
}

#[test]
fn an_unlicensed_plugin_cannot_be_unlocked() {
    assert_eq!(
        try_unlock::<GainPlugin>(&code(9, 3)),
        Err(UnlockError::NotLicensed)
    );
}
