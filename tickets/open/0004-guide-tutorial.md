---
id: "0004"
title: "User guide: tutorial from a blank SVG to a plugin in a DAW"
priority: high
created: 2026-11-24
epic: E001
depends: [0002, 0003, 0008]
---

## Summary

`design/` explains how silkscreen works; nothing walks a newcomer through
making something. Write a user guide, separate from the design docs,
starting with one tutorial.

## Acceptance criteria

- [ ] `guide/` (or an mdBook) in silkscreen, linked from the README, kept
      apart from `design/`.
- [ ] Tutorial: `cargo silkscreen new`, draw a faceplate in an SVG editor
      from blank (which ids and layers silkscreen reads), add a parameter
      and control, write the engine, run the standalone app, open the web
      page, bundle CLAP and VST3, load it in a DAW.
- [ ] Every command and code sample in it is run by CI (doc test, script,
      or a project kept in step), so it can't rot.
- [ ] Screenshots where they help, small.
- [ ] Someone who hasn't seen silkscreen follows it end to end; note what
      tripped them in the close-out.

## Notes

Depends on 0002, 0003 and 0008 so it teaches the final workflow, not the
manual one. Inkscape and Figma notes on exporting usable SVG belong here.
