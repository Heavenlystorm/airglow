// Exercise the actual editors, event bus and lazy router with deferred HTTP/import replies.
// Run: node --experimental-vm-modules --test tools/test-ui-state.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

const plain = (value) => JSON.parse(JSON.stringify(value));
const flush = () => new Promise(setImmediate);
const draftKey = "airglow.studio.unsaved.v1";

async function themeFocusHarness() {
  const timers = [], document = { activeElement: null };
  const control = id => ({ dataset: { designCard: id }, addEventListener() {},
    focus() { document.activeElement = this; } });
  const graphite = control("graphite"), paper = control("paper"), done = control(null);
  const sheet = {
    querySelectorAll: () => [graphite, paper],
    querySelector: selector => selector.includes("aria-checked") ? paper : graphite,
  };
  const context = vm.createContext({ document, setTimeout(fn) { timers.push(fn); } });
  const source = readFileSync(new URL("../web/js/settings.js", import.meta.url), "utf8");
  const imports = new Map([...source.matchAll(/import\s+\{([^}]+)\}\s+from\s+["']([^"']+)["']/g)]
    .map(m => [m[2], m[1].split(",").map(name => name.trim())]));
  const values = {
    $: () => sheet, esc: String, S: {},
    openSheet: () => { graphite.focus(); return sheet; },
    currentDesign: () => "paper",
    DESIGNS: [{ id: "graphite", name: "Graphite" }, { id: "paper", name: "Paper" }],
  };
  const module = new vm.SourceTextModule(source + "\nexport { designSheet };", { context });
  await module.link(specifier => new vm.SyntheticModule(imports.get(specifier), function () {
    for (const name of imports.get(specifier)) this.setExport(name, values[name] ?? (() => {}));
  }, { context }));
  await module.evaluate();
  return { show: module.namespace.designSheet, document, paper, done, timers };
}

test("theme chooser focuses the selected design when the modal opens", async () => {
  const h = await themeFocusHarness();
  h.show();
  assert.equal(h.document.activeElement, h.paper);
});

test("theme chooser cannot steal focus after the user moves to Done", async () => {
  const h = await themeFocusHarness();
  h.show();
  h.done.focus();
  for (const callback of h.timers) callback();
  assert.equal(h.document.activeElement, h.done);
});

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
    structuredClone, AbortController, TextEncoder, performance, confirm: () => true,
    localStorage: {
      getItem: (key) => storage.get(key) ?? null,
      setItem: (key, value) => storage.set(key, value), removeItem: (key) => storage.delete(key),
    },
    setTimeout(fn, ms) { timers.set(++timer, { fn, ms }); return timer; },
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
  const real = new Set(["core", "studio", "studio-start", "studio-document", "studio-preview", "studio-gallery", "studio-packs", "fans", "main", "lighting"]);
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
  function fireTimer(ms) {
    const next = [...timers].find(([, value]) => value.ms === ms);
    if (!next) return false;
    timers.delete(next[0]);
    next[1].fn();
    return true;
  }
  return { load, core, context, node, requests, storage, releaseStudio, fireTimer };
}

test("conflicting mutations serialize snapshots while unrelated zones stay responsive", async () => {
  const h = await harness();
  const body = { params: { speed: 1 } };
  const first = h.core.post("/api/scene", body);
  body.params.speed = 99;
  const second = h.core.post("/api/scene", { brightness: 80 });
  const other = h.core.post("/api/zones/device%2Fzone", { name: "Front" });
  await flush();
  assert.equal(h.requests.length, 2, "only one request per lighting resource may be in flight");
  assert.equal(h.requests[0].body.params.speed, 1);
  h.requests[1].resolve({ config: {} });
  await other;
  h.requests[0].resolve({ scene: body });
  await first;
  await flush();
  assert.equal(h.requests.length, 3);
  assert.equal(h.requests[2].body.brightness, 80);
  h.requests[2].resolve({ scene: {} });
  await second;
  assert.equal(h.core.mutationStates().find((s) => s.key === "lighting").status, "saved");
});

