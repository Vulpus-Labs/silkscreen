//! The host contract ([design/overview.md]), split per [design/hosts.md]:
//!
//! - [`EditorState`] lives as long as the plugin: model, controller, event
//!   log cursors, engine link. Engine traffic and host changes keep flowing
//!   while no editor window is open.
//! - [`EditorView`] lives as long as a window: gesture recognizer,
//!   compositor, faceplate raster, scale.
//! - [`Editor`] bundles the two for hosts that own both (web, standalone,
//!   tests).
//!
//! **Threading:** every method here is called on the main/UI thread. The
//! only cross-thread traffic is inside the [`EngineLink`].
//!
//! [design/overview.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/overview.md
//! [design/hosts.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/hosts.md

mod live;
mod presets;

use std::cell::RefCell;
use std::rc::Rc;

use crate::browser::{Place, PresetBrowser};
use crate::dropdown::{DropDownField, menu};
use crate::panel::{self, Panel, Reply, Route};
use crate::presets::PresetField;
use crate::theme::Theme;
use crate::tooltip::{Lettering, Tip};
use crate::widget::{self, EditorWidget, LiveMeter, LiveScope};
use live::Live;
pub use live::MeterSource;
use presets::{PresetSession, browser_for, import_drop};
use silkscreen_bridge::{EngineEvent, EngineLink, ParamIndex};
use silkscreen_core::tiny_skia::PixmapMut;
use silkscreen_core::{
    Compositor, ControlEvent, ControlId, GestureOutput, GestureRecognizer, IRect, Layout,
    RasterSource, RawEvent, RawKind, WidgetState,
};
use silkscreen_core::{MouseCursor, Overlay};
use silkscreen_model::{
    Binding, Controller, Cursor, HostEvent, Model, ModelEvent, Origin, ParamSpec, UiRequest,
};

/// A control's id as written in the SVG, for messages.
fn svg_id(control: &silkscreen_core::ControlSpec) -> String {
    match control.kind {
        silkscreen_core::ControlKind::Radio { group, position } => {
            format!("radio.{group}.{position}")
        }
        kind => format!("{}.{}", kind.as_str(), control.name),
    }
}

/// The long-lived half of an editor.
pub struct EditorState {
    layout: &'static Layout,
    model: Model,
    controller: Controller,
    link: Box<dyn EngineLink>,
    engine_cursor: Cursor,
    view_cursor: Cursor,
    /// For each param, the controls whose bindings display it.
    param_controls: Vec<Vec<ControlId>>,
    /// Something was sent to the engine since the last
    /// [`take_engine_activity`](Self::take_engine_activity).
    engine_activity: bool,
    /// The current preset, the plugin's presets and its random sounds.
    presets: PresetSession,
    /// Panels the plugin opens from its own controls (`UiRequest::Panel`).
    panel_makers: Vec<(ControlId, PanelMaker)>,
    /// Widgets the plugin draws its controls with (design/faceplates.md).
    widget_makers: Vec<(ControlId, WidgetMaker)>,
    /// What the scope and meter controls draw (design/engine-bridge.md).
    live: Live,
    /// The overlays' colours.
    theme: Theme,
    /// The view's clock, as of its last input or tick.
    now_ms: f64,
    /// Where the pointer is over the faceplate, logical, if it is.
    pointer: Option<silkscreen_core::Point>,
    /// Each page group's shown page (design/faceplates.md): UI state, not a param.
    pages: Vec<u16>,
    /// The shown pages changed since the view last looked.
    pages_changed: bool,
    /// The plugin's lock, if it is licensed (design/licensing-hook.md).
    licence: Option<Rc<dyn crate::Licence>>,
    /// The host's clipboard, for Paste.
    clipboard: Option<Rc<dyn crate::Clipboard>>,
}

/// Builds a plugin's panel when its control asks for it.
type PanelMaker = Box<dyn Fn(&EditorState) -> Box<dyn Panel>>;

/// Builds a plugin's widget for a control, once per view.
type WidgetMaker = Box<dyn Fn(&EditorState) -> Box<dyn EditorWidget>>;

impl std::fmt::Debug for EditorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditorState")
            .field("model", &self.model)
            .field("controller", &self.controller)
            .finish_non_exhaustive()
    }
}

impl EditorState {
    /// `bindings[i]` handles `ControlId(i)`: pass generated
    /// `Controls::into_array()` (boxed).
    pub fn new(
        layout: &'static Layout,
        specs: &'static [ParamSpec],
        bindings: impl IntoIterator<Item = Box<dyn Binding>>,
        link: Box<dyn EngineLink>,
    ) -> Self {
        let model = Model::new(specs);
        let mut controller = Controller::new(bindings);
        assert_eq!(
            controller.len(),
            layout.controls.len(),
            "one binding per control is required"
        );
        for control in layout.controls {
            let binding = controller.binding_mut(control.id);
            let checked = binding
                .params()
                .iter()
                .try_for_each(|&p| silkscreen_model::binding::spec_of(specs, p).map(|_| ()))
                .and_then(|()| binding.attach(control, specs, layout));
            if let Err(e) = checked {
                panic!("control \"{}\" {e}", svg_id(control));
            }
        }
        let mut param_controls = vec![Vec::new(); specs.len()];
        for spec in layout.controls {
            for p in controller.binding(spec.id).params() {
                param_controls[p.index()].push(spec.id);
            }
        }
        let cursor = model.log().cursor();
        let mut link = link;
        // The engine hears which page each group shows from the start, so
        // telemetry that follows the page (design/engine-bridge.md) matches a new view.
        for group in 0..layout.pages.len() {
            let group = u16::try_from(group).expect("page groups fit u16");
            link.send(EngineEvent::Shown { group, page: 0 });
        }
        Self {
            layout,
            model,
            controller,
            link,
            engine_cursor: cursor,
            view_cursor: cursor,
            param_controls,
            engine_activity: false,
            presets: PresetSession::default(),
            panel_makers: Vec::new(),
            widget_makers: Vec::new(),
            live: Live::new(layout),
            theme: Theme::default(),
            now_ms: 0.0,
            pointer: None,
            pages: vec![0; layout.pages.len()],
            pages_changed: false,
            licence: None,
            clipboard: None,
        }
    }

