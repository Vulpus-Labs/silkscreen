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

- [x] `cargo silkscreen bundle --vst3` (or a `formats` setting) builds a
      `.vst3` alongside the `.clap`, on macOS and Windows at least.
- [x] Wrapper and SDK sources are fetched at pinned versions and cached;
      missing build tools (CMake, a C++ compiler) fail with a message
      naming what to install.
- [x] On macOS the `.vst3` is signed and notarized like the `.clap`.
- [ ] The wrapped plugin passes the VST3 validator, and loads, plays,
      automates, and saves and restores state in at least one VST3-only
      host.
- [x] `design/building-and-shipping.md` and `hosts.md` updated; licence
      notes for the wrapper and SDK in the README.

## Notes

Check the VST3 SDK licence at the version pinned (it changed in 2025) and
state it. Also consider whether a static-link wrapper build is simpler
than shipping a CLAP inside the VST3 bundle.

## Progress (2026-11-24)

- `silkscreen-tool`'s new `vst3` module: `bundle --vst3 [--validate]`.
  The plugin package's library is built again as a `staticlib` (`lipo`ed
  for `--universal`), with `--print native-static-libs` read from
  cargo's JSON messages, which cargo replays when the build was fresh.
  clap-wrapper v0.18.0 is cloned once into `<target>/silkscreen-vst3/`;
  it fetches VST3 SDK v3.8.0_build_66 (MIT since 3.8) and CLAP 1.2.10
  itself (`CLAP_WRAPPER_DOWNLOAD_DEPENDENCIES`). A generated CMake
  project links the static library into the wrapper's VST3 module, with
  `STATICALLY_LINKED_CLAP_ENTRY` on `clap-wrapper-shared-detail`, so the
  wrapper takes `clap_entry` from the module. The `.vst3` stands alone.
- Answering the notes: the VST3 SDK is MIT at 3.8.0 (README states it,
  with the trademark line). The static link is simpler than shipping a
  `.clap` inside the bundle: it is one file to sign, and Windows has no
  embedded-CLAP layout.
- Checked by hand on macOS with vxn-free's Beverley: a universal (x86_64
  + arm64) `.vst3` that passes Steinberg's validator (47 of 47) and signs
  with a Developer ID. `--notarize` takes the same path as the `.clap`.
- Two snags fixed in the generated project: `-framework X` from rustc is
  two words, which CMake split and de-duplicated (now `-Wl,-framework,X`);
  MSVC's `/defaultlib:msvcrt` would be read as a library file (now
  dropped, since CMake links the runtime).
- `--validate` on a Mac with only the Command Line Tools: the SDK's CMake
  insists on an Xcode version, so the tool passes one.
- CI's new `vst3` job bundles and validates `gain-plugin` on macOS,
  Ubuntu and Windows.
- To do: the validator on Windows and Linux (CI), and a VST3-only host.
