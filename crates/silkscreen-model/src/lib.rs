//! Model, controller and event history for silkscreen editors.
//!
//! Every state change passes through the controller, is settled in the
//! model, and is published through the model's event log to the view and
//! the engine ([design/model.md]). Must stay buildable for `wasm32-unknown-unknown`.
//!
//! [design/model.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/model.md

pub mod binding;
mod controller;
mod log;
mod model;
mod param;
pub mod preset;
mod undo;

pub use binding::{
    Binding, BindingExt, Described, Dimmed, LevelCommand, PanelButton, ParamDropDown, ParamFader,
    ParamKnob, ParamRadio, ParamSelector, ParamToggle, PresetAction, PresetButton, PresetCommand,
    SyncedKnob, TabButton, Unbound, preset_arrow_w,
};
pub use controller::{Controller, HostEvent};
pub use log::{Cursor, EventLog, ModelEvent, Origin};
pub use model::{Edit, Model, UiRequest};
pub use param::{Display, ParamIndex, ParamKind, ParamSpec, Taper};
pub use preset::{AppVersion, FactoryPreset, Preset, UNVERSIONED};
pub use undo::UndoEntry;
