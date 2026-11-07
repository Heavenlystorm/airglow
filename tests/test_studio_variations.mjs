import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

const source = await readFile(new URL("../web/js/studio-variations.js", import.meta.url), "utf8").catch(() => "");
const m = await import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`);
const master = () => ({ palette: ["#ff0000", "#0000ff"], speed: 100, brightness: 60, react: 50 });
const doc = (id = "fx:mine.sun") => ({ fx_id: id, mode: "layers", name: "Sun", layers: [{ id: "0", type: "wave", params: { width: 20 } }], master: master() });
const storage = () => { const data = new Map(); return { data, getItem: k => data.get(k), setItem: (k, v) => data.set(k, v) }; };

test("restrained variations hold locked controls and never mutate the document or layer recipe", () => {
  assert.equal(typeof m.varyMaster, "function", "variation generator is available");
  const d = doc(), before = structuredClone(d);
  const varied = m.varyMaster(d.master, { palette: true, brightness: true }, () => 1);
  assert.deepEqual(varied.palette, ["#ff0000", "#0000ff"]);
  assert.equal(varied.brightness, 60);
  assert.ok(varied.speed > 100 && varied.speed <= 130);
  assert.ok(varied.react > 50 && varied.react <= 65);
  assert.deepEqual(d, before);
  assert.deepEqual(m.varyMaster(d.master, { palette: true, speed: true, brightness: true, react: true }), d.master);
});

test("capture and restore isolate copies, reject another document, and leave its metadata and recipe intact", () => {
  assert.equal(typeof m.captureVariation, "function");
  const d = doc(), snap = m.captureVariation(d);
  d.master.speed = 160; d.name = "Renamed"; d.layers[0].params.width = 40;
  assert.equal(m.restoreVariation(d, snap), true);
  assert.equal(d.master.speed, 100); assert.equal(d.name, "Renamed"); assert.equal(d.layers[0].params.width, 40);
  d.master.palette[0] = "#112233"; assert.equal(snap.master.palette[0], "#ff0000");
  const other = doc("fx:mine.moon"); other.master.speed = 190;
  assert.equal(m.restoreVariation(other, snap), false); assert.equal(other.master.speed, 190);
});

test("A/B blends bounded continuous values, holds locks, and declines mismatched palette stops or documents", () => {
  assert.equal(typeof m.interpolateVariation, "function");
  const d = doc(), a = m.captureVariation(d);
  d.master = { palette: ["#0000ff", "#ff0000"], speed: 200, brightness: 100, react: 0 };
  const b = m.captureVariation(d);
  assert.deepEqual(m.interpolateVariation(d, a, b, 0.5), { palette: ["#800080", "#800080"], speed: 150, brightness: 80, react: 25 });
  d.master.brightness = 23;
  assert.equal(m.interpolateVariation(d, a, b, 0.5, { brightness: true }).brightness, 23);
  assert.deepEqual(m.interpolateVariation(d, a, b, -4), a.master);
  assert.deepEqual(m.interpolateVariation(d, a, b, 7), b.master);
  assert.equal(m.interpolateVariation(doc("fx:mine.other"), a, b, 0.5), null);
  assert.equal(m.interpolateVariation(d, a, { ...b, master: { ...b.master, palette: ["#000000", "#888888", "#ffffff"] } }, 0.5), null);
  assert.equal(m.interpolateVariation(d, a, b, NaN), null);
});

test("variation persistence is document scoped, bounded, validated, and reports failed writes", () => {
  assert.equal(typeof m.createVariationStore, "function");
  const s = storage(), bank = m.createVariationStore(s), d = doc();
  let entries = [];
  for (let i = 0; i < 15; i++) entries = m.addVariation(entries, `Take ${i}`, m.captureVariation(d));
  assert.equal(entries.length, 8); assert.equal(entries[0].name, "Take 14");
  assert.equal(bank.write(m.documentKey(d), { locks: { speed: true, react: "yes" }, variations: entries }), true);
  const read = bank.read(m.documentKey(d));
  assert.equal(read.variations.length, 8); assert.equal(read.locks.speed, true); assert.equal(read.locks.react, false);
  assert.deepEqual(bank.read("effect:fx:mine.other").variations, []);
  read.variations[0].master.speed = 0; assert.equal(bank.read(m.documentKey(d)).variations[0].master.speed, 100);
  for (let i = 0; i < 30; i++) bank.write(`effect:fx:mine.${i}`, { variations: [] });
  assert.ok(JSON.parse(s.getItem(m.VARIATION_KEY)).documents.length <= 24);
  s.setItem(m.VARIATION_KEY, JSON.stringify({ version: 1, documents: [{ key: m.documentKey(d), locks: {}, variations: [{ id: "unsafe", name: "Unsafe", scope: m.documentKey(d), master: { ...master(), speed: 9000 } }] }] }));
  assert.deepEqual(bank.read(m.documentKey(d)).variations, []);
  s.setItem(m.VARIATION_KEY, "{oops"); assert.deepEqual(bank.read(m.documentKey(d)).variations, []);
  s.setItem = () => { throw new Error("full"); }; assert.equal(bank.write(m.documentKey(d), { variations: entries }), false);
});

test("unsaved documents get distinct recoverable keys and first save moves only that document's bank", () => {
  assert.equal(typeof m.documentKey, "function");
  const d = doc(null), other = doc(null), key = m.documentKey(d), otherKey = m.documentKey(other);
  assert.notEqual(key, otherKey); assert.equal(m.documentKey(structuredClone(d)), key);
  const s = storage(), bank = m.createVariationStore(s);
  const entries = m.addVariation([], "Original", m.captureVariation(d));
  bank.write(key, { variations: entries }); bank.write(otherKey, { variations: [] });
  d.fx_id = "fx:mine.saved";
  assert.equal(bank.move(key, m.documentKey(d)), true);
  const restored = bank.read(m.documentKey(d)).variations[0];
  d.master.speed = 170; assert.equal(m.restoreVariation(d, restored), true); assert.equal(d.master.speed, 100);
  assert.deepEqual(bank.read(key).variations, []); assert.deepEqual(bank.read(otherKey).variations, []);
});

test("scene animation requires a visible page and obeys either motion preference", () => {
  assert.equal(typeof m.canPlayScene, "function");
  assert.equal(m.canPlayScene({ hidden: false, reducedSystem: false, motion: "system" }), true);
  assert.equal(m.canPlayScene({ hidden: true }), false);
  assert.equal(m.canPlayScene({ reducedSystem: true }), false);
  assert.equal(m.canPlayScene({ motion: "reduced" }), false);
});
