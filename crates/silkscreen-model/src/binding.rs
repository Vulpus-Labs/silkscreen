//! Bindings: per-control interaction logic ([design/model.md]).
//!
//! A binding turns "this was clicked" into "this was changed". It holds the
//! interaction logic (sensitivity, fine adjustment, reset); widgets only
//! draw.
//!
//! [design/model.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/model.md

use silkscreen_core::{
    ControlEvent, ControlKind, ControlSpec, FaderGeometry, Layout, MouseButton, MouseCursor,
    WidgetState,
};

use crate::model::{Edit, Model};
use crate::param::{ParamIndex, ParamKind, ParamSpec, Taper};

pub trait Binding {
    /// React to an event on the bound control by editing the model.
    fn on_event(&mut self, event: &ControlEvent, edit: &mut Edit<'_>);

    /// What the control's widget should show. The view fills in the
    /// interaction flags.
    fn display(&self, model: &Model) -> WidgetState;

    /// Params whose changes alter [`display`](Self::display), so the view
    /// knows when to redraw the control.
    fn params(&self) -> &[ParamIndex];

    /// What a tooltip says for the control, overriding the default (its
    /// first param's name and value). For bindings whose control stands
    /// for more than one param, or none.
    fn describe(&self, model: &Model) -> Option<String> {
        let _ = model;
        None
    }

    /// The mouse cursor over this control, when it isn't its kind's
    /// ([`ControlKind::cursor`]): an LCD's handle, say, on a canvas that's
    /// otherwise only a picture. Read after each input event.
    fn cursor(&self) -> Option<MouseCursor> {
        None
    }

    /// Attach to `control`, once, when the editor state is built: check
    /// that this binding suits the control and the params it names
    /// ([design/model.md]), and keep whatever geometry it needs (e.g. a
    /// fader's track). An `Err` panics there, naming the control, so
    /// explain the mismatch and the fix. Custom bindings accept anything
    /// by default.
    ///
    /// [design/model.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/model.md
    fn attach(
        &mut self,
        control: &ControlSpec,
        specs: &[ParamSpec],
        layout: &Layout,
    ) -> Result<(), String> {
        let _ = (control, specs, layout);
        Ok(())
    }
}

/// A boxed binding is a binding, so a plugin can build its bindings as
/// `Box<dyn Binding>` and still wrap them ([`BindingExt::dimmed_when`]).
impl<B: Binding + ?Sized> Binding for Box<B> {
    fn on_event(&mut self, event: &ControlEvent, edit: &mut Edit<'_>) {
        (**self).on_event(event, edit);
    }

    fn display(&self, model: &Model) -> WidgetState {
        (**self).display(model)
    }

    fn params(&self) -> &[ParamIndex] {
        (**self).params()
    }

    fn describe(&self, model: &Model) -> Option<String> {
        (**self).describe(model)
    }

    fn cursor(&self) -> Option<MouseCursor> {
        (**self).cursor()
    }

    fn attach(
        &mut self,
        control: &ControlSpec,
        specs: &[ParamSpec],
        layout: &Layout,
    ) -> Result<(), String> {
        (**self).attach(control, specs, layout)
    }
}

/// What a [`Described`] binding's tooltip says.
pub type Describe = fn(&Model) -> Option<String>;

/// Ways to adapt any binding.
pub trait BindingExt: Binding + Sized {
    /// This binding, its tooltip and drag readout said by `describe`
    /// (`None` falls back to the default, its first param's name and
    /// value). For a control whose readout depends on other params, as
    /// a fader showing an interval under the tuning switch.
    fn describe_with(self, describe: Describe) -> Described<Self> {
        Described {
            inner: self,
            describe,
        }
    }

    /// This binding, its control dimmed while `dimmed` holds: drawn
    /// faded, still operable. `inputs` are the params `dimmed`
    /// reads, so the control repaints when any changes. For a control
    /// that does nothing in the current patch, as a legato switch in Poly
    /// mode.
    ///
    fn dimmed_when<F>(self, inputs: &[ParamIndex], dimmed: F) -> Dimmed<Self, F>
    where
        F: Fn(&Model) -> bool,
    {
        let mut params = self.params().to_vec();
        for &p in inputs {
            if !params.contains(&p) {
                params.push(p);
            }
        }
        Dimmed {
            inner: self,
            params,
            when: dimmed,
        }
    }
}

impl<B: Binding> BindingExt for B {}

/// A binding whose control dims while a predicate over the model holds
/// ([`BindingExt::dimmed_when`]).
pub struct Dimmed<B, F> {
    inner: B,
    /// The inner binding's params, then the predicate's other inputs.
    params: Vec<ParamIndex>,
    when: F,
}

impl<B: std::fmt::Debug, F> std::fmt::Debug for Dimmed<B, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dimmed")
            .field("inner", &self.inner)
            .field("params", &self.params)
            .finish_non_exhaustive()
    }
}

impl<B: Binding, F: Fn(&Model) -> bool> Binding for Dimmed<B, F> {
    fn on_event(&mut self, event: &ControlEvent, edit: &mut Edit<'_>) {
        self.inner.on_event(event, edit);
    }

    fn display(&self, model: &Model) -> WidgetState {
        let mut s = self.inner.display(model);
        s.dimmed = (self.when)(model);
        s
    }

    fn params(&self) -> &[ParamIndex] {
        &self.params
    }

    fn describe(&self, model: &Model) -> Option<String> {
        self.inner.describe(model)
    }

    fn cursor(&self) -> Option<MouseCursor> {
        self.inner.cursor()
    }

    fn attach(
        &mut self,
        control: &ControlSpec,
        specs: &[ParamSpec],
        layout: &Layout,
    ) -> Result<(), String> {
        self.inner.attach(control, specs, layout)
    }
}

