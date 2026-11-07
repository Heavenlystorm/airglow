import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

const file = new URL("../web/js/overview-rig.js", import.meta.url);
const tick = () => new Promise(setImmediate);
const zone = (key, positions, layout = {}) => ({ key, device: { online: true }, positions,
  config: { enabled: true, name: key, layout: { shape: "strip", x: .8, y: .5, length: .8, leds: positions.length, angle: 0, ...layout } } });

// Only the browser boundary is replaced. The Overview component and stage painter run unchanged.
async function harness({ actual3d = false } = {}) {
  assert.ok(existsSync(file), "Overview rig module exists");
  const paints = [], pending = new Map(), observers = [], renderers = [], viewChanges = [], selections = [];
  let sequence = 0, visible = true, imports = 0, resolveImport;
  const context2d = () => new Proxy({
    createRadialGradient: () => ({ addColorStop() {} }), createLinearGradient: () => ({ addColorStop() {} }),
    measureText: text => ({ width: text.length * 7 }),
    arc(...args) { paints.push(["arc", ...args]); },
  }, { get: (t, k) => k in t ? t[k] : () => {}, set(t, k, v) { t[k] = v; if (k === "fillStyle") paints.push([k, v]); return true; } });
  class Element {
    constructor(tag = "div") { this.tagName = tag.toUpperCase(); this.children = []; this.attrs = {}; this.events = new Map(); this.dataset = {}; this.hidden = false; this.isConnected = true; this.style = {}; this.value = ""; this.width = this.height = 0; this.classList = { add() {}, remove() {}, toggle() {} }; }
    setAttribute(k, v) { this.attrs[k] = String(v); }
    getAttribute(k) { return this.attrs[k] ?? null; }
    set innerHTML(html) {
      const stack = [this]; this.children = [];
      for (const match of html.matchAll(/<\/?([\w-]+)([^>]*)>/g)) {
        if (match[0].startsWith("</")) { stack.pop(); continue; }
        const child = new Element(match[1]);
        for (const attr of match[2].matchAll(/([\w-]+)="([^"]*)"/g)) { child.setAttribute(attr[1], attr[2]); if (attr[1] === "id" || attr[1] === "class") child[attr[1] === "class" ? "className" : "id"] = attr[2]; }
        stack.at(-1).append(child); if (match[1] !== "input") stack.push(child);
      }
    }
    querySelector(selector) { for (const child of this.children) { if (selector.startsWith("#") ? child.id === selector.slice(1) : child.tagName === selector.toUpperCase()) return child; const found = child.querySelector(selector); if (found) return found; } return null; }
    removeAttribute(k) { delete this.attrs[k]; }
    append(...els) { for (const e of els) { e.parentNode = this; this.children.push(e); } }
    replaceChildren(...els) { this.children.forEach(e => { e.parentNode = null; }); this.children = []; this.append(...els); }
    remove() { this.isConnected = false; if (this.parentNode) this.parentNode.children = this.parentNode.children.filter(e => e !== this); }
    contains(el) { return this === el || this.children.some(e => e.contains(el)); }
    closest() { return view; }
    getBoundingClientRect() { return { width: 600, height: 300, left: 100, top: 50 }; }
    getContext() { return this.ctx ||= context2d(); }
    addEventListener(type, fn, options = {}) { const list = this.events.get(type) || []; list.push(fn); this.events.set(type, list); options.signal?.addEventListener("abort", () => this.events.set(type, (this.events.get(type) || []).filter(x => x !== fn))); }
    fire(type, event = {}) { for (const fn of this.events.get(type) || []) fn({ target: this, preventDefault() {}, ...event }); }
    focus() {}
  }
  const view = new Element(), root = new Element(), host = new Element(); root.append(view); view.append(host);
  const document = new Element(); document.hidden = false; document.documentElement = root; document.body = root; root.dataset.rig = "frame";
  document.createElement = tag => new Element(tag);
  const observe = class { constructor(fn) { this.fn = fn; observers.push(this); } observe() {} disconnect() { this.stopped = true; } };
  const context = vm.createContext({ document, window: { devicePixelRatio: 1 }, AbortController, ResizeObserver: observe, MutationObserver: observe,
    localStorage: { getItem: () => null }, Option: function (text, value) { const option = new Element("option"); option.textContent = text; option.value = value; return option; },
    requestAnimationFrame(fn) { pending.set(++sequence, fn); return sequence; }, cancelAnimationFrame(id) { pending.delete(id); } });
  const rig3d = actual3d ? new vm.SourceTextModule(readFileSync(new URL("../web/js/rig-3d.js", import.meta.url), "utf8"), { context }) : new vm.SyntheticModule(["createRig3D"], function () { this.setExport("createRig3D", (target, options) => {
    const renderer = { target, options, frames: null, zones: null, visible: false, disposed: false,
      setFrames(frames) { this.frames = frames; }, setZones(zones) { this.zones = zones; }, select() {}, focus() {},
      setVisible(v) { this.visible = v; }, dispose() { this.disposed = true; } };
    renderers.push(renderer); return renderer;
  }); }, { context });
  await rig3d.link(() => {}); await rig3d.evaluate();
  const module = new vm.SourceTextModule(readFileSync(file, "utf8"), { context, identifier: "overview-rig.js",
    importModuleDynamically() { imports++; return new Promise(resolve => { resolveImport = () => resolve(rig3d); }); } });
  const stageSource = readFileSync(new URL("../web/js/stage.js", import.meta.url), "utf8");
  const stage = new vm.SourceTextModule(stageSource, { context });
  const names = stageSource.match(/import\s*\{([^}]+)\}/)[1].split(",").map(x => x.trim());
  const core = new vm.SyntheticModule(names, function () { for (const n of names) this.setExport(n, n === "clamp" ? (v, lo, hi) => Math.max(lo, Math.min(hi, v)) : n === "S" ? {} : () => {}); }, { context });
  await stage.link(() => core); await stage.evaluate();
  await module.link(() => stage); await module.evaluate();
  const rig = module.namespace.createOverviewRig(host, { onSelect: key => selections.push(key), onViewChange: view => viewChanges.push(view), isVisible: () => visible });
  const all = (el = host) => [el, ...el.children.flatMap(e => all(e))];
  return { ...module.namespace, rig, host, document, paints, pending, renderers, viewChanges, selections, rig3d: rig3d.namespace,
    get imports() { return imports; }, releaseImport: () => resolveImport?.(),
    elements: () => all(), find: cls => all().find(el => (el.className || "").split(" ").includes(cls)),
    flush() { const jobs = [...pending.values()]; pending.clear(); jobs.forEach(fn => fn()); },
    visible(v) { visible = v; observers.filter(o => !o.stopped).forEach(o => o.fn([])); },
    removed() { host.isConnected = false; observers.filter(o => !o.stopped).forEach(o => o.fn([])); },
  };
}