    /// Each page group's shown page, by group index (design/faceplates.md).
    pub fn shown_pages(&self) -> &[u16] {
        &self.pages
    }

    /// Show `page` of page group `group`. Not a param: never automated,
    /// saved with a preset or undone; the engine is told
    /// (`EngineEvent::Shown`, design/engine-bridge.md).
    ///
    /// # Panics
    /// If the group or page doesn't exist.
    pub fn show_page(&mut self, group: u16, page: u16) {
        let g = &self.layout.pages[usize::from(group)];
        assert!(
            usize::from(page) < g.pages.len(),
            "page group {} has no page {page}",
            g.name
        );
        if self.pages[usize::from(group)] != page {
            self.pages[usize::from(group)] = page;
            self.pages_changed = true;
            self.link.send(EngineEvent::Shown { group, page });
        }
    }

    /// The editor's UI state, to save with the project (design/faceplates.md): each
    /// page group's shown page as `("page.<group>", "<n>")`, and the
    /// current preset (design/presets.md) as `preset.name`, `preset.place`
    /// (`factory:<category>`, `user:` for Uncategorised, or
    /// `user:<folder>`) and `preset.modified` (`1`), each when there is
    /// one. Presets don't carry it.
    pub fn ui_state(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = self
            .layout
            .pages
            .iter()
            .zip(&self.pages)
            .map(|(g, n)| (format!("page.{}", g.name), n.to_string()))
            .collect();
        if let Some(name) = &self.presets.label.name {
            out.push(("preset.name".into(), name.clone()));
        }
        if let Some(comment) = &self.presets.comment {
            out.push(("preset.comment".into(), comment.clone()));
        }
        match &self.presets.place {
            Some(Place::Factory(c)) => out.push(("preset.place".into(), format!("factory:{c}"))),
            Some(Place::User(f)) => out.push((
                "preset.place".into(),
                format!("user:{}", f.as_deref().unwrap_or_default()),
            )),
            None => {}
        }
        if self.presets.label.modified {
            out.push(("preset.modified".into(), "1".into()));
        }
        out
    }

    /// Restore saved UI state. Unknown keys, and pages this build doesn't
    /// have, are ignored; a group not mentioned keeps its page.
    pub fn restore_ui_state(&mut self, ui: &[(String, String)]) {
        // `ui_state` leaves out what was empty (no name, no place, no
        // comment, unmodified): start from that, so nothing shown before
        // survives a state that didn't have it.
        self.presets.comment = None;
        self.set_preset_name(None);
        self.set_preset_place(None);
        self.set_preset_modified(false);
        for (key, value) in ui {
            match key.as_str() {
                "preset.name" => self.set_preset_name(Some(value.clone())),
                "preset.comment" => self.presets.comment = Some(value.clone()),
                "preset.place" => {
                    let place = if let Some(c) = value.strip_prefix("factory:") {
                        // The bank's own `&'static str`, if it still has
                        // the category.
                        self.presets.setup.as_ref().and_then(|s| {
                            s.factory
                                .iter()
                                .find(|p| p.category == c)
                                .map(|p| Place::Factory(p.category))
                        })
                    } else {
                        value
                            .strip_prefix("user:")
                            .map(|f| Place::User((!f.is_empty()).then(|| f.to_owned())))
                    };
                    self.set_preset_place(place);
                }
                "preset.modified" => self.set_preset_modified(value == "1"),
                _ => {}
            }
            let Some(name) = key.strip_prefix("page.") else {
                continue;
            };
            let (Some(group), Ok(page)) = (self.layout.page_group(name), value.parse::<u16>())
            else {
                continue;
            };
            if usize::from(page) < self.layout.pages[usize::from(group)].pages.len() {
                self.show_page(group, page);
            }
        }
    }

    /// The page tab `control` stands for: (group, page).
    fn tab_page(&self, control: ControlId) -> Option<(u16, u16)> {
        match self.layout.control(control).kind {
            silkscreen_core::ControlKind::Tab { group, position } => {
                Some((self.layout.page_group(group)?, position))
            }
            _ => None,
        }
    }

    pub fn layout(&self) -> &'static Layout {
        self.layout
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    pub fn controller(&self) -> &Controller {
        &self.controller
    }

    /// Apply a change from the host, then forward model events.
    ///
    /// A [`HostEvent::StateLoaded`] also asks the engine to load the new
    /// values: the model logs them as host-origin, which the engine cursor
    /// skips, but unlike automation the engine doesn't have them yet
    /// (design/engine-bridge.md).
    pub fn handle_host(&mut self, event: HostEvent) {
        let load = matches!(event, HostEvent::StateLoaded(_));
        self.controller.handle_host(&mut self.model, event);
        self.flush_engine();
        if load {
            let values: Vec<f32> = self.param_values().collect();
            self.link.load(&values);
            self.engine_activity = true;
        }
    }

    /// Undo the most recent user gesture; the engine (and so the host)
    /// sees it as a bracketed gesture. Returns `false` if there was nothing
    /// to undo or a gesture is in progress.
    pub fn undo(&mut self) -> bool {
        let done = self.model.undo();
        self.flush_engine();
        done
    }

