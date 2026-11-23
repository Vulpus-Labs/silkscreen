# Faceplates, canvas controls and pages

A plugin's faceplate is an SVG. At build time `silkscreen-build` renders
it to PNGs at several scales and generates typed Rust layout code from the
controls marked in it. At run time the faceplate is a static bitmap the
compositor restores under the widgets (see [rendering.md](rendering.md)).
Controls live in the same drawing as their printed legends, so a knob
can't drift away from its label, and a control with no binding fails to
compile.

## The SVG

- The root's `width`/`height` are the **logical size**. A `viewBox` that
  differs is scaled to fit, and control rects use the same scaled space.
- The rendered faceplate must be **fully opaque** (in practice, a
  background rect covering the canvas). The compositor relies on it; a
  translucent pixel is a build error.
- One group is the **controls layer**: `id="controls"`, or an Inkscape
  layer with `inkscape:label="controls"`. It is cut out before rendering.
- Each child of that layer is a **marker** for one control. Any shape
  works; its absolute bounding box, all transforms applied, is the
  control's rect. Its `id` is `kind.name` (`knob.cutoff`, `toggle.sync`),
  `name` a snake_case Rust identifier. The kinds are `knob`, `fader`,
  `selector`, `toggle`, `button`, `dropdown`, `preset`, `action`, `scope`,
  `meter` and `canvas`.
- Radio buttons are `radio.<group>.<position>` and tabs
  `tab.<group>.<position>`; each generates a control named
  `<group>_<position>`. A group's positions must be exactly `0..n`, or the
  build fails naming the group and the missing position.
- The SVG may place raster images (`<image href>`, resolved against its
  directory) and blend them with `mix-blend-mode`. Cargo is told to rerun
  only on the SVG, font directories and skin frames, so a crate that links
  images must print `cargo::rerun-if-changed` for them itself.

What each kind does and how bindings drive it is in [input.md](input.md)
and [model.md](model.md).

## Building with `silkscreen_build::Faceplate`

Call it from `build.rs`, once per SVG:

```rust
fn main() {
    silkscreen_build::Faceplate::new("synth.svg")
        // Letter the faceplate in the editor's runtime face.
        .font_dir(silkscreen_build::TEXT_FONT_DIR)
        .skin("knob_red", &["skins/knob_base.svg", "skins/knob_top.svg"])
        .skin_sweep("knob_red", 300.0)
        .build();
}
```

