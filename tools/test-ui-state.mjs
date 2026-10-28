// Exercise the actual editors, event bus and lazy router with deferred HTTP/import replies.
// Run: node --experimental-vm-modules --test tools/test-ui-state.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

const plain = (value) => JSON.parse(JSON.stringify(value));
const flush = () => new Promise(setImmediate);
const draftKey = "airglow.studio.unsaved.v1";

async function harness() {
  const elements = new Map(), storage = new Map(), requests = [], timers = new Map();
  let timer = 0, releaseStudio;
  const studioGate = new Promise((resolve) => { releaseStudio = resolve; });
  function element() {
    const events = new Map();
    return {
      events, dataset: {}, style: { setProperty() {} },
      classList: { add() {}, remove() {}, toggle() {} },
      addEventListener(name, fn) { (events.get(name) || events.set(name, []).get(name)).push(fn); },
      async fire(name, event = {}) { await Promise.all((events.get(name) || []).map((fn) => fn(event))); },
      querySelector(selector) { return node(selector); }, querySelectorAll() { return []; },
      append() {}, replaceChildren() {}, setAttribute() {}, removeAttribute() {},
      closest() { return { hidden: true }; }, contains() { return false; }, focus() {},
    };
  }
  function node(selector) {
    if (!elements.has(selector)) elements.set(selector, element());
    return elements.get(selector);
  }
  const document = Object.assign(element(), { createElement: element, visibilityState: "hidden" });
  const context = vm.createContext({
    document, window: element(), location: { hash: "#lighting" }, console,
    structuredClone, AbortController, performance, confirm: () => true,
    localStorage: {
      getItem: (key) => storage.get(key) ?? null,
      setItem: (key, value) => storage.set(key, value), removeItem: (key) => storage.delete(key),
    },
    setTimeout(fn) { timers.set(++timer, fn); return timer; },
    clearTimeout(id) { timers.delete(id); },
    fetch(path, options) {
      if (options.method === "GET") {
        if (path === "/api/effects") return Promise.resolve({ ok: true, json: async () => [] });
        return new Promise(() => {}); // Boot's other network calls stay pending.
      }
      return new Promise((resolve, reject) => requests.push({
        path, body: JSON.parse(options.body), reject,
        resolve: (body) => resolve({ ok: true, json: async () => body }),
      }));
    },
  });
  const real = new Set(["core", "studio", "studio-start", "fans", "main", "lighting"]);
  const extra = {
    studio: "export { E, save, quietSave, newDoc, snapDoc, saveDraftNow, pending };",
    fans: "export { E, renderEditor, movePoint };",
  };
  const sources = new Map([...real].map((name) => [name,
    readFileSync(new URL(`../web/js/${name}.js`, import.meta.url), "utf8")]));
  const stubNames = new Map();
  for (const source of sources.values()) {
    for (const m of source.matchAll(/import\s+\{([^}]+)\}\s+from\s+["']\.\/([^"']+)\.js["']/g)) {
      if (real.has(m[2])) continue;
      const names = stubNames.get(m[2]) || new Set();
      m[1].split(",").forEach((name) => names.add(name.trim().split(/\s+as\s+/)[0]));
      stubNames.set(m[2], names);
    }
  }
  const modules = new Map();
  function moduleFor(name) {
    if (modules.has(name)) return modules.get(name);
    const module = real.has(name)
      ? new vm.SourceTextModule(sources.get(name) + "\n" + (extra[name] || ""), {
        context, identifier: name,
        async importModuleDynamically(specifier) {
          if (specifier === "./studio.js") await studioGate;
          return load(specifier.slice(2, -3));
        },
      })
      : new vm.SyntheticModule([...stubNames.get(name)], function () {
        for (const key of stubNames.get(name)) this.setExport(key, () => {});
      }, { context });
    modules.set(name, module);
    return module;
  }
  async function load(name) {
    const module = moduleFor(name);
    if (module.status === "unlinked") await module.link((specifier) => moduleFor(specifier.slice(2, -3)));
    if (module.status === "linked") await module.evaluate();
    return module;
  }
  const core = (await load("core")).namespace;
  return { load, core, context, node, requests, storage, releaseStudio };
}

async function studioHarness() {
  const h = await harness(), studio = (await h.load("studio")).namespace;
  const doc = studio.newDoc({ name: "Effect A", fx_id: "mine.a" });
  doc.dirty = true;
  studio.E.doc = doc;
  studio.E.saved = "original A";
  return { ...h, studio, doc };
}

