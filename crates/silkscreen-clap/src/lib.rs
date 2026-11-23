//! CLAP plugin shell for silkscreen ([design/hosts.md]), built on clack.
//!
//! A plugin implements [`SilkscreenPlugin`] (descriptor, its editor side
//! as a [`PluginUi`], engine factory) and exports itself with [`export_clap!`]. This
//! crate supplies the clack plumbing generically:
//!
//! - **Main thread:** owns the long-lived [`EditorState`] (model, controller,
//!   the UI end of the bridge) for the plugin's whole life.
//! - **Audio thread:** owns the plugin's [`Engine`] and the bridge's
//!   `EngineEnd`. The engine end lives on the main thread while the plugin
//!   is inactive and moves into the processor on `activate`, back on
//!   `deactivate`, so no state is lost across reactivation.
//!
//! [design/hosts.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/hosts.md

// clack handle types have no Debug, so the wrappers holding them cannot
// derive it.
#![allow(missing_debug_implementations)]

use std::cell::RefCell;
use std::ffi::CStr;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64};

use clack_extensions::audio_ports::{
    AudioPortFlags, AudioPortInfo, AudioPortInfoWriter, AudioPortType, PluginAudioPorts,
    PluginAudioPortsImpl,
};
use clack_extensions::gui::PluginGui;
use clack_extensions::latency::{PluginLatency, PluginLatencyImpl};
use clack_extensions::note_ports::PluginNotePorts;
use clack_extensions::params::PluginParams;
use clack_extensions::state::PluginState;
use clack_extensions::timer::{HostTimer, PluginTimer, TimerId};
use clack_plugin::prelude::*;
use silkscreen::bridge::{Engine, bridge};
use silkscreen::{EditorState, EngineLink, PluginUi, PresetStore};
use silkscreen_host_native::EmbeddedWindow;

mod gui;
mod licence;
mod link;
mod notes;
mod params;
mod processor;
mod state;
#[cfg(feature = "test-host")]
pub mod test_host;
mod transport;

pub use licence::{
    CheckCode, GAP_FADE_S, GAP_HOLD_S, GAP_PERIOD_S, LICENCE_FILE, UnlockError, is_unlocked,
    try_unlock,
};
pub use params::ParamIds;
pub use processor::Processor;

use link::Link;

#[doc(hidden)]
pub use clack_plugin as __clack_plugin;
pub use clack_plugin::plugin::features;

/// Everything a silkscreen CLAP plugin supplies.
pub trait SilkscreenPlugin: 'static {
    /// Reverse-DNS plugin id, e.g. `"com.example.gain"`. Never change it
    /// once released: hosts key projects on it.
    const ID: &'static str;
    const NAME: &'static str;
    const VERSION: &'static str;
    /// CLAP features, from [`features`].
    const FEATURES: &'static [&'static CStr];
    /// A build identifier shown beside the version in the editor's corner
    /// badge, e.g. a git hash stamped by `build.rs`. Empty for none.
    const BUILD: &'static str = "";
    /// Show live diagnostics in the badge: host changes that reached the
    /// model (`hc`), host timer ticks (`tk`), and the transport the engine
    /// last saw (tempo in BPM, `play`/`stop`).
    const DIAGNOSTICS: bool = false;
    /// Whether the plugin takes a stereo audio input (effects) or not
    /// (instruments).
    const AUDIO_INPUT: bool;
    /// Whether the plugin takes notes (CLAP note events and MIDI) on one
    /// note input port. They reach [`Engine::note`].
    const NOTE_INPUT: bool = false;
    /// Whether the user's presets are kept on disk: in the folder named
    /// by [`PluginUi::product`] under [`PluginUi::vendor`] in the
    /// platform's audio-preset folder (design/presets.md).
    const USER_PRESETS: bool = false;
    /// The samples the engine delays its output by, reported to the host
    /// for delay compensation. Fixed: it mustn't change with a param, or the
    /// host must restart the plugin.
    const LATENCY: u32 = 0;
    /// Locks the plugin until it is unlocked with a licence code (design/licensing-hook.md).
    /// While locked it plays a silence gap every minute; it is
    /// otherwise fully usable. The plugin brings its own code format: the
    /// [`CheckCode`] says whether a code unlocks it. `None`, the default,
    /// is never locked.
    const LICENCE: Option<CheckCode> = None;

    /// The editor side, as every host reads it.
    const UI: PluginUi;

    type Engine: Engine;

    /// The user's presets: by default, with [`USER_PRESETS`](Self::USER_PRESETS),
    /// a [`DirStore`](silkscreen_host_native::DirStore) in the platform's
    /// audio-preset folder, if it has one.
    fn preset_store() -> Option<Box<dyn PresetStore>> {
        if !Self::USER_PRESETS {
            return None;
        }
        let store =
            silkscreen_host_native::DirStore::default_for(Self::UI.vendor, Self::UI.product)?;
        Some(Box::new(store))
    }

    /// Where the licence code is saved: by default `licence.txt` in the
    /// same vendor/product folder as the user's presets (design/licensing-hook.md).
    /// Only read with [`LICENCE`](Self::LICENCE) set.
    fn licence_file() -> Option<PathBuf> {
        licence::default_file(Self::UI.vendor, Self::UI.product)
    }

    /// A fresh engine for `sample_rate`. Called on the main thread at
    /// activation, so it may allocate.
    fn engine(sample_rate: f32, max_frames: u32) -> Self::Engine;
}

