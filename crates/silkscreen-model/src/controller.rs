//! The controller: routes control events to bindings and host events to the
//! model ([design/model.md]).
//!
//! [design/model.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/model.md

use silkscreen_core::{ControlEvent, ControlId};

use crate::binding::Binding;
use crate::log::Origin;
use crate::model::Model;
use crate::param::ParamIndex;

/// Changes the host makes, fed through the controller like user input.
#[derive(Clone, Debug, PartialEq)]
pub enum HostEvent {
    ParamChanged {
        param: ParamIndex,
        value: f32,
    },
    /// A whole saved state: each listed param takes its value, every other
    /// param returns to its default. Applied with `Origin::Host`, so it
    /// makes no undo entries; it also clears the undo history, which
    /// described edits to the state this one replaces.
    StateLoaded(Vec<(ParamIndex, f32)>),
}

/// One binding per control, indexed by [`ControlId`].
pub struct Controller {
    bindings: Vec<Box<dyn Binding>>,
}

impl std::fmt::Debug for Controller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Controller")
            .field("bindings", &self.bindings.len())
            .finish()
    }
}

impl Controller {
    /// `bindings[i]` handles `ControlId(i)`. Generated `Controls::into_array`
    /// yields them in exactly this order.
    pub fn new(bindings: impl IntoIterator<Item = Box<dyn Binding>>) -> Self {
        Self {
            bindings: bindings.into_iter().collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    pub fn binding(&self, id: ControlId) -> &dyn Binding {
        self.bindings[id.index()].as_ref()
    }

    pub fn binding_mut(&mut self, id: ControlId) -> &mut dyn Binding {
        self.bindings[id.index()].as_mut()
    }

    /// "This was clicked": hand the event to the control's binding, which
    /// edits the model with `Origin::Ui`.
    pub fn handle_control(&mut self, model: &mut Model, id: ControlId, event: &ControlEvent) {
        let mut edit = model.edit(Origin::Ui);
        self.bindings[id.index()].on_event(event, &mut edit);
    }

    /// A change from the host. Applied directly with `Origin::Host`; no
    /// binding is involved. If the user is mid-gesture on the same param,
    /// the last writer wins.
    pub fn handle_host(&mut self, model: &mut Model, event: HostEvent) {
        match event {
            HostEvent::ParamChanged { param, value } => model.edit(Origin::Host).set(param, value),
            HostEvent::StateLoaded(values) => {
                let mut state: Vec<f32> = model.specs().iter().map(|s| s.default).collect();
                for (param, value) in values {
                    // Non-finite values can't be clamped into range: treat
                    // them as missing.
                    if let Some(slot) = state.get_mut(param.index())
                        && value.is_finite()
                    {
                        *slot = value;
                    }
                }
                let mut edit = model.edit(Origin::Host);
                for (i, value) in state.into_iter().enumerate() {
                    edit.set(ParamIndex(i as u32), value);
                }
                model.clear_history();
            }
        }
    }
}
