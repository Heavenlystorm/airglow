// Exercise the actual renderer with a recording Canvas context, without a browser or hardware.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

async function renderer(rig = "frame") {
  const operations = [], gradients = [];
  const ctx = new Proxy({
    createRadialGradient(...args) { gradients.push(args); return { addColorStop() {} }; },
    createLinearGradient() { return { addColorStop() {} }; },
    arc(...args) { if (args[2] < 0) throw new RangeError("negative arc radius"); operations.push(["arc", ...args]); },
  }, { get(target, key) { return key in target ? target[key] : (...args) => operations.push([key, ...args]); },
    set(target, key, value) { target[key] = value; operations.push([key, value]); return true; } });
  const canvas = { width: 0, height: 0, getBoundingClientRect: () => ({ width: 640, height: 400 }), getContext: () => ctx };
  const zones = [{ key: "fan", device: { online: true }, positions: [[.4, .4], [.5, .5]],
    config: { enabled: true, layout: { shape: "fans", x: .45, y: .45, fans: 1, leds_per_fan: 8, size: .2, angle: 0 } } }];
  const context = vm.createContext({ document: { documentElement: { dataset: { rig } } }, window: { devicePixelRatio: 1 } });
  const core = { S: { zones: { zones } }, clamp: (n, lo, hi) => Math.max(lo, Math.min(hi, n)) };
  const source = readFileSync(new URL("../web/js/stage.js", import.meta.url), "utf8");
  const names = source.match(/import\s*\{([^}]+)\}/)[1].split(",").map(x => x.trim());
  const module = new vm.SourceTextModule(source, { context });
  await module.link(() => new vm.SyntheticModule(names, function () {
    for (const name of names) this.setExport(name, core[name] || (() => {}));
  }, { context }));
  await module.evaluate();
  return { ...module.namespace, canvas, zones, operations, gradients };
}

test("frame presentation adds neutral geometry without changing the emitted LED colors", async () => {
  const framed = await renderer(), minimal = await renderer("minimal");
  for (const h of [framed, minimal]) h.paintRig(h.canvas, { fan: "ff000000ff00" });
  const colors = h => h.operations.filter(([op, value]) => op === "fillStyle" && typeof value === "string" && value.startsWith("rgb("));
  assert.deepEqual(colors(framed), colors(minimal));
  assert.equal(colors(framed).length, 2);
  assert.ok(framed.operations.some(([op]) => op === "stroke"), "frame draws a neutral portrait and zone contours");
  assert.ok(!minimal.operations.some(([op]) => op === "stroke"), "minimal contains only LEDs");
});

test("empty layouts draw no invented rig and rendering never mutates zone geometry", async () => {
  const h = await renderer(), before = structuredClone(h.zones);
  h.paintRig(h.canvas, {}, { zones: [] });
  assert.ok(!h.operations.some(([op]) => op === "stroke"));
  h.paintRig(h.canvas, { fan: "ff000000ff00" });
  assert.deepEqual(h.zones, before);
});

test("short lighting tiles retain positive geometry when frame padding exceeds their height", async () => {
  const h = await renderer();
  h.canvas.getBoundingClientRect = () => ({ width: 180, height: 35 });
  assert.doesNotThrow(() => h.paintRig(h.canvas, { fan: "ff000000ff00" }, { pad: 42 }));
});

test("single-LED zones get a compact point mount rather than a long fictional strip", async () => {
  const h = await renderer();
  Object.assign(h.zones[0], { positions: [[.8, .5]], config: { enabled: true,
    layout: { shape: "strip", x: .8, y: .5, leds: 1, angle: 0, length: .6 } } });
  h.paintRig(h.canvas, { fan: "ff0000" });
  const frame = h.operations.find(([op]) => op === "roundRect");
  const [, x, , width] = frame;
  const linePoints = h.operations.filter(([op]) => op === "moveTo" || op === "lineTo");
  assert.ok(linePoints.every(([, px]) => px >= x && px <= x + width), "every mount stroke fits inside the frame");
  assert.ok(width <= 100, "a single point is not presented as a detected long strip");
});

test("matrix frame encompasses configured panel corners even with a sparse LED map", async () => {
  const h = await renderer();
  Object.assign(h.zones[0], { positions: [[.8, .5]], config: { enabled: true,
    layout: { shape: "matrix", x: .8, y: .5, cols: 8, rows: 4, leds: 1, angle: 25, length: .6 } } });
  h.paintRig(h.canvas, { fan: "ff0000" });
  const [, x, y, width, height] = h.operations.find(([op]) => op === "roundRect");
  const points = h.operations.filter(([op]) => op === "moveTo" || op === "lineTo");
  assert.ok(points.every(([, px, py]) => px >= x && px <= x + width && py >= y && py <= y + height));
});
