//! silkscreen example: one faceplate with every control kind.
//!
//! Every control uses its kind's default binding.

use silkscreen::core::EmbeddedRaster;
use silkscreen::core::telemetry::ScopeStyle;
use silkscreen::core::tiny_skia::Color;
use silkscreen::model::ParamSpec;
use silkscreen::{
    Binding, BindingExt, Editor, EditorState, EngineLink, MemoryStore, ParamDropDown, ParamFader,
    ParamIndex, ParamKnob, ParamRadio, ParamSelector, ParamToggle, PresetButton, PresetCommand,
    PresetStore, TabButton, Unbound,
};

/// Generated from `widgets.svg` by `silkscreen-build`.
pub mod faceplate {
    include!(concat!(env!("OUT_DIR"), "/widgets.rs"));
}

pub const LEVEL: ParamIndex = ParamIndex(0);
pub const CUTOFF: ParamIndex = ParamIndex(1);
pub const WAVE: ParamIndex = ParamIndex(2);
pub const STOP: ParamIndex = ParamIndex(3);
pub const SYNC: ParamIndex = ParamIndex(4);
pub const TUNING: ParamIndex = ParamIndex(5);
pub const SELECT: ParamIndex = ParamIndex(6);
pub const FINE: ParamIndex = ParamIndex(7);
pub const MODE: ParamIndex = ParamIndex(8);

pub static PARAMS: [ParamSpec; 9] = [
    ParamSpec::new("Level", 0.0, 1.0, 0.8).id(1),
    ParamSpec::new("Cutoff", 0.0, 1.0, 0.5).id(2),
    ParamSpec::stepped("Wave", &["Saw", "Square", "Pulse", "Tri", "Sine"], 0).id(3),
    ParamSpec::toggle("Stop", false).id(4),
    ParamSpec::toggle("Sync", false).id(5),
    ParamSpec::stepped("Tuning", &["Free", "Just", "Equal"], 2).id(6),
    ParamSpec::stepped("Select", &["1", "2", "3", "4", "5", "6"], 0).id(7),
    ParamSpec::new("Fine", -50.0, 50.0, 0.0).bipolar().id(8),
    ParamSpec::stepped(
        "Mode",
        &[
            "Mono", "Poly", "Unison", "Legato", "Duo", "Chord", "Arp Up", "Arp Down", "Random",
        ],
        0,
    )
    .id(9),
];

/// The bindings, one per control.
pub fn controls() -> faceplate::Controls<Box<dyn Binding>> {
    faceplate::Controls {
        level: Box::new(ParamKnob::new(LEVEL)),
        fine: Box::new(ParamKnob::new(FINE)),
        cutoff: Box::new(ParamFader::new(CUTOFF)),
        wave: Box::new(ParamSelector::new(WAVE)),
        stop: Box::new(ParamToggle::new(STOP)),
        sync: Box::new(ParamToggle::new(SYNC)),
        select: Box::new(ParamSelector::new(SELECT)),
        tuning_0: Box::new(ParamRadio::new(TUNING)),
        tuning_1: Box::new(ParamRadio::new(TUNING)),
        tuning_2: Box::new(ParamRadio::new(TUNING)),
        mode: Box::new(ParamDropDown::new(MODE)),
        program: Box::new(PresetButton::new()),
        save: Box::new(PresetCommand::save()),
        saveas: Box::new(PresetCommand::save_as()),
        init: Box::new(PresetCommand::init()),
    }
}

pub fn state(link: Box<dyn EngineLink>) -> EditorState {
    EditorState::new(&faceplate::LAYOUT, &PARAMS, controls().into_array(), link)
}

// The factory presets, `FACTORY`, from `presets/factory` (design/presets.md).
include!(concat!(env!("OUT_DIR"), "/factory_presets.rs"));

/// The editor, with presets kept in memory.
pub fn editor(link: Box<dyn EngineLink>) -> Editor {
    editor_with(link, Box::new(MemoryStore::default()))
}

