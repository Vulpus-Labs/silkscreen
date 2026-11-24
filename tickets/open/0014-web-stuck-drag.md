---
id: "0014"
title: "Web: a drag always ends when the mouse is released"
priority: high
created: 2026-11-12
epic: none
depends: []
---

## Summary

On web pages a drag sometimes carried on after the mouse button was
released, seen after dragging over an on-screen keyboard. The editor
heard a release only from a `pointerup` naming the button it had pressed.
It missed every other way a press ends: `lostpointercapture` wasn't
listened to; `pointercancel` carries `button == -1`, which the recognizer
ignores (a cancelled touch drag stuck too); after a chord (left down,
right down, left up, right up) the only `pointerup` names the right
button; and an event dropped while the host was borrowed was never
recovered.

## Acceptance criteria

- [x] `Translator` remembers the button reported pressed and reports
      that button released on `pointerup`, `pointercancel` or
      `lostpointercapture`, once.
- [x] A `pointermove` with `buttons == 0` while a press is held reports
      the missing release before the move.
- [x] A `pointerdown` while a press is held releases it first.
- [x] Unit tests for each case in `translate.rs`.
- [ ] `design/input.md` (pointer capture) or `design/hosts.md` (web)
      lists the ways a press ends on the web.
- [ ] Hand check in Chrome, Safari and Firefox: drag a knob over an
      on-screen keyboard and release there; release outside the window;
      chord left and right; the knob stops following the mouse each time.
