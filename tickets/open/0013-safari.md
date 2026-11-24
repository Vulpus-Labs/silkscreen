---
id: "0013"
title: "Safari: find what breaks, then support it or state why not"
priority: low
created: 2026-11-24
epic: E001
depends: []
---

## Summary

The web host's splash warns Safari off (macOS Safari, and every iOS
browser, being WebKit), from earlier findings: GC crackle in the
worklet, no Web MIDI. Outside users will ask. Re-test on current
Safari and decide.

## Acceptance criteria

- [ ] Test the `widgets` and template pages on current macOS Safari and
      iOS: audio start, glitching under load, input, rendering, storage.
- [ ] List what fails, with causes where found, and the cost of each fix.
- [ ] Decide: supported (fixes land, splash note goes),
      supported without MIDI, or a non-goal with reasons.
- [ ] README line (0001) and the splash text agree with the decision.
