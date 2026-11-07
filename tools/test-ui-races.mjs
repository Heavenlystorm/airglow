// Deferred requests exercise the actual Studio, automation and profile-preview modules.
// Run: node --experimental-vm-modules --test tools/test-ui-races.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

const plain = (value) => JSON.parse(JSON.stringify(value));
const tick = () => new Promise(setImmediate);

test("sensor catalog refreshes coalesce and carry the server identity revision", async () => {
  const h = await harness("readings", []);
  const first = h.loadCatalog(), second = h.loadCatalog();
  assert.equal(h.requests.length, 1);
  h.requests[0].resolve({ sensors: [{ id: "cpu", value: 40, source: "New CPU" }], lhm_online: true, revision: 7 });
  await Promise.all([first, second]);
  assert.equal(h.R.revision, 7);
  assert.equal(h.R.meta.cpu.source, "New CPU");
  const retry = h.loadCatalog();
  h.requests[1].reject(new Error("offline"));
  await assert.rejects(retry, /offline/);
  assert.equal(h.R.revision, 7);
  assert.equal(h.R.meta.cpu.source, "New CPU");
});

test("manual zone saves serialize accumulated edits across a delayed response", async () => {
  const h = await harness("devices", ["saveZone"]);
  const cfg = { name: "Strip", enabled: true, order: "RGB", layout: { shape: "strip", leds: 4, reverse: false } };
  const zone = { key: "d/z", config: cfg, device: { id: "d", name: "Device", kind: "native", online: true },
    zone: { id: "z", max_leds: 16, default_leds: 4 } };
  h.core.S.zones = { zones: [zone] };
  h.core.S.state = { openrgb: {} };
  const first = h.saveZone("d/z", c => { c.order = "GRB"; });
  const second = h.saveZone("d/z", c => { c.layout.reverse = true; });
  await tick();
  assert.equal(h.requests.length, 1);
  h.requests[0].resolve({ config: h.requests[0].body });
  await tick();
  h.requests[1].resolve({ zones: [{ ...zone, config: h.requests[0].body }] });
  await first;
  await tick();
  assert.equal(h.requests[2].body.order, "GRB");
  assert.equal(h.requests[2].body.layout.reverse, true);
  h.requests[2].resolve({ config: h.requests[2].body });
  await tick();
  h.requests[3].resolve({ zones: [{ ...zone, config: h.requests[2].body }] });
  await second;
  assert.equal(h.core.S.zones.zones[0].config.order, "GRB");
  assert.equal(h.core.S.zones.zones[0].config.layout.reverse, true);
});

test("selected zone rotation handles are above every other zone hit area", async () => {
  const h = await harness("stage", ["st"]);
  h.st.selected = "d/a";
  h.st.arranging = true;
  h.core.S.zones = { zones: ["a", "b"].map(id => ({ key: `d/${id}`, device: { online: true, name: "Hub" },
    positions: [[0, 0]], config: { name: id, enabled: true,
      layout: { shape: "fans", x: 0.2, y: 0.4, angle: 90, fans: 1, leds_per_fan: 8, size: 0.16 } } })) };
  h.renderOverlay();
  const html = h.element("#stage-svg").innerHTML;
  assert.ok(html.indexOf('data-rot="d/a"') > html.indexOf('data-key="d/b"'));
});

test("welcome waits for persistence, retains the dialog on failure and permits retry", async () => {
  const h = await harness("onboarding", []);
  h.core.S.state = { sensors: { online: false }, devices: [], setup_pending: 0 };
  h.core.S.settings = { settings: { online_checks: false } };
  h.element("#onb-online").checked = true;
  h.openWelcome();
  const click = h.element("#onb-start").events.get("click");
  const first = click();
  await tick();
  assert.equal(h.requests.length, 1, "save the consent choice before marking onboarding complete");
  h.requests[0].resolve({ settings: { online_checks: true } });
  await tick();
  assert.equal(h.requests[1].path, "/api/onboarded");
  h.requests[1].reject(new Error("disk full"));
  await first;
  assert.equal(h.errors[0].message, "disk full");
  assert.equal(h.element("#sheet").open, true);
  const retry = click();
  await tick();
  h.requests[2].resolve({ settings: { online_checks: true } });
  await tick();
  h.requests[3].resolve({ ok: true });
  await retry;
  assert.equal(h.element("#sheet").open, false);
});

