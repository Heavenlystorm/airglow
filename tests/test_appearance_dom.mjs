import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

// Real appearance/design modules with only browser I/O replaced. Tests observe styles and cache writes.
async function harness() {
  const cache = new Map(), styles = new Map(), listeners = new Map(), attributes = new Set();
  const root = {
    dataset: { design: "graphite" },
    style: { setProperty: (key, value) => styles.set(key, value), removeProperty: (key) => styles.delete(key) },
    toggleAttribute(name, on) { if (on) attributes.add(name); else attributes.delete(name); },
    setAttribute(name, value) { if (name === "data-design") this.dataset.design = value; },
  };
  const document = { documentElement: root,
    addEventListener(name, handler) { listeners.set(name, handler); },
    dispatchEvent(event) { listeners.get(event.type)?.(event); },
  };
  const context = vm.createContext({ document, structuredClone, console,
    CustomEvent: class { constructor(type, options) { this.type = type; this.detail = options?.detail; } },
    localStorage: { getItem: (key) => cache.get(key), setItem: (key, value) => cache.set(key, value) },
    getComputedStyle: () => ({ getPropertyValue: (key) => styles.get(key) || ({
      "--panel": root.dataset.design === "paper" ? "#f5f2eb" : "#1b1e21",
      "--ground": root.dataset.design === "paper" ? "#e8e4db" : "#111315",
      "--raised": root.dataset.design === "paper" ? "#fcfaf4" : "#25292d",
      "--well": root.dataset.design === "paper" ? "#ded9cf" : "#121517",
      "--hover": root.dataset.design === "paper" ? "#efeadd" : "#2d3237",
    })[key] || "" }),
  });
  const modules = new Map();
  const load = async (name) => {
    if (!modules.has(name)) {
      const mod = new vm.SourceTextModule(readFileSync(new URL(`../web/js/${name}`, import.meta.url), "utf8"), { context });
      modules.set(name, mod);
      await mod.link((specifier) => load(specifier.replace("./", "")));
    }
    return modules.get(name);
  };
  const appearance = await load("appearance.js");
  await appearance.evaluate();
  const boot = () => vm.runInContext(readFileSync(new URL("../web/js/design-boot.js", import.meta.url), "utf8"), context);
  return { appearance: appearance.namespace, design: modules.get("design.js").namespace, cache, styles, root, attributes, boot };
}

test("preview changes the page without replacing confirmed boot preferences", async () => {
  const h = await harness();
  h.design.applyDesign("graphite");
  h.appearance.applyAppearance({ scale: 110, accent: "#00aacc" });
  const saved = h.cache.get("airglow.appearance");
  h.design.applyDesign("paper", { persist: false });
  h.appearance.applyAppearance({ scale: 120, motion: "reduced", rig: "minimal" }, { persist: false });
  assert.equal(h.root.dataset.design, "paper");
  assert.equal(h.root.dataset.motion, "reduced");
  assert.equal(h.root.dataset.rig, "minimal");
  assert.equal(h.cache.get("airglow.design"), "graphite");
  assert.equal(h.cache.get("airglow.appearance"), saved);
  h.boot();
  assert.equal(h.root.dataset.design, "graphite");
  assert.equal(h.root.dataset.scale, "110");
});

test("switching dark to paper remaps the same accent without warning or LED tokens", async () => {
  const h = await harness();
  h.appearance.applyAppearance({ accent: "#ffffff" });
  const dark = h.styles.get("--brass");
  h.design.applyDesign("paper");
  const paper = h.styles.get("--brass");
  assert.notEqual(dark, paper);
  assert.ok(h.appearance.contrastRatio(paper, "#ded9cf") >= 4.5);
  for (const name of ["--void", "--hot", "--warm", "--ok", "--data-line"]) assert.equal(h.styles.has(name), false);
  h.design.applyDesign("contrast");
  assert.equal(h.styles.has("--brass"), false);
  assert.equal(h.attributes.has("data-custom-accent"), false);
  h.design.applyDesign("graphite");
  assert.equal(h.styles.get("--brass"), dark);
});

test("clearing the accent removes every override and defaults persist after reset", async () => {
  const h = await harness();
  h.appearance.applyAppearance({ accent: "#ff0044", motion: "reduced", density: "compact" });
  h.appearance.applyAppearance(null);
  assert.deepEqual([...h.styles.keys()], ["--ui-scale"]);
  assert.equal(h.root.dataset.motion, "system");
  assert.equal(h.root.dataset.density, "comfortable");
  assert.equal(JSON.parse(h.cache.get("airglow.appearance")).accent, null);
});

test("prepaint ignores invalid sizes and survives damaged cached JSON", async () => {
  const h = await harness();
  h.cache.set("airglow.appearance", JSON.stringify({ scale: 999, density: "giant", motion: "always", rig: "giant" }));
  h.boot();
  assert.equal(h.root.dataset.scale, "100");
  assert.equal(h.styles.get("--ui-scale"), "1");
  assert.equal(h.root.dataset.density, "comfortable");
  h.cache.set("airglow.appearance", "{");
  assert.doesNotThrow(h.boot);
});