/// A binding whose tooltip comes from a function ([`BindingExt::describe_with`]).
#[derive(Clone, Debug)]
pub struct Described<B> {
    inner: B,
    describe: Describe,
}

impl<B: Binding> Binding for Described<B> {
    fn on_event(&mut self, event: &ControlEvent, edit: &mut Edit<'_>) {
        self.inner.on_event(event, edit);
    }

    fn display(&self, model: &Model) -> WidgetState {
        self.inner.display(model)
    }

    fn params(&self) -> &[ParamIndex] {
        self.inner.params()
    }

    fn describe(&self, model: &Model) -> Option<String> {
        (self.describe)(model).or_else(|| self.inner.describe(model))
    }

    fn cursor(&self) -> Option<MouseCursor> {
        self.inner.cursor()
    }

    fn attach(
        &mut self,
        control: &ControlSpec,
        specs: &[ParamSpec],
        layout: &Layout,
    ) -> Result<(), String> {
        self.inner.attach(control, specs, layout)
    }
}

/// A button that opens the plugin's panel for its control
/// (`UiRequest::Panel`, which `EditorState::set_panel` answers). Its
/// lamp lights while `lit` holds, repainting when any of `params`
/// changes. Give it a tooltip with [`BindingExt::describe_with`].
#[derive(Clone, Debug)]
pub struct PanelButton {
    params: &'static [ParamIndex],
    lit: fn(&Model) -> bool,
    control: Option<silkscreen_core::ControlId>,
}

impl PanelButton {
    /// Lit while `lit` holds; `params` are what `lit` reads.
    pub fn new(params: &'static [ParamIndex], lit: fn(&Model) -> bool) -> Self {
        Self {
            params,
            lit,
            control: None,
        }
    }
}

impl Binding for PanelButton {
    fn on_event(&mut self, event: &ControlEvent, edit: &mut Edit<'_>) {
        if let (ControlEvent::Click { .. }, Some(control)) = (event, self.control) {
            edit.request(crate::model::UiRequest::Panel { control });
        }
    }

    fn display(&self, model: &Model) -> WidgetState {
        WidgetState::value(if (self.lit)(model) { 1.0 } else { 0.0 })
    }

    fn params(&self) -> &[ParamIndex] {
        self.params
    }

    fn attach(
        &mut self,
        control: &ControlSpec,
        _specs: &[ParamSpec],
        _layout: &Layout,
    ) -> Result<(), String> {
        if !matches!(control.kind, ControlKind::Button | ControlKind::Action) {
            return Err(format!(
                "is a {} but is bound with PanelButton, which opens a panel from a button or action. Make it button.<name> or action.<name>",
                control.kind.as_str()
            ));
        }
        self.control = Some(control.id);
        Ok(())
    }
}

/// A tempo-synced time knob: a free time param running free, and with
/// a sync switch on, a division of the tempo. Both params stay as they
/// are, each automatable; the knob drives whichever the switch selects.
/// Synced, its travel steps through the divisions shortest to longest,
/// so clockwise is always longer, whatever order the division param
/// lists them in.
#[derive(Debug, Clone)]
pub struct SyncedKnob {
    time: ParamKnob,
    /// The time first, so the readout running free is the time's.
    params: [ParamIndex; 3],
    /// Division indices, shortest to longest.
    order: Vec<usize>,
    /// Synced: the knob's position during a drag, 0..1 over the order.
    drag: Option<f32>,
}

impl SyncedKnob {
    /// The knob over `time`, `division` (a stepped param whose position
    /// `i` lasts `lengths[i]`, in any unit) and `sync` (a toggle).
    pub fn new(time: ParamIndex, division: ParamIndex, sync: ParamIndex, lengths: &[f32]) -> Self {
        let mut order: Vec<usize> = (0..lengths.len()).collect();
        order.sort_by(|a, b| lengths[*a].total_cmp(&lengths[*b]));
        Self {
            time: ParamKnob::new(time),
            params: [time, division, sync],
            order,
            drag: None,
        }
    }

    /// Division indices, shortest to longest: the knob's travel synced.
    pub fn order(&self) -> &[usize] {
        &self.order
    }

    fn division(&self) -> ParamIndex {
        self.params[1]
    }

    fn synced(sync: f32) -> bool {
        sync >= 0.5
    }

    fn last(&self) -> usize {
        self.order.len().saturating_sub(1)
    }

    /// Division `index`'s place in the order, 0..1.
    fn position(&self, index: f32) -> f32 {
        let index = (index.round().max(0.0) as usize).min(self.last());
        let rank = self.order.iter().position(|&d| d == index).unwrap_or(0);
        rank as f32 / self.last().max(1) as f32
    }

    /// The division at `position` (0..1) along the order.
    fn division_at(&self, position: f32) -> f32 {
        let rank = (position.clamp(0.0, 1.0) * self.last() as f32).round() as usize;
        self.order[rank] as f32
    }

    fn set_division(&self, edit: &mut Edit<'_>, division: f32) {
        if edit.get(self.division()) != division {
            edit.set(self.division(), division);
        }
    }
}