for (const quiet of [true, false]) {
  test(`Studio ${quiet ? "automatic" : "manual"} save cannot change another document or its draft`, async () => {
    const h = await studioHarness(), { studio, doc } = h;
    const saving = quiet ? studio.quietSave() : studio.save();
    const other = studio.newDoc({ name: "Effect B", fx_id: "mine.b" });
    other.dirty = true;
    studio.E.doc = other;
    studio.E.saved = "original B";
    studio.saveDraftNow();
    h.requests[0].resolve({ effect: { fx_id: "mine.a" } });
    await saving;
    assert.equal(other.fx_id, "mine.b");
    assert.equal(other.dirty, true);
    assert.equal(studio.E.saved, "original B");
    assert.equal(JSON.parse(h.storage.get(draftKey)).doc.name, "Effect B");
    assert.equal(doc.fx_id, "mine.a");
  });
}

test("Studio serializes automatic and manual writes without marking later edits saved", async () => {
  const h = await studioHarness(), { studio, doc } = h;
  const first = studio.quietSave();
  doc.name = "Second revision";
  const second = studio.save();
  doc.name = "Third revision";
  const third = studio.quietSave();
  studio.saveDraftNow();
  assert.equal(h.requests.length, 1, "writes for one document must not overlap");
  h.requests[0].resolve({ effect: { fx_id: "mine.a" } });
  await first;
  await flush();
  assert.equal(doc.dirty, true);
  assert.equal(h.requests[1].body.name, "Second revision");
  assert.ok(h.storage.has(draftKey));
  h.requests[1].resolve({ effect: { fx_id: "mine.a" } });
  await second;
  await flush();
  assert.equal(doc.dirty, true);
  assert.equal(JSON.parse(studio.E.saved).name, "Second revision");
  assert.equal(h.requests[2].body.name, "Third revision");
  h.requests[2].resolve({ effect: { fx_id: "mine.a" } });
  await third;
  assert.equal(doc.dirty, false);
  assert.equal(h.storage.has(draftKey), false);
});

test("queued first saves reuse the created effect ID and survive an earlier failure", async () => {
  const h = await studioHarness(), { studio, doc } = h;
  doc.fx_id = null;
  const first = studio.save();
  doc.name = "Second revision";
  const second = studio.save();
  assert.equal(h.requests.length, 1);
  h.requests[0].resolve({ effect: { fx_id: "mine.created" } });
  await first;
  await flush();
  assert.equal(h.requests[1].body.fx_id, "mine.created");
  doc.name = "Third revision";
  const third = studio.save();
  h.requests[1].reject(new Error("connection lost"));
  assert.equal(await second, null);
  await flush();
  assert.equal(h.requests[2].body.name, "Third revision");
  h.requests[2].resolve({ effect: { fx_id: "mine.created" } });
  await third;
  assert.equal(doc.dirty, false);
});

test("a queued obsolete revision cannot leave an undone edit marked saved or without its draft", async () => {
  const h = await studioHarness(), { studio, doc } = h;
  const first = studio.quietSave();
  doc.name = "Intermediate revision";
  const second = studio.quietSave();
  doc.name = "Effect A"; // Undo while both requested revisions are still pending.
  studio.saveDraftNow();
  h.requests[0].resolve({ effect: { fx_id: "mine.a" } });
  await first;
  await flush();
  assert.ok(h.storage.has(draftKey), "a later queued write still needs the draft safety net");
  h.requests[1].resolve({ effect: { fx_id: "mine.a" } });
  await second;
  assert.equal(doc.dirty, true, "the last persisted revision differs from the current document");
  assert.equal(JSON.parse(h.storage.get(draftKey)).doc.name, "Effect A");
});

for (const newlyCreated of [false, true]) {
  test(`reopening ${newlyCreated ? "a newly created" : "an existing"} effect keeps its pending writes in order`, async () => {
    const h = await studioHarness(), { studio, doc } = h;
    if (newlyCreated) doc.fx_id = null;
    const first = studio.save();
    doc.name = "Queued old editor";
    const second = studio.save();
    if (newlyCreated) {
      h.requests[0].resolve({ effect: { fx_id: "mine.a" } });
      await first;
      await flush();
    }
    studio.E.doc = null;
    const reopened = studio.newDoc({ name: "Latest reopened editor", fx_id: "mine.a" });
    reopened.dirty = true;
    studio.E.doc = reopened;
    studio.E.saved = "reopened baseline";
    studio.saveDraftNow();
    const third = studio.save();
    assert.equal(h.requests.length, newlyCreated ? 2 : 1, "reopening must share the effect's write queue");
    if (!newlyCreated) {
      h.requests[0].resolve({ effect: { fx_id: "mine.a" } });
      await first;
      await flush();
    }
    assert.equal(h.requests[1].body.name, "Queued old editor");
    h.requests[1].resolve({ effect: { fx_id: "mine.a" } });
    await second;
    await flush();
    assert.equal(reopened.dirty, true);
    assert.equal(studio.E.saved, "reopened baseline");
    assert.equal(h.requests[2].body.name, "Latest reopened editor");
    h.requests[2].resolve({ effect: { fx_id: "mine.a" } });
    await third;
    assert.equal(reopened.dirty, false);
    assert.equal(h.storage.has(draftKey), false);
  });
}

