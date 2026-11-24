---
id: "0001"
title: 'README front page: screenshot, live demo, "is this for you?", platforms, non-goals'
priority: high
created: 2026-11-24
epic: E001
depends: [0010, 0012, 0013]
---

## Summary

The README opens with a crate table. A visitor should instead see what a
silkscreen faceplate looks like, be able to play one, and learn in a few
lines whether silkscreen fits what they are making.

## Acceptance criteria

- [ ] A screenshot at the top: a faceplate that shows off the controls
      (the `widgets` example's snapshot, or a purpose-made one), checked
      into the repo.
- [ ] A live demo link under it, to a page deployed from the public repo
      (GitHub Pages via `cargo silkscreen deploy`, or the tool's target).
- [ ] "Is this for you?": yes if you draw a fixed faceplate, want CLAP,
      web and standalone from one codebase, and write the DSP in Rust; no
      if you need resizable or flowing layouts, a general GUI toolkit, or
      AU/AAX today.
- [ ] A "Tested on" section from 0010's record.
- [ ] One line each for accessibility and Safari, stating the position
      0012 and 0013 settle (before they close: "not supported yet", with
      a link to the roadmap item).
- [ ] The crate table moves below these.

## Notes

The demo is the `widgets` example (or the template) deployed from this
repository, so it shows only what the repository builds.
