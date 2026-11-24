---
id: E001
title: "silkscreen usable by newcomers"
status: open
created: 2026-11-24
---

## Goal

silkscreen is public at v0.2.0, but someone arriving at the repository
cannot tell quickly whether it is for them, starts a project by
hand-renaming the template, keeps three copies of each parameter in step
by hand, and finds design documents where a tutorial should be. The
standalone app can't be played and VST3 takes a manual wrap. Fix those,
in roughly that order of first impression.

## Decisions (2026-11-24)

- README opens with a screenshot, a live demo link and a short "is this
  for you?" section, and states which DAWs and operating systems have
  been tested.
- Accessibility and Safari each get an explicit README line, as a
  non-goal or a roadmap item, since they will be the first questions.
- Accessibility: the CLAP plugin need be no more accessible than a typical
  plugin. Look for wins in the web page and the standalone app.
- Keep a changelog and upgrade notes per tag, at least until 1.0.

## Tickets

| Ticket | What |
|---|---|
| 0001 | README front page: screenshot, live demo, "is this for you?", tested platforms, accessibility and Safari lines |
| 0002 | `cargo silkscreen new <name>` replaces the manual rename |
| 0003 | One parameter declaration generates indices, ids, the table and a typed engine match |
| 0004 | User guide: tutorial from a blank SVG to a plugin in a DAW |
| 0005 | User guide: cookbook for custom bindings and canvas controls |
| 0006 | Upgrade notes per tag and a release checklist |
| 0007 | MIDI input in the standalone app |
| 0008 | `cargo silkscreen bundle --vst3`: VST3 in one command |
| 0009 | Publish the publishable crates to crates.io |
| 0010 | Record which DAWs and operating systems are tested |
| 0011 | Snapshot tests on all three CI platforms |
| 0012 | Accessibility: study and take the cheap wins on web and standalone |
| 0013 | Safari: find what breaks, then support it or state why not |

## Dependency order

0002, 0003, 0006, 0007, 0008, 0010, 0011, 0012, 0013 in any order.
0009 after 0006. 0004 after 0002, 0003 and 0008; 0005 after 0003.
0001 after 0010, 0012 and 0013 for its final wording (a first cut can land
earlier with the current positions).

## Acceptance

- A stranger reading the README knows within a screen what silkscreen is,
  what it looks like, whether it fits their plugin, and what it does not
  do.
- `cargo silkscreen new` plus the tutorial takes them from nothing to a
  plugin loaded in a DAW, as CLAP and VST3, without editing names by hand.
- A parameter is declared once; an engine that forgets one fails to build.
- The standalone app plays from a MIDI keyboard.
