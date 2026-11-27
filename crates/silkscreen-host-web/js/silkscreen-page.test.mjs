// node --test crates/silkscreen-host-web/js (run by `cargo test` via
// tests/js.rs when node is installed).

import { test } from "node:test";
import assert from "node:assert/strict";
import { browserNote, loadReadout, pageOptions, tempoValue } from "./silkscreen-page.mjs";

const required = { wasm: {}, name: "Tone", blurb: "a one-oscillator synth", engine: "./e.wasm" };

test("options take their defaults", () => {
  const o = pageOptions(required);
  assert.equal(o.example, "Tone");
  assert.equal(o.vendor, null);
  assert.equal(o.homepage, null);
  assert.equal(o.low, 36);
  assert.equal(o.octaves, 5);
  assert.equal(o.keysHigh, 120);
  assert.equal(o.logo, null);
  // Given ones win, undefined ones don't.
  const p = pageOptions({ ...required, example: "tone", octaves: 4, logo: undefined });
  assert.equal(p.example, "tone");
  assert.equal(p.octaves, 4);
  assert.equal(p.logo, null);
});

test("tempo, links and signup are off unless given", () => {
  const o = pageOptions(required);
  assert.equal(o.tempo, null);
  assert.deepEqual(o.links, []);
  assert.equal(o.signup, null);
});

test("tempo and signup fill in their own defaults", () => {
  assert.deepEqual(pageOptions({ ...required, tempo: true }).tempo, { initial: 120, min: 40, max: 240 });
  assert.deepEqual(pageOptions({ ...required, tempo: { initial: 100 } }).tempo, { initial: 100, min: 40, max: 240 });
  const s = pageOptions({ ...required, signup: { action: "https://example.com/s", fields: { tag: "tone" } } }).signup;
  assert.equal(s.action, "https://example.com/s");
  assert.equal(s.label, "Get release news");
  assert.deepEqual(s.fields, { tag: "tone" });
  assert.throws(() => pageOptions({ ...required, signup: {} }), /"action"/);
});

test("a typed tempo is whole, held to the range, or the fallback", () => {
  const r = { min: 40, max: 240 };
  assert.equal(tempoValue("128", r, 120), 128);
  assert.equal(tempoValue("97.6", r, 120), 98);
  assert.equal(tempoValue("10", r, 120), 40);
  assert.equal(tempoValue("999", r, 120), 240);
  assert.equal(tempoValue("", r, 120), 120);
  assert.equal(tempoValue("fast", r, 120), 120);
  assert.equal(tempoValue(90, r, 120), 90);
});

test("a missing option is named", () => {
  for (const k of Object.keys(required)) {
    const o = { ...required };
    delete o[k];
    assert.throws(() => pageOptions(o), new RegExp(`"${k}"`));
  }
});

const CHROME_MAC = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36";
const SAFARI_MAC = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";
const FIREFOX = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:131.0) Gecko/20100101 Firefox/131.0";
const IPHONE = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/130.0 Mobile/15E148 Safari/604.1";

test("the browser note warns Safari and every iOS browser", () => {
  assert.deepEqual(browserNote({ userAgent: CHROME_MAC, platform: "MacIntel" }), {
    text: "Best in Chrome, Edge or Firefox. Safari is not supported.",
    warn: false,
  });
  assert.equal(browserNote({ userAgent: FIREFOX, platform: "MacIntel" }).warn, false);
  const safari = browserNote({ userAgent: SAFARI_MAC, platform: "MacIntel" });
  assert.ok(safari.warn);
  assert.match(safari.text, /appear to be using Safari/);
  // Chrome on an iPhone is WebKit too.
  const ios = browserNote({ userAgent: IPHONE, platform: "iPhone", maxTouchPoints: 5 });
  assert.ok(ios.warn);
  assert.match(ios.text, /iPhone and iPad/);
  // An iPad asks for the desktop site: a Mac that takes touches.
  assert.match(browserNote({ userAgent: SAFARI_MAC, platform: "MacIntel", maxTouchPoints: 5 }).text, /iPhone and iPad/);
  assert.equal(browserNote().warn, false);
});

test("the CPU readout", () => {
  assert.deepEqual(loadReadout(0.034), { text: "CPU 3.4%", width: "3.4000000000000004%", colour: "#5bbf6a" });
  assert.equal(loadReadout(0.6).text, "CPU 60%");
  assert.equal(loadReadout(0.6).colour, "#e0a23d");
  assert.equal(loadReadout(1.3).width, "100%");
  assert.equal(loadReadout(0.9).colour, "#e0533d");
});