impl Binding for SyncedKnob {
    fn on_event(&mut self, event: &ControlEvent, edit: &mut Edit<'_>) {
        // A drag stays with the param it began on, whatever the sync
        // switch does meanwhile (automation), so its gesture closes.
        let time_dragging = self.time.dragging();
        if time_dragging || (!Self::synced(edit.get(self.params[2])) && self.drag.is_none()) {
            self.time.on_event(event, edit);
            return;
        }
        let p = self.division();
        match event {
            ControlEvent::DragStart { .. } => {
                edit.begin(p);
                self.drag = Some(self.position(edit.get(p)));
            }
            ControlEvent::Drag { delta, mods, .. } => {
                if let Some(pos) = self.drag {
                    let gain = if mods.shift { self.time.fine } else { 1.0 };
                    let next = (pos - delta.y / self.time.drag_px * gain).clamp(0.0, 1.0);
                    self.drag = Some(next);
                    let division = self.division_at(next);
                    self.set_division(edit, division);
                }
            }
            ControlEvent::DragEnd { .. } if self.drag.is_some() => {
                self.drag = None;
                edit.end(p);
            }
            ControlEvent::Click { count, mods } if *count >= 2 || mods.cmd => edit.reset(p),
            ControlEvent::Wheel { delta, .. } if delta.y != 0.0 => {
                let step = delta.y.signum() / self.last().max(1) as f32;
                let next = self.division_at(self.position(edit.get(p)) + step);
                edit.begin(p);
                self.set_division(edit, next);
                edit.end(p);
            }
            _ => {}
        }
    }

    fn display(&self, model: &Model) -> WidgetState {
        if Self::synced(model.get(self.params[2])) {
            WidgetState::value(self.position(model.get(self.division())))
        } else {
            self.time.display(model)
        }
    }

    fn params(&self) -> &[ParamIndex] {
        &self.params
    }

    /// Synced, the division by name ("Delay Division  1/8."); running
    /// free, the time's own readout.
    fn describe(&self, model: &Model) -> Option<String> {
        Self::synced(model.get(self.params[2])).then(|| {
            let spec = &model.specs()[self.division().index()];
            format!("{}  {}", spec.name, spec.format(model.get(self.division())))
        })
    }

    fn attach(
        &mut self,
        control: &ControlSpec,
        specs: &[ParamSpec],
        layout: &Layout,
    ) -> Result<(), String> {
        let division = spec_of(specs, self.division())?;
        if self.order.is_empty() || !division.kind.is_discrete() {
            return Err(format!(
                "is bound with SyncedKnob over {}, which must be a stepped param with at least one division, given one length each",
                division.name
            ));
        }
        if division.position_labels().len() != self.order.len() {
            return Err(format!(
                "is bound with SyncedKnob over {}, which has {} positions but {} lengths were given. Give one length per division",
                division.name,
                division.position_labels().len(),
                self.order.len()
            ));
        }
        self.time.attach(control, specs, layout)
    }
}

/// The spec for `param`, or an error saying the index is out of range.
pub fn spec_of(specs: &[ParamSpec], param: ParamIndex) -> Result<&ParamSpec, String> {
    specs.get(param.index()).ok_or_else(|| {
        format!(
            "is bound to param {} but the param table has only {} entries",
            param.0,
            specs.len()
        )
    })
}

/// The default knob behaviour, bound to one param:
///
/// | Input | Effect |
/// |---|---|
/// | Vertical drag | full range over [`ParamKnob::drag_px`] logical px (up increases) |
/// | Shift held | × [`ParamKnob::fine`] sensitivity, switchable mid-drag |
/// | Double-click, or Cmd-click | reset to default |
/// | Wheel | [`ParamKnob::wheel_step`] per line (× fine with shift) |
///
/// Every edit is bracketed by a gesture. On a stepped param (a wave
/// rotary) a drag moves through the positions, the wheel
/// steps one position per line, and the widget is told the position
/// count for its detents.
#[derive(Clone, Debug)]
pub struct ParamKnob {
    param: [ParamIndex; 1],
    /// Pixels of vertical drag for the full range. Default 200.
    pub drag_px: f32,
    /// Sensitivity multiplier while Shift is held. Default 0.1.
    pub fine: f32,
    /// Normalized change per wheel line. Default 0.01.
    pub wheel_step: f32,
    /// Normalized position during a drag, kept apart from the model's value
    /// so it doesn't pick up rounding from the plain-value round trip.
    drag: Option<f32>,
    /// A stepped param's position count (from `attach`), else 0.
    steps: u16,
}

impl ParamKnob {
    pub fn new(param: ParamIndex) -> Self {
        Self {
            param: [param],
            drag_px: 200.0,
            fine: 0.1,
            wheel_step: 0.01,
            drag: None,
            steps: 0,
        }
    }

    /// Whether a drag of this knob is under way.
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub fn param(&self) -> ParamIndex {
        self.param[0]
    }
}

impl Binding for ParamKnob {
    fn on_event(&mut self, event: &ControlEvent, edit: &mut Edit<'_>) {
        let p = self.param();
        match event {
            ControlEvent::DragStart { .. } => {
                edit.begin(p);
                self.drag = Some(edit.get_normalized(p));
            }
            ControlEvent::Drag { delta, mods, .. } => {
                if let Some(pos) = self.drag {
                    let gain = if mods.shift { self.fine } else { 1.0 };
                    let next = (pos - delta.y / self.drag_px * gain).clamp(0.0, 1.0);
                    self.drag = Some(next);
                    edit.set_normalized(p, next);
                }
            }
            ControlEvent::DragEnd { .. } if self.drag.is_some() => {
                self.drag = None;
                edit.end(p);
            }
            ControlEvent::Click { count, mods } if *count >= 2 || mods.cmd => edit.reset(p),
            ControlEvent::Wheel { delta, .. } if delta.y != 0.0 && self.steps > 1 => {
                let next = edit.get(p) + delta.y.signum();
                edit.begin(p);
                edit.set(p, next);
                edit.end(p);
            }
            ControlEvent::Wheel { delta, mods } if delta.y != 0.0 => {
                let gain = if mods.shift { self.fine } else { 1.0 };
                let next = edit.get_normalized(p) + delta.y * self.wheel_step * gain;
                edit.begin(p);
                edit.set_normalized(p, next);
                edit.end(p);
            }
            _ => {}
        }
    }