/// The editor, with the user's presets in `store`.
pub fn editor_with(link: Box<dyn EngineLink>, store: Box<dyn PresetStore>) -> Editor {
    let mut state = state(link);
    state.set_presets(store, FACTORY);
    Editor::new(state, Box::new(EmbeddedRaster::new(&faceplate::FACEPLATE)))
}

/// The editor with dim rules: Level and Fine dim while Sync is on,
/// Cutoff while Stop is.
pub fn dimmed_editor(link: Box<dyn EngineLink>) -> Editor {
    let mut controls = controls();
    controls.level = Box::new(ParamKnob::new(LEVEL).dimmed_when(&[SYNC], |m| m.get(SYNC) >= 0.5));
    controls.fine = Box::new(ParamKnob::new(FINE).dimmed_when(&[SYNC], |m| m.get(SYNC) >= 0.5));
    controls.cutoff =
        Box::new(ParamFader::new(CUTOFF).dimmed_when(&[STOP], |m| m.get(STOP) >= 0.5));
    let state = EditorState::new(&faceplate::LAYOUT, &PARAMS, controls.into_array(), link);
    Editor::new(state, Box::new(EmbeddedRaster::new(&faceplate::FACEPLATE)))
}

/// Generated from `live.svg`: a scope and a meter (design/engine-bridge.md).
pub mod live {
    include!(concat!(env!("OUT_DIR"), "/live.rs"));
}

/// The live displays' editor: no params, a scope with its axis on the
/// panel's hairline, and a stereo meter, fed by `link`'s telemetry.
pub fn live_editor(link: Box<dyn EngineLink>) -> Editor {
    let controls = live::Controls::<Box<dyn Binding>> {
        wave: Box::new(Unbound),
        level: Box::new(Unbound),
    };
    let mut state = EditorState::new(&live::LAYOUT, &[], controls.into_array(), link);
    state.set_scope_style(ScopeStyle {
        axis: Some((Color::from_rgba8(0xdc, 0xdc, 0xdc, 0xbf), 0.6)),
        ..ScopeStyle::default()
    });
    Editor::new(state, Box::new(EmbeddedRaster::new(&live::FACEPLATE)))
}

/// Generated from `pages.svg`: a page group of three tabs (design/faceplates.md).
pub mod pages {
    include!(concat!(env!("OUT_DIR"), "/pages.rs"));
}

pub const AMOUNT: ParamIndex = ParamIndex(0);
pub const TIME: ParamIndex = ParamIndex(1);
pub const FIRST_ON: ParamIndex = ParamIndex(2);
pub const SECOND_ON: ParamIndex = ParamIndex(3);
pub const THIRD_ON: ParamIndex = ParamIndex(4);

pub static PAGE_PARAMS: [ParamSpec; 5] = [
    ParamSpec::new("Amount", 0.0, 1.0, 0.5).id(1),
    ParamSpec::new("Time", 0.0, 1.0, 0.25).id(2),
    ParamSpec::toggle("First On", true).id(3),
    ParamSpec::toggle("Second On", false).id(4),
    ParamSpec::toggle("Third On", false).id(5),
];

/// The page group's editor: each tab's lamp shows its page's ON switch.
pub fn pages_editor(link: Box<dyn EngineLink>) -> Editor {
    let controls = pages::Controls::<Box<dyn Binding>> {
        fx_0: Box::new(TabButton::new().lamp(FIRST_ON)),
        fx_1: Box::new(TabButton::new().lamp(SECOND_ON)),
        fx_2: Box::new(TabButton::new().lamp(THIRD_ON)),
        amount: Box::new(ParamKnob::new(AMOUNT)),
        first_on: Box::new(ParamToggle::new(FIRST_ON)),
        time: Box::new(ParamFader::new(TIME)),
        second_on: Box::new(ParamToggle::new(SECOND_ON)),
        third_on: Box::new(ParamToggle::new(THIRD_ON)),
    };
    let state = EditorState::new(&pages::LAYOUT, &PAGE_PARAMS, controls.into_array(), link);
    Editor::new(state, Box::new(EmbeddedRaster::new(&pages::FACEPLATE)))
}

