# Overlays, tooltips and text

Everything drawn above the controls is an overlay: tooltips, drag
readouts, menus, lists, tables, dialogs and the preset browser. Overlays
are drawn by silkscreen into the editor's own window, with runtime text
rasterised by `silkscreen-text`. CLAP has no tooltip mechanism, native OS
tooltips differ between platforms, and none exist on a web canvas, so
drawing them in-window is the only way to get the same behaviour on every
host.

## Overlays in the compositor

`silkscreen_core::Overlay` is a logical `rect` and a `draw(canvas,
transform)`. Everything an overlay draws must stay inside its rect, so
showing, changing or hiding it repaints exactly what it covers. Overlays
take part in damage tracking like widgets (see [rendering.md](rendering.md)).

The `Compositor` paints, in order: the faceplate, the widgets, the
overlays (bottom to top), then one **top** overlay, which is where the
tooltip goes. The stack is changed with `push_overlay`, `pop_overlay` and
`set_overlays`, and the top with `set_top`; each damages only what the old
and new overlays cover. An overlay may report an `opaque` rect it paints
over completely; controls wholly behind it aren't repainted while it
shows.

## Tooltips and drag readouts

The gesture recogniser reports `HoverDwell` when the pointer rests on a
control for `GestureConfig::dwell_ms` (500 ms by default). The view then
shows a tooltip. Movement, a press, a scroll or leaving the control hides
it.

From `DragStart` to `DragEnd` the same label becomes a live value readout
for the dragged control, updating as the value moves. A press or click
alone shows nothing, so a click doesn't flash a label.

The text comes from `EditorState::describe(id)`: the binding's own
`describe`, if it gives one, else the name of its first param and that
param's formatted value, e.g. `Cutoff  0.50`. A control bound to no param
that describes nothing gets no tip. The tip is placed below the control,
or above it when there is no room, centred on it and kept 2 units inside
the window. It is laid out again when the scale changes, and no tip shows
while a panel is open.

## Panels

A `Panel` is an overlay that takes input: a menu, a drop-down list, a
table, a dialog, the preset browser. The view keeps a stack of open
panels, drawn through the compositor's overlay layer, above the controls
and below any tooltip.

- `EditorView::open_panel` pushes one; a binding can also ask for one,
  and `EditorState::set_panel(control, make)` says what it opens.
- While a panel is open, pointer events inside it go to the top panel's
  `input`; a press outside closes it and goes no further; Esc closes it.
  The controls beneath lose hover and any drag when the first panel opens.
- `input` returns a `Reply`: ignored, handled (repaint the panel), repaint
  only some rects, close, open another panel above, or replace this one.
- `update` catches the panel up with the model (host automation moving a
  value it shows), and `poll` hears news from outside its input each tick,
  such as a host prompt's answer.
- `takes_text` says the panel wants every key. In a plugin, keys go to the
  host unless the top panel takes text, so a DAW's shortcuts keep working
  while the editor is open.

silkscreen's own panels include `List`, `Table`, the preset browser
(`PresetBrowser`) and the dialogs in `silkscreen::dialog`: `Confirm`,
`SaveAs`, and `Unlock` (see [licensing-hook.md](licensing-hook.md)).
Presets and their dialogs are in [presets.md](presets.md).

### Typing names through the host

A DAW takes some keys (Space, above all) before a plugin's view sees them,
so a name field drawn on the faceplate can't always be typed into. A host
that knows this gives the editor a `TextPrompt`: name fields then ask it
for their text, and it shows a small window of its own with the keyboard
to itself. The answer comes back through a `Prompt` and its
`PromptReply`. Hosts without one (the web, the standalone app) type into
the field directly. See [hosts.md](hosts.md).

## Theme

Overlays draw with the editor's `Theme`, a set of colours (background,
border, ink, dimmed ink, hover, selection, alert, scroll bar). The default
is dark with a red selection; `EditorState::set_theme` replaces it. Sizes
are not part of the theme, because they set row heights and so where
input lands: `theme::TEXT_SIZE` (11 logical units) and `theme::RADIUS`
(3) are constants. A plugin's own panels and editor widgets match the
built-in ones by drawing with `theme::current()` and these constants.
When the theme's background is opaque, panels report their bodies as
opaque and controls behind them stop repainting.

## Mouse cursor

The pointer shows what a press would do. `MouseCursor` is a short list:
`Default` (arrow), `Hand`, `Grabbing`, `Vertical`, `Horizontal`, `Text`.
`EditorView::cursor` works it out after each input event: with a panel
open, the top panel's `Panel::cursor` (the arrow off it); otherwise the
dragged control's, then the hovered one's. A control's cursor comes from
its kind (vertical drag for knobs, faders and selectors, a hand for what
is clicked) unless its binding's `Binding::cursor` overrides it, as a
canvas does over its handles. Hosts map it to the platform's cursor. See
[input.md](input.md).

## Runtime text: `silkscreen-text`

`silkscreen-core` has no fonts. Runtime text lives in its own crate,
`silkscreen-text`, which builds for `wasm32-unknown-unknown`.

- **Face.** `Face::new(bytes, id)` parses a TrueType or OpenType font
  embedded in the binary; fontdue rasterises it. The built-in UI uses
  `silkscreen_text::JOST_MEDIUM`, Jost at weight 500 (SIL OFL 1.1, in the
  crate's `fonts/` with the OFL text), a geometric sans. A plugin's own
  widgets and panels may embed and draw any other face; give each face a
  distinct `id`.
- **Cache.** `Text` rasterises each glyph once per face and physical size
  (sizes keyed in quarter pixels) and keeps it. Keep one per view; the
  built-in overlays share one per UI thread.
- **Layout.** Strings are laid out with the face's kerning in physical
  pixels: each glyph lands on a whole pixel and the baseline on a whole
  row, so text is crisp at every zoom step. `Text::width` measures the
  same way, so a width depends slightly on the scale.
- **Drawing.** `Text::draw(canvas, text, origin, style, transform)` blends
  coverage into the canvas through the widgets' logical-to-physical
  transform, clipped to the canvas, so it composes with damage like any
  widget. `draw_clipped` also clips to a logical rect, for a scrolling
  list. `Style` is the face, size in logical units and colour;
  `Face::line_metrics` gives ascent and descent.
- **Missing glyphs** draw as the face's notdef box. Jost covers Latin and
  Cyrillic but not arrows or most symbols: UI strings avoid characters the
  face lacks, or draw them as shapes. There is no fallback face.

Text is rasterised at run time rather than from pre-built glyph sheets
because much of it isn't known at build time (preset names typed by the
user, names from data in menus), and sheets would need a copy per size and
per zoom step. Run-time rasterising is cheap: a cold glyph takes about
1.5 µs in a release build, and a cached one a fraction of a microsecond.
The cost is the font in the binary, about 100 KB per weight.

Faceplate legends are not runtime text: they are part of the SVG, drawn at
build time. `silkscreen_build::TEXT_FONT_DIR` points at the same Jost
file so faceplate lettering can match the overlays (see
[faceplates.md](faceplates.md)).
