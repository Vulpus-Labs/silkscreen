// node --test crates/silkscreen-host-web/js (run by `cargo test` via
// tests/js.rs when node is installed).

import { test } from "node:test";
import assert from "node:assert/strict";
import { fitZoom } from "./silkscreen-fit.mjs";

const face = { w: 1120, h: 546 };

test("fits the tighter dimension", () => {
  // 1600×1000 window less the keyboard bar: width binds (1.40 < 1.52).
  const z = fitZoom({ w: 1568, h: 831 }, face);
  const want = Math.min(1568 / 1120, 831 / 546);
  assert.ok(z <= want && z > want - 1 / 64, `${z}`);
  // A wide, short window: height binds harder.
  assert.ok(fitZoom({ w: 2000, h: 400 }, face) <= 400 / 546);
  // A narrow, tall one: width binds.
  assert.ok(fitZoom({ w: 900, h: 2000 }, face) <= 900 / 1120);
});

test("never grows past max nor shrinks past min", () => {
  assert.equal(fitZoom({ w: 5000, h: 5000 }, face), 2);
  assert.equal(fitZoom({ w: 100, h: 100 }, face), 0.6);
  assert.equal(fitZoom({ w: 5000, h: 5000 }, face, { max: 1.5 }), 1.5);
});

test("a touch screen keeps the faceplate's own size", () => {
  assert.equal(fitZoom({ w: 390, h: 700 }, face, { coarse: true }), 1);
  assert.equal(fitZoom({ w: 3000, h: 2000 }, face, { coarse: true }), 1);
});

test("quantised to 1/64, so a pixel's drag doesn't re-render", () => {
  assert.equal(fitZoom({ w: 1000, h: 1000 }, face), fitZoom({ w: 1001, h: 1000 }, face));
  assert.equal((fitZoom({ w: 1000, h: 1000 }, face) * 64) % 1, 0);
});

test("a faceplate with no size yet isn't zoomed", () => {
  assert.equal(fitZoom({ w: 800, h: 600 }, { w: 0, h: 0 }), 1);
});

test("fitted as one column with the keyboard", () => {
  // 1600×1000: 1000 less the bar's 33 px of chrome and the stage's 16 of
  // padding, over a faceplate 546 high and a keyboard 120 high.
  const z = fitZoom({ w: 1568, h: 951 }, { w: 1120, h: 546 + 120 });
  const want = Math.min(1568 / 1120, 951 / 666);
  assert.ok(z <= want && z > want - 1 / 64, `${z}`);
  // Both at that zoom fit the column.
  assert.ok(546 * z + 120 * z <= 951);
});
