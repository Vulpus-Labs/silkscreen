---
id: "0020"
title: "Telemetry frames: many engine values, read whole"
priority: medium
created: 2026-11-25
epic: none
depends: []
---

## Summary

An engine display that draws many values at once (a flock of voices,
each with a position, velocity and gain) needs them as one consistent
snapshot. Engine values are 16 floats written one at a time, so a reader
can see half of one update and half of the next. Telemetry gains a
frame: a block of floats the engine publishes whole and the editor reads
whole.

## Acceptance criteria

- [x] A plugin declares a frame size at build time.
- [x] The engine writes a whole frame, without allocating or locking.
- [x] The UI reads the latest complete frame and never sees a torn one.
- [x] Works on both native and web hosts.
- [x] Covered by tests and by design/engine-bridge.md.

## Close-out (2026-11-25)

- `PluginUi::frame` declares the size in words (0 for none; at most
  `MAX_FRAME`, 65536). The CLAP layer makes its bridge with
  `bridge_with_frame`; the web page passes it to `connect`, and
  `SabLayout::with_frame` records it in the shared buffer's telemetry
  size, which the worklet reads back on attach. Layout version 4.
- `Engine::frame(write)`, called by `write_block` once per block after
  `process`, as `values` is, so every host publishes it.
- The frame is a seqlock over two buffers at the end of the telemetry
  block. The tap writes frame `n` to buffer `n & 1` and then publishes
  `n`; the reader copies word by word with acquire loads and checks the
  sequence again, retrying up to four times, else keeping the last
  frame. The tap never waits.
- `EditorState::engine_frame()` gives the latest frame and a
  generation; `Harness::engine_frame` publishes one in tests.
- Tests: frames clip to capacity, are read once, survive a sequence wrap
  and a new tap over a used block; a writer at full speed against a
  polling reader never yields a torn frame or length (and fails within
  ~1000 frames with the sequence check removed); no allocation; frames
  cross the shared buffer and the web engine; the editor takes them on
  a tick.
- Breaking (`PluginUi` field, `connect`/`sab_link` arguments,
  `LayoutError::Frame`): released as 0.3.0.

## Fix (2026-11-25): 0.3.1

In a DAW the frame never arrived: the CLAP processor wraps every engine
in the licence gate (`Gated`), which forwarded `meters`, `values` and
`scope` but not the new `frame`, so the default (publish nothing) ran.
The bridge, web and harness tests all drove an engine directly, past the
gate. `Gated` now forwards `frame`, and a test calls every `Engine` hook
through the gate and checks each reaches the engine (it fails with the
forward removed). Released as 0.3.1.