    fn display(&self, model: &Model) -> WidgetState {
        let p = self.param();
        if self.steps > 0 {
            return WidgetState::stepped(model.get_normalized(p), self.steps);
        }
        let origin = if model.spec(p).taper == Taper::Bipolar {
            0.5
        } else {
            0.0
        };
        WidgetState::from_origin(model.get_normalized(p), origin)
    }

    fn params(&self) -> &[ParamIndex] {
        &self.param
    }

    fn attach(
        &mut self,
        control: &ControlSpec,
        specs: &[ParamSpec],
        _layout: &Layout,
    ) -> Result<(), String> {
        let spec = spec_of(specs, self.param())?;
        control_is("ParamKnob", control, ControlKind::Knob)?;
        if let ParamKind::Stepped { labels } = spec.kind {
            self.steps = u16::try_from(labels.len()).unwrap_or(u16::MAX);
            return Ok(());
        }
        continuous("ParamKnob", spec)
    }
}

/// Requires `spec` to be continuous, for bindings that drive a range.
fn continuous(binding: &str, spec: &ParamSpec) -> Result<(), String> {
    match spec.kind {
        ParamKind::Continuous => Ok(()),
        ParamKind::Stepped { .. } | ParamKind::Toggle { .. } => Err(format!(
            "uses {binding} on param \"{}\", which is discrete ({}). {binding} needs a \
             continuous param; use a selector, toggle or radio control for discrete ones",
            spec.name,
            if matches!(spec.kind, ParamKind::Toggle { .. }) {
                "a toggle"
            } else {
                "stepped"
            }
        )),
    }
}

/// Requires `control` to be of `kind`.
fn control_is(binding: &str, control: &ControlSpec, kind: ControlKind) -> Result<(), String> {
    if control.kind == kind {
        Ok(())
    } else {
        Err(format!(
            "is a {} but is bound with {binding}, which drives {}s. Use the {}'s own binding",
            control.kind.as_str(),
            kind.as_str(),
            control.kind.as_str()
        ))
    }
}

/// The default fader behaviour, bound to one continuous param (or a
/// stepped one with many positions, as a split point over 128 notes:
/// the value lands on positions and the wheel steps one):
///
/// | Input | Effect |
/// |---|---|
/// | Drag (anywhere) | moves the value with the pointer, 1:1 with the cap |
/// | Shift held | × [`ParamFader::fine`] sensitivity |
/// | Click on the track | jumps the cap to the pointer |
/// | Double-click, or Cmd-click | reset to default |
/// | Wheel | [`ParamFader::wheel_step`] per line (× fine with shift) |
///
/// Every edit is bracketed by a gesture.
#[derive(Clone, Debug)]
pub struct ParamFader {
    param: [ParamIndex; 1],
    /// Sensitivity multiplier while Shift is held. Default 0.1.
    pub fine: f32,
    /// Normalized change per wheel line. Default 0.01.
    pub wheel_step: f32,
    geometry: Option<FaderGeometry>,
    drag: Option<f32>,
    /// Where a press on the track (not the cap) would jump to, if it
    /// becomes a click.
    jump: Option<f32>,
    /// A stepped param's position count (from `attach`), else 0.
    steps: u16,
}

impl ParamFader {
    pub fn new(param: ParamIndex) -> Self {
        Self {
            param: [param],
            fine: 0.1,
            wheel_step: 0.01,
            geometry: None,
            drag: None,
            jump: None,
            steps: 0,
        }
    }

    pub fn param(&self) -> ParamIndex {
        self.param[0]
    }
}

impl Binding for ParamFader {
    fn on_event(&mut self, event: &ControlEvent, edit: &mut Edit<'_>) {
        let p = self.param();
        match event {
            ControlEvent::Press {
                pos,
                button: MouseButton::Left,
                ..
            } => {
                self.jump = self.geometry.and_then(|g| {
                    (!g.cap(edit.get_normalized(p)).contains(*pos)).then(|| g.value_at(pos.y))
                });
            }
            ControlEvent::DragStart { .. } => {
                self.jump = None;
                edit.begin(p);
                self.drag = Some(edit.get_normalized(p));
            }
            ControlEvent::Drag { delta, mods, .. } => {
                if let (Some(pos), Some(g)) = (self.drag, self.geometry) {
                    let gain = if mods.shift { self.fine } else { 1.0 };
                    let next = (pos - delta.y / g.travel() * gain).clamp(0.0, 1.0);
                    self.drag = Some(next);
                    edit.set_normalized(p, next);
                }
            }
            ControlEvent::DragEnd { .. } if self.drag.is_some() => {
                self.drag = None;
                edit.end(p);
            }
            ControlEvent::Click { count, mods } if *count >= 2 || mods.cmd => {
                self.jump = None;
                edit.reset(p);
            }
            ControlEvent::Click { .. } => {
                if let Some(value) = self.jump.take() {
                    edit.begin(p);
                    edit.set_normalized(p, value);
                    edit.end(p);
                }
            }
            ControlEvent::Wheel { delta, .. } if delta.y != 0.0 && self.steps > 1 => {
                let next = edit.get(p) + delta.y.signum();
                edit.begin(p);
                edit.set(p, next);
                edit.end(p);
            }
            ControlEvent::Wheel { delta, mods } if delta.y != 0.0 => {
                let gain = if mods.shift { self.fine } else { 1.0 };
                let next = edit.get_normalized(p) + delta.y * self.wheel_step * gain;
                edit.begin(p);
                edit.set_normalized(p, next);
                edit.end(p);
            }
            _ => {}
        }
    }

    fn display(&self, model: &Model) -> WidgetState {
        WidgetState::value(model.get_normalized(self.param()))
    }

    fn params(&self) -> &[ParamIndex] {
        &self.param
    }

