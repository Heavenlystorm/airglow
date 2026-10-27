// Deferred requests exercise the actual Studio, automation and profile-preview modules.
// Run: node --experimental-vm-modules --test tools/test-ui-races.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

const plain = (value) => JSON.parse(JSON.stringify(value));
const tick = () => new Promise(setImmediate);

async function harness(file, exports) {
  const source = readFileSync(new URL(`../web/js/${file}.js`, import.meta.url), "utf8");
  const timers = new Map(), requests = [], errors = [], paints = [], storage = new Map(), elements = new Map();
  let nextTimer = 0;
  const element = (id) => {
    if (!elements.has(id)) elements.set(id, {
      events: new Map(), open: true, classList: { toggle() {} },
      addEventListener(type, fn) { this.events.set(type, fn); },
    });
    return elements.get(id);
  };
  const document = { visibilityState: "visible", events: new Map(),
    addEventListener(type, fn) { this.events.set(type, fn); } };
  const context = vm.createContext({
    document, window: {}, structuredClone, performance: { now: () => 0 },
    location: { hash: "#studio/edit" }, history: { replaceState() {} },
    localStorage: { getItem: (k) => storage.get(k), setItem: (k, v) => storage.set(k, v), removeItem: (k) => storage.delete(k) },
    sessionStorage: { getItem: () => null }, cancelAnimationFrame() {},
    setTimeout(fn, ms) { timers.set(++nextTimer, { fn, ms }); return nextTimer; },
    setInterval(fn, ms) { timers.set(++nextTimer, { fn, ms, repeat: true }); return nextTimer; },
    clearTimeout(id) { timers.delete(id); }, clearInterval(id) { timers.delete(id); },
  });
  const request = (method, path, body) => new Promise((resolve, reject) => {
    const r = { method, path, body: body === undefined ? undefined : plain(body), settled: false,
      resolve(value) { r.settled = true; resolve(value); }, reject(error) { r.settled = true; reject(error); } };
    requests.push(r);
  });
  const core = {
    S: { state: null, effects: [], presets: [], rules: [] }, $: element, $$: () => [],
    emit() {}, on() {}, toast() {}, coalesce: (fn) => fn, fail: (error) => errors.push(error),
    post: (path, body) => request("POST", path, body),
    get: (path) => path === "/api/effects" ? Promise.resolve([]) : request("GET", path),
  };
  const imports = new Map([...source.matchAll(/import\s+\{([^}]+)\}\s+from\s+["']([^"']+)["']/g)]
    .map((m) => [m[2], m[1].split(",").map((name) => name.trim())]));
  const module = new vm.SourceTextModule(source + `\nexport { ${exports.join(", ")} };`, { context });
  await module.link((specifier) => new vm.SyntheticModule(imports.get(specifier), function () {
    for (const name of imports.get(specifier)) {
      const value = specifier === "./core.js" && name in core ? core[name]
        : name === "paintMini" ? (canvas, zones) => paints.push({ canvas, zones }) : () => {};
      this.setExport(name, value);
    }
  }, { context }));
  await module.evaluate();
  const fire = (ms) => {
    const item = [...timers].find(([, t]) => t.ms === ms);
    assert.ok(item, `a ${ms} ms timer should be scheduled`);
    if (!item[1].repeat) timers.delete(item[0]);
    return item[1].fn();
  };
  const finishPreviewWrites = async () => {
    await tick();
    for (;;) {
      const r = requests.find((r) => !r.settled && r.method === "POST");
      if (!r) break;
      r.resolve({});
      await tick();
    }
  };
  return { ...module.namespace, core, document, element, timers, requests, errors, paints, storage, fire, finishPreviewWrites };
}

const studio = () => harness("studio", ["E", "newDoc", "quietSave", "save", "closeEditor", "DRAFT_KEY"]);
function effect(h, name, fx_id = `fx:me.${name}`) {
  const d = h.newDoc({ name, fx_id, layers: [] });
  d.dirty = true;
  return d;
}
const saved = (r, fx_id = r.body.fx_id) => r.resolve({ effect: { fx_id } });

test("Studio autosave completing after a document switch cannot change the new effect or its draft", async () => {
  const h = await studio();
  h.E.doc = effect(h, "first");
  const pending = h.quietSave();
  await tick();
  h.closeEditor(true);
  const second = h.E.doc = effect(h, "second");
  h.E.saved = "second baseline";
  h.storage.set(h.DRAFT_KEY, "second draft");
  saved(h.requests[0]);
  await pending;
  assert.equal(second.fx_id, "fx:me.second");
  assert.equal(second.dirty, true);
  assert.equal(h.E.saved, "second baseline");
  assert.equal(h.storage.get(h.DRAFT_KEY), "second draft");
  const next = h.save();
  await tick();
  assert.equal(h.requests[1].body.fx_id, "fx:me.second");
  saved(h.requests[1]);
  await next;
});