    /// Redo the most recently undone gesture. See [`undo`](Self::undo).
    pub fn redo(&mut self) -> bool {
        let done = self.model.redo();
        self.flush_engine();
        done
    }

    /// Whether anything was sent to the engine since the last call. Plugin
    /// hosts use it to ask for a flush when the audio thread isn't running
    /// to carry UI edits to the host.
    pub fn take_engine_activity(&mut self) -> bool {
        std::mem::take(&mut self.engine_activity)
    }

    /// Pull host-originated changes from the engine link into the
    /// controller, and forward pending model events to the engine. Call
    /// periodically (e.g. on the host's GUI timer) even with no view open.
    /// Returns how many host changes were applied.
    pub fn pump(&mut self) -> usize {
        let mut changes = Vec::new();
        self.link
            .poll_host(&mut |param, value| changes.push((param, value)));
        let n = changes.len();
        for (param, value) in changes {
            self.controller
                .handle_host(&mut self.model, HostEvent::ParamChanged { param, value });
        }
        self.flush_engine();
        n
    }

    /// Forward model events to the engine. Host-origin events are skipped:
    /// the engine already has them (design/engine-bridge.md).
    /// Tell the engine a whole patch follows (design/engine-bridge.md), after
    /// whatever is already pending, so it can fade the old one out.
    fn mark_patch_load(&mut self) {
        self.flush_engine();
        self.link.send(EngineEvent::PatchLoad);
        self.engine_activity = true;
    }

    fn flush_engine(&mut self) {
        let mut modified = false;
        for event in self.model.log().read(&mut self.engine_cursor) {
            if event.origin() == Origin::Host {
                continue;
            }
            modified |= matches!(event, ModelEvent::ParamChanged { .. });
            self.engine_activity = true;
            self.link.send(match *event {
                ModelEvent::ParamChanged { param, value, .. } => EngineEvent::ParamValue {
                    index: param,
                    value,
                },
                ModelEvent::GestureBegin { param, .. } => {
                    EngineEvent::GestureBegin { index: param }
                }
                ModelEvent::GestureEnd { param, .. } => EngineEvent::GestureEnd { index: param },
            });
        }
        if modified && !self.presets.loading && !self.presets.label.modified {
            self.set_preset_modified(true);
        }
        self.compact();
    }

    /// Controls whose display changed since the view last looked.
    /// Whether any param changed: including params no control shows,
    /// which a description may still read.
    fn drain_view(&mut self, mut damaged: impl FnMut(ControlId)) -> bool {
        let mut any = false;
        for event in self.model.log().read(&mut self.view_cursor) {
            any = true;
            for id in &self.param_controls[event.param().index()] {
                damaged(*id);
            }
        }
        self.compact();
        any
    }

    fn compact(&mut self) {
        let cursors = [self.engine_cursor, self.view_cursor];
        self.model.log_mut().compact(&cursors);
    }

    /// Edit params as the user would through a control (`Origin::Ui`): for
    /// panels and other UI outside the faceplate's bindings. Bracket
    /// changes in `begin`/`end` so the host sees a gesture.
    pub fn edit(&mut self, f: impl FnOnce(&mut silkscreen_model::Edit<'_>)) {
        f(&mut self.model.edit(Origin::Ui));
        self.flush_engine();
    }

    /// Open the panel `make` builds whenever `control`'s binding requests
    /// it (`UiRequest::Panel`). Registering again replaces it.
    pub fn set_panel(
        &mut self,
        control: ControlId,
        make: impl Fn(&EditorState) -> Box<dyn Panel> + 'static,
    ) {
        self.panel_makers.retain(|(c, _)| *c != control);
        self.panel_makers.push((control, Box::new(make)));
    }

    /// Draw tooltips, menus, tables, dialogs, the preset browser and
    /// preset and drop-down fields in `theme`'s colours.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    /// The overlays' theme: what to read outside drawing (in a panel's
    /// `input` or a widget's `update`); while drawing,
    /// [`theme::current`](crate::theme::current) gives the same.
    pub fn theme(&self) -> Theme {
        self.theme
    }

    /// The view's clock (ms), as of its last input or tick: for editor
    /// widgets that animate.
    pub fn now_ms(&self) -> f64 {
        self.now_ms
    }

    /// Where the pointer is over the faceplate, in logical units, if it
    /// is: for editor widgets that follow it (design/faceplates.md).
    pub fn pointer(&self) -> Option<silkscreen_core::Point> {
        self.pointer
    }

    /// Draw `control` with the widget `make` builds, once for each view
    /// (design/faceplates.md): a canvas control's painter, or any control drawn
    /// the plugin's own way. Registering again replaces it.
    /// Give the editor the plugin's lock: while it says locked, the
    /// faceplate shows the Unregistered strip (design/licensing-hook.md).
    pub fn set_licence(&mut self, licence: Rc<dyn crate::Licence>) {
        self.licence = Some(licence);
    }

    pub fn licence(&self) -> Option<Rc<dyn crate::Licence>> {
        self.licence.clone()
    }

    /// Whether the plugin is licensed and still locked.
    pub fn locked(&self) -> bool {
        self.licence.as_ref().is_some_and(|l| l.locked())
    }

    /// Give the editor the host's clipboard: Paste, and Cmd/Ctrl-V in
    /// text fields.
    pub fn set_clipboard(&mut self, clipboard: Rc<dyn crate::Clipboard>) {
        self.clipboard = Some(clipboard);
    }

    pub fn clipboard(&self) -> Option<Rc<dyn crate::Clipboard>> {
        self.clipboard.clone()
    }

    pub fn set_widget(
        &mut self,
        control: ControlId,
        make: impl Fn(&EditorState) -> Box<dyn EditorWidget> + 'static,
    ) {
        self.widget_makers.retain(|(c, _)| *c != control);
        self.widget_makers.push((control, Box::new(make)));
    }

    fn make_panel(&self, control: ControlId) -> Option<Box<dyn Panel>> {
        let (_, make) = self.panel_makers.iter().find(|(c, _)| *c == control)?;
        Some(make(self))
    }

    fn handle_control(&mut self, id: ControlId, event: &silkscreen_core::ControlEvent) {
        self.controller.handle_control(&mut self.model, id, event);
        self.flush_engine();
    }

    fn display(&self, id: ControlId) -> WidgetState {
        self.controller.binding(id).display(&self.model)
    }

    /// What a tooltip or drag readout says for `id`: the binding's own
    /// description, else its first param's name and formatted value.
    /// `None` for a control bound to no param that says nothing.
    pub fn describe(&self, id: ControlId) -> Option<String> {
        let binding = self.controller.binding(id);
        if let Some(text) = binding.describe(&self.model) {
            return Some(text);
        }
        // The preset field says what the current preset is.
        if self.layout.control(id).kind == silkscreen_core::ControlKind::Preset {
            return self.presets.comment.clone();
        }
        let p = *binding.params().first()?;
        let spec = &self.model.specs()[p.index()];
        Some(format!("{}  {}", spec.name, spec.format(self.model.get(p))))
    }

    /// Param values as the engine should see them right now, in spec order
    /// (the `i`th is `ParamIndex(i)`'s), for hosts that need to (re)seed an
    /// engine. Not the engine's telemetry: that is
    /// [`engine_values`](Self::engine_values).
    pub fn param_values(&self) -> impl Iterator<Item = f32> + '_ {
        (0..self.model.specs().len()).map(|i| self.model.get(ParamIndex(i as u32)))
    }
}