    fn attach(
        &mut self,
        control: &ControlSpec,
        specs: &[ParamSpec],
        _layout: &Layout,
    ) -> Result<(), String> {
        let spec = spec_of(specs, self.param())?;
        control_is("ParamFader", control, ControlKind::Fader)?;
        if let ParamKind::Stepped { labels } = spec.kind {
            self.steps = u16::try_from(labels.len()).unwrap_or(u16::MAX);
        } else {
            continuous("ParamFader", spec)?;
        }
        self.geometry = Some(FaderGeometry::new(control.rect));
        Ok(())
    }
}

/// The default selector behaviour, bound to one stepped param. It also
/// drives a three-way toggle (a `toggle` whose skin has a frame per
/// position): a click above, on or below the middle picks that position.
///
/// | Input | Effect |
/// |---|---|
/// | Drag (anywhere) | moves the cap with the pointer, snapping to detents |
/// | Click | selects the position nearest the pointer |
/// | Cmd-click | reset to default |
/// | Wheel | one position per line |
///
/// Every edit is bracketed by a gesture, and only whole positions ever
/// reach the model.
#[derive(Clone, Debug)]
pub struct ParamSelector {
    param: [ParamIndex; 1],
    geometry: Option<FaderGeometry>,
    positions: u16,
    /// Unsnapped position during a drag, so small moves accumulate.
    drag: Option<f32>,
    /// The position a press would select, if it becomes a click.
    pick: Option<f32>,
}

impl ParamSelector {
    pub fn new(param: ParamIndex) -> Self {
        Self {
            param: [param],
            geometry: None,
            positions: 0,
            drag: None,
            pick: None,
        }
    }

    pub fn param(&self) -> ParamIndex {
        self.param[0]
    }

    fn set(&self, edit: &mut Edit<'_>, normalized: f32) {
        let p = self.param();
        edit.begin(p);
        edit.set_normalized(p, normalized);
        edit.end(p);
    }
}

impl Binding for ParamSelector {
    fn on_event(&mut self, event: &ControlEvent, edit: &mut Edit<'_>) {
        let p = self.param();
        match event {
            ControlEvent::Press {
                pos,
                button: MouseButton::Left,
                ..
            } => self.pick = self.geometry.map(|g| g.value_at(pos.y)),
            ControlEvent::DragStart { .. } => {
                self.pick = None;
                edit.begin(p);
                self.drag = Some(edit.get_normalized(p));
            }
            ControlEvent::Drag { delta, .. } => {
                if let (Some(pos), Some(g)) = (self.drag, self.geometry) {
                    let next = (pos - delta.y / g.travel()).clamp(0.0, 1.0);
                    self.drag = Some(next);
                    edit.set_normalized(p, next);
                }
            }
            ControlEvent::DragEnd { .. } if self.drag.is_some() => {
                self.drag = None;
                edit.end(p);
            }
            ControlEvent::Click { mods, .. } if mods.cmd => {
                self.pick = None;
                edit.reset(p);
            }
            ControlEvent::Click { .. } => {
                if let Some(value) = self.pick.take() {
                    self.set(edit, value);
                }
            }
            ControlEvent::Wheel { delta, .. } if delta.y != 0.0 && self.positions > 1 => {
                let step = delta.y.signum() / f32::from(self.positions - 1);
                self.set(edit, edit.get_normalized(p) + step);
            }
            _ => {}
        }
    }

    fn display(&self, model: &Model) -> WidgetState {
        WidgetState::stepped(model.get_normalized(self.param()), self.positions)
    }

    fn params(&self) -> &[ParamIndex] {
        &self.param
    }

    fn attach(
        &mut self,
        control: &ControlSpec,
        specs: &[ParamSpec],
        _layout: &Layout,
    ) -> Result<(), String> {
        let spec = spec_of(specs, self.param())?;
        if control.kind != ControlKind::Toggle {
            control_is("ParamSelector", control, ControlKind::Selector)?;
        }
        let ParamKind::Stepped { labels } = spec.kind else {
            return Err(format!(
                "uses ParamSelector on param \"{}\", which is not stepped. Make it \
                 ParamSpec::stepped(..), or use a fader for a continuous param",
                spec.name
            ));
        };
        if let (ControlKind::Toggle, Some(skin)) = (control.kind, control.skin)
            && skin.frames().len() != labels.len()
        {
            return Err(format!(
                "is a toggle with skin \"{}\" of {} frames, but param \"{}\" has {} positions. \
                 Give the skin one frame per position, foot first (down, middle, up)",
                skin.name(),
                skin.frames().len(),
                spec.name,
                labels.len()
            ));
        }
        self.positions = labels.len() as u16;
        self.geometry = Some(FaderGeometry::new(control.rect));
        Ok(())
    }
}

/// A drop-down over a stepped param: a click asks the view for a menu of
/// the param's choices (`UiRequest::Menu`), and the view commits the one
/// chosen as a gesture. The wheel steps through the choices.
#[derive(Debug)]
pub struct ParamDropDown {
    param: [ParamIndex; 1],
    control: Option<silkscreen_core::ControlId>,
    positions: u16,
}

impl ParamDropDown {
    pub fn new(param: ParamIndex) -> Self {
        Self {
            param: [param],
            control: None,
            positions: 0,
        }
    }

    pub fn param(&self) -> ParamIndex {
        self.param[0]
    }
}

