// The real Overview persistence/commands with deferred HTTP and history responses.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

const plain = value => JSON.parse(JSON.stringify(value));
const tick = () => new Promise(setImmediate);
async function harness() {
  const requests = [], timers = new Map(), errors = [];
  let timerId = 0, historyResponse = Promise.resolve();
  const elements = new Map();
  const element = id => {
    if (id === "#ov-workspaces") return null; // Persistence is independent of toolbar mounting.
    if (!elements.has(id)) elements.set(id, { hidden: true, open: false, classList: { toggle() {} } });
    return elements.get(id);
  };
  const context = vm.createContext({ structuredClone, document: { hidden: false }, window: { addEventListener() {} },
    setTimeout(fn, ms) { timers.set(++timerId, { fn, ms }); return timerId; }, clearTimeout(id) { timers.delete(id); } });
  const sources = new Map(["overview", "overview-workspaces"].map(name => [`./${name}.js`, readFileSync(new URL(`../web/js/${name}.js`, import.meta.url), "utf8")]));
  sources.set("./overview.js", sources.get("./overview.js") + "\nexport { O, save, flushOverview, resetOverview, historyStep, workspaceChange, addTile };");
  const core = { $: element, S: { state: null }, emit() {}, fail: error => errors.push(error),
    post: (path, body) => new Promise((resolve, reject) => requests.push({ path, body: body == null ? body : plain(body), resolve, reject })) };
  const imports = new Map(), modules = new Map();
  for (const source of sources.values()) for (const m of source.matchAll(/import\s*\{([^}]+)\}\s*from\s*["']([^"']+)/g)) {
    imports.set(m[2], m[1].split(",").map(s => s.trim()));
  }
  const moduleFor = name => {
    if (modules.has(name)) return modules.get(name);
    const module = sources.has(name) ? new vm.SourceTextModule(sources.get(name), { context, identifier: name })
      : new vm.SyntheticModule(imports.get(name), function () {
        for (const key of imports.get(name)) this.setExport(key, name === "./core.js" && key in core ? core[key]
          : key === "ensureHistory" ? () => historyResponse : () => {});
      }, { context });
    modules.set(name, module); return module;
  };
  const module = moduleFor("./overview.js"); await module.link(moduleFor); await module.evaluate();
  const ui = module.namespace, W = moduleFor("./overview-workspaces.js").namespace;
  ui.O.layout = { tiles: [{ id: "cpu", type: "sensor", sensors: ["auto/cpu_temp"], style: "spark", w: 3, h: 1, label: "Original" }], pins: ["auto/cpu_temp"] };
  W.initializeWorkspaces(ui.O.layout); ui.O.history = W.createLayoutHistory(ui.O.layout);
  const fire = ms => {
    const entry = [...timers].find(([, item]) => item.ms === ms);
    assert.ok(entry, `expected ${ms}ms timer`); timers.delete(entry[0]); return entry[1].fn();
  };
  return { ...ui, W, requests, timers, errors, core, fire, deferReadings(promise) { historyResponse = promise; } };
}

test("workspace switches serialize behind old saves and stale responses cannot revive the old workspace", async () => {
  const h = await harness();
  h.O.layout.tiles[0].w = 4; h.save(); const first = h.fire(350);
  h.workspaceChange(() => h.W.createWorkspace(h.O.layout, "Evening"));
  h.O.layout.tiles[0].w = 1; h.O.layout.pins = []; h.save(); h.fire(350);
  assert.equal(h.requests.length, 1);
  assert.equal(h.requests[0].body.active_workspace, "my-workspace");
  assert.equal(h.requests[0].body.tiles[0].w, 4);
  h.requests[0].resolve(h.requests[0].body); await first;
  assert.equal(h.O.layout.active_workspace, "evening"); assert.equal(h.O.layout.tiles[0].w, 1);
  const second = h.fire(0);
  assert.equal(h.requests[1].body.active_workspace, "evening");
  assert.deepEqual(h.requests[1].body.pins, []);
  assert.equal(h.requests[1].body.workspaces[0].tiles[0].w, 4);
  h.requests[1].resolve(h.requests[1].body); await second;
  assert.equal(h.O.saveState, "saved"); assert.equal(h.timers.size, 0);
});

test("undo during a save persists the restored snapshot after the older write", async () => {
  const h = await harness(), tile = h.O.layout.tiles[0];
  delete tile.label; h.save(); const first = h.fire(350);
  h.historyStep("undo");
  assert.equal(h.O.layout.tiles[0], tile); assert.equal(tile.label, "Original");
  h.requests[0].resolve(h.requests[0].body); await first;
  const second = h.fire(350);
  assert.equal(h.requests[1].body.tiles[0].label, "Original");
  h.requests[1].resolve(h.requests[1].body); await second;
  h.historyStep("redo");
  assert.equal(Object.hasOwn(tile, "label"), false);
});

test("failed final writes keep local edits and remain retriable without corrupting history", async () => {
  const h = await harness();
  h.O.layout.tiles[0].w = 4; h.save(); const first = h.fire(350);
  h.requests[0].reject(new Error("write failed")); await first;
  assert.equal(h.O.saveState, "unsaved"); assert.equal(h.O.layout.tiles[0].w, 4);
  h.save(null, false); const retry = h.fire(350);
  h.requests[1].resolve(h.requests[1].body); await retry;
  h.historyStep("undo"); assert.equal(h.O.layout.tiles[0].w, 3);
  assert.equal(h.errors.length, 1);
});

test("a reading fetch finishing after a workspace switch cannot insert into that workspace", async () => {
  const h = await harness(); let release;
  h.deferReadings(new Promise(resolve => { release = resolve; }));
  const pending = h.addTile({ id: "gpu", type: "sensor", sensors: ["auto/gpu_temp"], style: "spark", w: 1, h: 1 });
  h.W.createWorkspace(h.O.layout, "Evening");
  release(); await pending;
  assert.equal(h.O.layout.tiles.length, 1);
  assert.equal(h.timers.size, 0);
});

test("reset waits for older writes and is a reversible workspace edit", async () => {
  const h = await harness();
  h.O.layout.tiles[0].w = 4; h.save(); const first = h.fire(350);
  const resetting = h.resetOverview(); await tick(); assert.equal(h.requests.length, 1);
  h.requests[0].resolve(h.requests[0].body); await first; await tick();
  const reset = plain(h.O.layout); reset.tiles[0].w = 2; h.W.syncWorkspace(reset);
  h.requests[1].resolve(reset); await resetting;
  assert.equal(h.O.layout.tiles[0].w, 2);
  h.historyStep("undo"); assert.equal(h.O.layout.tiles[0].w, 4);
  assert.equal(h.O.layout.workspaces.length, 1);
});

test("reset preserves newly named workspaces even before the debounced save reaches the server", async () => {
  const h = await harness();
  h.workspaceChange(() => h.W.createWorkspace(h.O.layout, "Evening"));
  const reset = h.resetOverview(); await tick();
  assert.equal(h.requests[0].path, "/api/overview", "save the local collection before resetting its active layout");
  assert.equal(h.requests[0].body.workspaces.length, 2);
  h.requests[0].resolve(h.requests[0].body); await tick();
  assert.equal(h.requests[1].path, "/api/overview/reset");
  const response = plain(h.requests[0].body); response.tiles = []; h.W.syncWorkspace(response);
  h.requests[1].resolve(response); await reset;
  assert.equal(h.O.layout.active_workspace, "evening");
  assert.equal(h.O.layout.workspaces[0].tiles[0].w, 3);
});
