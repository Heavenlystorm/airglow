import test from "node:test";
import assert from "node:assert/strict";
import { signals, traceFor, sharedTimes, nearestRow, elapsedText, formatValue, displayScale, graphBounds, stepTime } from "../web/js/session-traces.js";

const field = { id: "a/b", unit: "W", name: "Package power", group: "CPU" };
const run = (pairs, sensors = [field]) => ({ sensors, samples: pairs.map(([elapsed, value]) =>
  ({ elapsed, readings: [value], cpu: value })) });

test("shares elapsed time without drawing across missing rows, long pauses, or trailing time", () => {
  const r = { ...run([[0, 0], [2, 10], [4, null], [6, 12], [17, 20], [19, 4]]), duration_seconds: 25 };
  const key = signals(r)[0].key, a = traceFor(r, key);
  assert.deepEqual(a.segments.map(s => s.map(p => p.time)), [[0, 2], [6], [17, 19]]);
  assert.equal(a.count, 5); assert.equal(a.mean, 9.2); assert.equal(a.low.value, 0); assert.equal(a.duration, 25);
  const b = traceFor(run([[2.1, 8], [4, 7]]), key);
  assert.deepEqual(sharedTimes([a, b]), [0, 2, 2.1, 4, 6, 17, 19]);
  assert.equal(nearestRow(a.rows, 2.1).time, 2);
  assert.equal(nearestRow(a.rows, 4).value, null);
  assert.equal(nearestRow(a.rows, 13), null);
  assert.equal(nearestRow(a.rows, 25), null);
});

test("matches a catalog sensor by identity and unit across reordered and old recordings", () => {
  const one = run([[0, 4]]), key = signals(one)[0].key;
  const two = { sensors: [{ ...field, id: "a/volts", unit: "V" }, field], samples: [{ elapsed: 0, readings: [1.202, 12] }] };
  assert.equal(traceFor(two, key).mean, 12);
  assert.equal(traceFor(run([[0, 8]], [{ ...field, unit: "kW" }]), key).metric, undefined);
  const legacy = { samples: [{ elapsed: 2, cpu: 0, rpm: null }] };
  assert.equal(traceFor(legacy, "cpu").mean, 0);
  assert.equal(traceFor(legacy, "rpm").count, 0);
  assert.equal(traceFor(legacy, key).mean, null);
});

test("formats absolute and relative temperatures, percentage points and units on a common scale", () => {
  const trace = traceFor(run([[1, 0], [4, 30]]), "cpu");
  const temp = displayScale("°C", [trace], true);
  assert.equal(formatValue(0, temp), "32°F");
  assert.equal(formatValue(10, temp, true), "+18°F");
  assert.equal(formatValue(-1.25, temp, true), `${(-2.25).toLocaleString()}°F`);
  assert.equal(formatValue(2.5, displayScale("%", [trace]), true), `+${(2.5).toLocaleString()} pp`);
  assert.equal(formatValue(null, temp), "—");
  const transfer = traceFor(run([[1, 4096], [2, 4 * 1024 ** 2]]), "cpu");
  assert.equal(displayScale("B/s", [trace, transfer]).unit, "MiB/s");
  assert.equal(displayScale("GB", []).unit, "GiB");
  const limits = graphBounds([trace], temp);
  assert.ok(limits.lo <= 32 && limits.hi >= 86);
  assert.equal(graphBounds([traceFor(run([[0, null]]), "cpu")], temp), null);
});

test("single points, signed readings and fractional seconds retain a valid chart range", () => {
  for (const value of [0, -8, 1, 100]) {
    const t = traceFor(run([[0.02, value]]), "cpu");
    const range = graphBounds([t], displayScale("%", [t]));
    assert.ok(Number.isFinite(range.lo) && range.lo <= value && value <= range.hi && range.lo < range.hi);
  }
  assert.equal(elapsedText(60.004, true), "1:00.004");
  assert.equal(elapsedText(59.9999, true), "1:00");
  assert.equal(elapsedText(5.200, true), "0:05.2");
});

test("keyboard steps visit actual timestamps and boundaries even after scrubbing into a long gap", () => {
  const stops = [0, 0.525, 2.615, 18.125, 19];
  assert.equal(stepTime(stops, 12, 1), 18.125);
  assert.equal(stepTime(stops, 12, -1), 2.615);
  assert.equal(stepTime(stops, 2.615, -1), .525);
  assert.equal(stepTime(stops, 0, -1), 0);
  assert.equal(stepTime(stops, 19, 1), 19);
});