/// `P`'s editor state, with its presets set up (design/presets.md): the
/// user's store if it keeps one, else a store in memory if it ships a
/// factory bank, else none.
fn editor_state<P: SilkscreenPlugin>(link: Box<dyn EngineLink>) -> EditorState {
    let ui = P::UI;
    let mut state = (ui.state)(link);
    // Every preset written is marked as this build's (design/presets.md).
    state.set_app(ui.app());
    // The Unregistered strip and the unlock dialog (design/licensing-hook.md).
    if P::LICENCE.is_some() {
        state.set_licence(Rc::new(licence::Lock::<P>::default()));
    }
    match P::preset_store() {
        Some(store) => state.set_presets(store, ui.factory),
        None if !ui.factory.is_empty() => {
            state.set_presets(Box::new(silkscreen::MemoryStore::default()), ui.factory);
        }
        None => {}
    }
    state
}

/// Export `$plugin` (a [`SilkscreenPlugin`]) as this library's CLAP entry.
#[macro_export]
macro_rules! export_clap {
    ($plugin:ty) => {
        $crate::__clack_plugin::clack_export_entry!(
            $crate::__clack_plugin::entry::SinglePluginEntry<$crate::Clap<$plugin>>
        );
    };
}

/// The clack [`Plugin`] for a [`SilkscreenPlugin`].
pub struct Clap<P>(PhantomData<P>);

impl<P: SilkscreenPlugin> Plugin for Clap<P> {
    type AudioProcessor<'a> = Processor<'a, P>;
    type Shared<'a> = Shared<P>;
    type MainThread<'a> = MainThread<'a, P>;

    fn declare_extensions(builder: &mut PluginExtensions<Self>, _shared: Option<&Shared<P>>) {
        builder
            .register::<PluginAudioPorts>()
            .register::<PluginParams>()
            .register::<PluginGui>()
            .register::<PluginState>()
            .register::<PluginNotePorts>()
            .register::<PluginLatency>()
            .register::<PluginTimer>();
    }
}

impl<P: SilkscreenPlugin> DefaultPluginFactory for Clap<P> {
    fn get_descriptor() -> PluginDescriptor {
        PluginDescriptor::new(P::ID, P::NAME)
            .with_vendor(P::UI.vendor)
            .with_version(P::VERSION)
            .with_features(P::FEATURES.iter().copied())
    }

    fn new_shared(_host: HostSharedHandle<'_>) -> Result<Shared<P>, PluginError> {
        let ids = ParamIds::new(P::UI.specs).map_err(|e| {
            eprintln!("{}: {e}", P::ID);
            PluginError::Message("invalid parameter table (details on stderr)")
        })?;
        Ok(Shared {
            ids,
            unlocked: licence::load::<P>(),
            processing: AtomicBool::new(false),
            tempo: AtomicU64::new(silkscreen::bridge::Transport::FALLBACK_TEMPO.to_bits()),
            playing: AtomicBool::new(false),
            _plugin: PhantomData,
        })
    }

