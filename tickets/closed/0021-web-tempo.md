---
id: "0021"
title: The web host supplies a transport, with a tempo the page sets
priority: medium
created: 2026-11-27
epic: E002
depends: []
---

## Summary

`WebEngine::render` never calls `Engine::transport`, so in a browser every
tempo-synced feature runs at whatever the engine assumed (in practice
`Transport::FALLBACK_TEMPO`) and nothing can change it. The web host
should supply a transport each block, as native hosts do, with a tempo the
page sets.

## Acceptance criteria

- [x] `WebEngine` holds a `Transport` (default: `Transport::default()`)
      and passes it to `Engine::transport` once per block, after the UI's
      values and before notes and audio
- [x] `WebEngine::set_tempo(bpm)` sets its tempo; a non-finite or
      non-positive tempo is ignored
- [x] `export_web_engine!` exports `silkscreen_engine_set_tempo(bpm: f64)`
- [x] the worklet forwards a `{ type: "tempo", bpm }` message, and does
      nothing for an engine module built before the export existed
- [x] `startAudio`'s result has `setTempo(bpm)`
- [x] `mountSynthPage({ tempo })` shows a BPM field beside the CPU meter,
      remembered between visits, and sends it once audio starts and on
      every change; without the option there is no field
- [x] design/engine-bridge.md and design/hosts.md describe it

## Notes

Tempo only: the transport stays stopped at beat 0. A play button would
change engines' restart and stop behaviour (vxn-2b sends all-notes-off on
stop) and is not wanted for a demo page.

## Close-out (2026-11-27)

Landed as planned. `WebEngine` calls `Engine::transport` after the UI's
values and before notes, so a note's block already has its tempo. The
page's field selects its value on focus, so typing replaces it, and Enter
hands the keys back. Checked in headless Chrome against vxn-1c's web
build: the field sends the tempo, the engine module exports
`silkscreen_engine_set_tempo`, and the value survives a reload.
