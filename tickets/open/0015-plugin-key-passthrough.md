---
id: "0015"
title: "Plugin: pass keys to the DAW unless typing"
priority: high
created: 2026-11-12
epic: none
depends: []
---

## Summary

On macOS the plugin view becomes first responder when it opens and the
native host returned `Captured` for every key, so the DAW's shortcuts
(space for transport, above all) did nothing while the editor was open.
The editor's only key use in a plugin is typing: the preset browser's
search and name fields. Every other key should go to the DAW.

## Acceptance criteria

- [x] `Panel::takes_text` (default false); `TextField`, `PresetBrowser`
      and `SaveAs` say true. `EditorView::takes_text` / `Editor::takes_text`
      report the top panel's.
- [x] `WindowOptions::in_host` (default false; the CLAP GUI sets it).
      Then a key is `Ignored`, and not delivered, unless the editor takes
      text. With 0017 only the preset browser (its search) takes text in
      a plugin; name fields use the host's prompt.
- [x] Test: the faceplate and a menu don't take text; the browser and a
      name field do.
- [x] Shift-drag is unaffected: native modifiers come with mouse events.
- [x] `design/input.md` describes it.
- [ ] Hand check, macOS, in two DAWs: with the editor open and focused,
      space starts and stops the transport; typing in the browser's
      search doesn't; Shift-drag still fine-adjusts.

## Notes

On Windows baseview's keyboard hook consumes every key addressed to the
plugin window whatever the handler returns. Nothing gives that window
focus on click, so in practice keys stay with the host there. If a host
does focus it, keys would be trapped; fixing that is baseview's job.