    fn new_main_thread<'a>(
        host: HostMainThreadHandle<'a>,
        shared: &'a Shared<P>,
    ) -> Result<MainThread<'a, P>, PluginError> {
        let defaults: Vec<f32> = P::UI.specs.iter().map(|s| s.default).collect();
        let (ui, engine_end) = bridge(&defaults, silkscreen::bridge::RING_CAPACITY);
        Ok(MainThread {
            host,
            shared,
            state: Rc::new(RefCell::new(editor_state::<P>(Box::new(ui)))),
            gui: None,
            timer: None,
            host_scale: 1.0,
            zoom: 1.0,
            host_changes: 0,
            ticks: 0,
            badge_from: None,
            link: Some(Link::new(engine_end)),
            _plugin: PhantomData,
        })
    }
}

/// Thread-safe plugin state: read-only lookups. Everything mutable crosses
/// the bridge.
pub struct Shared<P> {
    ids: ParamIds,
    /// The plugin type's unlocked flag, if it is licensed.
    unlocked: Option<&'static AtomicBool>,
    /// The audio thread is between `start_processing` and `stop_processing`.
    processing: AtomicBool,
    /// Diagnostics: the last block's tempo (f64 bits) and play state.
    tempo: AtomicU64,
    playing: AtomicBool,
    _plugin: PhantomData<fn() -> P>,
}

impl<P: SilkscreenPlugin> PluginShared<'_> for Shared<P> {}

pub struct MainThread<'a, P: SilkscreenPlugin> {
    host: HostMainThreadHandle<'a>,
    shared: &'a Shared<P>,
    /// Shared with the editor window while one is open.
    state: silkscreen_host_native::SharedState,
    gui: Option<EmbeddedWindow>,
    /// The host timer, registered while the editor is open.
    timer: Option<(HostTimer, TimerId)>,
    /// Scale the host asked for via `gui.set_scale` (non-macOS only).
    host_scale: f64,
    /// Editor zoom, one of `ZOOM_STEPS`. Kept while the editor is closed
    /// and saved with the plugin state.
    zoom: f32,
    /// Diagnostics: host changes applied to the model, host timer ticks.
    host_changes: u64,
    ticks: u64,
    /// What the badge last showed was made from, so it's rebuilt only
    /// when that changes.
    badge_from: Option<(u64, u64, u64, bool)>,
    /// Held here while inactive; moved into the processor while active.
    link: Option<Link>,
    _plugin: PhantomData<P>,
}

impl<'a, P: SilkscreenPlugin> PluginMainThread<'a, Shared<P>> for MainThread<'a, P> {}

impl<P: SilkscreenPlugin> PluginLatencyImpl for MainThread<'_, P> {
    fn get(&mut self) -> u32 {
        P::LATENCY
    }
}

impl<P: SilkscreenPlugin> MainThread<'_, P> {
    /// The editor state. Borrow briefly: the editor window shares it.
    pub fn state(&self) -> std::cell::Ref<'_, EditorState> {
        self.state.borrow()
    }
}

impl<P: SilkscreenPlugin> PluginAudioPortsImpl for MainThread<'_, P> {
    fn count(&mut self, is_input: bool) -> u32 {
        u32::from(!is_input || P::AUDIO_INPUT)
    }

    fn get(&mut self, index: u32, is_input: bool, writer: &mut AudioPortInfoWriter) {
        if index != 0 || (is_input && !P::AUDIO_INPUT) {
            return;
        }
        writer.set(&AudioPortInfo {
            id: ClapId::new(0),
            name: b"main",
            channel_count: 2,
            flags: AudioPortFlags::IS_MAIN,
            port_type: Some(AudioPortType::STEREO),
            in_place_pair: P::AUDIO_INPUT.then(|| ClapId::new(0)),
        });
    }
}