test("setup completion reports pending lights instead of unconditional success", async () => {
  const h = await harness("setup", ["finished", "G"]);
  h.G.skipped = 2;
  const finish = h.finished();
  assert.equal(h.requests[0]?.path, "/api/state");
  h.requests[0].resolve({ setup_pending: 2 });
  await finish;
  const html = h.element("#sheet").innerHTML;
  assert.ok(html.includes("2 lights still need setup"));
  assert.ok(!html.includes("Your lights are set up"));
});

test("setup completion distinguishes verified success, skipped work and a failed verification", async () => {
  for (const [skipped, response, expected] of [
    [0, { setup_pending: 0 }, "Your lights are set up"],
    [1, { setup_pending: 0 }, "You skipped some steps"],
    [0, new Error("state unavailable"), "couldn't check what remains"],
  ]) {
    const h = await harness("setup", ["finished", "G"]);
    h.G.skipped = skipped;
    const pending = h.finished();
    if (response instanceof Error) h.requests[0].reject(response);
    else h.requests[0].resolve(response);
    await pending;
    assert.ok(h.element("#sheet").innerHTML.includes(expected));
    if (skipped || response instanceof Error) assert.ok(!h.element("#sheet").innerHTML.includes("Your lights are set up"));
  }
});

test("welcome discovery retains the busy conflict control so failed requests remain retriable", async () => {
  const h = await harness("onboarding", ["renderWelcomeStatus"]);
  h.core.S.state = { sensors: { online: false }, devices: [], setup_pending: 0,
    conflicts: [{ name: "RGB app", conflicts_with: "controller" }] };
  const button = { dataset: { closeApp: "RGB app" }, disabled: false,
    focus() { h.document.activeElement = this; } };
  let visible = button;
  const status = h.element("#onb-status");
  status.querySelectorAll = () => [visible];
  Object.defineProperty(status, "innerHTML", { set() {
    visible = { dataset: { closeApp: "RGB app" }, disabled: false,
      replaceWith(retained) { visible = retained; }, focus() { h.document.activeElement = this; } };
  } });
  button.focus();
  const pending = h.closeConflict(button);
  h.renderWelcomeStatus();
  assert.equal(visible, button);
  assert.equal(h.document.activeElement, button);
  assert.equal(visible.disabled, true);
  h.requests[0].reject(new Error("close denied"));
  await pending;
  assert.equal(visible.disabled, false);
});

test("probe errors are visible, renewal does not spam and recovery clears the error", async () => {
  const h = await harness("setup", ["probe", "G"]);
  h.probe(["d/z"], "blink");
  h.requests[0].reject(new Error("unplugged"));
  await tick();
  assert.equal(h.errors.length, 1);
  assert.match(h.G.probe.error, /unplugged/);
  h.fire(5000);
  h.requests[1].reject(new Error("unplugged"));
  await tick();
  assert.equal(h.errors.length, 1);
  h.document.events.get("click")({ target: { closest: () => true } });
  h.requests[2].resolve({ ok: true });
  await tick();
  assert.equal(h.G.probe.error, "");
});

test("probe renewal cannot overlap a pending attempt and the latest count is not lost", async () => {
  const h = await harness("setup", ["probe", "probeCount", "G"]);
  h.probe(["d/z"], "count", 1);
  h.fire(5000);
  assert.equal(h.requests.length, 1);
  h.probeCount(3);
  h.fire(60);
  assert.equal(h.requests.length, 1);
  h.requests[0].resolve({ ok: true });
  await tick();
  assert.equal(h.requests[1].body.n, 3);
  h.requests[1].resolve({ ok: true });
  await tick();
});

