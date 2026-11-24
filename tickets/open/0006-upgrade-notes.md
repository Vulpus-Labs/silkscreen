---
id: "0006"
title: "Upgrade notes per tag and a release checklist"
priority: medium
created: 2026-11-24
epic: E001
depends: []
---

## Summary

`CHANGELOG.md` records each release and CI runs semver checks against the
latest tag. Breaking releases also need upgrade notes saying what to
change in a plugin, and the release steps in CLAUDE.md should be a
checklist that covers them.

## Acceptance criteria

- [ ] Each breaking release has an "Upgrading from X.Y" section in the
      changelog with before/after code for every break.
- [ ] The release steps in CLAUDE.md become a checklist: changelog and
      upgrade notes, version bump, semver check, template and `cargo
      silkscreen new` point at the new tag, tested-platforms record
      (0010), tag, GitHub release whose body is the changelog section.
- [ ] Policy stated in the changelog header: kept per tag at least until
      1.0.
