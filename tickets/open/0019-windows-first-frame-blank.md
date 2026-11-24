---
id: "0019"
title: "Native (Windows): the faceplate is blank until hovered"
priority: high
created: 2026-11-16
epic: none
depends: []
---

## Summary

Seen in a CLAP plugin on Windows (2026-11-16): the editor opens blank,
and controls appear only as the mouse passes over them.

`EditorWindow::new` presents the first frame before the window is
shown. On Windows that blit never reaches the screen, and it cleared
`full`, so later presents sent only the render's damage
(`present_with_damage`). The `WM_PAINT` that follows the show reaches
baseview's `WindowHandler::damage`, which the native host ignored.
macOS is unaffected: the `CALayer` keeps its image.

Fix: `damage` marks the next present `Repaint::Exposed`. The soft
presenter then converts only the render's damage but sends the whole
softbuffer buffer (which keeps the last frame) to the window. Windows
reports the whole window with every redraw, ours included, so this is a
full-window blit per frame there; the conversion stays damage-only.

## Acceptance criteria

- [x] `WindowHandler::damage` forces the next present to send the whole
      buffer; the soft presenter converts only the damage.
- [x] Clippy for `x86_64-pc-windows-msvc`; host-native tests.
- [ ] Hand check on Windows: the template's and `widgets`' plugins open
      fully drawn; reopening the editor and uncovering it after a host
      dialog draws it fully too.
- [ ] Hand check on Windows: resizing redraws fully at each zoom step
      (dragging the host's window edge, the zoom keys), and so does
      moving the window to a monitor with other scaling; no unpainted
      strip along the right or bottom edge.
- [ ] Linux: the same open and uncover check (X11 `Expose` takes the
      same path).
