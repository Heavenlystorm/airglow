// Personal presentation only. These recipes never contain lighting or cooling controls.
import { isDesign } from "./design.js";

export const DEFAULT_APPEARANCE = Object.freeze({ accent: null, density: "comfortable", scale: 100, rig: "frame", motion: "system" });
const HEX = /^#[0-9a-f]{6}$/i;
const plain = (value) => !!value && typeof value === "object" && !Array.isArray(value);
const copy = (value) => structuredClone(value);

export function normalizeAppearance(value) {
  const v = plain(value) ? value : {};
  return {
    accent: typeof v.accent === "string" && HEX.test(v.accent) ? v.accent.toLowerCase() : null,
    density: v.density === "compact" ? "compact" : "comfortable",
    scale: [90, 100, 110, 120].includes(v.scale) ? v.scale : 100,
    rig: v.rig === "minimal" ? "minimal" : "frame",
    motion: v.motion === "reduced" ? "reduced" : "system",
  };
}

export function normalizeLooks(value) {
  const ids = new Set();
  return (Array.isArray(value) ? value : []).filter((v) => {
    if (!plain(v) || typeof v.id !== "string" || !/^[a-z0-9][a-z0-9-]{0,31}$/.test(v.id) || ids.has(v.id)
      || typeof v.name !== "string" || !v.name.trim() || v.name.trim().length > 40 || /[\x00-\x1f\x7f]/.test(v.name) || !isDesign(v.design)) return false;
    ids.add(v.id);
    return true;
  }).slice(0, 12).map((v) => ({ id: v.id, name: v.name.trim(), design: v.design, appearance: normalizeAppearance(v.appearance) }));
}

export function parseLookRecipe(text) {
  if (typeof text !== "string" || text.length > 65536) throw new Error("Choose an appearance recipe smaller than 64 KB.");
  let value;
  try { value = JSON.parse(text); } catch { throw new Error("This file is not valid JSON."); }
  if (!plain(value) || value.format !== "airglow-look" || value.version !== 1) throw new Error("Choose an Airglow appearance recipe (version 1).");
  if (!isDesign(value.design) || !plain(value.appearance)) throw new Error("The recipe has an unknown design or invalid appearance.");
  const appearance = normalizeAppearance(value.appearance);
  // Import is stricter than old stored preferences: never silently apply a damaged recipe.
  for (const [key, original] of Object.entries(value.appearance)) {
    if (!(key in DEFAULT_APPEARANCE) || (key === "accent" && typeof original === "string" ? original.toLowerCase() : original) !== appearance[key]) {
      throw new Error(`The recipe has an invalid ${key} value.`);
    }
  }
  if (typeof value.name !== "string" || !value.name.trim() || value.name.trim().length > 40 || /[\x00-\x1f\x7f]/.test(value.name)) throw new Error("Recipe names need 1–40 readable characters.");
  return { name: value.name.trim(), design: value.design, appearance };
}

export function encodeLookRecipe(value) {
  return JSON.stringify({ format: "airglow-look", version: 1, name: String(value.name || "My look").trim().slice(0, 40),
    design: isDesign(value.design) ? value.design : "graphite", appearance: normalizeAppearance(value.appearance) }, null, 2);
}

export function serializeWrites(write) {
  let tail = Promise.resolve();
  return (value) => {
    const snapshot = copy(value);
    const next = tail.then(() => write(snapshot));
    tail = next.catch(() => {});
    return next;
  };
}

const normalizeState = (value) => ({ design: isDesign(value?.design) ? value.design : "graphite",
  appearance: normalizeAppearance(value?.appearance), looks: normalizeLooks(value?.looks) });

export function createAppearanceTransaction(initial, { apply, save }) {
  const baseline = normalizeState(initial);
  let draft = copy(baseline), closed = false, pending = null;
  return {
    get draft() { return copy(draft); },
    get closed() { return closed; },
    get saving() { return !!pending; },
    preview(patch) {
      if (closed || pending) return;
      draft = normalizeState({ ...draft, ...patch });
      apply(copy(draft));
    },
    cancel() {
      if (closed || pending) return false;
      closed = true;
      apply(copy(baseline));
      return true;
    },
    keep() {
      if (pending) return pending;
      if (closed) return Promise.resolve(copy(draft));
      // Lock first, including if a save callback throws synchronously.
      pending = Promise.resolve();
      try {
        const result = save(copy(draft));
        pending = Promise.resolve(result).then((confirmed) => {
          draft = normalizeState(confirmed || draft);
          closed = true;
          apply(copy(draft));
          return copy(draft);
        }).finally(() => { pending = null; });
      } catch (error) {
        pending = null;
        return Promise.reject(error);
      }
      return pending;
    },
  };
}

