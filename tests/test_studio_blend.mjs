import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

const source = await readFile(new URL("../web/js/studio-blend-state.js", import.meta.url), "utf8");
const m = await import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`);
const effect = name => ({ kind: "code", fx_id: `fx:mine.${name}`, name, source: `<title>${name}</title><meta name="airglow-effect" content="1"><script>airglow.effect({point(){return [0,0,0]}})</script>` });
const memory = () => { const values = new Map(); return { getItem: key => values.get(key), setItem: (key, value) => values.set(key, value) }; };
const payload = () => ({ a: m.snapshotEffect(effect("A")), b: m.snapshotEffect(effect("B")), name: "Dusk", amount: 50 });
const deferred = () => { let resolve, reject; const promise = new Promise((a, b) => { resolve = a; reject = b; }); return { promise, resolve, reject }; };
const flush = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };

test("blend recovery stores endpoint copies, survives original edits/deletion, rejects corrupt and nested recovery", () => {
  const storage = memory(), store = m.createBlendStorage(storage), original = effect("A"), state = payload();
  state.a = m.snapshotEffect(original); assert.equal(store.write(state), true);
  original.source = "changed"; state.a.name = "changed";
  const restored = store.read(); assert.equal(restored.a.name, "A"); assert.ok(restored.a.source.includes("airglow-effect"));
  for (const invalid of ["no JSON", "x".repeat(150001), JSON.stringify({ version: 2 }),
    JSON.stringify({ version: 1, ...payload(), amount: -1 }), JSON.stringify({ version: 1, ...payload(), a: { ...restored.a, source: m.BLEND_MARKER } })]) {
    storage.setItem(m.BLEND_KEY, invalid); assert.equal(store.read(), null);
  }
  assert.equal(m.createBlendStorage(null).write(payload()), false);
  assert.throws(() => m.snapshotEffect({ ...effect("A"), source: m.BLEND_MARKER }), /nested/);
});

test("latest preview generation wins; stale build, callbacks and post-close completion cannot restart or repaint", async () => {
  const requests = [], runners = [], paints = [], errors = [];
  const preview = m.createBlendPreview({ points: [], build: body => { const next = deferred(); requests.push({ ...next, body }); return next.promise; },
    createRunner: h => { const runner = { h, stopped: 0, values: [], load: async () => {}, destroy() { this.stopped++; }, setParams(p) { this.values.push(p); }, tick() {} }; runners.push(runner); return runner; },
    onFrame: data => paints.push(data), onError: error => errors.push(error),
  });
  const first = preview.load(payload()); const second = preview.load({ ...payload(), amount: 70 });
  requests[1].resolve({ effect: { code: "second" } }); await second;
  assert.equal(runners.length, 1);
  requests[0].resolve({ effect: { code: "obsolete" } }); await first;
  assert.equal(runners.length, 1);
  const third = preview.load(payload());
  runners[0].h.onFrame("old"); runners[0].h.onError({ message: "old error" });
  assert.deepEqual(paints, []); assert.deepEqual(errors, []);
  preview.setAmount(23); requests[2].resolve({ effect: { code: "third" } }); await third;
  assert.equal(runners[1].values.at(-1).blend, 23);
  runners[1].h.onFrame("current"); assert.deepEqual(paints, ["current"]);
  const fourth = preview.load(payload()); preview.destroy(); requests[3].resolve({ effect: { code: "closed" } }); await fourth;
  assert.equal(runners.length, 2);
  runners[1].h.onFrame("after close"); assert.deepEqual(paints, ["current"]);
});

test("closing during native preview load destroys its completed lease and changing the slider survives startup", async () => {
  const pending = deferred(); let stopped = 0, parameters = [];
  const preview = m.createBlendPreview({ points: [], build: async () => ({ effect: { code: "test" } }),
    createRunner: () => ({ load: () => pending.promise, destroy: () => stopped++, setParams: p => parameters.push(p) }), onFrame() {} });
  const task = preview.load(payload()); await flush(); preview.setAmount(88); preview.destroy(); pending.resolve(); await task;
  assert.equal(stopped, 2); assert.deepEqual(parameters, [{ blend: 88 }]);
});

test("the blend panel builds/previews/saves but has no lighting mutation route", async () => {
  const ui = await readFile(new URL("../web/js/studio-blend.js", import.meta.url), "utf8");
  assert.ok(ui.includes('/api/studio/save'));
  assert.ok(!/\/api\/(?:scene|studio\/(?:draft|use))/.test(ui));
});