/// The editor widgets a view keeps up to date, by control.
type LiveWidgets = Vec<(ControlId, widget::Shared)>;

/// Each control's widget, and the editor widgets the view keeps up to
/// date: the plugin's where it gave one, else the built-in one.
fn widgets(state: &EditorState) -> (Vec<Box<dyn silkscreen_core::Widget>>, LiveWidgets) {
    use silkscreen_core::ControlKind;
    let mut live = Vec::new();
    let mut skins = silkscreen_core::SkinCache::default();
    let widgets = state
        .layout
        .controls
        .iter()
        .map(|spec| {
            let made = state
                .widget_makers
                .iter()
                .find(|(c, _)| *c == spec.id)
                .map(|(_, make)| make(state));
            let editor: Box<dyn EditorWidget> = match (made, spec.kind) {
                (Some(w), _) => w,
                (None, ControlKind::Preset) => Box::new(PresetField::new()),
                (None, ControlKind::Scope) => Box::new(LiveScope::new(state.live.scope_style)),
                (None, ControlKind::Meter) => {
                    Box::new(LiveMeter::new(spec.id, state.live.meter_style))
                }
                (None, ControlKind::Canvas) => panic!(
                    "control \"{}\" has nothing to paint it: give it a widget with \
                     EditorState::set_widget",
                    svg_id(spec)
                ),
                (None, ControlKind::DropDown) => {
                    let labels = state
                        .controller
                        .binding(spec.id)
                        .params()
                        .first()
                        .map_or(&[][..], |p| state.model.spec(*p).position_labels());
                    return Box::new(DropDownField::new(labels))
                        as Box<dyn silkscreen_core::Widget>;
                }
                (None, _) => return silkscreen_core::widget_for(spec, &mut skins),
            };
            let shared: widget::Shared = Rc::new(RefCell::new(editor));
            live.push((spec.id, Rc::clone(&shared)));
            Box::new(widget::Drawn(shared)) as Box<dyn silkscreen_core::Widget>
        })
        .collect();
    (widgets, live)
}

/// The page shown in tab `spec`'s group.
fn spec_page(state: &EditorState, spec: &silkscreen_core::ControlSpec) -> Option<u16> {
    let (group, _) = state.tab_page(spec.id)?;
    state.pages.get(usize::from(group)).copied()
}

/// How long a preset field shows a message ("Saved").
const FLASH_MS: f64 = 1500.0;

/// The per-window half of an editor.
pub struct EditorView {
    recognizer: GestureRecognizer,
    compositor: Compositor,
    /// Widgets that catch up with the editor on each sync.
    live: LiveWidgets,
    /// The theme last drawn with: a new one repaints everything.
    theme: Theme,
    /// Something changed that the next dispatch must sync, though it
    /// brings no events (a tip hidden by movement).
    unsynced: bool,
    /// Nothing has reached the view since its last sync: no input, view
    /// change or tick. [`render`](EditorView::render) then syncs only if
    /// the state moved (see [`EditorView::behind`]).
    synced: bool,
    raster: Box<dyn RasterSource>,
    states: Vec<WidgetState>,
    out: GestureOutput,
    /// Interaction state as of the last sync: (hover, active, focus).
    flags: (Option<ControlId>, Option<ControlId>, Option<ControlId>),
    lettering: Lettering,
    /// The control the pointer has rested on, until it moves or leaves.
    dwell: Option<ControlId>,
    /// The control being dragged, from `DragStart` to `DragEnd`.
    dragging: Option<ControlId>,
    /// The tip shown: its control, label and the scale it was laid out at.
    tip: Option<(ControlId, String, f32)>,
    /// Open panels, bottom to top; the top one takes input.
    panels: Vec<panel::Shared>,
    /// Time of the last input or tick, for events the view makes itself.
    now_ms: f64,
    /// The preset field's message and when it goes.
    flash: Option<(String, f64)>,
    /// The editor's random generator has been seeded from the clock.
    seeded: bool,
    /// Where the Unregistered strip is, while it shows (design/licensing-hook.md).
    strip: Option<silkscreen_core::Rect>,
}