test("failed saves stay recoverable but an obsolete failure cannot replay over a newer edit", async () => {
  const h = await harness();
  const first = h.core.post("/api/scene", { brightness: 20 }).catch((e) => e);
  await flush();
  h.requests[0].reject(new Error("disk full"));
  await first;
  const failed = h.core.mutationStates().find((s) => s.key === "lighting");
  assert.equal(failed.status, "failed");
  assert.equal(failed.error, "disk full");
  assert.equal(failed.retry, true);
  const second = h.core.post("/api/scene", { brightness: 80 });
  await flush();
  h.requests[1].resolve({ scene: { brightness: 80 } });
  await second;
  assert.equal(await h.core.retryMutation("lighting", failed.id), false);
  assert.equal(h.requests.length, 2, "the old value must never be replayed after success");
});

test("recovery retries idempotent saves but never uncertain profile creation", async () => {
  const h = await harness();
  const first = h.core.post("/api/settings", { energy_price: 0.3 }).catch((e) => e);
  await flush(); h.requests[0].reject(new Error("temporarily unavailable")); await first;
  const failed = h.core.mutationStates().find((s) => s.key === "settings");
  const retry = h.core.retryMutation(failed.key, failed.id);
  await flush();
  assert.deepEqual(h.requests[1].body, { energy_price: 0.3 });
  h.requests[1].resolve({}); await retry;
  const creation = h.core.post("/api/presets/from-current", { name: "Evening" }).catch((e) => e);
  await flush(); h.requests[2].reject(new Error("request timed out")); await creation;
  const profile = h.core.mutationStates().find((s) => s.key === "profiles");
  assert.equal(profile.retry, false);
  assert.equal(await h.core.retryMutation(profile.key, profile.id), false);
});

test("a silent Studio mutation invalidates recovery of an older lighting edit", async () => {
  const h = await harness();
  const failedWrite = h.core.post("/api/scene", { brightness: 10 }).catch(e => e);
  await flush(); h.requests[0].reject(new Error("save failed")); await failedWrite;
  const failed = h.core.mutationStates().find(s => s.key === "lighting");
  const draft = h.core.post("/api/studio/draft", { lights: true });
  await flush(); h.requests[1].resolve({ lights: true }); await draft;
  const retry = h.core.retryMutation("lighting", failed.id);
  await flush();
  assert.equal(h.requests.length, 2, "a newer silent draft invalidates the old retry");
  assert.equal(await retry, false);
});

test("global automation recovery installs the confirmed list before the next edit", async () => {
  const h = await harness(); await h.load("main");
  const rule = { id: "retained", when: { type: "idle", minutes: 10 }, then: { type: "lights_off" } };
  const write = h.core.post("/api/automations", { rules: [rule] }).catch(e => e);
  await flush(); h.requests[0].reject(new Error("save failed")); await write;
  const failed = h.core.mutationStates().find(s => s.key === "automations");
  const button = { dataset: { retryMutation: "automations", id: String(failed.id) } };
  const recovery = h.node("#mutation-status").fire("click", { target: { closest: selector => selector === "[data-retry-mutation]" ? button : null } });
  await flush(); h.requests[1].resolve([rule]); await recovery;
  assert.deepEqual(plain(h.core.S.rules), [rule]);
  const next = h.core.post("/api/automations", { rules: [...h.core.S.rules, { ...rule, id: "new" }] });
  await flush(); assert.deepEqual(h.requests[2].body.rules.map(r => r.id), ["retained", "new"]);
  h.requests[2].resolve(h.requests[2].body.rules); await next;
});

test("scene pack download checks the actual UTF-8 serialization including small floats and Unicode", async () => {
  const h = await harness(), packs = (await h.load("studio-packs")).namespace;
  const pack = { name: "café", values: { speed: 1e-6 }, source: "" };
  const overhead = Buffer.byteLength(JSON.stringify(pack, null, 2), "utf8");
  pack.source = "x".repeat(240 * 1024 - overhead);
  assert.equal(Buffer.byteLength(packs.serializePack(pack), "utf8"), 240 * 1024);
  pack.source += "é";
  assert.throws(() => packs.serializePack(pack), /240 KiB/);
});

