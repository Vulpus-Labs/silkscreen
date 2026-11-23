//! silkscreen: SVG-faceplate UI for audio plugins.
//!
//! This crate is the facade. It holds the [`Editor`], the one contract every
//! host drives ([design/overview.md]), and re-exports the layers below it:
//!
//! - [`core`]: geometry, input, gestures, layout, rendering
//! - [`model`]: params, model, controller, bindings
//! - [`bridge`]: the engine protocol
//!
//! Generated faceplate code (from `silkscreen-build`) names types through
//! `::silkscreen::core`.
//!
//! [design/overview.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/overview.md

mod browser;
pub mod dialog;
mod dropdown;
mod editor;
pub mod library;
pub mod licence;
pub mod list;
pub mod panel;
mod plugin;
pub mod presets;
pub mod prompt;
pub mod random;
pub mod table;
pub mod textfield;
pub mod theme;
mod tooltip;
pub mod widget;

pub use silkscreen_bridge as bridge;
pub use silkscreen_core as core;
pub use silkscreen_model as model;

pub use browser::{Place, PresetBrowser};
pub use dialog::Unlock;
pub use editor::{Editor, EditorState, EditorView, MeterSource};
pub use licence::{Clipboard, Licence};
pub use list::{Glyph, List};
pub use panel::{Panel, Reply, Update};
pub use plugin::PluginUi;
pub use presets::{Folder, IoEvent, MemoryStore, PresetIo, PresetPath, PresetStore};
pub use prompt::{Prompt, PromptReply, TextPrompt};
pub use silkscreen_bridge::{EngineEvent, EngineLink, ParamIndex};
pub use silkscreen_core::{ControlId, IRect, RawEvent, RawKind};
pub use silkscreen_model::{
    AppVersion, Binding, BindingExt, Described, Dimmed, FactoryPreset, HostEvent, LevelCommand,
    Model, PanelButton, ParamDropDown, ParamFader, ParamKnob, ParamRadio, ParamSelector, ParamSpec,
    ParamToggle, Preset, PresetAction, PresetButton, PresetCommand, SyncedKnob, TabButton, Unbound,
};
pub use table::{Cell, Choose, Column, Row, Table};
pub use textfield::TextField;
pub use theme::Theme;
pub use widget::EditorWidget;
