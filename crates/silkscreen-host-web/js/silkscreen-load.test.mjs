import { test } from "node:test";
import assert from "node:assert/strict";
import { LoadMeter } from "./silkscreen-load.mjs";

test("a whole-millisecond clock averages to the true load", () => {
  // Quanta every 2.667 ms on average, each rendering for 0.4 ms: 15% of
  // the budget. Callbacks jitter (the system hands out audio in bursts),
  // which spreads their phases against the clock's ticks; exact periods
  // would visit only three phases and bias the sum.
  const quantum = 128 / 48;
  let seed = 1;
  const jitter = () => ((seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648) * 0.5;
  let t = 0.37;
  const meter = new LoadMeter(quantum, 375, () => Math.floor(t));
  let load = -1;
  for (let i = 0; i < 375; i++) {
    const start = meter.begin();
    t += 0.4;
    load = meter.end(start);
    t += quantum - 0.4 + jitter() - 0.25;
  }
  assert.ok(Math.abs(load - 0.15) < 0.01, `load ${load}`);
});

test("reports once a window, then starts again", () => {
  let t = 0;
  const meter = new LoadMeter(2, 4, () => t);
  const reports = [];
  for (let i = 0; i < 8; i++) {
    const start = meter.begin();
    t += i < 4 ? 1 : 0.5;
    reports.push(meter.end(start));
  }
  assert.deepEqual(reports, [-1, -1, -1, 0.5, -1, -1, -1, 0.25]);
});