test("Studio manual and automatic saves serialize immutable snapshots and preserve later edits", async () => {
  const h = await studio(), d = h.E.doc = effect(h, "first");
  const first = h.quietSave();
  await tick();
  d.name = "second";
  const second = h.save();
  d.name = "third";
  await tick();
  assert.equal(h.requests.length, 1);
  assert.equal(h.requests[0].body.name, "first");
  saved(h.requests[0]);
  await first;
  await tick();
  assert.equal(h.requests[1].body.name, "second");
  saved(h.requests[1]);
  await second;
  assert.equal(d.name, "third");
  assert.equal(d.dirty, true, "unsent edits must remain dirty");
  assert.equal(JSON.parse(h.E.saved).name, "second", "Revert uses the deliberate saved snapshot");
});

test("a failed Studio autosave does not block the queued manual save", async () => {
  const h = await studio(), d = h.E.doc = effect(h, "first");
  const first = h.quietSave();
  await tick();
  d.name = "latest";
  const second = h.save();
  h.requests[0].reject(new Error("temporary failure"));
  await first;
  await tick();
  assert.equal(h.requests[1].body.name, "latest");
  saved(h.requests[1]);
  await second;
  assert.equal(d.dirty, false);
  assert.equal(h.errors.length, 0, "autosave keeps its existing quiet failure behavior");
});

test("returning to an older Studio snapshot stays dirty until it saves after a different queued snapshot", async () => {
  const h = await studio(), d = h.E.doc = effect(h, "first");
  const first = h.quietSave();
  await tick();
  d.name = "second";
  const second = h.save();
  d.name = "first";
  d.dirty = true;
  saved(h.requests[0]);
  await first;
  await tick();
  assert.equal(d.dirty, true, "the matching old reply must not mark a newer queued save clean");
  assert.equal(h.requests[1].body.name, "second");
  const latest = h.quietSave();       // the current edit's debounce expires while the second save is in flight
  await tick();
  assert.equal(h.requests.length, 2);
  saved(h.requests[1]);
  await second;
  await tick();
  assert.equal(d.dirty, true);
  assert.equal(h.requests[2].body.name, "first");
  saved(h.requests[2]);
  await latest;
  assert.equal(d.dirty, false);
  assert.equal(d.name, "first");
});

test("closing Studio during a save keeps the recovery draft and does not revive the editor", async () => {
  const h = await studio();
  h.E.doc = effect(h, "first");
  const pending = h.save();
  await tick();
  h.closeEditor(true);
  const draft = h.storage.get(h.DRAFT_KEY);
  saved(h.requests[0]);
  const result = await pending;
  assert.equal(h.E.doc, null);
  assert.equal(h.E.saved, null);
  assert.equal(h.storage.get(h.DRAFT_KEY), draft);
  assert.equal(result, null, "a stale save cannot continue Use/Share in another editor");
});

test("two first saves of a new Studio effect reuse the first assigned file identity", async () => {
  const h = await studio(), d = h.E.doc = effect(h, "first", null);
  const first = h.save();
  d.name = "renamed";
  const second = h.save();
  await tick();
  assert.equal(h.requests.length, 1);
  saved(h.requests[0], "fx:me.first");
  await first;
  await tick();
  assert.equal(h.requests[1].body.fx_id, "fx:me.first");
  saved(h.requests[1]);
  await second;
  assert.equal(d.fx_id, "fx:me.first");
});

async function automations() {
  const h = await harness("automations", ["saveRules"]);
  h.core.S.rules = [{ id: "a", enabled: true }, { id: "b", enabled: true }];
  h.initAutomations();
  h.toggle = (id, checked) => h.element("#rules").events.get("change")({ target: {
    checked, matches: () => true, closest: () => ({ dataset: { id } }),
  } });
  return h;
}

test("rapid automation toggles retain both changes even if shared state refreshes while saving", async () => {
  const h = await automations();
  h.toggle("a", false);
  await tick();
  h.core.S.rules = [{ id: "a", enabled: true }, { id: "b", enabled: true }];
  h.toggle("b", false);
  await tick();
  assert.equal(h.requests.length, 1);
  assert.deepEqual(plain(h.core.S.rules).map((r) => r.enabled), [false, false]);
  h.requests[0].resolve(h.requests[0].body.rules);
  await tick();
  assert.deepEqual(h.requests[1].body.rules.map((r) => r.enabled), [false, false]);
  assert.deepEqual(plain(h.core.S.rules).map((r) => r.enabled), [false, false]);
  h.requests[1].resolve(h.requests[1].body.rules);
  await tick();
});