test("an older full zones response cannot replace a newer acknowledged snapshot", async () => {
  const h = await harness("core", []);
  const a = h.refreshZones(), b = h.refreshZones();
  h.requests[1].resolve({ zones: [{ key: "b", config: { reverse: true } }] });
  await b;
  h.requests[0].resolve({ zones: [{ key: "b", config: { reverse: false } }] });
  await a;
  assert.equal(h.S.zones.zones[0].config.reverse, true);
});

async function harness(file, exports) {
  const source = readFileSync(new URL(`../web/js/${file}.js`, import.meta.url), "utf8");
  const timers = new Map(), requests = [], errors = [], paints = [], storage = new Map(), elements = new Map();
  let nextTimer = 0;
  const element = (id) => {
    if (!elements.has(id)) elements.set(id, {
      events: new Map(), open: true, dataset: {}, classList: { toggle() {}, add() {}, remove() {} },
      addEventListener(type, fn) { this.events.set(type, fn); },
      setAttribute() {}, removeAttribute() {}, querySelectorAll() { return []; }, focus() {},
      removeEventListener(type, fn) { if (this.events.get(type) === fn) this.events.delete(type); },
    });
    return elements.get(id);
  };
  const document = { visibilityState: "visible", events: new Map(),
    addEventListener(type, fn) { this.events.set(type, fn); } };
  const context = vm.createContext({
    document, window: { addEventListener() {} }, structuredClone, AbortController, performance: { now: () => 0 },
    fetch(path, options) { return new Promise((resolve, reject) => requests.push({ path, method: options.method,
      reject, resolve: body => resolve({ ok: true, json: async () => body }) })); },
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
    S: { state: null, effects: [], presets: [], rules: [] }, $: element, $$: () => [], QUICK: [],
    esc: (s) => String(s).replaceAll("&", "&amp;").replaceAll('"', "&quot;").replaceAll("<", "&lt;").replaceAll(">", "&gt;"),
    emit() {}, on() {}, toast() {}, setHTML(el, html) { el.innerHTML = html; }, coalesce: (fn) => fn, fail: (error) => errors.push(error),
    post: (path, body) => request("POST", path, body),
    get: (path) => path === "/api/effects" ? Promise.resolve([]) : request("GET", path),
    refreshZones: () => request("GET", "/api/zones").then(z => { core.S.zones = z; return z; }),
  };
  const sources = new Map([[`./${file}.js`, source + `\nexport { ${exports.join(", ")} };`]]);
  for (const name of ["studio-document", "studio-preview", "studio-gallery", "studio-packs", "studio-variations"]) {
    sources.set(`./${name}.js`, readFileSync(new URL(`../web/js/${name}.js`, import.meta.url), "utf8"));
  }
  const imports = new Map(), modules = new Map();
  for (const text of sources.values()) for (const m of text.matchAll(/import\s+\{([^}]+)\}\s+from\s+["']([^"']+)["']/g)) {
    const names = imports.get(m[2]) || new Set();
    m[1].split(",").forEach((name) => names.add(name.trim().split(/\s+as\s+/)[0]));
    imports.set(m[2], names);
  }
  function moduleFor(specifier) {
    if (modules.has(specifier)) return modules.get(specifier);
    const linked = sources.has(specifier) ? new vm.SourceTextModule(sources.get(specifier), { context, identifier: specifier })
      : new vm.SyntheticModule([...imports.get(specifier)], function () {
        for (const name of imports.get(specifier)) {
          const value = specifier === "./core.js" && name in core ? core[name]
            : name === "closeSheet" ? () => { element("#sheet").open = false; }
            : name === "openSheet" ? (html) => { element("#sheet").innerHTML = html; element("#sheet").open = true; }
            : name === "ensureHistory" ? () => Promise.resolve()
            : name === "paintMini" ? (canvas, zones) => paints.push({ canvas, zones }) : () => {};
          this.setExport(name, value);
        }
      }, { context });
    modules.set(specifier, linked);
    return linked;
  }
  const module = moduleFor(`./${file}.js`);
  await module.link(moduleFor);
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

test("overview reset cancels the debounce and waits for an older save before resetting", async () => {
  const h = await harness("overview", ["O", "save", "flushOverview", "resetOverview"]);
  h.O.layout = { tiles: [{ id: "old" }], pins: ["old"] };
  h.save();
  const saving = h.fire(350);
  const resetting = h.resetOverview();
  await tick();
  assert.equal(h.requests.length, 1, "reset cannot race an older write at the server");
  h.requests[0].resolve({ tiles: [{ id: "old" }], pins: ["old"] });
  await saving;
  await tick();
  assert.equal(h.requests[1].path, "/api/overview/reset");
  h.requests[1].resolve({ tiles: [{ id: "default" }], pins: [] });
  await resetting;
  assert.deepEqual(plain(h.O.layout), { tiles: [{ id: "default" }], pins: [] });
  assert.equal(h.timers.size, 0);
});

test("overview reset drops a pending debounce without sending the old layout", async () => {
  const h = await harness("overview", ["O", "save", "resetOverview"]);
  h.O.layout = { tiles: [], pins: [] };
  h.save();
  const resetting = h.resetOverview();
  await tick();
  assert.equal(h.timers.size, 0);
  assert.equal(h.requests.length, 1);
  assert.equal(h.requests[0].path, "/api/overview/reset");
  h.requests[0].resolve({ tiles: [], pins: [] });
  await resetting;
});

test("closing or replacing setup during either snap request cannot restart the probe", async () => {
  for (const afterSave of [false, true]) {
    const h = await harness("setup", ["G", "snap", "stopProbe"]);
    h.G.task = { type: "fixed", keys: ["d/z"], dev: { id: "d" } };
    const snapping = h.snap("front");
    if (afterSave) { h.requests[0].resolve({}); await tick(); }
    h.G.task = null;
    h.stopProbe();
    if (!afterSave) { h.requests[0].resolve({}); await tick(); }
    const zones = h.requests.find(r => r.path === "/api/zones");
    zones?.resolve({ zones: [] });
    await snapping;
    assert.equal(h.G.probe, null);
    assert.equal(h.timers.size, 0);
    assert.equal(h.requests.filter(r => r.path === "/api/setup/probe").length, 0);
  }
});

test("declared community controls reject colour markup and unsafe parameter ids", async () => {
  const h = await harness("studio", ["declaredParams"]);
  const params = h.declaredParams(`airglow.params({tone:{type:'color',default:'#000\" autofocus onfocus=\"bad'},
    palette:{type:'colors',default:['#112233','bad']}, 'bad\"id':{type:'color',default:'#aabbcc'}});`);
  assert.ok(params.every(p => /^[a-zA-Z][a-zA-Z0-9_]{0,31}$/.test(p.id)));
  assert.ok(/^#[0-9a-f]{6}$/i.test(params.find(p => p.id === "tone").default));
  assert.ok(params.find(p => p.id === "palette").default.every(c => /^#[0-9a-f]{6}$/i.test(c)));
});

test("shared colour fields cannot interpolate attribute or CSS markup", async () => {
  const h = await harness("lighting", []);
  const bad = '#000\" autofocus onfocus=\"bad';
  for (const [spec, value] of [[{ id: "tone", label: "Tone", type: "color" }, bad],
    [{ id: "palette", label: "Palette", type: "colors", min: 2, max: 4 }, [bad, "#112233"]]]) {
    const html = h.fieldHTML(spec, value, "test");
    assert.ok(!html.includes('onfocus="bad'));
    assert.ok(!html.includes('background:#000"'));
  }
});
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
