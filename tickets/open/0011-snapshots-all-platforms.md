---
id: "0011"
title: "Snapshot tests on all three CI platforms"
priority: medium
created: 2026-11-24
epic: E001
depends: []
---

## Summary

PNG snapshots are bit-exact and blessed on ARM64 macOS; on Linux and
Windows CI they skip themselves, so visual regressions there go unseen.
Either compare with tolerance or bless per platform.

## Acceptance criteria

- [ ] Measure first: run the snapshot suite on x86-64 Linux and Windows
      and record how the images differ (pixel count, max channel delta,
      where).
- [ ] From that, choose: a tolerance (per-channel delta and changed-pixel
      budget, with a diff image on failure) if differences are small and
      scattered, or per-platform blessed sets if they are systematic.
      Record the choice in `design/` and CLAUDE.md.
- [ ] All three CI jobs run the snapshot tests; none skip.
- [ ] A deliberate one-pixel-wide change still fails everywhere.
- [ ] Blessing on another platform is documented (CI artifact or a
      command).
