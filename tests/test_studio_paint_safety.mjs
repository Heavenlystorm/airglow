import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

async function harness() {
  const requests = [], timers = new Map(); let timer = 0;
  const core = { $: () => null, $$: () => [], S: {}, esc: String, emit() {}, fail() {}, on() {}, toast() {}, rangeFill() {},
    coalesce: fn => fn, post: async (path, body) => { requests.push({ path, body }); return { effect: { fx_id: "fx:mine.test" } }; }, get: async () => ({}) };
  const context = vm.createContext({ structuredClone, performance, console, TextEncoder,
    localStorage: { getItem: () => null, setItem() {}, removeItem() {} },
    setTimeout(fn) { timers.set(++timer, fn); return timer; }, clearTimeout(id) { timers.delete(id); } });
  const real = ["studio", "studio-document", "studio-preview", "studio-paint", "studio-variations"];
  const sources = new Map(real.map(name => [`./${name}.js`, readFileSync(new URL(`../web/js/${name}.js`, import.meta.url), "utf8")
    + (name === "studio" ? "\nrerenderSides = () => {};\nexport { E, LIVE, HIST, snapDoc, histGo, changed, save, scheduleSave, beginPaintEdit, finishPaintEdit, cancelPaintEdit };" : "")]));
  const imports = new Map(), modules = new Map();
  for (const source of sources.values()) for (const match of source.matchAll(/import\s+\{([^}]+)\}\s+from\s+["']([^"']+)["']/g)) {
    const names = imports.get(match[2]) || new Set(); match[1].split(",").forEach(name => names.add(name.trim().split(/\s+as\s+/)[0])); imports.set(match[2], names);
  }
  function moduleFor(specifier) {
    if (modules.has(specifier)) return modules.get(specifier);
    const module = sources.has(specifier) ? new vm.SourceTextModule(sources.get(specifier), { context })
      : new vm.SyntheticModule([...imports.get(specifier)], function () {
        for (const name of imports.get(specifier)) this.setExport(name, specifier === "./core.js" ? core[name]
          : name === "createStudioGallery" ? () => ({}) : () => {});
      }, { context });
    modules.set(specifier, module); return module;
  }
  const module = moduleFor("./studio.js"); await module.link(moduleFor); await module.evaluate();
  const studio = module.namespace;
  studio.E.doc = { fx_id: "fx:mine.test", mode: "layers", name: "Test", layers: [{ id: "one", type: "solid", params: {} }], master: {}, expose: {}, values: {}, tags: [], dirty: true, paintPending: true };
  studio.LIVE.state.wanted = true;
  return { studio, requests, timers };
}

test("painting blocks both queued live syncing and quiet saves until an explicit action", async () => {
  const h = await harness();
  h.studio.changed(true); await h.studio.save({ quiet: true }); await Promise.resolve();
  assert.deepEqual(h.requests, [], "preview-only edits cannot upload a live draft or replace an active saved effect");
  assert.equal(h.studio.E.auto, 0, "painting cannot schedule an autosave");
});

test("a painting gesture makes one undo snapshot and cancel restores dirty and pending flags", async () => {
  const h = await harness(), s = h.studio;
  assert.equal(typeof s.beginPaintEdit, "function");
  s.E.doc.dirty = false; s.E.doc.paintPending = false;
  s.HIST.reset(JSON.stringify({ layers: s.E.doc.layers }));
  s.beginPaintEdit(); s.E.doc.layers[0].paint = { v: 1, points: [[1, 2]] }; s.changed(true, { hist: false, auto: false });
  s.E.doc.layers[0].paint.points.push([3, 4]); s.changed(true, { hist: false, auto: false }); s.finishPaintEdit();
  assert.equal(s.HIST.stack.length, 2);
  assert.equal(s.E.doc.paintPending, true);
  s.E.doc.dirty = false; s.E.doc.paintPending = false;
  s.beginPaintEdit(); s.changed(true, { hist: false, auto: false }); s.cancelPaintEdit();
  assert.equal(s.E.doc.dirty, false); assert.equal(s.E.doc.paintPending, false);
  await Promise.resolve(); assert.deepEqual(h.requests, []);
});

test("an explicit successful Save persists paint and clears the automatic-save guard", async () => {
  const h = await harness(), s = h.studio;
  s.E.doc.layers[0].paint = { v: 1, points: [[1250, 2500]] };
  await s.save();
  assert.equal(h.requests.length, 1); assert.equal(h.requests[0].path, "/api/studio/save");
  assert.equal(JSON.stringify(h.requests[0].body.layers[0].paint), '{"v":1,"points":[[1250,2500]]}');
  assert.equal(s.E.doc.paintPending, false); assert.equal(s.E.doc.dirty, false);
});

test("undo and redo of saved paint return to preview-only editing and stop live syncing", async () => {
  const h = await harness(), s = h.studio;
  s.LIVE.state.wanted = false;
  s.E.doc.layers[0].paint = { v: 1, points: [[1, 2]] };
  s.HIST.reset(s.snapDoc());
  s.E.doc.layers[0].paint.points.push([3, 4]);
  s.HIST.push(s.snapDoc(), 1000);
  await s.save(); h.requests.length = 0;
  s.LIVE.state.wanted = true;
  await s.histGo(-1); await s.save({ quiet: true }); await Promise.resolve();
  assert.equal(s.E.doc.paintPending, true); assert.equal(s.E.auto, 0);
  assert.equal(s.E.doc.layers[0].paint.points.length, 1);
  assert.equal(s.LIVE.state.wanted, false);
  assert.ok(h.requests.every(r => r.path === "/api/studio/draft" && r.body.lights === false));
  await s.save(); h.requests.length = 0;
  await s.histGo(1); await s.save({ quiet: true });
  assert.equal(s.E.doc.paintPending, true); assert.equal(s.E.auto, 0);
  assert.equal(s.E.doc.layers[0].paint.points.length, 2);
  assert.deepEqual(h.requests, []);
});
