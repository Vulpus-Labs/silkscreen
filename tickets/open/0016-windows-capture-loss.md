---
id: "0016"
title: "Native (Windows): end a drag when mouse capture is lost"
priority: low
created: 2026-11-12
epic: none
depends: []
---

## Summary

On Windows baseview captures the mouse on button down. On
`WM_CAPTURECHANGED` it releases capture and sends nothing. If capture is
taken mid-drag (a host popup or modal, Alt-Tab), the release goes to
another window and the drag carries on until the next click on the
plugin. `FocusLost` doesn't save it: a plugin child window rarely has
focus on Windows.

## Acceptance criteria

- [ ] A drag ends when capture is lost: either baseview reports it (an
      upstream change, preferred), or the native host checks the real
      button state on the next `CursorMoved` and releases a press it
      finds over, as the web host does (0014).
- [ ] Hand check on Windows: start a knob drag, Alt-Tab away and back;
      the knob doesn't follow the mouse.