| Method | Effect |
| --- | --- |
| `scales(&[f32])` | Scales to pre-render. Default `1.0, 1.5, 2.0`. |
| `font_dir(dir)` | A directory of `.ttf`/`.otf`/`.ttc` fonts for `<text>`. Repeatable. |
| `system_fonts(bool)` | Also load system fonts. Off by default. |
| `skin`, `skin_sweep`, `skin_cycle` | Register control sprites (see [Skins](#skins)). |
| `core_path(path)` | How generated code names core types. Default `::silkscreen::core`. |
| `fetch(prefix)` | Name PNGs by URL instead of embedding them, for the web host. |
| `out_dir(dir)` | Write somewhere other than `$OUT_DIR`. |
| `build()` / `generate()` | Generate, panicking with the message / returning `Result<Generated, BuildError>`. |
| `analyze()` / `analyze_str(text)` | Validate without writing anything. |

The build finds the controls layer with roxmltree, reads each marker's
bounding box from a usvg parse of the whole document, and renders the rest
with resvg at each scale. With `stem` the SVG's file stem (so a crate can
hold several faceplates), it writes `{stem}@{scale}x.png`,
`{stem}.page.{group}.{n}@{scale}x.png` per page,
`{stem}.skin.{name}.{i}@{scale}x.png` per skin frame, the code as
`{stem}.rs`, and in fetch mode a manifest, `{stem}.assets`. Fetch mode is
on by default in a build script when the crate's `fetch` feature is, with
the prefix `assets/<crate name>`; see [hosts.md](hosts.md) and
[building-and-shipping.md](building-and-shipping.md).

## Generated code

```rust
pub mod faceplate {
    include!(concat!(env!("OUT_DIR"), "/synth.rs"));
}
```

The module holds:

- `SIZE: Size`, the logical size, and `mod ids` with one
  `pub const NAME: ControlId` per control (in a module so a control called
  `size` can't collide with `SIZE`); `mod radios` lists each radio group's
  ids in order.
- `CONTROLS: [ControlSpec; N]` (id, name, kind, rect, skin, page) and
  `LAYOUT: Layout`.
- `FACEPLATE: EmbeddedPngs`, the PNGs per scale (or their URLs). It is
  immutable; `EmbeddedRaster` decodes it, and resamples the best scale for
  zooms that weren't pre-rendered.
- `mod skins`, one `Skin` static per skin in use; for page groups,
  `PAGES`, `mod page_groups` and `mod tabs`.
- `Controls<B>`, one field per control and `into_array() -> [B; N]` in id
  order. Every field must be filled, so an unbound control won't compile.

```rust
let state = EditorState::new(&faceplate::LAYOUT, &PARAMS, controls.into_array(), link);
let editor = Editor::new(state, Box::new(EmbeddedRaster::new(&faceplate::FACEPLATE)));
```

`examples/gain` is the smallest complete case; `examples/widgets` adds a
canvas control (`canvas.svg`) and a page group (`pages.svg`).

## Errors

Failures are messages naming the file, the element (with its line) and
the fix; they are the main feedback loop when a person or a language model
edits the SVG. `BuildError` covers a controls layer that is missing,
duplicated or hidden (`display:none`, `visibility:hidden`, or a hidden
ancestor); a marker without an id, with a malformed id, an unknown kind or
an invalid name; duplicate names; a marker with no geometry or a zero-size
box; a missing font; a faceplate that isn't opaque; radio or page groups
with gaps; and the skin errors below.

## Fonts

Faceplate text is drawn by resvg at build time. For output that is the
same on every machine, fonts come only from `font_dir` directories; system
fonts are opt-in. A `<text>` element whose font can't be found is a build
error, not a silent fallback.

`silkscreen_build::TEXT_FONT_DIR` is the folder holding the editor's
runtime face, Jost Medium. Pass it to `font_dir` and write
`font-family="'Jost*'" font-weight="500"` (the bundled file's family name
is `Jost*`) to letter the faceplate in the face tooltips and readouts use
(see [overlays-and-text.md](overlays-and-text.md)). Faceplate legends
never need runtime text.

## The render cache

A build script reruns whenever any build dependency changes; one that
needs the plugin's engine for its param table would otherwise re-render
every PNG on each engine edit. So each PNG's key is written beside it as
`name.png.key`, and when it matches nothing is parsed, rendered or
encoded. The key covers:

- the renderer: `silkscreen-build`'s source and the `Cargo.lock` of the
  workspace being built, so the resvg, tiny-skia and png versions in use
  count;
- every file in each font directory, and whether system fonts are on;
- the source's bytes and every local file it links with `href`;
- the output's size and scale.

The lockfile is found when the build script runs, from its
`CARGO_MANIFEST_DIR` upwards, so the key follows the consumer's resolved
versions however silkscreen was fetched. `SILKSCREEN_BUILD_LOCK`, if set,
stands in for it. With neither, nothing is cached. Setting `opt-level = 3`
under `[profile.dev.build-override]` and `[profile.release.build-override]`
keeps uncached renders fast.

## Skins

Animated control art comes from the same pipeline as skins. `build.rs`
registers each with `Faceplate::skin(name, &frames)`; a marker selects one
with `data-skin="name"`. Controls without a skin draw their kind's vector
widget.

Each frame renders at the size of the controls using it, viewBox
stretched to fill, at every scale; frames may be transparent. A frame may
also be a **PNG** (a 3D render, say), resampled to the same sizes: halved
by 2×2 means until within 2× of the target, then bicubic. Render PNG
frames at least as large as their largest use. Only the frames are build
inputs, not the tool that made them.

| Kind | Frames |
| --- | --- |
| knob | 2: a static base, then a top rotated across the sweep (`skin_sweep`, default 270°, centred on the top's drawn orientation). 3+: a filmstrip. Cycled: 2+. |
| toggle | 2: off, on. 3: a three-way switch (down, middle, up) on a three-position stepped param. |
| button, radio | 2: off, on. |
| tab | 2: unselected, selected. 4: unselected unlit, unselected lit, selected unlit, selected lit. |
| fader | 1: the cap over the vector slot. 3: empty track, filled track, cap. 4: those and a dark cap, shown when dimmed. |
| selector | 1: the cap. 2: a track with its detents, then the cap. |

A fader's or selector's cap renders at the cap's size and is drawn where
the value puts it (`FaderGeometry::cap`); a filled track shows from the
cap's centre down.

A filmstrip knob shows frame `round(value × (n − 1))`, keeping baked
lighting still at the cost of many frames. A **cycled** knob
(`skin_cycle(name, period_deg)`) is usually better, since most knobs
repeat with their knurl. Frame 0 is the pointer alone, drawn at 12
o'clock and rotated exactly; frames 1.. are the body turned evenly across
one period, shown by angle modulo the period. Keep the period coarse: a
drag moves a knob a few degrees per redraw, and a finer pattern aliases
like a wagon wheel. 15° or more reads as turning with the pointer.

At run time `SkinRaster` decodes and scales frames per view. Skin build
errors cover an unregistered skin, the wrong number of frames, a skin on a
kind that takes none, a skin used at two sizes, and a name that isn't an
identifier.

## Vector drawing and canvas controls

Runtime vector drawing uses tiny-skia (`silkscreen::core::tiny_skia`) into
the compositor's scratch pixmap, through a `Transform` that already holds
the scale and damage offset, so widgets draw in logical units. It covers
anti-aliased fills and strokes, caps, joins, dashes, hairlines, curves and
affine transforms. Text comes from `silkscreen-text`.

A **canvas** control, `canvas.<name>`, is painted by the plugin, which
calls `EditorState::set_widget(control, make)`; `make` builds an
`EditorWidget` per view. Any control may be given one. A canvas with none
panics when the view is built, naming the control and the fix. Its binding
is any binding, or `Unbound` for a display. An `EditorWidget` has:

- `update(&EditorState) -> bool`, called on every sync: copy what `draw`
  needs and return true to repaint. This is the one damage channel for
  display-only content; the built-in scope, meters and preset field use it.
  It can read the model, `pointer()`, `now_ms()`, telemetry and
  `engine_values()` (each with a generation to compare; see
  [engine-bridge.md](engine-bridge.md)).
- `draw(canvas, rect, transform, state)`, as a core `Widget` draws, inside
  `rect` only. `silkscreen::theme::current()` gives the editor's theme.

Input reaches a canvas through its binding: drags carry the pointer's
position, and `ControlEvent::Hover { pos }` follows moves over a hovered
control. The binding decides which handle a press takes, in logical
coordinates, and may edit several params. Tooltips follow the control,
not a handle. Live displays damage their whole rect each tick while they
move, so reduce waveform or spectrum data to about one point per pixel
column.

### Dimmed controls

`BindingExt::dimmed_when(inputs, predicate)` fades a control while a
predicate over the `Model` holds, for a control that does nothing in the
current patch. `inputs`, the params it reads, join the binding's params so
the control repaints when they change. The compositor keeps what lay under
the control, lets the widget draw, and blends back toward it, leaving
`DIM_OPACITY` (0.4) of the widget, so every widget dims alike. A widget
whose `Widget::draws_dimmed` is true is drawn at full opacity and shows
`WidgetState::dimmed` itself. Faceplate labels stay bright, and a dimmed
control still takes input.

### LCD mode

`silkscreen_core::lcd` draws low-resolution LCDs: dots lit or unlit behind
glass, the same dots at every zoom. A widget draws into an `LcdBitmap` in
dots (`line`, `fill_rect`, `stroke_rect`, `text`), then calls
`LcdBitmap::draw`, which bypasses tiny-skia's anti-aliasing, writes opaque
pixels inside its rect and allocates nothing.

Each dot is a tile of whole device pixels with sharp edges. Where
`pitch × scale` isn't whole, dot `i` starts `round(i × pitch × scale)`
pixels in, so tiles alternate a pixel wider or narrower and the grid fills
its rect at any zoom. The pitch shrinks if the rect is too small, and the
grid is centred. Hit testing uses `LcdBitmap::dot_at`; a drag divides by
`pitch_at` to move in dots. `LcdStyle` holds the pitch, the gap (a share
of the pitch), glass, unlit and lit colours, a `halo` for a backlit
panel's bleed and a `sheen` on the glass.

LCD text uses an `LcdFont` generated by `silkscreen_build::BitmapFont`:

```rust
silkscreen_build::BitmapFont::new(silkscreen_build::LCD_FONT)
    .require("0123456789.-+ HzdBms")
    .build();
```

`LCD_FONT` is silkscreen's own 5×7 face (printable ASCII and the degree
sign). A source has a `size WIDTH HEIGHT ADVANCE` line, then a glyph per
line: the character (`space` for a space) and `HEIGHT` rows of `.` and
`#`; `//` starts a comment. The output, `{stem}.rs`, holds
`pub static FONT: LcdFont`. `require` fails the build naming any character
the face lacks; require every character runtime readouts can produce,
since at run time a missing one draws as a box.

## Tabbed pages

A page group is a region showing one of several pages of art and
controls, picked by tabs.

- A page's art is `<g id="page.<group>.<n>">` outside the controls layer.
  The group's region is the union of its pages' art bounds.
- A page's markers sit in the controls layer inside
  `<g data-page="<group>.<n>">` and must lie inside the region.
- Each page has a tab marker `tab.<group>.<n>`. Tabs and pages must both
  be `0..n`, or the build fails naming the group and what is missing.
- Everything outside pages is the base faceplate, always shown.

The base renders with all page art removed, and each page as the whole
faceplate with only its own art. Pages that render byte for byte alike
share one raster. At run time the compositor crops the shown page to the
region at the current scale, so it lines up exactly with the base.
Controls on pages carry `.on_page(group, n)` in `CONTROLS`.

- `EditorState::show_page` and `shown_pages` hold each group's page. It is
  view state, never automated, undone or saved in a preset; the engine
  hears it as `EngineEvent::Shown`, for telemetry that follows the page.
- Only controls on the base or a shown page draw, and
  `Layout::hit_test_shown` skips the rest, so hidden controls take no
  input, hover or tooltip. Their params keep running and still need
  bindings.
- `TabButton` shows its page on a press; `.lamp(param)` lights the tab's
  lamp while a toggle param is on, so it follows automation on a hidden
  page.
- The shown pages are saved with the project as UI state
  (`EditorState::ui_state`, keys `page.<group>`) and restored on load.