impl std::fmt::Debug for EditorView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditorView")
            .field("compositor", &self.compositor)
            .finish_non_exhaustive()
    }
}

impl EditorView {
    /// A view at scale 1 over `state`. Everything starts damaged.
    pub fn new(state: &mut EditorState, raster: Box<dyn RasterSource>) -> Self {
        // Anything already in the log predates this view, which repaints
        // fully anyway.
        state.drain_view(|_| {});
        let (widgets, live) = widgets(state);
        let mut view = Self {
            recognizer: GestureRecognizer::default(),
            compositor: Compositor::with_widgets(state.layout, widgets),
            live,
            theme: state.theme,
            unsynced: false,
            synced: false,
            raster,
            states: vec![WidgetState::default(); state.layout.controls.len()],
            out: Vec::new(),
            flags: (None, None, None),
            lettering: Lettering::shared(),
            dwell: None,
            dragging: None,
            tip: None,
            panels: Vec::new(),
            now_ms: 0.0,
            flash: None,
            seeded: false,
            strip: None,
        };
        view.sync_pages(state);
        view
    }

    pub fn recognizer(&self) -> &GestureRecognizer {
        &self.recognizer
    }

    /// The mouse cursor for where the pointer is (design/overlays-and-text.md): the top
    /// panel's, or the dragged or hovered control's. Hosts show it after
    /// each input event.
    pub fn cursor(&self, state: &EditorState) -> MouseCursor {
        if let Some(top) = self.panels.last() {
            let top = top.borrow();
            return state
                .pointer
                .filter(|&pos| top.rect().contains(pos))
                .map_or(MouseCursor::Default, |pos| top.cursor(pos));
        }
        if self
            .strip
            .zip(state.pointer)
            .is_some_and(|(r, pos)| r.contains(pos))
        {
            return MouseCursor::Hand;
        }
        self.recognizer
            .active()
            .or_else(|| self.recognizer.hover())
            .map_or(MouseCursor::Default, |id| {
                state
                    .controller
                    .binding(id)
                    .cursor()
                    .unwrap_or_else(|| state.layout.control(id).kind.cursor())
            })
    }

    /// Feed one input event.
    pub fn handle(&mut self, state: &mut EditorState, event: &RawEvent) {
        self.synced = false;
        self.now_ms = event.time_ms;
        state.now_ms = event.time_ms;
        match &event.kind {
            RawKind::PointerMove { pos, .. }
            | RawKind::PointerDown { pos, .. }
            | RawKind::PointerUp { pos, .. } => state.pointer = Some(*pos),
            RawKind::PointerLeave => state.pointer = None,
            _ => {}
        }
        // A preset file dropped anywhere on the editor imports it
        // (design/presets.md); other drops go to the control under them.
        if let RawKind::Drop { payload, .. } = &event.kind
            && import_drop(state, payload)
        {
            self.sync(state);
            return;
        }
        if let Some(top) = self.panels.last().cloned() {
            let rect = top.borrow().rect();
            match panel::route(&event.kind, rect) {
                Route::Panel => {
                    let reply = top.borrow_mut().input(&event.kind, state);
                    self.apply_reply(state, &top, rect, reply);
                }
                Route::CloseTop => self.close_panel(),
                Route::Swallow => {}
            }
            self.sync(state);
            return;
        }
        // The Unregistered strip sits above the controls: a press on it
        // opens the unlock dialog and goes no further.
        if let RawKind::PointerDown { pos, .. } = event.kind
            && self.strip.is_some_and(|r| r.contains(pos))
        {
            self.open_panel(state, Box::new(crate::Unlock::centred(state.layout.size)));
            self.sync(state);
            return;
        }
        // Movement, a press, a scroll or leaving hides a tooltip (design/overlays-and-text.md).
        if !matches!(event.kind, RawKind::Key { .. } | RawKind::PointerUp { .. })
            && self.dwell.take().is_some()
        {
            self.unsynced = true;
        }
        self.recognizer.handle(state.layout, event, &mut self.out);
        self.dispatch(state);
    }

    /// Carry out the top panel's `reply`; `rect` is where it was before.
    fn apply_reply(
        &mut self,
        state: &mut EditorState,
        top: &panel::Shared,
        rect: silkscreen_core::Rect,
        reply: Reply,
    ) {
        let scale = self.compositor.scale();
        match reply {
            Reply::Ignored => {}
            Reply::Handled => {
                let after = top.borrow().rect();
                for r in [rect, after] {
                    self.compositor.damage(r.to_physical(scale));
                }
            }
            Reply::Repaint(rects) => {
                for r in rects {
                    self.compositor.damage(r.to_physical(scale));
                }
            }
            Reply::Close => self.close_panel(),
            Reply::Replace(next) => {
                self.close_panel();
                self.push_panel(next);
                self.show_tip(state);
            }
            Reply::Open(next) => {
                self.compositor.damage(rect.to_physical(scale));
                self.push_panel(next);
                self.show_tip(state);
            }
        }
    }