impl Binding for ParamDropDown {
    fn on_event(&mut self, event: &ControlEvent, edit: &mut Edit<'_>) {
        let p = self.param();
        match event {
            ControlEvent::Click { mods, .. } if mods.cmd => edit.reset(p),
            ControlEvent::Click { .. } => {
                if let Some(control) = self.control {
                    edit.request(crate::model::UiRequest::Menu { control, param: p });
                }
            }
            // Scrolling up moves to the previous choice, as in the menu.
            ControlEvent::Wheel { delta, .. } if delta.y != 0.0 && self.positions > 1 => {
                let next =
                    (edit.get(p) - delta.y.signum()).clamp(0.0, f32::from(self.positions - 1));
                edit.begin(p);
                edit.set(p, next);
                edit.end(p);
            }
            _ => {}
        }
    }

    fn display(&self, model: &Model) -> WidgetState {
        WidgetState::stepped(model.get_normalized(self.param()), self.positions)
    }

    fn params(&self) -> &[ParamIndex] {
        &self.param
    }

    fn attach(
        &mut self,
        control: &ControlSpec,
        specs: &[ParamSpec],
        _layout: &Layout,
    ) -> Result<(), String> {
        let spec = spec_of(specs, self.param())?;
        control_is("ParamDropDown", control, ControlKind::DropDown)?;
        let ParamKind::Stepped { labels } = spec.kind else {
            return Err(format!(
                "uses ParamDropDown on param \"{}\", which is not stepped. Make it \
                 ParamSpec::stepped(..) with a label per choice",
                spec.name
            ));
        };
        self.positions = labels.len() as u16;
        self.control = Some(control.id);
        Ok(())
    }
}

/// The preset field: a click on its name asks the view for the preset
/// browser (`UiRequest::Presets`); a click on the ‹ or › at its ends
/// steps to the previous or next preset (`UiRequest::PresetStep`,
/// design/presets.md). Bound to no param; the field shows the current preset's
/// name, which the editor keeps.
#[derive(Debug, Default)]
pub struct PresetButton {
    control: Option<silkscreen_core::ControlId>,
    rect: silkscreen_core::Rect,
    /// Where the last press landed: -1 the ‹, 1 the ›, 0 the name.
    zone: i8,
}

/// How wide a preset field's ‹ and › zones are: `w`'s quarter, at most 16.
pub fn preset_arrow_w(w: f32) -> f32 {
    (w / 4.0).min(16.0)
}

impl PresetButton {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Binding for PresetButton {
    fn on_event(&mut self, event: &ControlEvent, edit: &mut Edit<'_>) {
        use crate::model::UiRequest;
        match (event, self.control) {
            (ControlEvent::Press { pos, .. }, _) => {
                let arrow = preset_arrow_w(self.rect.w);
                self.zone = if pos.x < self.rect.x + arrow {
                    -1
                } else {
                    i8::from(pos.x >= self.rect.right() - arrow)
                };
            }
            (ControlEvent::Click { .. }, Some(control)) => edit.request(match self.zone {
                0 => UiRequest::Presets { control },
                z => UiRequest::PresetStep { forward: z > 0 },
            }),
            _ => {}
        }
    }

    fn display(&self, _: &Model) -> WidgetState {
        WidgetState::default()
    }

    fn params(&self) -> &[ParamIndex] {
        &[]
    }

    fn attach(
        &mut self,
        control: &ControlSpec,
        _specs: &[ParamSpec],
        _layout: &Layout,
    ) -> Result<(), String> {
        control_is("PresetButton", control, ControlKind::Preset)?;
        self.control = Some(control.id);
        self.rect = control.rect;
        Ok(())
    }
}

/// What a preset bar's [`PresetCommand`] button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresetAction {
    Save,
    SaveAs,
    Init,
    Randomise,
    Previous,
    Next,
    Browse,
}

/// A preset bar button (design/presets.md): an `action.<name>` control whose
/// click saves, saves as, inits, steps, or opens the browser.
#[derive(Debug)]
pub struct PresetCommand {
    action: PresetAction,
    control: Option<silkscreen_core::ControlId>,
}

impl PresetCommand {
    pub fn new(action: PresetAction) -> Self {
        Self {
            action,
            control: None,
        }
    }
    pub fn save() -> Self {
        Self::new(PresetAction::Save)
    }
    pub fn save_as() -> Self {
        Self::new(PresetAction::SaveAs)
    }
    pub fn init() -> Self {
        Self::new(PresetAction::Init)
    }
    pub fn randomise() -> Self {
        Self::new(PresetAction::Randomise)
    }
}

impl Binding for PresetCommand {
    fn on_event(&mut self, event: &ControlEvent, edit: &mut Edit<'_>) {
        use crate::model::UiRequest;
        let (ControlEvent::Click { .. }, Some(control)) = (event, self.control) else {
            return;
        };
        edit.request(match self.action {
            PresetAction::Save => UiRequest::PresetSave,
            PresetAction::SaveAs => UiRequest::PresetSaveAs,
            PresetAction::Init => UiRequest::PresetInit,
            PresetAction::Randomise => UiRequest::PresetRandomise,
            PresetAction::Previous => UiRequest::PresetStep { forward: false },
            PresetAction::Next => UiRequest::PresetStep { forward: true },
            PresetAction::Browse => UiRequest::Presets { control },
        });
    }

    fn display(&self, _: &Model) -> WidgetState {
        WidgetState::default()
    }

    fn params(&self) -> &[ParamIndex] {
        &[]
    }

    fn describe(&self, _: &Model) -> Option<String> {
        Some(
            match self.action {
                PresetAction::Save => "Save preset",
                PresetAction::SaveAs => "Save preset as…",
                PresetAction::Init => "Init: every control to its default",
                PresetAction::Randomise => "Random: a new sound from scratch",
                PresetAction::Previous => "Previous preset",
                PresetAction::Next => "Next preset",
                PresetAction::Browse => "Browse presets",
            }
            .to_owned(),
        )
    }

    fn attach(
        &mut self,
        control: &ControlSpec,
        _specs: &[ParamSpec],
        _layout: &Layout,
    ) -> Result<(), String> {
        control_is("PresetCommand", control, ControlKind::Action)?;
        self.control = Some(control.id);
        Ok(())
    }
}

