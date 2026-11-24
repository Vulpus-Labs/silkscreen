---
id: "0017"
title: "Plugin: type names in a prompt window, out of the DAW's reach"
priority: high
created: 2026-11-12
epic: none
depends: ["0015"]
---

## Summary

A DAW takes some keys before a plugin's view sees them (some take Space
for the transport), so a preset or folder name with a space can't be
typed into a field on the faceplate. silkscreen gets round this through a
host service: name fields ask a `TextPrompt` for their text when the host
gives one, and the host opens a small window of its own that has the
keyboard. Search stays on the faceplate: it needs no Space.

## Acceptance criteria

- [x] `silkscreen::prompt`: `TextPrompt`, `Prompt` (dropping it unanswered
      closes the host's window), `PromptReply`, and `ScriptedPrompt` for
      tests. `EditorState::set_text_prompt`.
- [x] `Panel::poll`, called on the top panel each tick, its reply applied
      as input replies are.
- [x] `TextField` with a host prompt asks it, takes no keys, commits
      what it answers, asks again when the commit keeps it open (a taken
      name), and closes on a cancel. Titles: "Preset Name",
      "Folder Name", "Save Preset As".
- [x] `SaveAs` asks at once; the prompt's Enter saves; a cancel leaves
      the dialog (folder still choosable), and a click on the name asks
      again.
- [x] Tests: rename, taken name, cancel, closing dismisses the prompt;
      Save As.
- [x] macOS: an `NSAlert` sheet with a text field on the DAW's window,
      with a completion handler. No Objective-C class declared.
- [x] Windows: an owned topmost popup with an `EDIT` control, its class
      name unique per DLL. Type-checked only.
- [x] `WindowOptions::in_host` installs the prompt; the CLAP GUI sets it.
- [x] `design/input.md` and `design/overlays-and-text.md` describe it.
- [ ] Hand check, macOS, in three DAWs: rename a preset to a name with a
      space; Save As a new name and Enter; Esc cancels; Space still
      drives the transport afterwards.
- [ ] Hand check, two silkscreen plugins open in one DAW project: both
      prompt.
- [ ] Hand check, Windows, in a DAW: the popup takes the name, Enter
      and Esc work, and a click elsewhere cancels.
