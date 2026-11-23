// node --test crates/silkscreen-host-web/js (run by `cargo test` via
// tests/js.rs when node is installed).

import { test } from "node:test";
import assert from "node:assert/strict";
import {
  TouchPan, Voices, clampBase, computerLabels, computerNote, isBlack, keyAt, keyDepth, keyLayout, velocityAt,
  wheelScroll,
} from "./silkscreen-keyboard.mjs";

test("two octaves from C3: 15 white keys, 10 black", () => {
  const l = keyLayout(48, 2);
  assert.equal(l.whites, 15);
  assert.equal(l.keys.filter((k) => k.black).length, 10);
  assert.equal(l.keys[0].key, 48);
  assert.equal(l.keys.at(-1).key, 72);
  assert.ok(isBlack(49) && !isBlack(52) && !isBlack(53) && isBlack(54));
});

test("black keys sit on top of the upper part of the strip", () => {
  const l = keyLayout(48, 1);
  // Between C and D at the top: C#; lower down: C or D.
  assert.equal(keyAt(l, 1.0, 0.2), 49);
  assert.equal(keyAt(l, 0.9, 0.8), 48);
  assert.equal(keyAt(l, 1.1, 0.8), 50);
  // E and F have no black key between them.
  assert.equal(keyAt(l, 3.0, 0.2), 53);
  assert.equal(keyAt(l, 2.99, 0.2), 52);
  // Off the strip.
  assert.equal(keyAt(l, -0.1, 0.5), null);
  assert.equal(keyAt(l, 8.0, 0.5), null);
  assert.equal(keyAt(l, 2.0, 1.2), null);
});

test("velocity grows down the key and stays a MIDI velocity", () => {
  assert.ok(velocityAt(0.9) > velocityAt(0.1));
  assert.equal(velocityAt(1), 127);
  assert.equal(velocityAt(5), 127);
  assert.ok(velocityAt(-1) >= 1);
});

test("velocity is measured along the key pressed, not the strip", () => {
  const l = keyLayout(48, 1);
  // A black key's front edge is full velocity, though it stops 62% down.
  const blackFront = 0.6199;
  assert.equal(keyAt(l, 1.0, blackFront), 49);
  assert.equal(velocityAt(keyDepth(49, blackFront)), 127);
  // Halfway down each key is the same velocity, black or white.
  assert.equal(velocityAt(keyDepth(49, 0.31)), velocityAt(keyDepth(48, 0.5)));
  // A white key's depth is the strip's.
  assert.equal(keyDepth(48, 0.5), 0.5);
  assert.equal(velocityAt(keyDepth(48, 0.999)), 127);
});

test("a press, a glide and a release", () => {
  const v = new Voices(2);
  assert.deepEqual(v.down(1, 60, 100), [[0x92, 60, 100]]);
  assert.deepEqual(v.move(1, 60, 90), [], "same key: nothing");
  assert.deepEqual(v.move(1, 62, 90), [[0x82, 60, 0], [0x92, 62, 90]]);
  assert.deepEqual(v.move(1, null, 90), [[0x82, 62, 0]], "off the strip");
  assert.deepEqual(v.move(1, 64, 80), [[0x92, 64, 80]], "back on");
  assert.deepEqual(v.up(1), [[0x82, 64, 0]]);
  assert.deepEqual(v.up(1), [], "already up");
});

test("several pointers, and releasing everything", () => {
  const v = new Voices();
  v.down(1, 60, 100);
  v.down(2, 64, 100);
  assert.deepEqual([...v.keys()].sort(), [60, 64]);
  // Pointer 2 slides onto pointer 1's key: it takes it over.
  assert.deepEqual(v.move(2, 60, 100), [[0x80, 64, 0], [0x90, 60, 100]]);
  assert.deepEqual(v.up(1), [], "60 is pointer 2's now");
  v.down(3, 67, 100);
  assert.deepEqual(v.releaseAll(), [[0x80, 60, 0], [0x80, 67, 0]]);
  assert.equal(v.keys().size, 0);
});

test("computer keys: the home row is white from C, the row above black", () => {
  assert.equal(computerNote("KeyA", 48), 48);
  assert.equal(computerNote("KeyW", 48), 49);
  assert.equal(computerNote("KeyF", 48), 53);
  assert.equal(computerNote("KeyK", 48), 60);
  assert.equal(computerNote("Quote", 48), 65);
  assert.equal(computerNote("KeyQ", 48), null);
  for (const [key, label] of computerLabels(48)) {
    assert.equal(isBlack(key), "WETYUOP".includes(label), `${label} on ${key}`);
  }
});

test("the computer keys' base stays on a C, on the strip where it fits", () => {
  // Four octaves from C2: the keys span 17 semitones, so C2, C3 or C4.
  assert.equal(clampBase(36, 4, 48), 48);
  assert.equal(clampBase(36, 4, 72), 60);
  assert.equal(clampBase(36, 4, 0), 36);
  assert.equal(clampBase(36, 4, 50), 48, "onto a C");
  // Two octaves: only the lowest C keeps all of them on the strip.
  assert.equal(clampBase(48, 2, 60), 48);
});

test("a computer key and a pointer hold keys side by side", () => {
  const v = new Voices();
  assert.deepEqual(v.down("kb:KeyA", 48, 100), [[0x90, 48, 100]]);
  assert.deepEqual(v.down(1, 52, 90), [[0x90, 52, 90]]);
  assert.deepEqual(v.up("kb:KeyA"), [[0x80, 48, 0]]);
  assert.deepEqual([...v.keys()], [52]);
});

test("one finger plays; a second makes both pan and releases their notes", () => {
  const p = new TouchPan();
  assert.deepEqual(p.down(1, 100, true), []);
  assert.ok(!p.pans(1));
  assert.equal(p.move(1, 120), null, "one finger glides, doesn't pan");
  assert.deepEqual(p.down(2, 300, true).sort(), [1, 2]);
  assert.ok(p.pans(1) && p.pans(2));
  // Both move 40 px right: the content moves 40 in all.
  assert.equal(p.move(1, 160) + p.move(2, 340), 40);
});

test("a finger that panned plays nothing until it lifts", () => {
  const p = new TouchPan();
  p.down(1, 0, true);
  p.down(2, 50, true);
  p.up(2);
  assert.ok(p.pans(1), "the finger left behind still pans");
  assert.equal(p.move(1, 10), 10);
  p.up(1);
  assert.deepEqual(p.down(3, 0, true), []);
  assert.ok(!p.pans(3), "a fresh finger plays");
});

test("a mouse never pans", () => {
  const p = new TouchPan();
  assert.deepEqual(p.down(1, 0, false), []);
  assert.deepEqual(p.down(2, 0, true), []);
  assert.ok(!p.pans(1) && !p.pans(2));
});

test("a wheel scrolls by its larger axis, in pixels", () => {
  assert.equal(wheelScroll({ deltaX: 30, deltaY: 5, deltaMode: 0 }, 400), 30);
  assert.equal(wheelScroll({ deltaX: 0, deltaY: -3, deltaMode: 1 }, 400), -48);
  assert.equal(wheelScroll({ deltaX: 1, deltaY: 0, deltaMode: 2 }, 400), 400);
});