    /// Advance time: fires dwells, pulls host changes, forwards engine
    /// events, and feeds scopes and meters the engine's latest output.
    pub fn tick(&mut self, state: &mut EditorState, now_ms: f64) {
        if !self.seeded {
            // Random sounds differ from session to session.
            state.seed_random(now_ms.to_bits());
            self.seeded = true;
        }
        self.now_ms = now_ms;
        state.now_ms = now_ms;
        self.recognizer.tick(now_ms, &mut self.out);
        self.dispatch(state);
        if let Some(io) = state.preset_io() {
            while let Some(event) = io.take_event() {
                state.handle_io(event);
            }
        }
        if let Some(top) = self.panels.last().cloned() {
            let rect = top.borrow().rect();
            let reply = top.borrow_mut().poll(state);
            self.apply_reply(state, &top, rect, reply);
        }
        if self
            .flash
            .as_ref()
            .is_some_and(|(_, until)| now_ms >= *until)
        {
            state.clear_preset_flash();
            self.flash = None;
        }
        let _ = state.pump();
        state.pump_telemetry(now_ms);
        self.sync(state);
    }

    /// Change the effective scale (display scale × zoom). Repaints fully.
    pub fn set_scale(&mut self, scale: f32) {
        self.synced = false;
        self.compositor.set_scale(scale);
        // Lay any tip out again at the new scale on the next sync.
        self.tip = None;
        self.compositor.set_top(None);
    }

    pub fn scale(&self) -> f32 {
        self.compositor.scale()
    }

    /// Frame buffer size at the current scale.
    pub fn physical_size(&self) -> (u32, u32) {
        self.compositor.frame_size()
    }

    /// Show (or clear) a small diagnostic label in the bottom-left corner,
    /// e.g. a version and build id.
    pub fn set_badge(&mut self, text: Option<String>) {
        self.compositor.set_badge(text);
    }

    /// Whether [`render`](Self::render) has anything to paint. Hosts use
    /// this to request a redraw only when needed.
    pub fn needs_render(&self) -> bool {
        self.compositor.has_damage()
    }

    /// Repaint damage into `target` (exactly [`physical_size`]) and return
    /// the rects to present.
    ///
    /// [`physical_size`]: Self::physical_size
    pub fn render(&mut self, state: &mut EditorState, target: &mut PixmapMut<'_>) -> &[IRect] {
        // A host ticks, then renders: the tick's sync is enough unless
        // something moved between them.
        if !self.synced || self.behind(state) {
            self.sync(state);
        }
        let (hover, active, focus) = self.flags;
        // The states below are built for exactly this damage: none may
        // arrive between here and the render, or a control could draw
        // with last frame's state.
        self.compositor.prepare();
        let mark = self.compositor.damage_mark();
        // Only what this render draws: the rest keep last frame's state.
        for spec in state.layout.controls {
            if !self.compositor.will_draw(spec.id) {
                continue;
            }
            let mut s = state.display(spec.id);
            s.hover = hover == Some(spec.id);
            s.active = active == Some(spec.id);
            s.focused = focus == Some(spec.id);
            if let silkscreen_core::ControlKind::Tab { position, .. } = spec.kind {
                s.selected = spec_page(state, spec) == Some(position);
            }
            self.states[spec.id.index()] = s;
        }
        debug_assert_eq!(
            self.compositor.damage_mark(),
            mark,
            "damage arrived after the widget states were built"
        );
        let (compositor, raster, states) = (&mut self.compositor, &mut self.raster, &self.states);
        crate::theme::with(state.theme, || {
            compositor.render(target, raster.as_mut(), states)
        })
    }

    /// End any drag in progress, e.g. before dropping the view, so no host
    /// gesture is left open.
    pub fn cancel(&mut self, state: &mut EditorState) {
        self.synced = false;
        self.recognizer.cancel(&mut self.out);
        self.dispatch(state);
    }

    fn dispatch(&mut self, state: &mut EditorState) {
        // A pointer moving within a control (`Hover`) needs a sync only
        // if a binding acted on it, which shows in the model's log.
        let mut busy = self.unsynced
            || self
                .out
                .iter()
                .any(|(_, e)| !matches!(e, ControlEvent::Hover { .. }));
        let log_end = state.model.log().end();
        for (id, event) in self.out.drain(..) {
            match event {
                ControlEvent::HoverDwell => self.dwell = Some(id),
                ControlEvent::DragStart { .. } => self.dragging = Some(id),
                ControlEvent::DragEnd { .. } => self.dragging = None,
                ControlEvent::HoverLeave | ControlEvent::Press { .. } => self.dwell = None,
                _ => {}
            }
            state.handle_control(id, &event);
        }
        let requests = state.model.take_requests();
        busy |= !requests.is_empty() || state.model.log().end() != log_end;
        for request in requests {
            match request {
                UiRequest::Menu { control, param } => {
                    let list = menu(state, control, param, self.compositor.scale());
                    self.open_panel(state, Box::new(list));
                }
                UiRequest::Presets { .. } => {
                    if let Some(browser) = browser_for(state) {
                        self.open_panel(state, Box::new(browser));
                    }
                }
                UiRequest::Panel { control } => {
                    if let Some(panel) = state.make_panel(control) {
                        self.open_panel(state, panel);
                    }
                }
                UiRequest::ShowPage { control } => {
                    if let Some((group, page)) = state.tab_page(control) {
                        state.show_page(group, page);
                    }
                }
                UiRequest::PresetStep { forward } => {
                    if let Err(e) = state.step_preset(forward) {
                        state.flash_preset(&e);
                    }
                }
                UiRequest::PresetSave => match state.save_preset_in_place() {
                    Some(Ok(())) => {
                        let name = state.preset_name().unwrap_or_default().to_owned();
                        state.flash_preset(&format!("Saved \u{201c}{name}\u{201d}"));
                    }
                    Some(Err(e)) => state.flash_preset(&e),
                    None => self.open_save_as(state),
                },
                UiRequest::PresetSaveAs => self.open_save_as(state),
                UiRequest::PresetInit => state.init_preset(),
                UiRequest::PresetRandomise => state.randomise_preset(),
                UiRequest::MatchLevel {
                    meter,
                    param,
                    target_db,
                } => {
                    state.match_level(meter, param, target_db);
                }
            }
        }
        if busy {
            self.sync(state);
        }
    }

