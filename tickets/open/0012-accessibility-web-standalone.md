---
id: "0012"
title: "Accessibility: study and take the cheap wins on web and standalone"
priority: low
created: 2026-11-24
epic: E001
depends: []
---

## Summary

`design/overview.md` says silkscreen has no accessibility tree. Decided
(2026-11-24): the CLAP plugin need be no more accessible than a typical
plugin, but the web page and standalone app may have wins worth taking.
Find them, take the cheap ones, and state the position in the README.

## Acceptance criteria

- [ ] Study: what a screen-reader and keyboard-only user meets today on
      the web page and in the standalone app, and the cost of each fix.
- [ ] Web candidates: a hidden DOM mirror of the controls (ARIA sliders,
      buttons and selects with names, values and units from the param
      table), tab order, arrow keys adjusting the focused control, value
      announcements; respecting reduced-motion.
- [ ] Standalone candidates: AccessKit through the window layer, or
      keyboard focus and adjustment alone.
- [ ] Chosen wins land, generated from the param table and layout so a
      plugin gets them without extra code.
- [ ] README line (0001): what is supported, where, and that plugins
      inside a DAW are out of scope.
