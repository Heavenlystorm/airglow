import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

function layers() {
  const context = vm.createContext({ self: {}, Float32Array });
  vm.runInContext(readFileSync(new URL("../web/runtime/layers.js", import.meta.url), "utf8"), context);
  return context.self.AirglowLayers;
}

test("spectrum puts the middle band at the rig center for every direction", () => {
  for (const angle of [0, 45, 135, 180, 225, 270, 315]) {
    const r = layers().TYPES.spectrum({ angle, sensitivity: 100, hue: 0 });
    const color = r.at(.5, .5, 0, { audio: { bands: [0, 0, 1, 0] } });
    assert.ok(color[1] * color[3] > .9, `middle band at ${angle} degrees`);
  }
});

test("reverse Chase retains its moving bands after many complete cycles", () => {
  const lib = layers();
  for (const reverse of [false, true]) {
    const r = lib.TYPES.chase({ color: "#ffffff", count: 3, width: 50, speed: 100, reverse });
    for (const time of [0, 2, 10, 120, 3600]) {
      const strengths = Array.from({ length: 120 }, (_, i) => r.at(i / 120, 0, i, { time }, { local: i / 120 })[3]);
      assert.ok(strengths.filter(a => a > 0).length >= 45, `${reverse ? "reverse" : "forward"} at ${time}s`);
      assert.ok(strengths.filter(a => a === 0).length >= 45, "the chase still has gaps between its bands");
    }
  }
});

test("audio speed reaction advances from the preceding phase rather than jumping at long elapsed times", () => {
  const lib = layers(), seen = [];
  lib.TYPES.clockProbe = () => ({ frame: f => seen.push({ ...f }), at: () => [1, 1, 1, 1] });
  const fx = lib.effect({ layers: [{ id: "clock", type: "clockProbe", react: { what: "speed", amount: 100 } }] });
  fx.init({ params: {} });
  fx.frame({ time: 600, dt: 1 / 30, audio: { bass: 0, beat: false } });
  fx.frame({ time: 600 + 1 / 30, dt: 1 / 30, audio: { bass: 1, beat: false } });
  assert.ok(Math.abs(seen[1].time - seen[0].time - 2 / 30) < 1e-7);
  fx.frame({ time: 600 + 2 / 30, dt: 1 / 30, audio: { bass: 0, beat: false } });
  assert.ok(Math.abs(seen[2].time - seen[1].time - 1 / 30) < 1e-7);
});

test("parameter changes keep layer motion continuous and rewinding resets its clock", () => {
  const lib = layers(), seen = [];
  lib.TYPES.clockProbe = () => ({ frame: f => seen.push(f.time), at: () => [1, 1, 1, 1] });
  const fx = lib.effect({ layers: [{ id: "clock", type: "clockProbe", react: { what: "speed", amount: 100 } }] });
  fx.init({ params: {} });
  fx.frame({ time: 50, dt: .1, audio: { bass: 1 } });
  fx.frame({ time: 50.1, dt: .1, audio: { bass: 1 } });
  fx.onParamChange("m_brightness", 70, { params: { m_brightness: 70 } });
  fx.frame({ time: 50.2, dt: .1, audio: { bass: 0 } });
  assert.ok(Math.abs(seen[2] - seen[1] - .1) < 1e-7);
  fx.frame({ time: 0, dt: 0, audio: { bass: 0 } });
  assert.equal(seen[3], 0);
});

test("coalesced or dropped forward ticks preserve accumulated audio phase", () => {
  const lib = layers(), seen = [];
  lib.TYPES.clockProbe = () => ({ frame: f => seen.push(f.time), at: () => [1, 1, 1, 1] });
  const fx = lib.effect({ layers: [{ id: "clock", type: "clockProbe", react: { what: "speed", amount: 100 } }] });
  fx.init({ params: {} });
  fx.frame({ time: 100, dt: .1, audio: { bass: 1 } });
  fx.frame({ time: 100.1, dt: .1, audio: { bass: 1 } });
  fx.frame({ time: 100.3, dt: .1, audio: { bass: 0 } });
  assert.ok(Math.abs(seen[2] - seen[1] - .2) < 1e-7, "a missed tick advances elapsed phase rather than resetting it");
});