test("coalesced slider writes wait for the active save and persist the latest merged edit", async () => {
  const h = await harness(), sends = [];
  let saved;
  const change = h.core.coalesce((body) => new Promise((resolve) => sends.push({ body,
    resolve() { saved = body; resolve(); },
  })), 70);
  change({ brightness: 20 });
  assert.equal(h.fireTimer(70), true);
  change({ brightness: 40 });
  h.fireTimer(70);
  change({ brightness: 80, params: { speed: 2 } });
  h.fireTimer(70);
  assert.equal(sends.length, 1, "slow saves must not overlap and arrive out of order");
  sends[0].resolve();
  await flush();
  h.fireTimer(70);
  assert.equal(sends.length, 2, "intermediate slider positions are merged into one pending write");
  assert.deepEqual(plain(sends[1].body), { brightness: 80, params: { speed: 2 } });
  sends[1].resolve();
  await flush();
  assert.equal(saved.brightness, 80);
  assert.equal(h.fireTimer(70), false);
});

test("a failed coalesced save still sends the latest queued edit and allows future edits", async () => {
  const h = await harness(), sends = [];
  const change = h.core.coalesce((body) => new Promise((resolve, reject) => sends.push({ body, resolve, reject })), 70);
  change({ brightness: 20 });
  h.fireTimer(70);
  change({ brightness: 80 });
  h.fireTimer(70);
  assert.equal(sends.length, 1);
  sends[0].reject(new Error("temporary save failure"));
  await flush();
  h.fireTimer(70);
  assert.equal(sends.length, 2);
  assert.equal(sends[1].body.brightness, 80);
  sends[1].resolve();
  await flush();
  change({ brightness: 60 });
  h.fireTimer(70);
  assert.equal(sends.length, 3);
  sends[2].resolve();
  await flush();
});

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

test("the fan graph shows commanded live duty while a different curve draft is unsaved", async () => {
  const h = await fansHarness();
  Object.assign(h.core.S.state.fans, { temp: 50, duty: 37, target: 58 });
  h.fans.renderEditor();
  assert.match(h.node("#editor").innerHTML, /Now 50°C, 37%/);
  h.fans.movePoint(0, 30, 70);
  assert.match(h.node("#editor").innerHTML, /Now 50°C, 37%/);
  h.core.S.state.fans.duty = 42;
  h.fans.renderEditor();
  assert.match(h.node("#editor").innerHTML, /Now 50°C, 42%/, "ramping duty must repaint even at a steady temperature");
  h.core.S.state.fans.duty = null;
  h.fans.renderEditor();
  assert.ok(!h.node("#editor").innerHTML.includes("Now 50°C"), "missing live duty must not be guessed from a draft");
});

test("Add curve point creates a keyboard-editable draft without applying it", async () => {
  const h = await fansHarness();
  await h.node("#curve-add").fire("click");
  assert.deepEqual(plain(h.fans.E.draft), [[30, 40], [55, 70], [80, 100]]);
  assert.equal(h.fans.E.sel, 1);
  assert.equal(h.requests.length, 0, "adding a point edits only the local draft");
  assert.deepEqual(plain(h.core.S.curves.curves.balanced), [[30, 30], [80, 100]]);
});

test("bulk fan copy uses only explicit targets and reports partial failure", async () => {
  const h = await fansHarness();
  const channels = [{ id: "source", name: "Source" }, { id: "fan1", name: "Front" },
    { id: "fan2", name: "Rear" }, { id: "pump", name: "Pump", pump: true }];
  h.core.S.state.fans.channels = channels;
  const copying = h.fans.copyChannelSettings(channels.slice(1, 3), { mode: "steady", duty: 50 });
  await flush();
  assert.equal(h.requests[0].body.id, "fan1");
  h.requests[0].resolve({ channels });
  await flush();
  assert.equal(h.requests[1].body.id, "fan2");
  h.requests[1].reject(new Error("Disconnected"));
  const result = await copying;
  assert.deepEqual(plain(result.applied), ["fan1"]);
  assert.equal(result.failed[0].id, "fan2");
  assert.equal(result.failed[0].error, "Disconnected");
  assert.ok(h.requests.every((r) => r.body.id !== "pump"), "pumps need explicit selection");
});

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