/// A level key: an `action.<name>` control whose click
/// sets `param`, a linear gain, so the slow peak the engine reports as
/// meter `meter` (measured before that gain) lands at `target_db` dBFS
/// (`UiRequest::MatchLevel`). One press, one undo step; nothing keeps
/// acting after it.
#[derive(Debug)]
pub struct LevelCommand {
    meter: usize,
    param: [ParamIndex; 1],
    target_db: f32,
    name: Option<&'static str>,
    control: Option<silkscreen_core::ControlId>,
}

impl LevelCommand {
    pub fn new(meter: usize, param: ParamIndex, target_db: f32) -> Self {
        Self {
            meter,
            param: [param],
            target_db,
            name: None,
            control: None,
        }
    }
}

impl Binding for LevelCommand {
    fn on_event(&mut self, event: &ControlEvent, edit: &mut Edit<'_>) {
        if let (ControlEvent::Click { .. }, Some(_)) = (event, self.control) {
            edit.request(crate::model::UiRequest::MatchLevel {
                meter: self.meter,
                param: self.param[0],
                target_db: self.target_db,
            });
        }
    }

    fn display(&self, _: &Model) -> WidgetState {
        WidgetState::default()
    }

    /// None: the key sets its param by request, not by gesture.
    fn params(&self) -> &[ParamIndex] {
        &[]
    }

    fn describe(&self, _: &Model) -> Option<String> {
        Some(format!(
            "{} so the loudest of the last few seconds peaks at {} dB",
            self.name.unwrap_or("Level"),
            fmt_db(self.target_db)
        ))
    }

    fn attach(
        &mut self,
        control: &ControlSpec,
        specs: &[ParamSpec],
        _layout: &Layout,
    ) -> Result<(), String> {
        control_is("LevelCommand", control, ControlKind::Action)?;
        let spec = specs.get(self.param[0].index()).ok_or_else(|| {
            format!(
                "sets param {}, which isn't in the table. Give LevelCommand a param of this plugin's",
                self.param[0].index()
            )
        })?;
        if spec.kind.is_discrete() || spec.min < 0.0 {
            return Err(format!(
                "sets {}, which isn't a linear gain (continuous, from 0 up). Give LevelCommand a gain param",
                spec.name
            ));
        }
        self.name = Some(spec.name);
        self.control = Some(control.id);
        Ok(())
    }
}

/// `db` as a key's tooltip says it: "−3", "+6".
fn fmt_db(db: f32) -> String {
    if db < 0.0 {
        format!("\u{2212}{}", -db)
    } else if db > 0.0 {
        format!("+{db}")
    } else {
        "0".to_owned()
    }
}

/// A tab of a page group ([design/faceplates.md]): a click shows its page. Its
/// lamp, if given a toggle param, is lit while that param is on, so it
/// follows automation on a hidden page.
///
/// [design/faceplates.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/faceplates.md
#[derive(Clone, Debug, Default)]
pub struct TabButton {
    control: Option<silkscreen_core::ControlId>,
    lamp: Option<[ParamIndex; 1]>,
}

impl TabButton {
    pub fn new() -> Self {
        Self::default()
    }

    /// Light the tab's lamp while toggle `param` is on.
    #[must_use]
    pub fn lamp(mut self, param: ParamIndex) -> Self {
        self.lamp = Some([param]);
        self
    }
}

impl Binding for TabButton {
    fn on_event(&mut self, event: &ControlEvent, edit: &mut Edit<'_>) {
        if let (ControlEvent::Press { .. }, Some(control)) = (event, self.control) {
            edit.request(crate::model::UiRequest::ShowPage { control });
        }
    }

    fn display(&self, model: &Model) -> WidgetState {
        let on = self.lamp.is_some_and(|[p]| model.get(p) >= 0.5);
        WidgetState::value(if on { 1.0 } else { 0.0 })
    }

    fn params(&self) -> &[ParamIndex] {
        self.lamp.as_ref().map_or(&[], |p| &p[..])
    }

    fn describe(&self, _: &Model) -> Option<String> {
        None
    }

    fn attach(
        &mut self,
        control: &ControlSpec,
        _specs: &[ParamSpec],
        _layout: &Layout,
    ) -> Result<(), String> {
        if !matches!(control.kind, ControlKind::Tab { .. }) {
            return Err(format!(
                "is a {} but is bound with TabButton, which drives tabs. Use the {}'s own binding",
                control.kind.as_str(),
                control.kind.as_str()
            ));
        }
        self.control = Some(control.id);
        Ok(())
    }
}

/// A display-only control's binding (`scope`, `meter`, or a `canvas`
/// that only shows): no params, no input, nothing to describe. The
/// editor feeds its widget directly ([design/engine-bridge.md]).
///
/// [design/engine-bridge.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/engine-bridge.md
#[derive(Clone, Copy, Debug, Default)]
pub struct Unbound;

impl Binding for Unbound {
    fn on_event(&mut self, _: &ControlEvent, _: &mut Edit<'_>) {}

    fn display(&self, _: &Model) -> WidgetState {
        WidgetState::default()
    }

    fn params(&self) -> &[ParamIndex] {
        &[]
    }

    fn attach(
        &mut self,
        control: &ControlSpec,
        _specs: &[ParamSpec],
        _layout: &Layout,
    ) -> Result<(), String> {
        if control.kind.is_display() || control.kind == ControlKind::Canvas {
            Ok(())
        } else {
            Err(format!(
                "is a {} but is bound with Unbound, which binds only display controls (scope, meter, canvas). Use the {}'s own binding",
                control.kind.as_str(),
                control.kind.as_str()
            ))
        }
    }
}

/// The default switch behaviour for toggles and latching buttons, bound
/// to one toggle param: each click flips it, as a bracketed gesture.
#[derive(Clone, Debug)]
pub struct ParamToggle {
    param: [ParamIndex; 1],
}

impl ParamToggle {
    pub fn new(param: ParamIndex) -> Self {
        Self { param: [param] }
    }