/// Generated from `canvas.svg`: an attack/decay curve between its two
/// knobs, a canvas control (design/faceplates.md).
pub mod canvas {
    include!(concat!(env!("OUT_DIR"), "/canvas.rs"));
}

pub const ATTACK: ParamIndex = ParamIndex(0);
pub const DECAY: ParamIndex = ParamIndex(1);

pub static CANVAS_PARAMS: [ParamSpec; 2] = [
    ParamSpec::new("Attack", 0.0, 1.0, 0.25).id(1),
    ParamSpec::new("Decay", 0.0, 1.0, 0.5).id(2),
];

/// The canvas's curve: a rise over ATTACK and a fall over DECAY, drawn
/// from the model, and a marker where the engine says the envelope is
/// (engine value 0, design/engine-bridge.md), as a playing envelope's position
/// would be. Drawn in the editor's theme.
#[derive(Debug, Default)]
pub struct Curve {
    attack: f32,
    decay: f32,
    marker: f32,
    /// The engine values' generation last read.
    seen: Option<u64>,
}

impl silkscreen::EditorWidget for Curve {
    fn update(&mut self, state: &EditorState) -> bool {
        let (values, generation) = state.engine_values();
        let marker = if self.seen == Some(generation) {
            self.marker
        } else {
            self.seen = Some(generation);
            values[0]
        };
        let now = (state.model().get(ATTACK), state.model().get(DECAY), marker);
        let changed = self.seen.is_none() || now != (self.attack, self.decay, self.marker);
        (self.attack, self.decay, self.marker) = now;
        changed
    }

    fn draw(
        &self,
        canvas: &mut silkscreen::core::tiny_skia::PixmapMut<'_>,
        rect: silkscreen::core::Rect,
        transform: silkscreen::core::tiny_skia::Transform,
        _: &silkscreen::core::WidgetState,
    ) {
        use silkscreen::core::tiny_skia::{Paint, PathBuilder, Stroke};
        // Inset, so the stroke stays inside the rect the view repaints.
        let rect =
            silkscreen::core::Rect::new(rect.x + 2.0, rect.y + 2.0, rect.w - 4.0, rect.h - 4.0);
        // The rise takes up to half the width, the fall the rest.
        let x = |t: f32| rect.x + t * rect.w;
        let peak = x(0.5 * self.attack);
        let end = peak + 0.5 * self.decay * rect.w;
        let mut pb = PathBuilder::new();
        pb.move_to(rect.x, rect.bottom());
        pb.line_to(peak, rect.y);
        pb.line_to(end, rect.bottom());
        let mut paint = Paint {
            anti_alias: true,
            ..Paint::default()
        };
        paint.set_color(Color::from_rgba8(0xe2, 0x26, 0x2e, 0xff));
        let stroke = Stroke {
            width: 1.5,
            line_join: silkscreen::core::tiny_skia::LineJoin::Round,
            ..Stroke::default()
        };
        if let Some(path) = pb.finish() {
            canvas.stroke_path(&path, &paint, &stroke, transform, None);
        }
        let mut pb = PathBuilder::new();
        let m = x(self.marker.clamp(0.0, 1.0));
        pb.move_to(m, rect.y);
        pb.line_to(m, rect.bottom());
        paint.set_color(silkscreen::theme::color(
            silkscreen::theme::current().border,
        ));
        if let Some(path) = pb.finish() {
            canvas.stroke_path(&path, &paint, &Stroke::default(), transform, None);
        }
    }
}

/// The canvas editor, the curve's input handled by `curve`.
pub fn canvas_editor(link: Box<dyn EngineLink>, curve: Box<dyn Binding>) -> Editor {
    let controls = canvas::Controls::<Box<dyn Binding>> {
        attack: Box::new(ParamKnob::new(ATTACK)),
        curve,
        decay: Box::new(ParamKnob::new(DECAY)),
    };
    let mut state = EditorState::new(&canvas::LAYOUT, &CANVAS_PARAMS, controls.into_array(), link);
    state.set_widget(canvas::ids::CURVE, |_| Box::new(Curve::default()));
    Editor::new(state, Box::new(EmbeddedRaster::new(&canvas::FACEPLATE)))
}
