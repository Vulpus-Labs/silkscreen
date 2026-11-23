// node --test crates/silkscreen-host-web/js (run by `cargo test` via
// tests/js.rs when node is installed).

import { test } from "node:test";
import assert from "node:assert/strict";
import { CLOSE, SNOOZE_MS, SurveyRun, charDelay, parseSurvey, shouldAsk, surveyFlags } from "./silkscreen-survey.mjs";

const MD = `---
id: test-survey
endpoint: https://example.com/answers
send: on
---

<!-- a note for editors -->

## intro

Hi! I'm Dominic.
Still the first paragraph.

Would you answer a few questions?

- A) Sure! -> like
- b) Not now -> close

## like

Do you like it?

- A) Yes -> thanks
- B) No -> candour

## thanks

Thanks!

## candour

Thank you for your candour.
`;

test("a survey parses into screens", () => {
  const s = parseSurvey(MD);
  assert.equal(s.id, "test-survey");
  assert.equal(s.endpoint, "https://example.com/answers");
  assert.equal(s.send, true);
  assert.equal(s.start, "intro");
  assert.deepEqual(Object.keys(s.screens), ["intro", "like", "thanks", "candour"]);
  assert.deepEqual(s.screens.intro.paragraphs, [
    "Hi! I'm Dominic. Still the first paragraph.",
    "Would you answer a few questions?",
  ]);
  assert.deepEqual(s.screens.intro.options, [
    { key: "A", label: "Sure!", next: "like" },
    { key: "B", label: "Not now", next: CLOSE },
  ]);
  assert.deepEqual(s.screens.thanks.options, []);
});

test("sending is off unless asked for", () => {
  const s = parseSurvey("---\nid: x\nendpoint:\n---\n## a\nHello.\n");
  assert.equal(s.endpoint, "");
  assert.equal(s.send, false);
});

test("mistakes are named, with the fix", () => {
  const bad = [
    ["## a\nHi.\n", /needs an "id:"/],
    ["---\nid: x\n---\nHi.\n", /before the first "## screen"/],
    ["---\nid: x\n---\n", /no screens/],
    ["---\nid: x\n---\n## a\nHi.\n- A) Go -> nowhere\n", /screen "a", option A goes to "nowhere".*add "## nowhere"/],
    ["---\nid: x\n---\n## a\nHi.\n- A Go -> a\n", /screen "a".*should read "- A\) label -> next"/],
    ["---\nid: x\n---\n## a\nHi.\n- A) Go -> close\n- a) Again -> close\n", /two options A/],
    ["---\nid: x\n---\n## a\nHi.\n## a\nAgain.\n", /two screens are called "a"/],
    ["---\nid: x\n---\n## a\n- A) Go -> close\n", /screen "a" has nothing to say/],
    ["---\nid: x\n---\n## a\nHi.\n- A) Go -> close\nLate words.\n", /after its options/],
    ["---\nid: x\n---\n## close\nHi.\n", /reserved/],
    ["---\nid: x\nsend: yes\n---\n## a\nHi.\n", /"send: on" or "send: off"/],
    ["---\nid: x\nsend: on\n---\n## a\nHi.\n", /needs an "endpoint:"/],
  ];
  for (const [md, error] of bad) assert.throws(() => parseSurvey(md), error, md);
});

test("a run follows the answers and records them", () => {
  const run = new SurveyRun(parseSurvey(MD));
  assert.equal(run.at, "intro");
  assert.equal(run.choose("C"), null);
  assert.equal(run.choose("a"), "like");
  assert.equal(run.ended, false);
  assert.equal(run.choose("B"), "candour");
  assert.equal(run.ended, true);
  assert.deepEqual(run.payload(), {
    survey: "test-survey",
    answers: [
      { screen: "intro", key: "A", label: "Sure!" },
      { screen: "like", key: "B", label: "No" },
    ],
  });
});

test("not now closes", () => {
  const run = new SurveyRun(parseSurvey(MD));
  assert.equal(run.choose("B"), CLOSE);
  assert.equal(run.screen, null);
  assert.equal(run.ended, false);
});

test("?survey asks anyway, sooner if given seconds; &test shows the answers", () => {
  assert.deepEqual(surveyFlags("", 120_000), { force: false, waitMs: 120_000, test: false });
  assert.deepEqual(surveyFlags("?survey", 120_000), { force: true, waitMs: 120_000, test: false });
  assert.deepEqual(surveyFlags("?survey=5&test", 120_000), { force: true, waitMs: 5000, test: true });
  assert.equal(surveyFlags("?survey=soon", 120_000).waitMs, 120_000);
});

test("on localhost, survey and test are on without flags", () => {
  for (const host of ["localhost", "127.0.0.1", "[::1]"]) {
    assert.deepEqual(surveyFlags("", 120_000, host), { force: true, waitMs: 120_000, test: true });
  }
  assert.equal(surveyFlags("?survey=3", 120_000, "localhost").waitMs, 3000);
  assert.equal(surveyFlags("", 120_000, "example.com").force, false);
});

test("asked again only after a snooze, never once answered", () => {
  const now = 1_000_000_000_000;
  assert.equal(shouldAsk(null, now), true);
  assert.equal(shouldAsk({ state: "done", at: 0 }, now), false);
  assert.equal(shouldAsk({ state: "declined", at: now - 1000 }, now), false);
  assert.equal(shouldAsk({ state: "declined", at: now - SNOOZE_MS }, now), true);
});

test("punctuation pauses", () => {
  assert.ok(charDelay(".") > charDelay(",") && charDelay(",") > charDelay("a"));
});
