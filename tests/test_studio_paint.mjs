import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import vm from "node:vm";

const source = await readFile(new URL("../web/js/studio-paint.js", import.meta.url), "utf8").catch(() => "");
const m = await import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`);
const zones = [{ key: "front", spot: "front", zone: { name: "Front" }, positions: [[.16, .5], [.8, .5], [1.44, .5]] },
  { key: "rear", spot: "rear", positions: [[.8, .8]] }];

test("a fast stroke selects every crossed LED while respecting the selected area", () => {
  assert.equal(typeof m.strokeMask, "function");
  const points = m.paintPoints(zones, "front");
  const paint = m.strokeMask(undefined, points, { x: 10, y: 50 }, { x: 150, y: 50 }, 4, false, { ox: 0, oy: 0, s: .1, u: 1000 });
  assert.deepEqual(paint, { v: 1, points: [[1000, 5000], [5000, 5000], [9000, 5000]] });
  assert.deepEqual(m.strokeMask(paint, points, { x: 80, y: 50 }, { x: 80, y: 50 }, 6, true, { ox: 0, oy: 0, s: .1, u: 1000 }).points,
    [[1000, 5000], [9000, 5000]]);
});

test("erase begins with the current visible LEDs and invert respects the area without mutating input", () => {
  assert.equal(typeof m.invertMask, "function");
  const points = m.paintPoints(zones, "front");
  const paint = m.strokeMask(undefined, points, { x: 80, y: 50 }, { x: 80, y: 50 }, 6, true, { ox: 0, oy: 0, s: .1, u: 1000 });
  assert.deepEqual(paint.points, [[1000, 5000], [9000, 5000]]);
  assert.deepEqual(m.invertMask(paint, points).points, [[5000, 5000]]);
  assert.deepEqual(paint.points, [[1000, 5000], [9000, 5000]]);
  assert.equal(m.paintPoints([{ positions: [[.8, .5], [.8, .5]] }]).length, 2, "numbered LEDs remain individually discoverable even at one position");
});

test("paint rejects malformed or excessive data and preserves exact off-stage positions", () => {
  assert.equal(typeof m.cleanPaint, "function");
  for (const value of [null, {}, { v: 2, points: [] }, { v: 1, points: [[1.2, 3]] }, { v: 1, points: [[100001, 0]] }, { v: 1, points: [[true, 2]] },
    { v: 1, points: [[1, 2], [1, 2]] }, { v: 1, points: Array.from({ length: 2049 }, (_, i) => [i, 0]) }]) assert.throws(() => m.cleanPaint(value));
  assert.deepEqual(m.paintPoints([{ positions: [[-.16, 1.2]] }])[0].pair, [-1000, 12000]);
});

test("a brush click in empty space leaves an unpainted layer unchanged", () => {
  assert.equal(m.strokeMask(undefined, m.paintPoints(zones), { x: 0, y: 0 }, { x: 0, y: 0 }, 4, false,
    { ox: 0, oy: 0, s: .1, u: 1000 }), undefined);
});

test("browser LED transport carries the same exact mask position as the hardware host", async () => {
  const code = await readFile(new URL("../web/js/fxrun.js", import.meta.url), "utf8");
  const fx = await import(`data:text/javascript;base64,${Buffer.from(code).toString("base64")}`);
  const pts = fx.ledPoints([{ key: "fixture", positions: [[.1235, .2346], [-.16, 1.2]] }]).points;
  assert.deepEqual(pts.map(p => p[6]), [[772, 2346], [-1000, 12000]]);
});

test("pointer capture makes one history commit; cancel restores the original optional mask", () => {
  assert.equal(typeof m.bindPaintCanvas, "function");
  class Canvas extends EventTarget {
    captured = null;
    getBoundingClientRect() { return { left: 40, top: 20 }; }
    setPointerCapture(id) { this.captured = id; }
    hasPointerCapture(id) { return this.captured === id; }
    releasePointerCapture() { this.captured = null; }
  }
  const canvas = new Canvas(), layer = { mask: "front" }, events = [];
  const binding = m.bindPaintCanvas(canvas, { getLayer: () => layer, getPoints: () => m.paintPoints(zones, layer.mask),
    getMap: () => ({ ox: 0, oy: 0, s: .1, u: 1000 }), getSettings: () => ({ enabled: true, radius: 6, erase: false }),
    onBegin: () => events.push("begin"), onChange: () => events.push("change"), onCommit: () => events.push("commit"), onCancel: () => events.push("cancel") });
  const fire = (type, x, y) => { const e = new Event(type, { cancelable: true }); Object.assign(e, { pointerId: 7, button: 0, clientX: x, clientY: y }); canvas.dispatchEvent(e); };
  fire("pointerdown", 56, 70); fire("pointermove", 184, 70); fire("pointerup", 184, 70);
  assert.equal(canvas.captured, null);
  assert.equal(events.filter(x => x === "commit").length, 1);
  assert.deepEqual(layer.paint.points, [[1000, 5000], [5000, 5000], [9000, 5000]]);
  delete layer.paint; events.length = 0;
  fire("pointerdown", 56, 70); fire("pointercancel", 56, 70);
  assert.equal(Object.hasOwn(layer, "paint"), false);
  assert.deepEqual(events, ["begin", "change", "cancel"]);
  binding.destroy();
});

test("the real renderer intersects painted positions with area and leaves old layers unchanged", async () => {
  const context = vm.createContext({ self: {} });
  vm.runInContext(await readFile(new URL("../web/runtime/layers.js", import.meta.url), "utf8"), context);
  const render = layer => { const effect = context.self.AirglowLayers.effect([layer]); effect.init({ params: {} }); effect.frame({ time: 0, dt: .016 }); return pt => Array.from(effect.point(pt, {})); };
  const base = { type: "solid", params: { color: "#ff0000" }, mask: "front" };
  const point = { x: .1, y: .5, area: "front", index: 0 };
  assert.deepEqual(render(base)(point), [255, 0, 0]);
  const painted = render({ ...base, paint: { v: 1, points: [[1000, 5000]] } });
  assert.deepEqual(painted(point), [255, 0, 0]);
  assert.deepEqual(painted({ ...point, x: .5 }), [0, 0, 0]);
  assert.deepEqual(painted({ ...point, area: "rear" }), [0, 0, 0]);
  assert.deepEqual(render({ ...base, paint: { v: 1, points: [] } })(point), [0, 0, 0]);
  assert.deepEqual(render({ ...base, paint: { v: 99, points: [[1000, 5000]] } })(point), [0, 0, 0], "invalid imported paint fails closed");
  assert.deepEqual(painted({ ...point, x: 0, y: 1, paintPosition: [1000, 5000] }), [255, 0, 0], "paint position survives hardware coordinate clamping");
});

test("the actual worker transports painted coordinates through initial load and updated points", async () => {
  const sent = [], context = vm.createContext({ self: { postMessage: message => sent.push(message) }, performance,
    console: { log() {}, info() {}, debug() {}, warn() {}, error() {} },
    OffscreenCanvas: class { getContext() { return {}; } } });
  vm.runInContext(await readFile(new URL("../web/runtime/worker.js", import.meta.url), "utf8"), context);
  vm.runInContext(await readFile(new URL("../web/runtime/layers.js", import.meta.url), "utf8"), context);
  vm.runInContext('self.airglow.effect(self.AirglowLayers.effect([{type:"solid",params:{color:"#00ff00"},paint:{v:1,points:[[-1000,12000]]}}]));', context);
  await context.self.onmessage({ data: { type: "start", params: {}, points: [[0, 200, "front", 0, 0, "outside", [-1000, 12000]]] } });
  await context.self.onmessage({ data: { type: "tick", time: 0, dt: .016 } });
  assert.deepEqual(Array.from(sent.at(-1).colors), [0, 255, 0]);
  await context.self.onmessage({ data: { type: "points", points: [[0, 200, "front", 0, 0, "outside", [0, 10000]]] } });
  await context.self.onmessage({ data: { type: "tick", time: .016, dt: .016 } });
  assert.deepEqual(Array.from(sent.at(-1).colors), [0, 0, 0]);
  await context.self.onmessage({ data: { type: "points", points: [[-32, 240, "front", 0, 0, "legacy"]] } });
  await context.self.onmessage({ data: { type: "tick", time: .032, dt: .016 } });
  assert.deepEqual(Array.from(sent.at(-1).colors), [0, 255, 0], "legacy six-field tuples still render using their own coordinates");
});

test("paint masks and the preview-only guard survive document recovery and undo/redo", async () => {
  const code = await readFile(new URL("../web/js/studio-document.js", import.meta.url), "utf8");
  const doc = await import(`data:text/javascript;base64,${Buffer.from(code).toString("base64")}`);
  const layer = { id: "one", type: "solid", mask: "front", paint: { v: 1, points: [[1000, 5000]] } };
  const d = doc.newDoc({ name: "Painted", layers: [layer], paintPending: true }), values = new Map();
  const store = doc.createDraftStorage({ getItem: k => values.get(k), setItem: (k, v) => values.set(k, v), removeItem: k => values.delete(k) }, () => 1000);
  assert.equal(store.write(d), true); const recovered = store.read();
  assert.deepEqual(recovered.layers[0].paint, { v: 1, points: [[1000, 5000]] });
  assert.equal(recovered.layers[0].mask, "front"); assert.equal(recovered.paintPending, true);
  const history = doc.createHistory(); history.reset(doc.snapDoc(recovered));
  recovered.layers[0].paint.points.push([9000, 5000]); history.push(doc.snapDoc(recovered), 1000);
  assert.deepEqual(JSON.parse(history.go(-1)).layers[0].paint.points, [[1000, 5000]]);
  assert.deepEqual(JSON.parse(history.go(1)).layers[0].paint.points, [[1000, 5000], [9000, 5000]]);
});
