---
id: "0002"
title: "cargo silkscreen new <name> replaces the manual rename"
priority: high
created: 2026-11-24
epic: E001
depends: []
---

## Summary

`template/README.md` step 1 asks the user to copy the folder and rename
`my-synth`, `MySynthPlugin` and the rest by hand, which is easy to get
partly wrong. `cargo silkscreen new <name>` should write a ready project.

## Acceptance criteria

- [ ] `cargo silkscreen new acme-filter` writes `acme-filter/` with every
      crate, package, type, web prefix, product name and bundle id derived
      from the name (kebab, snake, Camel), and a fresh CLAP id.
- [ ] Options for vendor name and bundle-id prefix; prompts not required.
- [ ] Silkscreen dependencies point at the tag of the tool that ran it.
- [ ] The template stays the single source: the tool embeds it at build
      time (or reads it at the tool's tag), so the two can't drift.
- [ ] Refuses to overwrite a non-empty directory; names that aren't valid
      crate names fail with a message naming the fix.
- [ ] CI generates a project into a temp dir and runs its fmt, test and
      clippy.
- [ ] `template/README.md` step 1 becomes this command.

## Notes

Considered cargo-generate: one more tool to install, and its placeholders
would make the template unbuildable in place, so CI could no longer test
it directly. Prefer the tool doing a rename over a buildable template.
