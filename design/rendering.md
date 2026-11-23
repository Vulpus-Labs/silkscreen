# Rendering, presentation and scaling

silkscreen draws every frame on the CPU with tiny-skia, into a frame
buffer that each host presents in its own way. This document covers how
pixels are produced, how they reach the screen on each target, and how
display scale, zoom and resizing feed into both.

## Why the CPU

A plugin editor is a few hundred thousand to a few million pixels. Most
of them are a static faceplate, and only the controls change between
frames. GPU rendering inside a DAW brings context sharing, driver and
host-specific problems for very little gain at this workload. The same
pixels also have to reach three different presenters: a native window, a
browser canvas and a PNG file. A plain RGBA buffer serves all three.

## The frame buffer

- The compositor (`silkscreen_core::Compositor`) renders into a
  premultiplied RGBA8 tiny-skia pixmap at physical resolution: the
  faceplate's logical size times the effective scale.
  `EditorView::physical_size` gives its dimensions, and `render` asserts
  that the target matches.
- The faceplate is a pre-rendered bitmap from a `RasterSource` (see
  below and [faceplates.md](faceplates.md)), copied in row by row.
- Widgets draw vector paths with tiny-skia, through a transform from
  logical units to the frame's pixels.
- The composited frame is opaque, so premultiplied and straight alpha are
  the same. Presenters can hand the bytes on without converting alpha.

## Damage, not full redraws

`render` repaints only what changed and returns the damaged rects, in
physical pixels, for the host to present. A control is damaged when the
value it displays changes, when its hover, press or focus state changes,
or when an overlay (a tooltip, menu or panel; see
[overlays-and-text.md](overlays-and-text.md)) appears over it, moves or
goes away. A scale change damages the whole frame.

Damage rects are coalesced: a rect already covered by pending damage is
dropped, and pending rects that a new rect covers are removed. For each
remaining rect the compositor:

