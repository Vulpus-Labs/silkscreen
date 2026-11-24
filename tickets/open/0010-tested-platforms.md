---
id: "0010"
title: "Record which DAWs and operating systems are tested"
priority: medium
created: 2026-11-24
epic: E001
depends: []
---

## Summary

Outside users will ask which hosts and systems silkscreen works on. Keep
a dated record of what has been tested, per target, and show it in the
README.

## Acceptance criteria

- [ ] `design/` (or `guide/`) holds a table: target (CLAP, VST3, web,
      standalone), OS and version, host or browser and version, date,
      silkscreen version, result and known issues.
- [ ] Filled from hand checks already done (a DAW on macOS ARM, the
      browsers the web page has been tried in) and marked untested where
      not (Windows and Linux DAWs so far).
- [ ] The README's "Tested on" section (0001) summarises it.
- [ ] The release checklist (0006) asks for it to be updated.