function rgb(color) {
  if (/^#[0-9a-f]{3}$/i.test(color)) return [...color.slice(1)].map((c) => parseInt(c + c, 16));
  if (HEX.test(color)) return [1, 3, 5].map((i) => parseInt(color.slice(i, i + 2), 16));
  const values = String(color).match(/^rgba?\(\s*(\d+)[, ]+\s*(\d+)[, ]+\s*(\d+)/);
  return values ? values.slice(1, 4).map(Number) : [27, 30, 33];
}
const hex = (values) => "#" + values.map((n) => Math.round(n).toString(16).padStart(2, "0")).join("");
const mix = (a, b, amount) => a.map((n, i) => n + (b[i] - n) * amount);
const luminance = (color) => rgb(color).map((n) => n / 255).map((n) => n <= .04045 ? n / 12.92 : ((n + .055) / 1.055) ** 2.4)
  .reduce((sum, n, i) => sum + n * [.2126, .7152, .0722][i], 0);
export const contrastRatio = (a, b) => (Math.max(luminance(a), luminance(b)) + .05) / (Math.min(luminance(a), luminance(b)) + .05);

export function accentPalette(accent, backgrounds = ["#1b1e21"]) {
  const light = luminance(backgrounds[0]) > .4;
  const target = light ? [0, 0, 0] : [255, 255, 255];
  let chosen = accent;
  for (let step = 0; step <= 100; step++) {
    chosen = hex(mix(rgb(accent), target, step / 100));
    if (backgrounds.every((bg) => contrastRatio(chosen, bg) >= 4.6)) break;
  }
  const highlight = hex(mix(rgb(chosen), target, .12));
  const ink = contrastRatio(chosen, "#000000") >= contrastRatio(chosen, "#ffffff") ? "#000000" : "#ffffff";
  return { accent: chosen, highlight, ink };
}

const ACCENT_TOKENS = ["--brass", "--brass-hi", "--brass-glow", "--primary-ink", "--primary-edge", "--primary-hover-top", "--knob-on-top", "--knob-on-bottom", "--focus-color"];
let activeAppearance = normalizeAppearance();

export function applyAppearance(value, { persist = true } = {}) {
  const appearance = normalizeAppearance(value);
  activeAppearance = appearance;
  const root = document.documentElement;
  Object.assign(root.dataset, { density: appearance.density, scale: String(appearance.scale), rig: appearance.rig, motion: appearance.motion });
  root.style.setProperty("--ui-scale", String(appearance.scale / 100));
  for (const token of ACCENT_TOKENS) root.style.removeProperty(token);
  // High contrast deliberately keeps its established yellow/white focus language.
  root.toggleAttribute("data-custom-accent", !!appearance.accent && root.dataset.design !== "contrast");
  if (appearance.accent && root.dataset.design !== "contrast") {
    const style = getComputedStyle(root);
    const surfaces = ["--panel", "--ground", "--raised", "--well", "--hover"].map((name) => style.getPropertyValue(name).trim());
    const { accent, highlight, ink } = accentPalette(appearance.accent, surfaces);
    const values = [accent, highlight, `${accent}40`, ink, accent, highlight, highlight, accent, accent];
    ACCENT_TOKENS.forEach((token, i) => root.style.setProperty(token, values[i]));
  }
  if (persist) {
    try { localStorage.setItem("airglow.appearance", JSON.stringify(appearance)); } catch { /* server still retains confirmed preferences */ }
  }
  document.dispatchEvent(new CustomEvent("appearancechange", { detail: copy(appearance) }));
  return appearance;
}

if (typeof document !== "undefined") {
  document.addEventListener("designchange", () => applyAppearance(activeAppearance, { persist: false }));
}
