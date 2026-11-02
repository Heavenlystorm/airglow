// Actual profile form handlers with a tiny DOM and deferred network replies.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

async function harness() {
  const elements = new Map(), events = new Map(), storage = new Map(), requests = [];
  const node = (selector) => {
    if (!elements.has(selector)) elements.set(selector, {
      value: "", checked: false, hidden: false, disabled: false, events: new Map(),
      addEventListener(event, handler) { this.events.set(event, handler); },
      querySelectorAll() { return []; }, setAttribute() {}, focus() {},
      fire(event, data = {}) { return this.events.get(event)?.({ preventDefault() {}, ...data }); },
    });
    return elements.get(selector);
  };
  const openSheet = (html) => {
    node("#sheet").innerHTML = html;
    for (const tag of html.matchAll(/<(?:input|button|form|p)[^>]*\bid="([^"]+)"[^>]*>/g)) {
      const element = node(`#${tag[1]}`);
      element.value = tag[0].match(/\bvalue="([^"]*)"/)?.[1] || "";
      element.checked = /\bchecked\b/.test(tag[0]);
      element.disabled = /\bdisabled\b/.test(tag[0]);
    }
  };
  const core = {
    $: node, $$: () => [], S: { state: { scene: { effect: "solid", brightness: 75 }, active_preset: "night",
      fans: { base_curve: "balanced" } }, presets: [{ id: "night", name: "Night", fan_curve: "silent" }],
      rules: [], zones: { zones: [] }, curves: { curves: { balanced: [] } } },
    on: (event, fn) => events.set(event, fn), emit: (event, value) => events.get(event)?.(value),
    post: (path, body) => new Promise((resolve, reject) => requests.push({ path, body, resolve, reject })),
    get: async () => ({}), esc: String, curveLabel: String, effectName: String, toast() {}, fail() {}, sceneSwatch() {},
  };
  const source = readFileSync(new URL("../web/js/profiles.js", import.meta.url), "utf8");
  const context = vm.createContext({ console, structuredClone, document: { addEventListener() {} },
    sessionStorage: { getItem: (key) => storage.get(key), setItem: (key, value) => storage.set(key, value),
      removeItem: (key) => storage.delete(key) }, localStorage: { getItem: () => null },
    setTimeout, clearTimeout, setInterval, clearInterval });
  const imports = new Map([...source.matchAll(/import\s+\{([^}]+)\}\s+from\s+["']([^"']+)["']/g)]
    .map((match) => [match[2], match[1].split(",").map((name) => name.trim())]));
  const module = new vm.SourceTextModule(source, { context });
  await module.link((specifier) => new vm.SyntheticModule(imports.get(specifier), function () {
    for (const name of imports.get(specifier)) this.setExport(name,
      specifier === "./core.js" ? core[name] : name === "openSheet" ? openSheet : () => {});
  }, { context }));
  await module.evaluate();
  return { ui: module.namespace, core, node, requests, storage };
}

test("apply then adjust retains the stable profile update target", async () => {
  const h = await harness();
  h.core.emit("state");
  h.core.S.state.active_preset = null;
  h.core.emit("state");
  h.ui.saveProfileSheet();
  h.node("#sp-name").value = "Evening";
  const pending = h.node("#sp-form").fire("submit");
  assert.equal(h.requests[0].body.id, "night");
  assert.equal(h.requests[0].body.include_areas, true);
  h.requests[0].resolve({ presets: [{ id: "night", name: "Evening" }], id: "night" });
  await pending;
  assert.equal(h.core.S.state.active_preset, "night");
});

test("Save as new omits the update ID and keeps the include-fans default", async () => {
  const h = await harness();
  h.ui.saveProfileSheet();
  h.node("#sp-form").fire("change", { target: { id: "sp-new", checked: true } });
  h.node("#sp-name").value = "Another night";
  const pending = h.node("#sp-form").fire("submit");
  assert.equal(h.requests[0].body.id, undefined);
  assert.equal(h.requests[0].body.include_fans, true);
  h.requests[0].resolve({ presets: [{ id: "another-night" }], id: "another-night" });
  await pending;
});

test("a pending or failed save preserves draft fields and does not submit a duplicate", async () => {
  const h = await harness();
  h.ui.saveProfileSheet();
  h.node("#sp-name").value = "Keep this draft";
  const pending = h.node("#sp-form").fire("submit");
  h.node("#sp-form").fire("submit");
  assert.equal(h.requests.length, 1);
  h.requests[0].reject(new Error("disk full"));
  await pending;
  assert.equal(h.node("#sp-name").value, "Keep this draft");
  assert.equal(h.node("#sp-err").hidden, false);
  assert.match(h.node("#sp-err").textContent, /disk full/);
});
