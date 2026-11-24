---
id: "0007"
title: "MIDI input in the standalone app"
priority: high
created: 2026-11-24
epic: E001
depends: []
---

## Summary

The standalone app "takes no notes yet" (template main.rs), so an
instrument can't be played standalone, which undercuts that target.

## Acceptance criteria

- [ ] The standalone host opens MIDI inputs (midir or equivalent) and
      sends note on/off, velocity, pitch bend, sustain and CCs to the
      engine as the CLAP host does, sample-accurately enough not to flam.
- [ ] Input device choice in the app menu, remembered between runs; all
      inputs by default; devices plugged in while running appear.
- [ ] No allocation or locking on the audio thread; events cross on the
      existing lock-free path.
- [ ] Optional computer-keyboard notes when no MIDI device is present.
- [ ] Template and `tone` example play standalone; the template's main.rs
      comment goes.
- [ ] Hand check on macOS with a hardware keyboard; Windows and Linux
      noted if tested (0010).