test("2D fits sparse real geometry and paints current frame colors without mutating data", async () => {
  const h = await harness(), zones = [zone("point", [[.8, .5]])], before = structuredClone(zones);
  h.rig.update({ zones, frames: { point: "ff0000" }, view: "2d" }); h.flush();
  assert.ok(h.paints.some(([kind, value]) => kind === "fillStyle" && value === "rgb(255,89.25,89.25)"));
  assert.ok(h.paints.some(([kind, x, y]) => kind === "arc" && x === 300 && y === 150), "isolated LED centers inside the canvas");
  assert.deepEqual(zones, before);
  assert.equal(h.imports, 0, "2D never loads 3D");
  h.paints.length = 0;
  h.rig.update({ zones: [zone("strip", [[.3, .5], [.5, .5]])], frames: { strip: "00ff000000ff" }, view: "2d" }); h.flush();
  const points = h.paints.filter(([kind, x, y]) => kind === "arc" && y === 150 && x > 0);
  assert.ok(Math.max(...points.map(p => p[1])) - Math.min(...points.map(p => p[1])) >= 250, "short strips use the focal area instead of the old thumbnail scale cap");
  assert.ok(h.paints.some(([kind, value]) => kind === "fillStyle" && value === "rgb(89.25,89.25,255)"));
  h.rig.dispose();
});

test("zone picking uses the paint transform and actual LED positions under CSS zoom", async () => {
  const h = await harness(), zones = [zone("point", [[.8, .5]])];
  h.rig.update({ zones, frames: {}, view: "2d" }); h.flush();
  h.find("overview-rig-canvas").fire("click", { clientX: 400, clientY: 200 });
  assert.deepEqual(h.selections, ["point"]);
  h.find("overview-rig-canvas").fire("click", { clientX: 200, clientY: 200 });
  assert.deepEqual(h.selections, ["point"], "nominal strip length must not create a fictional hit target");
  const pick = h.pickRigZone;
  const transform = { ox: 10, oy: 20, s: 1, u: 1000 };
  assert.equal(pick([zone("fan", [[.6, .5]], { shape: "fans", x: .5, y: .5, size: .2, fans: 1 })], transform, 510, 520), "fan");
  assert.equal(pick([zone("panel", [[.8, .5]], { shape: "matrix", cols: 4, rows: 2, length: .4, angle: 90 })], transform, 880, 680), "panel");
  h.rig.dispose();
});