1. copies the faceplate pixels under the rect, plus a one-pixel margin,
   into a scratch buffer (page groups restore their region from the shown
   page's bitmap instead),
2. draws every widget that intersects it, then the overlays in order,
   then the top overlay (a tooltip),
3. copies only the rect itself, without the margin, back into the frame.

The margin matters. A shape that starts just outside a buffer's edge
anti-aliases that edge's pixels short, so without it a partial repaint
would show a faint seam along the rect's sides. Fills that move with a
value, such as a scrollbar thumb or a table's value bar, sit on whole
pixels so their edges don't depend on the scratch's origin at all.

Even with the margin, tiny-skia's coverage varies slightly with the
scratch's origin, so a rect that cuts through a widget can differ from a
full render by a level or two at anti-aliased edges. That is invisible,
but it means a frame is reproducible bit for bit only for the same
sequence of damage. Tests that compare a partial repaint with a full
render allow a difference of 2 per channel.

An overlay can declare an `opaque` rect it covers completely. Controls
wholly behind it aren't repainted while it shows.

When nothing has changed, `render` touches nothing and returns an empty
slice, and `needs_render` lets a host skip asking. An idle editor costs
close to nothing.

## Presenters

The same frame buffer reaches four presenters.

| Target | How it presents | Conversion |
|---|---|---|
| macOS | A `CALayer` sublayer of the view's layer whose `contents` is a `CGImage` of the frame | None: the RGBA8 bytes are copied into the image as they are (`NoneSkipLast`). The whole frame each present. |
| Windows, Linux | softbuffer | RGBA to `0RGB` `u32`, damaged rects only |
| Web | `putImageData` on a `CanvasRenderingContext2d`, once per damaged rect | None. The `ImageData` wraps a view of the frame in wasm memory, rebuilt when the memory grows or the frame is reallocated. |
| Headless | PNG encode | None |

macOS doesn't use softbuffer. Its Core Graphics backend registers an
Objective-C class with a fixed name. Every plugin binary carries its own
copy of softbuffer, so the second plugin in a DAW process to open an
editor would panic re-registering that class and show no editor.
silkscreen's macOS presenter does what that backend does without the
class: the window code already knows the view's size and scale and
passes them in. softbuffer's other backends register nothing
process-wide, so Windows and Linux keep it.
`examples/tone-plugin/examples/gui_smoke` opens several installed
bundles' editors in one process to catch collisions of this kind.

## Timing

The host supplies time. The CLAP host ticks from the host's GUI timer
(every 16 ms while the editor is open), since a DAW may run its event
loop in a mode where baseview's own timer doesn't fire. The standalone
window ticks from baseview's timer, the web host from
`requestAnimationFrame`, and the harness from its script. Each tick
advances gesture timing, pulls host and engine changes into the model
and queues damage; the following `render` paints it.

## Cost

A large editor at 2× (1600×1000 logical, 3200×2000 physical) is about
25 MB per full frame. Full repaints happen only on a scale change or
resize, so steady-state cost is proportional to damage.

Animated content such as meters and scopes costs CPU in proportion to
its area and repaints on every frame it moves. Measure it against the
engine's own DSP cost. Stroking a trace with tiny-skia and blurring it
for a glow is expensive at 60 fps; drawing straight into the scratch
pixels from each pixel's distance to the trace is an order of magnitude
cheaper.

tiny-skia has no text shaping (silkscreen-text handles text) and no
general blur or filter pass. Static effects belong in the pre-rendered
faceplate. A live effect is drawn analytically: for a glow, a table of
the blurred profile looked up by each pixel's distance to the shape, only
near the shape. If animated content ever outgrows the CPU, a GPU
presenter can sit behind the same frame-buffer seam.

## Effective scale

Scale comes from several places: the display (1× or 2× on macOS,
fractional steps such as 1.25 and 1.75 on Windows), the browser's
`devicePixelRatio`, and the editor size the user chooses. silkscreen
folds them into one number:

```text
effective scale = display scale × zoom
```

All layout is in logical units, multiplied by the effective scale at
render time. Every source of change ends in the same call,
`set_scale(effective_scale)`, which repaints fully.

## Zoom and resizing

Editors keep a fixed aspect ratio. Natively the zoom moves in discrete
steps, `ZOOM_STEPS` in `silkscreen-host-native`: 75%, 100%, 125%, 150%
and 200%.

In a CLAP host:

- `gui.can_resize` returns true, and `get_resize_hints` asks the host to
  preserve the faceplate's aspect ratio.
- `adjust_size` snaps a proposed size to the nearest step, and
  `set_size` applies it.
- A proposed size maps to a zoom through `zoom_for_size`, which takes the
  geometric mean of the two axes' ratios to the faceplate's size and
  snaps it with `nearest_zoom`. That is exact when the host keeps the
  aspect, and still responds when a host drags only one edge.
- CLAP sizes are converted to logical units first: divided by the host's
  scale (from `gui.set_scale`), except on macOS where CLAP sizes are
  already in points and the backing scale comes from the window.
- The zoom belongs to the plugin instance, not the window. It is kept
  while the editor is closed and saved in the plugin state. A zoom
  restored from state while the editor is open is applied only if the
  host accepts `request_resize`.
- baseview may call its `resized` callback synchronously from inside
  `resize` (it does on macOS), so the window code never holds a borrow
  of its state across a `resize` call.

The standalone window is resizable between the smallest and largest
steps and snaps a drag to the nearest step. Its `WindowOptions` can
enable number keys 1 to 5 to select a step (`zoom_keys`), which previews
use; plugins resize through the host instead.

The web host is not limited to steps. `WebHost::set_zoom` takes any zoom,
in CSS pixels per logical pixel, clamped to 0.25–4, and the editor
renders at `devicePixelRatio × zoom` so the faceplate stays sharp. The
page's `silkscreen-fit.mjs` uses it to fit the faceplate to the space it
has; on a device with a coarse pointer (a phone) the faceplate keeps its
own size and the page pans instead. The device pixel ratio is read every
frame rather than watched with a `matchMedia` listener: one property
read catches every change, including emulated ones that fire no media
query events.

## Faceplate pixels at any scale

Faceplate bitmaps come from a `RasterSource`:

```rust
pub trait RasterSource {
    fn size(&self) -> Size;
    fn faceplate(&mut self, scale: f32) -> &Pixmap;
}
```

The implementation silkscreen provides is `EmbeddedRaster`, over
`EmbeddedPngs` that `silkscreen-build` generates. By default the build
renders scales 1, 1.5 and 2, and a plugin can choose others. The PNGs are
either embedded in the binary or, for the web, named by URL and fetched
when first needed.

For a requested scale, `EmbeddedRaster` picks the smallest available
scale at least as large and resamples it (bicubic) if it isn't exact, so
it only ever downsamples. If no scale is large enough it upscales the
largest. Results are cached for the two scales most recently asked for,
so a zoom step back is instant and a resize doesn't keep a bitmap for
every step it passed. While a fetched PNG hasn't arrived, the best one at
hand stands in (or an opaque blank, if none has), and the host repaints
in full when it arrives.

A fractional scale such as 1.25 gets a downsampled 1.5× bitmap and is
slightly softer than an exact render. Widgets are vector-drawn, so they
follow the effective scale on their own. Because the faceplate sits
behind a trait, a source that rasterizes the SVG at the exact scale can
replace `EmbeddedRaster` without any change to layout.
