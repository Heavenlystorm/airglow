import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

// Real Studio and gallery lifecycle; only the DOM, transport and sandbox renderer are adapters.
async function harness() {
  const requests = [], renderers = [], timers = new Map(), view = { hidden: false };
  let timer = 0;
  const card = { dataset: { card: "remote" }, isConnected: true, classList: { add() {}, remove() {} }, querySelector: () => null };
  const canvas = { closest: () => card }, events = new Map();
  const root = { closest: () => view, querySelectorAll: () => [],
    addEventListener(type, handler) { const handlers = events.get(type) || new Set(); handlers.add(handler); events.set(type, handlers); },
    fire(type, event) { for (const handler of events.get(type) || []) handler(event); },
  };
  const document = { hidden: false, documentElement: { dataset: {} } };
  const core = {
    $: selector => selector === "#studio" ? root : selector.includes(".st-live") ? canvas : null,
    $$: selector => selector === ".st-card" ? [card] : [],
    S: {}, esc: String, emit() {}, fail() {}, on() {}, toast() {}, rangeFill() {},
    setHTML: (element, html) => { element.innerHTML = html; }, coalesce: fn => fn,
    get: path => new Promise(resolve => requests.push({ path, resolve })), post() { throw new Error("Unexpected hardware mutation"); },
  };
  const context = vm.createContext({
    document, window: { matchMedia: () => ({ matches: false }) }, CSS: { escape: String },
    structuredClone, performance, console, location: { hash: "#overview" }, history: { replaceState() {} },
    clearInterval() {}, cancelAnimationFrame() {}, requestAnimationFrame: () => 1,
    setTimeout(fn) { timers.set(++timer, fn); return timer; }, clearTimeout(id) { timers.delete(id); },
    localStorage: { getItem: () => null, setItem() {}, removeItem() {} },
    IntersectionObserver: class {
      constructor(callback) { this.callback = callback; this.disconnected = false; }
      observe() {} unobserve() {} disconnect() { this.disconnected = true; }
    },
  });
  const real = ["studio", "studio-gallery", "studio-document", "studio-preview", "studio-variations"];
  const sources = new Map(real.map(name => [`./${name}.js`, readFileSync(new URL(`../web/js/${name}.js`, import.meta.url), "utf8")
    + (name === "studio" ? "\nexport { H, STILL, paintCommunity, renderCommunity, addStill, pump };" : "")]));
  const imports = new Map(), modules = new Map();
  for (const source of sources.values()) for (const match of source.matchAll(/import\s+\{([^}]+)\}\s+from\s+["']([^"']+)["']/g)) {
    const names = imports.get(match[2]) || new Set();
    match[1].split(",").forEach(name => names.add(name.trim().split(/\s+as\s+/)[0])); imports.set(match[2], names);
  }
  function moduleFor(specifier) {
    if (modules.has(specifier)) return modules.get(specifier);
    const module = sources.has(specifier) ? new vm.SourceTextModule(sources.get(specifier), { context })
      : new vm.SyntheticModule([...imports.get(specifier)], function () {
        for (const name of imports.get(specifier)) this.setExport(name,
          specifier === "./core.js" ? core[name]
          : name === "PreviewRunner" ? class {
            constructor() { renderers.push(this); }
            load() {} destroy() {} tick() {}
          } : name === "ledPoints" ? () => ({ points: [], slices: [] })
          : ["litZones", "sampleZones"].includes(name) ? () => [] : () => {});
      }, { context });
    modules.set(specifier, module); return module;
  }
  const module = moduleFor("./studio.js"); await module.link(moduleFor); await module.evaluate();
  const studio = module.namespace;
  studio.H.view = "community";
  studio.H.gallery = { effects: [{ id: "remote", name: "Remote", author: "Creator", description: "", tags: [] }] };
  return { studio, requests, renderers, view, root, card };
}

test("leaving Studio prevents a pending community effect response from starting a renderer in its hidden section", async () => {
  const h = await harness(); h.studio.paintCommunity();
  const observer = h.studio.H.io;
  const pending = observer.callback([{ isIntersecting: true, target: h.card }]);
  assert.equal(h.requests.length, 1);
  h.studio.leaveStudio(); h.view.hidden = true;
  h.requests[0].resolve({ effect: { kind: "layers", layers: [], schema: [] } }); await pending;
  assert.equal(h.renderers.length, 0, "the delayed effect must not render off-screen");
  assert.equal(h.studio.STILL.items.length, 0, "the abandoned card must not reenter the still queue");
});

test("an observer callback queued before leaving cannot fetch more effects after Studio closes", async () => {
  const h = await harness(); h.studio.paintCommunity();
  const observer = h.studio.H.io;
  h.studio.leaveStudio(); h.view.hidden = true;
  const pending = observer.callback([{ isIntersecting: true, target: h.card }]);
  assert.equal(h.requests.length, 0, "disconnecting must also invalidate already-queued observer deliveries");
  await pending;
});

test("a gallery listing arriving after leaving cannot replace the hidden Studio page", async () => {
  const h = await harness(), pending = h.studio.renderCommunity();
  h.studio.leaveStudio(); h.view.hidden = true;
  const before = h.root.innerHTML;
  h.requests[0].resolve({ effects: [] }); await pending;
  assert.equal(h.root.innerHTML, before);
});

test("returning to the gallery keeps the newer listing when the abandoned request arrives last", async () => {
  const h = await harness(), old = h.studio.renderCommunity();
  h.studio.leaveStudio(); h.studio.H.view = "community";
  const current = h.studio.renderCommunity();
  h.requests[1].resolve({ effects: [{ id: "new", name: "New", tags: [] }] }); await current;
  h.requests[0].resolve({ effects: [{ id: "old", name: "Old", tags: [] }] }); await old;
  assert.equal(h.studio.H.gallery.effects[0].name, "New");
});

test("a hidden Studio section refuses still work even while the document itself remains visible", async () => {
  const h = await harness(); h.view.hidden = true;
  h.studio.addStill({ key: "remote", eff: { kind: "layers", layers: { layers: [] } } });
  await h.studio.pump();
  assert.equal(h.renderers.length, 0);
});

test("keyboard focus on a scene starts its preview through the browser focusin event", async () => {
  const h = await harness(); h.studio.paintCommunity();
  h.studio.STILL.items.push({ key: "remote", eff: { kind: "layers", layers: { layers: [] } } });
  h.root.fire("focusin", { target: { closest: selector => selector === "[data-card]" ? h.card : null } });
  assert.equal(h.renderers.length, 1, "keyboard focus should start the scene preview");
});
