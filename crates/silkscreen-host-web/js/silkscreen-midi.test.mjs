import { test } from "node:test";
import assert from "node:assert/strict";
import { attachMidi, channelMessage } from "./silkscreen-midi.mjs";

test("channel messages pass through, padded; system messages don't", () => {
  assert.deepEqual(channelMessage([0x90, 60, 100]), [0x90, 60, 100]);
  assert.deepEqual(channelMessage([0x83, 60, 0]), [0x83, 60, 0]);
  assert.deepEqual(channelMessage([0xb0, 1, 64]), [0xb0, 1, 64]);
  assert.deepEqual(channelMessage([0xe0, 0, 64]), [0xe0, 0, 64]);
  // Two bytes: channel pressure and program change.
  assert.deepEqual(channelMessage([0xd0, 90]), [0xd0, 90, 0]);
  assert.deepEqual(channelMessage([0xc0, 5]), [0xc0, 5, 0]);
  for (const system of [[0xf8], [0xfe], [0xf0, 0x7e, 0x7f, 0xf7], [0xf2, 0, 0]]) {
    assert.equal(channelMessage(system), null);
  }
  assert.equal(channelMessage([0x40, 1, 2]), null, "a data byte, not a status");
  assert.equal(channelMessage([]), null);
});

/// A fake MIDIAccess with ports that can come and go.
function fakeAccess(...names) {
  const port = (name) => ({ id: name, name, type: "input", state: "connected", onmidimessage: null });
  const inputs = new Map(names.map((n) => [n, port(n)]));
  const access = { inputs, onstatechange: null };
  return {
    access,
    port: (n) => inputs.get(n),
    plug(n) {
      const p = port(n);
      inputs.set(n, p);
      access.onstatechange?.({ port: p });
      return p;
    },
    unplug(n) {
      const p = inputs.get(n);
      p.state = "disconnected";
      inputs.delete(n);
      access.onstatechange?.({ port: p });
    },
  };
}

test("every input plays, and hotplug adds and drops them", async () => {
  const fake = fakeAccess("Keys");
  const got = [];
  const seen = [];
  const midi = await attachMidi({
    onMidi: (m) => got.push(m),
    onInputs: (names) => seen.push(names.join(",")),
    requestMIDIAccess: async () => fake.access,
  });
  assert.equal(midi.granted, true);
  fake.port("Keys").onmidimessage({ data: new Uint8Array([0x90, 60, 100]) });
  fake.port("Keys").onmidimessage({ data: new Uint8Array([0xf8]) });
  const pads = fake.plug("Pads");
  pads.onmidimessage({ data: new Uint8Array([0x99, 36, 127]) });
  fake.unplug("Keys");
  assert.deepEqual(got, [[0x90, 60, 100], [0x99, 36, 127]]);
  assert.deepEqual(seen, ["Keys", "Keys,Pads", "Pads"]);
  midi.detach();
  assert.equal(pads.onmidimessage, null);
});

test("no Web MIDI, or a refusal, resolves ungranted", async () => {
  const refused = await attachMidi({
    onMidi() {},
    requestMIDIAccess: async () => {
      throw new DOMException("denied", "SecurityError");
    },
  });
  assert.equal(refused.granted, false);
  assert.match(String(refused.error), /denied/);
  const saved = globalThis.navigator;
  try {
    Object.defineProperty(globalThis, "navigator", { value: {}, configurable: true });
    const absent = await attachMidi({ onMidi() {} });
    assert.equal(absent.granted, false);
  } finally {
    Object.defineProperty(globalThis, "navigator", { value: saved, configurable: true });
  }
});
