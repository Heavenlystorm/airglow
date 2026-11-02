import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

const tick = () => new Promise(setImmediate);
async function harness() {
  const path = new URL("../web/js/rig-migration.js", import.meta.url);
  assert.ok(existsSync(path), "migration UI module is missing");
  const elements = new Map(), requests = [], refreshes = [];
  let slowRefresh = false;
  const node = (selector) => {
    if (!elements.has(selector)) elements.set(selector, { disabled: false, hidden: false, events: new Map(),
      addEventListener(event, fn) { this.events.set(event, fn); }, querySelectorAll() { return []; }, focus() {},
      setAttribute() {}, fire(event, data = {}) { return this.events.get(event)?.({ preventDefault() {}, ...data }); } });
    return elements.get(selector);
  };
  const core = { $: node, $$: () => [], S: {}, get: () => slowRefresh
    ? new Promise((resolve) => refreshes.push(resolve)) : Promise.resolve({}), emit() {}, toast() {}, fail() {},
    refreshZones: async () => ({}), esc: (value) => String(value ?? "").replace(/[&<>"']/g,
      (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c])),
    post: (path, body) => new Promise((resolve, reject) => requests.push({ path, body, resolve, reject })) };
  const source = readFileSync(path, "utf8");
  const context = vm.createContext({ console, structuredClone, TextEncoder, Blob, URL, Date,
    document: { activeElement: null }, setTimeout, clearTimeout });
  const imports = new Map([...source.matchAll(/import\s+\{([^}]+)\}\s+from\s+["']([^"']+)["']/g)]
    .map((match) => [match[2], match[1].split(",").map((name) => name.trim())]));
  const module = new vm.SourceTextModule(source, { context });
  await module.link((specifier) => new vm.SyntheticModule(imports.get(specifier), function () {
    for (const name of imports.get(specifier)) this.setExport(name,
      specifier === "./core.js" ? core[name] : name === "openSheet"
        ? (html) => { node("#sheet").innerHTML = html; node("#sheet").open = true; return node("#sheet"); } : () => {});
  }, { context }));
  await module.evaluate();
  return { ui: module.namespace, node, requests, refreshes, delayRefresh: () => { slowRefresh = true; } };
}

test("an oversized upload is rejected before reading its contents", async () => {
  const h = await harness();
  let read = false;
  await assert.rejects(h.ui.readRigFile({ size: 192 * 1024 + 1, text: async () => { read = true; return "{}"; } }), /192/);
  assert.equal(read, false);
  await assert.rejects(h.ui.readRigFile({ size: 1, text: async () => "[" }), /JSON/);
});

test("mapping requires a reviewed token; a failed apply keeps the import for recheck", async () => {
  const h = await harness();
  h.ui.openRigImport();
  const bundle = { format: "airglow-rig", version: 1, zones: [{ id: "source/front" }] };
  h.node("#rig-file").files = [{ size: 100, text: async () => JSON.stringify(bundle) }];
  const loading = h.node("#rig-file").fire("change");
  await tick();
  assert.equal(h.requests[0].path, "/api/rig/preview");
  const preview = { ready: false, preview_token: null, changes: { settings: [], curves: [], profiles: [], automations: [] },
    zones: [{ id: "source/front", name: "Front", device: "Source", area: "front", leds: 8, status: "unresolved", changes: [] }],
    targets: [{ key: "target/main", name: "Main", device: "Target", max_leds: 16 }], unresolved: ["source/front"], skipped: [], warnings: [] };
  h.requests[0].resolve(preview);
  await loading;
  assert.equal(h.node("#rig-apply").disabled, true);
  const changed = h.node("#rig-preview").fire("change", { target: { dataset: { rigSource: "source/front" }, value: "target:target/main" } });
  await tick();
  assert.equal(h.requests[1].body.mapping["source/front"], "target/main");
  assert.equal(h.node("#rig-apply").disabled, true);
  h.requests[1].resolve({ ...preview, ready: true, preview_token: "reviewed", unresolved: [],
    zones: [{ ...preview.zones[0], status: "mapped", target: "target/main" }] });
  await changed;
  assert.equal(h.node("#rig-apply").disabled, false);
  const applying = h.node("#rig-apply").fire("click");
  assert.equal(h.requests[2].path, "/api/rig/apply");
  assert.equal(h.requests[2].body.preview_token, "reviewed");
  h.requests[2].reject(new Error("disk full"));
  await applying;
  assert.equal(h.node("#rig-apply").disabled, true);
  assert.match(h.node("#rig-error").textContent, /disk full/);
  const recheck = h.node("#rig-recheck").fire("click");
  assert.equal(h.requests[3].body.mapping["source/front"], "target/main");
  h.requests[3].resolve({ ...preview, ready: true, preview_token: "new-review", unresolved: [] });
  await recheck;
});

test("a confirmed import stays disabled while the screen refreshes", async () => {
  const h = await harness();
  h.ui.openRigImport();
  h.node("#rig-file").files = [{ size: 100, text: async () => JSON.stringify({ format: "airglow-rig", version: 1 }) }];
  const loading = h.node("#rig-file").fire("change");
  await tick();
  const result = { ready: true, preview_token: "reviewed", changes: { settings: [], curves: [], profiles: [], automations: [] },
    zones: [], targets: [], unresolved: [], skipped: [], warnings: [] };
  h.requests[0].resolve(result);
  await loading;
  h.delayRefresh();
  const applying = h.node("#rig-apply").fire("click");
  h.requests[1].resolve({ ...result, applied: true, preview_token: null });
  await tick();
  assert.equal(h.node("#rig-apply").disabled, true);
  await h.node("#rig-apply").fire("click");
  assert.equal(h.requests.length, 2);
  h.refreshes.forEach((resolve) => resolve({}));
  await applying;
});

test("a dismissed import does not reopen after its delayed successful commit", async () => {
  const h = await harness();
  h.ui.openRigImport();
  h.node("#rig-file").files = [{ size: 100, text: async () => "{}" }];
  const loading = h.node("#rig-file").fire("change"); await tick();
  const result = { ready: true, preview_token: "reviewed", changes: { settings: [], curves: [], profiles: [], automations: [] },
    zones: [], targets: [], unresolved: [], skipped: [], warnings: [] };
  h.requests[0].resolve(result); await loading;
  const applying = h.node("#rig-apply").fire("click");
  h.node("#sheet").open = false;
  h.requests[1].resolve({ ...result, applied: true }); await applying;
  assert.equal(h.node("#sheet").open, false);
});
