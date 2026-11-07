import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { test } from "node:test";

const file = new URL("../web/js/rig-3d.js", import.meta.url);
let mod;
async function viewer() {
  assert.ok(existsSync(file), "The lazy 3D renderer is implemented");
  return mod ||= import(`data:text/javascript;base64,${readFileSync(file).toString("base64")}`);
}
const zone = (key, positions, layout = {}) => ({ key, device: { online: true }, positions,
  config: { enabled: true, name: key, layout: { shape: "strip", x: .8, y: .5, length: .8, angle: 0, ...layout } } });

test("orbit rotates real depth and perspective makes near points larger", async () => {
  const { projectPoint } = await viewer();
  const scene = { center: [0, 0, 0], radius: 1 };
  const camera = { yaw: 0, pitch: 0, distance: 3, panX: 0, panY: 0 };
  const near = projectPoint([.5, 0, 1], scene, camera, 800, 600);
  const far = projectPoint([.5, 0, -1], scene, camera, 800, 600);
  assert.ok(near.x > far.x && near.scale > far.scale, "depth affects position and LED size");
  const turned = projectPoint([0, 0, 1], scene, { ...camera, yaw: Math.PI / 2 }, 800, 600);
  assert.ok(turned.x > 400 && Math.abs(turned.depth - 3) < 1e-8, "orbit turns Z into screen X");
});

test("scene uses configured geometry, excludes offline zones, and never changes source layouts", async () => {
  const { buildScene } = await viewer();
  const zones = [zone("point", [[.3, .4]]), zone("panel", [[.8, .5]], { shape: "matrix", cols: 4, rows: 2, length: .4 }),
    zone("fan", [[1.3, .5]], { shape: "fans", x: 1.2, y: .5, size: .2, fans: 1 })];
  const off = zone("offline", [[9, 9]]); off.device.online = false; zones.push(off);
  const before = structuredClone(zones);
  const scene = buildScene(zones, { fan: .3 });
  assert.equal(scene.leds.length, 3);
  assert.equal(scene.leds.find(x => x.key === "fan").point[0], 1.3);
  assert.ok(scene.leds.find(x => x.key === "fan").point[2] > .3);
  const pointGeometry = scene.surfaces.filter(x => x.key === "point").flatMap(x => x.points);
  assert.ok(pointGeometry.every(p => Math.abs(p[0] - .3) < .04), "one LED cannot create a nominal long strip");
  const panelGeometry = scene.surfaces.filter(x => x.key === "panel").flatMap(x => x.points);
  assert.ok(panelGeometry.some(p => Math.abs(p[0] - .6) < 1e-8), "matrix uses configured panel extents");
  assert.ok(new Set(scene.surfaces.flatMap(x => x.points.map(p => p[2]))).size > 2, "geometry has volume");
  assert.deepEqual(zones, before);
  assert.equal(buildScene([]).leds.length, 0);
});

test("LED colors come from the indexed live frame and incomplete frames stay unlit", async () => {
  const { frameColor } = await viewer();
  assert.equal(frameColor({ rig: "ff00000123ab" }, "rig", 0), "#ff0000");
  assert.equal(frameColor({ rig: "ff00000123ab" }, "rig", 1), "#0123ab");
  assert.equal(frameColor({ rig: "ff0000ff" }, "rig", 1), null);
  assert.equal(frameColor({ rig: "nothex" }, "rig", 0), null);
  assert.equal(frameColor({}, "rig", 0), null);
});

test("stored depths reject corrupt or inherited values and stay inside presentation bounds", async () => {
  const { parseDepths } = await viewer();
  const depths = parseDepths('{"rig":0.2,"far":99,"bad":"0.3","__proto__":{"polluted":true}}');
  assert.equal(depths.rig, .2);
  assert.equal(depths.far, .6);
  assert.equal(depths.bad, undefined);
  assert.equal(depths.polluted, undefined);
  assert.equal(Object.keys(parseDepths("broken")).length, 0);
});

test("render queue coalesces live frames, stops hidden work, and never starts an animation loop", async () => {
  const { createRenderQueue } = await viewer();
  let visible = true, paints = 0, sequence = 0;
  const pending = new Map();
  const queue = createRenderQueue({ isVisible: () => visible, paint: () => paints++, request: fn => { pending.set(++sequence, fn); return sequence; }, cancel: id => pending.delete(id) });
  queue.request(); queue.request();
  assert.equal(pending.size, 1);
  const first = [...pending.values()][0]; pending.clear(); first();
  assert.equal(paints, 1); assert.equal(pending.size, 0, "no automatic orbit or continuous frames");
  queue.request(); visible = false; queue.stop();
  assert.equal(pending.size, 0);
  queue.request(); assert.equal(pending.size, 0);
  visible = true; queue.request(); visible = false;
  const stale = [...pending.values()][0]; pending.clear(); stale();
  assert.equal(paints, 1, "visibility is checked again at paint time");
});