    /// The Save As dialog, centred over the faceplate.
    fn open_save_as(&mut self, state: &mut EditorState) {
        let over = PresetBrowser::centred(state.layout.size);
        if let Some(dialog) = crate::browser::save_as_dialog(state, over, None) {
            self.open_panel(state, Box::new(dialog));
        }
    }

    /// Turn model changes and interaction-state changes into damage.
    fn sync(&mut self, state: &mut EditorState) {
        // A preset field's message shows for FLASH_MS from when it was
        // set, restarting if it changes.
        match (state.preset_flash(), &self.flash) {
            (None, _) => self.flash = None,
            (Some(text), Some((shown, _))) if text == shown => {}
            (Some(text), _) => self.flash = Some((text.to_owned(), self.now_ms + FLASH_MS)),
        }
        self.unsynced = false;
        let strip = state
            .locked()
            .then(|| crate::licence::strip_rect(state.layout.size, &self.lettering));
        let restack = strip != self.strip;
        self.strip = strip;
        if state.theme != self.theme {
            self.theme = state.theme;
            // Panels' opacity follows the theme's background.
            self.show_panels();
            self.compositor.damage_all();
        } else if restack {
            self.show_panels();
        }
        // Whether anything a label might read changed: a description
        // may read params its binding doesn't list (VCO 2's reads TUNING).
        let compositor = &mut self.compositor;
        let mut tip_touched = state.drain_view(|id| compositor.damage_control(id));
        if std::mem::take(&mut state.pages_changed) {
            self.sync_pages(state);
        }
        let flags = (
            self.recognizer.hover(),
            self.recognizer.active(),
            self.recognizer.focus(),
        );
        if flags != self.flags {
            for (old, new) in [
                (self.flags.0, flags.0),
                (self.flags.1, flags.1),
                (self.flags.2, flags.2),
            ] {
                if old != new {
                    for id in [old, new].into_iter().flatten() {
                        self.compositor.damage_control(id);
                    }
                }
            }
            self.flags = flags;
        }
        for (id, widget) in &self.live {
            if widget.borrow_mut().update(state) {
                // A widget's own news (a meter's reading) changes no
                // label but its control's (the preset field's comment).
                tip_touched |= self.tip.as_ref().is_some_and(|(t, _, _)| t == id);
                self.compositor.damage_control(*id);
            }
        }
        for p in &self.panels {
            let mut p = p.borrow_mut();
            let scale = self.compositor.scale();
            match p.update(state) {
                panel::Update::Unchanged => {}
                panel::Update::Rects(rects) => {
                    for r in rects {
                        self.compositor.damage(r.to_physical(scale));
                    }
                }
                panel::Update::Whole => self.compositor.damage(p.rect().to_physical(scale)),
            }
        }
        self.sync_tip(state, tip_touched);
        self.synced = true;
    }

    /// Whether the state moved since the last sync in a way the view
    /// shows: params changed (a host's load, a page's console), a page
    /// shown, the theme or the preset field's message. Anything else a
    /// host changes between a tick and a render shows from the next tick.
    fn behind(&self, state: &EditorState) -> bool {
        state.model.log().cursor() != state.view_cursor
            || state.pages_changed
            || state.theme != self.theme
            || state.preset_flash() != self.flash.as_ref().map(|(text, _)| text.as_str())
            || state.locked() != self.strip.is_some()
    }

    /// Show the state's pages: their regions and every tab repaint, and
    /// hidden controls stop being hit.
    fn sync_pages(&mut self, state: &EditorState) {
        for (group, &page) in state.pages.iter().enumerate() {
            let group = u16::try_from(group).expect("page groups fit u16");
            if self.compositor.shown_pages()[usize::from(group)] != page {
                self.compositor.show_page(group, page);
                for tab in state.layout.tabs(group) {
                    self.compositor.damage_control(tab.id);
                }
            }
        }
        self.recognizer
            .set_shown_pages(state.layout, self.compositor.shown_pages());
    }

    /// Show the drag readout for the control being dragged, else a tooltip for
    /// the control the pointer rests on, else nothing; repaint only when
    /// that changes.
    fn sync_tip(&mut self, state: &EditorState, touched: bool) {
        let want = if self.panels.is_empty() {
            self.dragging
                .or_else(|| self.dwell.filter(|&d| self.recognizer.hover() == Some(d)))
        } else {
            None
        };
        let scale = self.compositor.scale();
        // The same control, unchanged, at the same scale: the same label.
        let same = self.tip.as_ref().map(|(id, _, s)| (*id, *s)) == want.map(|id| (id, scale));
        if same && !touched {
            return;
        }
        let next = want.and_then(|id| state.describe(id).map(|label| (id, label, scale)));
        if next == self.tip {
            return;
        }
        self.tip = next;
        self.show_tip(state);
    }

    /// The tooltip or readout shown, if any.
    pub fn tip(&self) -> Option<&str> {
        self.tip.as_ref().map(|(_, label, _)| label.as_str())
    }

