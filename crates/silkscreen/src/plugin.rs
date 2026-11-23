//! A plugin's editor side, written once and read by every host: the
//! CLAP shell (`silkscreen-clap`) and the web's `PresetPage`
//! (`silkscreen-host-web`).

use silkscreen_bridge::EngineLink;
use silkscreen_core::{EmbeddedPngs, EmbeddedRaster, RasterSource};
use silkscreen_model::{AppVersion, FactoryPreset, ParamSpec};

use crate::EditorState;

/// What a plugin gives each host for its editor: who it is, its params,
/// its factory bank, its faceplate and its editor state.
#[derive(Clone, Copy)]
pub struct PluginUi {
    /// Who makes it, e.g. `"Example Audio"`: the CLAP vendor, and the
    /// folder its user presets live under (design/presets.md).
    pub vendor: &'static str,
    /// Its name, e.g. `"Tone"`: its user presets' folder under
    /// [`vendor`](Self::vendor) (design/presets.md).
    pub product: &'static str,
    /// Its version, e.g. `"0.1.0"`: the CLAP descriptor's, and what every
    /// preset and library it writes is marked with (design/presets.md). Usually
    /// `env!("CARGO_PKG_VERSION")`.
    pub version: &'static str,
    /// Its web page's database name prefix, e.g. `"tone"`
    /// (design/presets.md).
    pub web_prefix: &'static str,
    /// The parameter table; index `i` is `ParamIndex(i)`.
    pub specs: &'static [ParamSpec],
    /// The factory presets it ships (a bank `silkscreen-build` generates,
    /// design/presets.md); `&[]` for none.
    pub factory: &'static [FactoryPreset],
    /// The faceplate pixels.
    pub faceplate: &'static EmbeddedPngs,
    /// The editor state over a link to the engine, without presets: each
    /// host sets them up with [`factory`](Self::factory) and its own
    /// store.
    pub state: fn(Box<dyn EngineLink>) -> EditorState,
}

impl PluginUi {
    /// Its product and version, as presets are marked (design/presets.md).
    pub const fn app(&self) -> AppVersion {
        AppVersion {
            product: self.product,
            version: self.version,
        }
    }

    /// The faceplate as an editor draws it.
    pub fn raster(&self) -> Box<dyn RasterSource> {
        Box::new(EmbeddedRaster::new(self.faceplate))
    }
}

impl std::fmt::Debug for PluginUi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginUi")
            .field("vendor", &self.vendor)
            .field("product", &self.product)
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}
