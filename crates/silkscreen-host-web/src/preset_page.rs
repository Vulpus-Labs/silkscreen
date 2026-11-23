//! A preset-capable plugin page (design/presets.md): the user's presets in
//! `IndexedDB`, the session and share links restored on load, Export,
//! Import and Share in the browser's footer, the session kept as it
//! changes, and the console methods hand checks use. A plugin's page is
//! its `#[wasm_bindgen]` surface over one of these.

use std::rc::Rc;

use js_sys::SharedArrayBuffer;
use silkscreen::{Editor, ParamIndex, PluginUi, PresetPath, PresetStore};
use wasm_bindgen::JsValue;
use web_sys::HtmlCanvasElement;

use crate::restore::{initial_values, restore};
use crate::store::{JournaledStore, preset_key};
use crate::{Connection, WebHost, connect, open_preset_store};

/// A running preset-capable page.
#[derive(Debug)]
pub struct PresetPage {
    host: Rc<WebHost>,
    #[cfg(target_arch = "wasm32")]
    io: Rc<crate::session::WebPresetIo>,
    engine: Connection,
    presets: JournaledStore,
}

impl PresetPage {
    /// Open the user's presets and the restored sound, connect the
    /// engine (starting from that sound), mount the editor on `canvas`
    /// and start drawing and keeping the session.
    pub async fn start(canvas: HtmlCanvasElement, plugin: PluginUi) -> Result<Self, JsValue> {
        let presets = open_preset_store(plugin.web_prefix).await;
        let restore = restore(plugin.web_prefix).await;
        // The engine starts from the restored sound.
        let (link, engine) = connect(&initial_values(restore.as_ref(), plugin.specs))?;
        let mut state = (plugin.state)(link);
        // Every preset written is marked as this build's (design/presets.md).
        state.set_app(plugin.app());
        state.set_presets(Box::new(presets.clone()), plugin.factory);
        #[cfg(target_arch = "wasm32")]
        let io = Rc::new(crate::session::WebPresetIo::new());
        #[cfg(target_arch = "wasm32")]
        state.set_preset_io(io.clone());
        if let Some(restore) = restore {
            restore.apply(&mut state);
        }
        let editor = Editor::new(state, plugin.raster());
        let host = Rc::new(crate::mount(canvas, editor)?);
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(crate::session::autosave(
            plugin.web_prefix.to_owned(),
            Rc::clone(&host),
        ));
        Ok(Self {
            host,
            #[cfg(target_arch = "wasm32")]
            io,
            engine,
            presets,
        })
    }

    pub fn host(&self) -> &WebHost {
        &self.host
    }

    /// The shared buffer for `startAudio`, if the page is cross-origin
    /// isolated.
    pub fn sab(&self) -> Option<SharedArrayBuffer> {
        self.engine.sab.clone()
    }

    /// Send one MIDI 1.0 channel message to the engine.
    pub fn midi(&mut self, status: u8, data1: u8, data2: u8) -> bool {
        self.engine.midi(status, data1, data2)
    }

    /// Whether the editor wants the keys: a panel is open.
    pub fn editor_wants_keys(&self) -> bool {
        self.host.panels() > 0
    }

    /// Show the faceplate at `zoom` CSS pixels per logical pixel
    /// ([`WebHost::set_zoom`]).
    pub fn set_zoom(&self, zoom: f64) {
        self.host.set_zoom(zoom);
    }

    /// Rects put on the canvas since mount.
    pub fn puts(&self) -> u32 {
        self.host.puts()
    }

    /// Save the current sound as user preset `name` at the root.
    pub fn save_preset(&mut self, name: &str) -> Result<(), JsValue> {
        let text = self.host.with_editor(|e| {
            let state = e.state();
            state.capture_preset(name).to_text(state.model().specs())
        });
        self.presets
            .write(&PresetPath::root(name), &text)
            .map_err(Into::into)
    }

    /// Every user preset's key (`name` or `folder/name`).
    pub fn preset_keys(&self) -> Vec<String> {
        self.presets
            .tree()
            .into_iter()
            .flat_map(|f| {
                f.presets
                    .into_iter()
                    .map(move |n| preset_key(&PresetPath::new(f.name.as_deref(), &n)))
            })
            .collect()
    }

    /// Why presets can't be kept, if they can't.
    pub fn preset_problem(&self) -> Option<String> {
        self.presets.problem()
    }

    /// Set param `index` to plain `value` as the user would.
    pub fn set_param(&self, index: u32, value: f32) {
        let p = ParamIndex(index);
        self.host.with_editor(|e| {
            e.state.edit(|edit| {
                edit.begin(p);
                edit.set(p, value);
                edit.end(p);
            });
        });
    }

    /// Param `index`'s plain value.
    pub fn get_param(&self, index: u32) -> f32 {
        self.host
            .with_editor(|e| e.state.model().get(ParamIndex(index)))
    }

    /// The current preset's name, if any.
    pub fn preset_name(&self) -> Option<String> {
        self.host
            .with_editor(|e| e.state.preset_name().map(str::to_owned))
    }

    /// Share the current sound (as the browser's Share does) and return
    /// the link.
    #[cfg(target_arch = "wasm32")]
    pub fn share_link(&self) -> Option<String> {
        use silkscreen::PresetIo as _;
        let text = self.host.with_editor(|e| e.state.current_preset_text().1);
        self.io.share(&text).ok()?;
        self.io.last_link()
    }

    /// Whether preset changes are still waiting to be written.
    pub fn presets_pending(&self) -> bool {
        self.presets.is_dirty()
    }
}
