---
id: "0005"
title: "User guide: cookbook for custom bindings and canvas controls"
priority: medium
created: 2026-11-24
epic: E001
depends: [0003]
---

## Summary

Beyond knobs bound to parameters, real plugins need custom bindings
(one control driving several params, derived readouts, mode-dependent
dimming) and canvas controls (envelopes, scopes, editable graphs). Write
short recipes for these.

## Acceptance criteria

- [ ] Recipes, each a working example under test: a custom `Binding`; a
      read-only readout; a control that dims or hides on another param; a
      canvas control that draws engine telemetry (a scope or meter); a
      canvas control edited by dragging (an envelope); presets and a
      factory bank; a dialog or overlay.
- [ ] Each recipe states which `design/` document explains the mechanism.
- [ ] Linked from the guide's index.

## Notes

The `widgets` example already covers much of this; recipes can lift from
it but should be small enough to read in one sitting.
