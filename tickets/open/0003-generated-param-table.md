---
id: "0003"
title: "One parameter declaration generates indices, ids, the table and a typed engine match"
priority: high
created: 2026-11-24
epic: E001
depends: []
---

## Summary

The template keeps a parameter in three places that nothing ties
together: `pub const WAVE: ParamIndex = ParamIndex(0)`, the matching row
of `PARAMS` (with its stable `id`), and the engine's `set_param` match,
whose `_ => {}` arm hides a forgotten parameter. Declare each parameter
once and generate the rest, so they can't drift.

## Acceptance criteria

- [ ] One declaration per parameter (name, range or steps, unit, id)
      produces the index constants, `PARAMS`, and a typed enum (or
      equivalent) the engine matches on exhaustively, so a missing arm is
      a build error.
- [ ] Engine side stays allocation-free and depends only on
      `silkscreen-bridge`; the typed value converts from `ParamIndex`
      without a lookup table walk.
- [ ] Duplicate ids fail the build, naming both parameters.
- [ ] Id reuse is caught: a committed ledger of ids ever issued (or an
      equivalent check), so deleting a parameter and reusing its id fails
      with a message saying why ids must never be reused.
- [ ] Template and examples use it; `design/model.md` describes it.
- [ ] CHANGELOG entry and upgrade note (0006).

## Notes

Choose between a declarative macro, a proc macro, and a build-time source
(TOML/KDL or the SVG's control ids) after a short study; record the
choice and reasons in `design/model.md`. CLAUDE.md prefers generated,
typed code over runtime lookup, with build errors that name the element
and the fix. Generating from the SVG would join controls and params, but
many params have no control. The design must cope with plugins of a few
hundred parameters in sections.
