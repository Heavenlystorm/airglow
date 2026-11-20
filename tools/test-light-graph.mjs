// The geometry behind the Overview's light graphs, and the scale the pulse and the detail view fit to a reading.
// Run: node --experimental-vm-modules --test tools/test-light-graph.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

async function load(file, stubs = {}) {
  const context = vm.createContext({ globalThis: {}, document: undefined, Math, Number, String, Map, Set, Array, Object });
  const module = new vm.SourceTextModule(readFileSync(new URL(`../web/js/${file}`, import.meta.url), "utf8"), { context });
  await module.link((specifier) => {
    const names = Object.keys(stubs[specifier] || {});
    return new vm.SyntheticModule(names, function () { for (const name of names) this.setExport(name, stubs[specifier][name]); }, { context });
  });
  await module.evaluate();
  return module.namespace;
}
const graph = await load("light-graph.js", { "./motion.js": { reduced: () => true } });
const sensors = await load("overview-sensors.js");
const plain = (value) => JSON.parse(JSON.stringify(value));

test("a line breaks at an outage and at a missing reading, and keeps one reading beyond each edge", () => {
  const points = [[0, 1], [2, 2], [4, 3], [6, null], [8, 5], [10, 6], [30, 7], [32, 8], [34, 9]];
  assert.deepEqual(plain(graph.splitRuns(points, 3, 31)), [[[2, 2], [4, 3]], [[8, 5], [10, 6]], [[30, 7], [32, 8]]]);
  assert.deepEqual(plain(graph.splitRuns(points, 100, 200)), [[[34, 9]]], "only the newest reading before the window is kept");
  assert.deepEqual(plain(graph.splitRuns([], 0, 10)), []);
  assert.deepEqual(plain(graph.splitRuns(null, 0, 10)), []);
});

test("the smooth curve passes through every reading and never overshoots one", () => {
  const run = [[0, 10], [2, 10], [4, 80], [6, 80], [8, 20], [10, 21]];
  for (const [t, v] of run) assert.equal(graph.valueAt(run, t), v);
  for (let t = 0; t <= 10; t += .05) {
    const value = graph.valueAt(run, t);
    assert.ok(value >= 10 - 1e-9 && value <= 80 + 1e-9, `overshoot at ${t}: ${value}`);
  }
  // flat stretches stay flat instead of dipping before a rise
  for (let t = 0; t <= 2; t += .1) assert.ok(Math.abs(graph.valueAt(run, t) - 10) < 1e-9);
  for (let t = 4; t <= 6; t += .1) assert.ok(Math.abs(graph.valueAt(run, t) - 80) < 1e-9);
  const rising = [[0, 0], [1, 1], [2, 4], [3, 9]];
  let previous = -Infinity;
  for (let t = 0; t <= 3; t += .05) { const value = graph.valueAt(rising, t); assert.ok(value >= previous - 1e-9); previous = value; }
});

test("stepped and straight curves read the way they are drawn", () => {
  const run = [[0, 10], [2, 30]];
  assert.equal(graph.valueAt(run, 1, "step"), 10);
  assert.equal(graph.valueAt(run, 2, "step"), 30);
  assert.equal(graph.valueAt(run, 1, "linear"), 20);
  assert.equal(graph.valueAt(run, -1), null);
  assert.equal(graph.valueAt(run, 2.1), null);
  assert.equal(graph.valueAt([[5, 42]], 5), 42);
  assert.equal(graph.valueAt([], 5), null);
});

test("tangents are zero at a turning point and finite for uneven sampling", () => {
  assert.deepEqual(plain(graph.tangents([0, 1, 2], [0, 5, 0])), [5, 0, -5]);
  const uneven = graph.tangents([0, 2, 9, 10], [0, 4, 6, 20]);
  assert.ok(uneven.every(Number.isFinite));
  assert.deepEqual(plain(graph.tangents([0], [3])), [0]);
});

test("colours parse without a canvas and blend and print predictably", () => {
  assert.deepEqual(plain(graph.rgb("#a5dece")), [165, 222, 206, 1]);
  assert.deepEqual(plain(graph.rgb("#fff")), [255, 255, 255, 1]);
  assert.deepEqual(plain(graph.rgb("rgb(47, 230, 255)")), [47, 230, 255, 1]);
  assert.deepEqual(plain(graph.rgb("rgba(10, 20, 30, 0.5)")), [10, 20, 30, .5]);
  assert.deepEqual(plain(graph.rgb("")), [0, 0, 0, 0]);
  assert.equal(graph.paint([165, 222, 206, 1], .5), "rgba(165,222,206,0.500)");
  assert.deepEqual(plain(graph.blend([0, 0, 0, 1], [100, 200, 50, 1], .5)), [50, 100, 25, 1]);
});

test("a live scale fits recent readings with headroom and respects each unit's limits", () => {
  const at = (values) => values.map((v, i) => [100 + i * 2, v]);
  let [lo, hi] = sensors.liveRange({ unit: "°C" }, at([50, 51, 52]), 0);
  assert.ok(lo < 50 && hi > 52 && hi - lo >= 8, "a nearly flat temperature still gets a readable band");
  [lo, hi] = sensors.liveRange({ unit: "%" }, at([96, 99, 100]), 0);
  assert.equal(hi, 100);
  assert.ok(lo >= 0 && lo < 96);
  [lo, hi] = sensors.liveRange({ unit: "%" }, at([0, 2, 1]), 0);
  assert.equal(lo, 0);
  assert.ok(hi >= 24);
  [lo, hi] = sensors.liveRange({ unit: "W" }, at([3, 4]), 0);
  assert.equal(lo, 0, "power cannot be negative");
  [lo, hi] = sensors.liveRange({ unit: "°C" }, at([-12, -10]), 0);
  assert.ok(lo < -12, "a temperature may be below zero");
  assert.deepEqual(plain(sensors.liveRange({ unit: "W" }, at([null, NaN]), 0, [0, 400])), [0, 400]);
  assert.deepEqual(plain(sensors.liveRange({ unit: "W" }, at([50, 60]), 1000, [0, 400])), [0, 400], "readings before the span do not count");
  assert.deepEqual(plain(sensors.liveRange(null, null, 0)), [0, 100]);
});
