import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { demand, displayTemp, largestGapPoint, parseRows, pointRows, targetLine } from "../web/js/curve-sandbox-model.js";

const points = [[30, 30], [50, 55], [70, 45], [99, 65]];
const policy = { failsafe: 35, safety: 90, maxStep: 5, previous: 45, hot: false };
test("matches control interpolation, flat endpoints and an immediate safety override", () => {
  assert.deepEqual(demand(points, 20, policy), { target: 30, next: 40 });
  assert.deepEqual(demand(points, 40, policy), { target: 42.5, next: 42 });
  assert.deepEqual(demand(points, 60, policy), { target: 50, next: 50 });
  assert.deepEqual(demand(points, 90, policy), { target: 100, next: 100 });
  assert.deepEqual(demand(points, 60, { ...policy, hot: true }), { target: 100, next: 100 });
  assert.equal(demand(points, 100, { ...policy, safety: 110, previous: null }).next, 65);
  assert.deepEqual(demand(points, null, policy), { target: 45, next: 45 });
  assert.deepEqual(demand(points, null, { ...policy, previous: null }), { target: 35, next: 35 });
  assert.deepEqual(demand(points, null, { ...policy, hot: true }), { target: 100, next: 100 });
  assert.equal(demand(points, null, { ...policy, failsafe: undefined }).target, null);
  assert.equal(demand(points, 70, { ...policy, maxStep: undefined }).next, null);
  assert.equal(demand([[30, 45], [50, 50]], 40, { ...policy, previous: null }).next, 48);
});

test("draws the discontinuity at the safety temperature and extends endpoint duties", () => {
  assert.deepEqual(targetLine([[35, 35], [95, 65]], 90), [[20, 35], [35, 35], [90, 62.5], [90, 100], [100, 100]]);
  assert.deepEqual(targetLine(points, 90, true), [[20, 100], [100, 100]]);
  assert.deepEqual(targetLine([[30, 20], [80, 60]], 110), [[20, 20], [30, 20], [80, 60], [100, 60]]);
});

test("turns Fahrenheit entries into unique, validated Celsius points while preserving zero and blank distinctions", () => {
  for (const f of [false, true]) {
    assert.deepEqual(parseRows(pointRows(points, f), 20, f).points, points);
    assert.ok(parseRows([["", "30"], ["80", "40"]], 20, f).error);
    assert.ok(parseRows([["80", "19"], ["100", "20"]], 20, f).error);
  }
  assert.equal(displayTemp(37, true), 98.6);
  assert.deepEqual(parseRows([["99","20"],["100","25"]], 20, true).points, [[37,20],[38,25]]);
  assert.ok(parseRows([["98","20"],["99","25"]], 20, true).error);
  assert.ok(parseRows([["30.5","20"],["50","30"]], 20, false).error);
  assert.deepEqual(parseRows([["70","25"],["30","20"]], 20, false).points, [[30,20],[70,25]]);
});

test("adds a point on the current shape, finding space when the selected temperature is occupied", () => {
  assert.deepEqual(largestGapPoint([[30,30],[80,100]], 45), [45,51]);
  assert.deepEqual(largestGapPoint([[30,30],[80,100]], 30), [55,65]);
});

test("offline commands agree with the production fan maths across a full sweep and lost-sensor conditions", () => {
  const temperatures = [null, 0, 19.9, ...Array.from({length: 81}, (_, i) => 20 + i), 42.5, 89.99, 90.01, 105];
  const cases = temperatures.flatMap(temp => [null, 20, 57, 99].flatMap(previous =>
    [false, true].map(hot => ({temp, previous, hot}))));
  const oracle = spawnSync(process.env.PYTHON || "python", ["-c", `
import json, sys
from airglow import fans
points, cases = json.load(sys.stdin)
out = []
for case in cases:
    t, last = case["temp"], case["previous"]
    target = fans.target_duty(points, t, 35)
    if t is None: target = max(target, last or 0)
    if case["hot"]: target = 100
    command = round(target) if t is None else fans.step_toward(last, target, 5)
    out.append({"target": target, "next": command})
print(json.dumps(out))
`], { input: JSON.stringify([points, cases]), encoding: "utf8", cwd: fileURLToPath(new URL("../", import.meta.url)) });
  assert.equal(oracle.status, 0, oracle.stderr);
  const commands = cases.map(c => demand(points, c.temp, {...policy, previous: c.previous, hot: c.hot}));
  assert.deepEqual(commands, JSON.parse(oracle.stdout));
});