    /// Open `panel` above everything; it takes input until it closes. The
    /// controls beneath stop any drag and lose hover.
    pub fn open_panel(&mut self, state: &mut EditorState, panel: Box<dyn Panel>) {
        if self.panels.is_empty() {
            self.recognizer.cancel(&mut self.out);
            let leave = RawEvent::new(self.now_ms, RawKind::PointerLeave);
            self.recognizer.handle(state.layout, &leave, &mut self.out);
            self.dispatch(state);
            self.dwell = None;
            self.dragging = None;
        }
        self.push_panel(panel);
        self.show_tip(state);
    }

    fn push_panel(&mut self, panel: Box<dyn Panel>) {
        self.synced = false;
        let shared: panel::Shared = Rc::new(RefCell::new(panel));
        self.compositor.push_overlay(Box::new(panel::Drawn(
            Rc::clone(&shared),
            self.opaque_panels(),
        )));
        self.panels.push(shared);
        self.tip = None;
    }

    /// Whether the theme fills panels' bodies, so what they cover needn't
    /// repaint.
    fn opaque_panels(&self) -> bool {
        self.theme.background[3] == 255
    }

    /// Close the top panel, if any.
    pub fn close_panel(&mut self) {
        if self.panels.pop().is_some() {
            self.synced = false;
            self.tip = None;
            self.compositor.pop_overlay();
            self.compositor.set_top(None);
        }
    }

    /// Number of open panels.
    pub fn panels(&self) -> usize {
        self.panels.len()
    }

    /// Whether the top panel takes typed text ([`Panel::takes_text`]).
    /// Hosts beside other key users (a DAW) deliver keys only then.
    pub fn takes_text(&self) -> bool {
        self.panels.last().is_some_and(|p| p.borrow().takes_text())
    }

    /// The top panel's logical bounds, if one is open.
    pub fn top_panel_rect(&self) -> Option<silkscreen_core::Rect> {
        self.panels.last().map(|p| p.borrow().rect())
    }

    /// Where the Unregistered strip is, while it shows (design/licensing-hook.md).
    pub fn strip_rect(&self) -> Option<silkscreen_core::Rect> {
        self.strip
    }

    /// Show the tip, alone: the panels beneath stay as they are.
    fn show_tip(&mut self, state: &EditorState) {
        let tip = self.tip.as_ref().map(|(id, label, scale)| {
            Box::new(Tip::new(
                label.clone(),
                state.layout.control(*id).rect,
                state.layout.size,
                *scale,
                &self.lettering,
            )) as Box<dyn Overlay>
        });
        self.compositor.set_top(tip);
    }

    /// The panels as overlays, bottom to top, all at once (a theme
    /// change); the stack itself pushes and pops one at a time.
    fn show_panels(&mut self) {
        let opaque = self.opaque_panels();
        let strip = self.strip.map(|rect| {
            Box::new(crate::licence::Strip {
                rect,
                lettering: self.lettering.clone(),
            }) as Box<dyn Overlay>
        });
        let overlays: Vec<Box<dyn Overlay>> = strip
            .into_iter()
            .chain(
                self.panels
                    .iter()
                    .map(|p| Box::new(panel::Drawn(Rc::clone(p), opaque)) as Box<dyn Overlay>),
            )
            .collect();
        self.compositor.set_overlays(overlays);
    }
}

/// An [`EditorState`] and its [`EditorView`] together.
#[derive(Debug)]
pub struct Editor {
    pub state: EditorState,
    pub view: EditorView,
}

impl Editor {
    pub fn new(state: EditorState, raster: Box<dyn RasterSource>) -> Self {
        let mut state = state;
        let view = EditorView::new(&mut state, raster);
        Self { state, view }
    }

    pub fn handle(&mut self, event: &RawEvent) {
        self.view.handle(&mut self.state, event);
    }

    pub fn tick(&mut self, now_ms: f64) {
        self.view.tick(&mut self.state, now_ms);
    }

    /// The mouse cursor for where the pointer is.
    pub fn cursor(&self) -> MouseCursor {
        self.view.cursor(&self.state)
    }

    pub fn set_scale(&mut self, scale: f32) {
        self.view.set_scale(scale);
    }

    pub fn physical_size(&self) -> (u32, u32) {
        self.view.physical_size()
    }

    pub fn needs_render(&self) -> bool {
        self.view.needs_render()
    }

    pub fn render(&mut self, target: &mut PixmapMut<'_>) -> &[IRect] {
        self.view.render(&mut self.state, target)
    }

    pub fn model(&self) -> &Model {
        self.state.model()
    }

    /// The tooltip or drag readout shown, if any.
    pub fn tip(&self) -> Option<&str> {
        self.view.tip()
    }

    pub fn state(&self) -> &EditorState {
        &self.state
    }

    /// Open a panel above everything (see [`EditorView::open_panel`]).
    pub fn open_panel(&mut self, panel: Box<dyn Panel>) {
        self.view.open_panel(&mut self.state, panel);
    }

    pub fn close_panel(&mut self) {
        self.view.close_panel();
    }

    /// Number of open panels.
    pub fn panels(&self) -> usize {
        self.view.panels()
    }

    /// Whether the top panel takes typed text ([`Panel::takes_text`]).
    pub fn takes_text(&self) -> bool {
        self.view.takes_text()
    }

    /// The top panel's logical bounds, if one is open.
    pub fn top_panel_rect(&self) -> Option<silkscreen_core::Rect> {
        self.view.top_panel_rect()
    }

    /// Where the Unregistered strip is, while it shows.
    pub fn strip_rect(&self) -> Option<silkscreen_core::Rect> {
        self.view.strip_rect()
    }

    pub fn undo(&mut self) -> bool {
        self.state.undo()
    }

    pub fn redo(&mut self) -> bool {
        self.state.redo()
    }
}
