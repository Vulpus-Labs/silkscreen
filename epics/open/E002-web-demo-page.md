---
id: E002
title: "The web demo page leads somewhere and keeps time"
status: open
created: 2026-11-27
---

## Goal

`mountSynthPage` makes a playable instrument page, but once its splash is
dismissed the page has no way back to the synth's own site and no way to
follow it, and every tempo-synced feature (arpeggiators, synced LFOs and
delays) runs at the 120 BPM fallback because the web host never supplies a
transport.

## Tickets

- 0021: the web host supplies a transport, with a tempo the page sets
- 0022: the page carries links and an optional mailing-list form