test("view and zone controls are accessible and only explicit view clicks request persistence", async () => {
  const h = await harness();
  h.rig.update({ zones: [zone("<Fan>", [[.8, .5]])], frames: {}, view: "2d" });
  const buttons = h.elements().filter(el => el.tagName === "BUTTON");
  assert.deepEqual(buttons.filter(el => el.getAttribute("aria-pressed") != null).map(el => el.textContent), ["2D", "3D"]);
  const select = h.find("overview-rig-zones");
  assert.ok(select.getAttribute("aria-label"));
  assert.ok(select.children.some(el => el.textContent === "<Fan>"), "zone names are assigned as text");
  select.value = "<Fan>"; select.fire("change"); assert.deepEqual(h.selections, ["<Fan>"]);
  buttons.find(el => el.textContent === "3D").fire("click");
  assert.deepEqual(h.viewChanges, ["3d"]);
  h.visible(false); h.visible(true); h.flush();
  assert.deepEqual(h.viewChanges, ["3d"], "visibility and resize do not save presentation");
  h.rig.dispose();
});

test("empty and offline rigs stay empty, with no selectable imaginary geometry", async () => {
  const h = await harness(), offline = zone("gone", [[.8, .5]]); offline.device.online = false;
  h.rig.update({ zones: [offline], frames: { gone: "ff0000" }, view: "2d" }); h.flush();
  assert.equal(h.find("overview-rig-empty").hidden, false);
  assert.equal(h.find("overview-rig-zones").disabled, true);
  assert.equal(h.paints.filter(([op]) => op === "arc").length, 0);
  h.rig.dispose();
});

test("hidden, removed, and disposed rigs cancel paint work and stale lazy loads", async () => {
  for (const stop of [h => h.visible(false), h => h.removed(), h => h.rig.dispose(), h => h.rig.update({ zones: [], frames: {}, view: "2d" })]) {
    const h = await harness(); h.rig.update({ zones: [zone("point", [[.8, .5]])], frames: {}, view: "3d" });
    assert.equal(h.imports, 1); stop(h); h.releaseImport(); await tick(); h.flush();
    assert.equal(h.renderers.length, 0, "late 3D import cannot resurrect hidden or replaced content");
    h.rig.dispose(); assert.equal(h.pending.size, 0);
  }
});

test("3D receives the latest unmodified live state and suspends between visits", async () => {
  const h = await harness(), zones = [zone("strip", [[.3, .5], [.5, .5]])], first = { strip: "ff000000ff00" }, latest = { strip: "abcdef012345" };
  h.rig.update({ zones, frames: first, view: "3d" });
  h.rig.update({ zones, frames: latest, view: "3d" }); h.releaseImport(); await tick();
  assert.equal(h.renderers.length, 1);
  const r = h.renderers[0]; assert.equal(r.frames, latest); assert.equal(r.zones, zones);
  assert.equal(r.options.compact, true); assert.ok(r.options.idPrefix);
  assert.equal(r.visible, true); h.visible(false); assert.equal(r.visible, false);
  h.visible(true); assert.equal(r.visible, true);
  h.rig.dispose(); assert.equal(r.disposed, true); assert.equal(h.pending.size, 0);
});

test("the shared 3D renderer gives each Overview instance isolated IDs and a transparent canvas", async () => {
  const h = await harness({ actual3d: true });
  const a = h.rig3d.createRig3D(h.host, { compact: true, idPrefix: "overview-first", onSelect() {}, isVisible: () => true });
  const b = h.rig3d.createRig3D(h.host, { compact: true, idPrefix: "overview-second", onSelect() {}, isVisible: () => true });
  const ids = h.elements().map(el => el.id).filter(Boolean);
  assert.equal(new Set(ids).size, ids.length, "multiple preview instances must not duplicate IDs");
  assert.ok(ids.includes("overview-first-canvas")); assert.ok(ids.includes("overview-second-canvas"));
  a.setZones([zone("point", [[.8, .5]])]); a.setVisible(true); h.flush();
  assert.ok(!h.paints.some(([kind, color]) => kind === "fillStyle" && color === "#080c11"), "the compact canvas lets its theme surface show through");
  a.dispose(); b.dispose(); h.rig.dispose();
});
