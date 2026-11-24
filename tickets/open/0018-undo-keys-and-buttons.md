---
id: "0018"
title: "Undo and redo buttons; undo keys only where the editor owns the keyboard"
priority: medium
created: 2026-11-12
epic: none
depends: ["0015"]
---

## Summary

The model's undo history (`design/model.md`, Undo) is complete and
tested: one step per gesture, grouped steps for preset loads, Init,
Random and multi-param edits, 100 deep. Little reaches it. The hosts
translate Cmd-Z to `Key::Char('z')` but `Editor` ignores that key. The
CLAP GUI never calls `Editor::undo`. Only the `gain` example's standalone
app has an Edit menu.

**In a plugin, undo and redo are reached only through buttons.** The
editor never traps Cmd-Z or Ctrl-Z there, because that would break the
key policy of 0015 (`design/input.md`): in a plugin, keys belong to the
DAW unless text is being typed. Cmd-Z in a DAW is the DAW's undo. The DAW
sees our gesture brackets, so its own undo can revert parameter changes.

This ticket adds:

- **Undo and Redo button actions** a faceplate can place. They work in
  every host.
- **Keys only in hosts that own the keyboard:** the web page and the
  standalone app. The binding lives in those hosts, not in `Editor`, so
  a plugin has no code path that acts on an undo key.

## Acceptance criteria

### Plugins: no undo keys

- [ ] Neither `Editor` nor the CLAP GUI binds any undo or redo key. In
      a plugin, Cmd-Z and Ctrl-Z behave exactly as they do after 0015:
      passed to the DAW when nothing takes text, and kept by a text
      field when one does. Either way, the editor's history is
      untouched.
- [ ] Test at the native window's key handler with plugin options
      (`all_keys` false): Cmd-Z leaves the editor's history unchanged in
      both cases.

### Keys in the web and standalone hosts

- [ ] Web host: when no panel takes text, Cmd-Z undoes and Shift-Cmd-Z
      redoes on macOS (`is_mac`). Elsewhere, Ctrl-Z undoes and Ctrl-Y
      or Shift-Ctrl-Z redoes. The host calls `Editor::undo` and
      `Editor::redo`, and suppresses the browser's default only for the
      keys it handles.
- [ ] Standalone: an Edit menu (Undo ⌘Z, Redo ⇧⌘Z) is the only key path,
      for every app, not just `gain`'s. One Cmd-Z is one undo step.
- [ ] While a text field or the browser's search takes text, these keys
      go to the field and the history is untouched.

### Buttons

- [ ] `PresetAction::Undo` and `PresetAction::Redo`, with
      `PresetCommand::undo()` and `::redo()` constructors, are bound to
      `action.<name>` push areas like Save and Init.
- [ ] A button draws disabled when there's nothing to undo or redo
      (`Model::can_undo` / `can_redo`) and does nothing when clicked in
      that state. It also does nothing while a gesture is open.
- [ ] Each button's tooltip says "Undo" or "Redo", plus the shortcut on
      hosts that take keys. Plugins show no shortcut.
- [ ] The `widgets` example places Undo and Redo next to its preset
      controls; snapshots re-blessed and the diffs inspected.

### Host changes retire stale history

Our history and the DAW's can't be joined: VST3 has no undo API, and
VST3 builds wrap the CLAP build. The two histories therefore have to
tolerate each other. When the DAW reverts a knob move, the change reaches
us as a host change, but our history still holds that move. Without this
fix, our Undo then sets the knob to the value it already has, and the
click seems to do nothing.

- [ ] A host change (`Origin::Host`) that actually changes param `p`
      removes `p`'s entries from every undo and redo step. A step left
      empty is dropped. A grouped step (a preset load, Init, Random)
      keeps its other params, so undoing it restores them and leaves
      `p` where the host put it.
- [ ] A host write of the current value changes nothing (`Edit::set`
      already logs nothing then). An open user gesture on `p` is
      unaffected, as today.
- [ ] Under automation playback, `p`'s history is retired continually.
      That's intended, because the host owns an automated param.
- [ ] Model tests in `silkscreen-model/tests/undo.rs`:
      - drag, then a host change to the same param: nothing to undo;
      - a host change to another param leaves the drag undoable;
      - preset load, then a host change to one param: undo restores the
        rest;
      - same for the redo stack;
      - a host write of the current value keeps the history.

### Tests and docs

- [ ] Editor tests (`silkscreen-harness`): `Editor::undo` and `redo`
      revert and re-apply a knob drag. A call made mid-drag is refused.
      A preset load undoes as one step. A key event alone (Cmd-Z, sent
      to `Editor`) changes nothing.
- [ ] Web host tests: the key-to-command mapping for Mac and non-Mac.
      The undo and redo keys lose their default only when no panel is
      taking text.
- [ ] Button tests: disabled state follows `can_undo` and `can_redo`,
      and a click undoes and redoes.
- [ ] `design/model.md` (Undo): host changes retire history.
      `design/input.md`: a plugin binds no undo or redo keys, and undo
      there is through buttons only; the web and standalone hosts bind
      them. `design/presets.md`: Undo and Redo are preset-bar actions.
- [ ] CHANGELOG entry.
- [ ] Hand check:
  - Web page in Chrome, Safari and Firefox: undo and redo a knob move
    and a preset load from the keyboard and from the buttons. The page
    doesn't scroll or navigate.
  - Plugin in two DAWs: the buttons undo and redo. To check that Cmd-Z
    is the DAW's alone, move a knob, then make a DAW edit (move an
    item), then press Cmd-Z. Only the DAW edit is reverted, and the knob
    stays where it is.

## Notes

- An undo replayed inside a begin/end bracket reaches the host as an
  automation gesture (`design/model.md`). The DAW may therefore record an
  undo made with our button as a new edit on its own stack. That's
  expected.
- Out of scope: CLAP's draft `clap.undo` extension, which would put one
  shared history in the DAW. CLAP is the route to VST3, and VST3 has
  nothing like it.
- Out of scope: labelling steps (for example "Undo CUTOFF"). `UndoEntry`
  holds a param index, but grouped steps carry no name.
