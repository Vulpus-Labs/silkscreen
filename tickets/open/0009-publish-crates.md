---
id: "0009"
title: "Publish the publishable crates to crates.io"
priority: medium
created: 2026-11-24
epic: E001
depends: [0006]
---

## Summary

Users take silkscreen from git at a tag. Publish every crate that can
be, so it can be found, versioned by cargo and documented on docs.rs.

## Acceptance criteria

- [ ] Crate names checked and reserved on crates.io.
- [ ] Every crate whose dependencies are all on crates.io is published at
      the release version: today all but `silkscreen-clap` (clack is a
      git dependency).
- [ ] `silkscreen-clap` stays a git dependency until clack is released;
      the README says so and shows the mixed dependency lines.
- [ ] docs.rs builds every published crate (features and targets set in
      metadata).
- [ ] The release checklist (0006) includes publishing; `cargo silkscreen
      new` writes registry versions where it can.

## Notes

CI already runs `cargo package` for every crate but `silkscreen-clap`.
