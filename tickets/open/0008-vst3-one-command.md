---
id: "0008"
title: "cargo silkscreen bundle --vst3: VST3 in one command"
priority: high
created: 2026-11-24
epic: E001
depends: []
---

## Summary

`design/hosts.md` says VST3 comes from wrapping the CLAP build with a
CLAP-to-VST3 wrapper such as free-audio's clap-wrapper, by hand. Most
buyers still ask for VST3, so the tool should produce it.

## Acceptance criteria

- [ ] `cargo silkscreen bundle --vst3` (or a `formats` setting) builds a
      `.vst3` alongside the `.clap`, on macOS and Windows at least.
- [ ] Wrapper and SDK sources are fetched at pinned versions and cached;
      missing build tools (CMake, a C++ compiler) fail with a message
      naming what to install.
- [ ] On macOS the `.vst3` is signed and notarized like the `.clap`.
- [ ] The wrapped plugin passes the VST3 validator, and loads, plays,
      automates, and saves and restores state in at least one VST3-only
      host.
- [ ] `design/building-and-shipping.md` and `hosts.md` updated; licence
      notes for the wrapper and SDK in the README.

## Notes

Check the VST3 SDK licence at the version pinned (it changed in 2025) and
state it. Also consider whether a static-link wrapper build is simpler
than shipping a CLAP inside the VST3 bundle.
