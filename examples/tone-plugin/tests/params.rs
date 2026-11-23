//! Discrete params (design/model.md) through clack-host: stepped and toggle flags,
//! integer ranges, labels, and snapping of host values.

use clack_extensions::params::{ParamInfoBuffer, ParamInfoFlags, PluginParams};
use clack_host::events::event_types::ParamValueEvent;
use clack_host::prelude::*;
use clack_host::utils::Cookie;
use silkscreen_clap::test_host::{self, Host};
use tone_plugin::TonePlugin;

const WAVE_ID: u32 = 2;
const OCTAVE_ID: u32 = 3;

fn load() -> (PluginEntry, PluginInstance<Host>) {
    test_host::load::<TonePlugin>()
}

#[test]
fn discrete_params_are_stepped_enums_with_integer_ranges() {
    let (_entry, mut plugin) = load();
    let mut h = plugin.plugin_handle();
    let params = h.get_extension::<PluginParams>().unwrap();
    assert_eq!(params.count(&mut h), 3);
    let mut buf = ParamInfoBuffer::new();

    let level = params.get_info(&mut h, 0, &mut buf).unwrap();
    assert!(!level.flags.contains(ParamInfoFlags::IS_STEPPED));

    let wave = params.get_info(&mut h, 1, &mut buf).unwrap();
    assert_eq!(wave.id, ClapId::new(WAVE_ID));
    assert!(
        wave.flags
            .contains(ParamInfoFlags::IS_STEPPED | ParamInfoFlags::IS_ENUM)
    );
    assert_eq!(
        (wave.min_value, wave.max_value, wave.default_value),
        (0.0, 2.0, 0.0)
    );

    let octave = params.get_info(&mut h, 2, &mut buf).unwrap();
    assert!(
        octave
            .flags
            .contains(ParamInfoFlags::IS_STEPPED | ParamInfoFlags::IS_ENUM)
    );
    assert_eq!((octave.min_value, octave.max_value), (0.0, 1.0));
}

#[test]
fn labels_both_ways() {
    let (_entry, mut plugin) = load();
    let mut h = plugin.plugin_handle();
    let params = h.get_extension::<PluginParams>().unwrap();
    let mut text = [0u8; 32];
    let shown = params
        .value_to_text(&mut h, ClapId::new(WAVE_ID), 1.0, &mut text)
        .unwrap();
    assert_eq!(shown, b"Triangle");
    let shown = params
        .value_to_text(&mut h, ClapId::new(OCTAVE_ID), 1.0, &mut text)
        .unwrap();
    assert_eq!(shown, b"Down");
    assert_eq!(
        params.text_to_value(&mut h, ClapId::new(WAVE_ID), c"square"),
        Some(2.0)
    );
    assert_eq!(
        params.text_to_value(&mut h, ClapId::new(OCTAVE_ID), c"Normal"),
        Some(0.0)
    );
}

#[test]
fn host_values_between_positions_snap() {
    let (_entry, mut plugin) = load();
    let mut events = EventBuffer::new();
    events.push(&ParamValueEvent::new(
        0,
        ClapId::new(WAVE_ID),
        Pckn::match_all(),
        1.6,
        Cookie::empty(),
    ));
    {
        let mut inactive = plugin.inactive_plugin_handle().expect("inactive");
        let params = inactive.get_extension::<PluginParams>().unwrap();
        let mut out = EventBuffer::new();
        params.flush(&mut inactive, &events.as_input(), &mut out.as_output());
    }
    let mut h = plugin.plugin_handle();
    let params = h.get_extension::<PluginParams>().unwrap();
    assert_eq!(params.get_value(&mut h, ClapId::new(WAVE_ID)), Some(2.0));
}