test("an early automation failure still sends the latest complete pending list", async () => {
  const h = await automations();
  h.toggle("a", false);
  h.toggle("b", false);
  await tick();
  h.requests[0].reject(new Error("temporary failure"));
  await tick();
  assert.equal(h.errors.length, 1);
  assert.deepEqual(h.requests[1].body.rules.map((r) => r.enabled), [false, false]);
  h.requests[1].resolve(h.requests[1].body.rules);
  await tick();
  assert.deepEqual(plain(h.core.S.rules).map((r) => r.enabled), [false, false]);
});

test("a final automation failure rolls back to the last successful list and permits a retry", async () => {
  const h = await automations();
  h.toggle("a", false);
  h.toggle("b", false);
  await tick();
  h.requests[0].resolve(h.requests[0].body.rules);
  await tick();
  h.requests[1].reject(new Error("write failed"));
  await tick();
  assert.deepEqual(plain(h.core.S.rules).map((r) => r.enabled), [false, true]);
  h.toggle("b", false);
  await tick();
  assert.deepEqual(h.requests[2].body.rules.map((r) => r.enabled), [false, false]);
  h.requests[2].resolve(h.requests[2].body.rules);
  await tick();
});

const preview = () => harness("profiles", ["startPreview", "stopPreview", "G"]);
const frames = (h) => h.requests.filter((r) => r.path === "/api/preview/frames");

test("slow gallery frames have one request in flight and schedule the next only after completion", async () => {
  const h = await preview();
  const start = h.startPreview({ id: "one" });
  await h.finishPreviewWrites();
  await start;
  const first = h.fire(90);
  await tick();
  assert.equal(frames(h).length, 1);
  assert.equal([...h.timers.values()].filter((t) => t.ms === 90).length, 0);
  frames(h)[0].resolve({ zones: ["first"] });
  await first;
  assert.equal(h.paints.length, 1);
  const second = h.fire(90);
  await tick();
  assert.equal(frames(h).length, 2);
  h.stopPreview();
  frames(h)[1].resolve({ zones: ["closed"] });
  await second;
  await h.finishPreviewWrites();
  assert.equal(h.paints.length, 1);
  assert.equal(h.timers.size, 0);
});

test("switching gallery previews waits for the old frame and never paints its stale response", async () => {
  const h = await preview();
  const start = h.startPreview({ id: "one" });
  await h.finishPreviewWrites();
  await start;
  const first = h.fire(90);
  await tick();
  h.stopPreview();
  const next = h.startPreview({ id: "two" });
  await h.finishPreviewWrites();
  await next;
  const second = h.fire(90);
  await tick();
  assert.equal(frames(h).length, 1, "the old request still occupies the only frame slot");
  frames(h)[0].resolve({ zones: ["old"] });
  await first;
  await tick();
  assert.equal(h.paints.length, 0);
  assert.equal(frames(h).length, 2);
  frames(h)[1].resolve({ zones: ["new"] });
  await second;
  assert.deepEqual(h.paints.map((p) => p.zones), [["new"]]);
  h.stopPreview();
  await h.finishPreviewWrites();
});

test("closing during gallery startup clears it after the pending start and cannot revive polling", async () => {
  const h = await preview();
  const start = h.startPreview({ id: "one" });
  await tick();
  h.requests[0].resolve({});             // initial clear completes, leaving the start request pending
  await tick();
  assert.deepEqual(h.requests[1].body, { profile: { id: "one" } });
  h.element("#sheet").open = false;
  h.stopPreview();
  assert.equal(h.requests.length, 2, "clear must wait for the in-flight start");
  h.requests[1].resolve({});
  await start;
  await tick();
  assert.deepEqual(h.requests[2].body, {});
  h.requests[2].resolve({});
  await tick();
  assert.equal(h.timers.size, 0);
});

test("hidden gallery previews stop polling and resume without overlapping the prior frame", async () => {
  const h = await preview();
  const start = h.startPreview({ id: "one" });
  await h.finishPreviewWrites();
  await start;
  const first = h.fire(90);
  await tick();
  h.document.visibilityState = "hidden";
  h.document.events.get("visibilitychange")?.();
  await h.finishPreviewWrites();
  assert.equal(h.timers.size, 0);
  h.document.visibilityState = "visible";
  h.document.events.get("visibilitychange")?.();
  await h.finishPreviewWrites();
  const second = h.fire(90);
  await tick();
  assert.equal(frames(h).length, 1);
  frames(h)[0].resolve({ zones: ["before hiding"] });
  await first;
  await tick();
  assert.equal(h.paints.length, 0);
  frames(h)[1].resolve({ zones: ["visible"] });
  await second;
  assert.deepEqual(h.paints.map((p) => p.zones), [["visible"]]);
  h.stopPreview();
  await h.finishPreviewWrites();
});