    pub fn param(&self) -> ParamIndex {
        self.param[0]
    }
}

impl Binding for ParamToggle {
    fn on_event(&mut self, event: &ControlEvent, edit: &mut Edit<'_>) {
        if let ControlEvent::Click { .. } = event {
            let p = self.param();
            let flipped = 1.0 - edit.get_normalized(p).round();
            edit.begin(p);
            edit.set_normalized(p, flipped);
            edit.end(p);
        }
    }

    fn display(&self, model: &Model) -> WidgetState {
        WidgetState::stepped(model.get_normalized(self.param()), 2)
    }

    fn params(&self) -> &[ParamIndex] {
        &self.param
    }

    fn attach(
        &mut self,
        control: &ControlSpec,
        specs: &[ParamSpec],
        _layout: &Layout,
    ) -> Result<(), String> {
        let spec = spec_of(specs, self.param())?;
        if !matches!(control.kind, ControlKind::Toggle | ControlKind::Button) {
            return Err(format!(
                "is a {} but is bound with ParamToggle, which drives toggles and buttons. Use the {}'s own binding",
                control.kind.as_str(),
                control.kind.as_str()
            ));
        }
        if let Some(skin) = control.skin
            && skin.frames().len() != 2
        {
            return Err(format!(
                "has skin \"{}\" of {} frames, a {}-way switch, but is bound with ParamToggle, \
                 which flips between two. Bind it with ParamSelector to a stepped param",
                skin.name(),
                skin.frames().len(),
                skin.frames().len()
            ));
        }
        if !matches!(spec.kind, ParamKind::Toggle { .. }) {
            return Err(format!(
                "uses ParamToggle on param \"{}\", which is not a toggle. Make it ParamSpec::toggle(..)",
                spec.name
            ));
        }
        Ok(())
    }
}

/// One button of a radio group: clicking it sets the group's stepped param
/// to this button's position. The position comes from the control
/// (`radio.<group>.<position>`), so every button of a group is bound the
/// same way: `ParamRadio::new(TUNING)`.
#[derive(Clone, Debug)]
pub struct ParamRadio {
    param: [ParamIndex; 1],
    position: u16,
}

impl ParamRadio {
    pub fn new(param: ParamIndex) -> Self {
        Self {
            param: [param],
            position: 0,
        }
    }

    pub fn param(&self) -> ParamIndex {
        self.param[0]
    }
}

impl Binding for ParamRadio {
    fn on_event(&mut self, event: &ControlEvent, edit: &mut Edit<'_>) {
        let p = self.param();
        let position = f32::from(self.position);
        if let ControlEvent::Click { .. } = event
            && edit.get(p) != position
        {
            edit.begin(p);
            edit.set(p, position);
            edit.end(p);
        }
    }

    fn display(&self, model: &Model) -> WidgetState {
        let on = model.get(self.param()) == f32::from(self.position);
        WidgetState::stepped(if on { 1.0 } else { 0.0 }, 2)
    }

    fn params(&self) -> &[ParamIndex] {
        &self.param
    }

    fn attach(
        &mut self,
        control: &ControlSpec,
        specs: &[ParamSpec],
        layout: &Layout,
    ) -> Result<(), String> {
        let spec = spec_of(specs, self.param())?;
        let ControlKind::Radio { group, position } = control.kind else {
            return Err(format!(
                "is a {} but is bound with ParamRadio, which drives radio buttons \
                 (radio.<group>.<position>). Use the {}'s own binding",
                control.kind.as_str(),
                control.kind.as_str()
            ));
        };
        let buttons = layout.radio_group(group).len();
        let ParamKind::Stepped { labels } = spec.kind else {
            return Err(format!(
                "uses ParamRadio on param \"{}\", which is not stepped. Make it \
                 ParamSpec::stepped(..) with one label per button of group \"{group}\"",
                spec.name
            ));
        };
        if labels.len() != buttons {
            return Err(format!(
                "is in radio group \"{group}\", which has {buttons} buttons, but param \"{}\" has {} \
                 positions. Give the param one label per button",
                spec.name,
                labels.len()
            ));
        }
        self.position = position;
        Ok(())
    }
}

/// A binding built from closures, for custom behaviour.
pub struct FnBinding<E, D> {
    params: Vec<ParamIndex>,
    on_event: E,
    display: D,
}

impl<E, D> std::fmt::Debug for FnBinding<E, D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FnBinding")
            .field("params", &self.params)
            .finish_non_exhaustive()
    }
}

/// Build a binding from closures. `params` lists the params `display`
/// reads.
pub fn from_fn<E, D>(params: &[ParamIndex], on_event: E, display: D) -> FnBinding<E, D>
where
    E: FnMut(&ControlEvent, &mut Edit<'_>),
    D: Fn(&Model) -> WidgetState,
{
    FnBinding {
        params: params.to_vec(),
        on_event,
        display,
    }
}

impl<E, D> Binding for FnBinding<E, D>
where
    E: FnMut(&ControlEvent, &mut Edit<'_>),
    D: Fn(&Model) -> WidgetState,
{
    fn on_event(&mut self, event: &ControlEvent, edit: &mut Edit<'_>) {
        (self.on_event)(event, edit);
    }

    fn display(&self, model: &Model) -> WidgetState {
        (self.display)(model)
    }

    fn params(&self) -> &[ParamIndex] {
        &self.params
    }
}