async function fansHarness() {
  const h = await harness(), fans = (await h.load("fans")).namespace;
  h.core.S.curves = {
    curves: { balanced: [[30, 30], [80, 100]], performance: [[30, 50], [80, 100]] },
    defaults: { balanced: [[30, 30], [80, 100]], performance: [[30, 50], [80, 100]] },
    min_duty: 20, safety_temp: 90,
  };
  h.core.S.state = { fans: { curve: "balanced", base_curve: "balanced" }, sensors: {} };
  fans.initFans();
  fans.renderEditor();
  fans.movePoint(0, 30, 40);
  return { ...h, fans };
}

test("a Balanced save response leaves the selected Performance curve and its edits alone", async () => {
  const h = await fansHarness(), { fans } = h;
  const saving = h.node("#curve-save").fire("click");
  const switching = h.node("#curve-seg").fire("click", {
    target: { closest: () => ({ dataset: { c: "performance" } }) },
  });
  h.requests[1].resolve({});
  await switching;
  fans.movePoint(0, 30, 60);
  h.requests[0].resolve({ points: [[30, 40], [80, 100]] });
  await saving;
  assert.deepEqual(plain(h.core.S.curves.curves.balanced), [[30, 40], [80, 100]]);
  assert.deepEqual(plain(h.core.S.curves.curves.performance), [[30, 50], [80, 100]]);
  assert.equal(fans.E.curve, "performance");
  assert.deepEqual(plain(fans.E.draft), [[30, 60], [80, 100]]);
  const next = h.node("#curve-save").fire("click");
  assert.equal(h.requests[2].path, "/api/curves/performance");
  assert.deepEqual(h.requests[2].body.points, [[30, 60], [80, 100]]);
  h.requests[2].resolve({ points: h.requests[2].body.points });
  await next;
});

test("a fan save response preserves newer edits to the same curve", async () => {
  const h = await fansHarness(), { fans } = h;
  const saving = h.node("#curve-save").fire("click");
  fans.movePoint(0, 30, 45);
  h.requests[0].resolve({ points: [[30, 40], [80, 100]] });
  await saving;
  assert.deepEqual(plain(fans.E.draft), [[30, 45], [80, 100]]);
  assert.deepEqual(plain(h.core.S.curves.curves.balanced), [[30, 40], [80, 100]]);
  assert.equal(h.node("#curve-save").disabled, false);
});

test("fan saves serialize snapshots so older requests cannot overwrite later saved revisions", async () => {
  const h = await fansHarness(), { fans } = h;
  const first = h.node("#curve-save").fire("click");
  fans.movePoint(0, 30, 45);
  const second = h.node("#curve-save").fire("click");
  fans.movePoint(0, 30, 50);
  const third = h.node("#curve-save").fire("click");
  assert.equal(h.requests.length, 1, "one curve must never have overlapping writes");
  h.requests[0].resolve({ points: [[30, 40], [80, 100]] });
  await first;
  await flush();
  assert.deepEqual(h.requests[1].body.points, [[30, 45], [80, 100]]);
  assert.deepEqual(plain(fans.E.draft), [[30, 50], [80, 100]]);
  h.requests[1].resolve({ points: [[30, 45], [80, 100]] });
  await second;
  await flush();
  assert.deepEqual(h.requests[2].body.points, [[30, 50], [80, 100]]);
  h.requests[2].resolve({ points: [[30, 50], [80, 100]] });
  await third;
  assert.deepEqual(plain(h.core.S.curves.curves.balanced), [[30, 50], [80, 100]]);
  assert.deepEqual(plain(fans.E.draft), [[30, 50], [80, 100]]);
  assert.equal(h.node("#curve-save").disabled, true);
});

for (const [selector, dataset, expected] of [
  ["[data-studio-open]", { studioOpen: "mine.a" }, { fx_id: "mine.a" }],
  ["[data-studio-remix]", { studioRemix: "solid" }, { builtin: "solid", params: { color: "#ff0000" } }],
]) {
  test(`Lighting ${selector} opens Studio on first use after its module finishes loading`, async () => {
    const h = await harness();
    h.core.S.state = { scene: { params: { color: "#ff0000" } } };
    await h.load("main");
    await h.load("lighting");
    await h.node("#inspector").fire("click", {
      target: { closest: (query) => query === selector ? { dataset } : null },
    });
    assert.equal(h.context.location.hash, "#lighting");
    h.releaseStudio();
    await flush();
    const studio = (await h.load("studio")).namespace;
    assert.equal(h.context.location.hash, "#studio/edit");
    assert.deepEqual(plain(studio.pending), expected);
    assert.equal(h.node("#studio").events.get("click").length, 1, "Studio initializes before delivery");
  });
}
