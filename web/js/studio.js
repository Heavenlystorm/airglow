// The Studio: make lighting effects by stacking layers (or in code), see them on your own rig as you go, and share
// them with the community. docs/EFFECT-STUDIO.md has the design; effects run sandboxed through fxrun.js.
//
// What lives here, in order: the effect document and its history, the home page and gallery, the editor (layers,
// controls, master settings), the code path, the checks, sharing and the in-app guide.

import { $, $$, S, coalesce, emit, esc, fail, get, on, post, rangeFill, setHTML, toast } from "./core.js";
import { fieldHTML, wireFields } from "./lighting.js";
import { litZones, paintRig } from "./stage.js";
import { EffectRunner, ledPoints, runtimeAssets, sampleZones, testAudio, zoneFrames } from "./fxrun.js";
import { CODE_TEMPLATES, LAYER_TEMPLATES, REMIX, surprise } from "./studio-start.js";
import { closeSheet, openSheet } from "./sheet.js";

const AREAS = { all: "Whole case", front: "Front", top: "Top", rear: "Rear", bottom: "Bottom", side: "Side panel",
  cooler: "CPU cooler", ram: "Memory", board: "Motherboard", gpu: "Graphics card", desk: "Desk" };
const BLENDS = { normal: "Cover", add: "Add light", screen: "Brighten", multiply: "Darken" };
const LICENSES = ["MIT", "Apache-2.0", "BSD-3-Clause", "MPL-2.0", "GPL-3.0-or-later", "CC0 is not allowed"];
const MASTER_LABELS = { palette: "Colours", speed: "Speed", brightness: "Brightness", react: "Reactivity" };
const MAX_LAYERS = 12;
const MASTER = { palette: ["#ff2fb4", "#2f5bff"], speed: 100, brightness: 100, react: 50 };
const remember = (k, v) => { try { if (v === undefined) return localStorage.getItem(k) || ""; localStorage.setItem(k, v); } catch { return ""; } return v; };
const slug = (s) => String(s || "effect").toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 40) || "effect";
const uid = () => "k" + Math.random().toString(36).slice(2, 6);

const H = { lib: null, gallery: null, info: null, view: "home", search: "", tag: "" };   // home and community
const E = { doc: null, runner: null, slices: [], sel: "effect", lights: false, beat: false, raf: 0, last: 0,
            backdrop: null, frames: {}, ms: 0, fps: 0, frameCount: 0, framesSeen: 0, painted: -1, fpsAt: 0,
            problem: null, cm: null, logs: [], layersCopy: null, watch: null, gen: 0,
            t: 0, paused: false, scale: 1, overlay: false, strobeHz: 0, userParams: null,
            auto: 0, saved: null, applying: false, solo: null, errLine: null };
const HIST = { stack: [], i: -1, at: 0 };       // undo/redo: snapshots of the document, coalesced per gesture
const SW = { last: 0, dir: 0, peaks: [], warned: 0 };
let pending = null;                             // something Lighting asked the Studio to open
let sheetEffect = null;                         // the effect a sheet's buttons act on
let sheetGallery = null;                        // a gallery effect shown in the detail sheet

const box = () => $("#studio");

// ------------------------------------------------------------ unsaved work
// Switching tabs or closing the window used to throw away an effect you hadn't saved. Edits are kept here (in the
// window's own storage) and offered back on the Studio's home page, so nothing is lost and no dialog interrupts you.
const DRAFT_KEY = "airglow.studio.unsaved.v1";
let draftTimer = 0;

function rememberDraft() {
  clearTimeout(draftTimer);
  draftTimer = setTimeout(() => {
    try {
      if (E.doc?.dirty) localStorage.setItem(DRAFT_KEY, JSON.stringify({ doc: E.doc, at: Date.now() }));
      else localStorage.removeItem(DRAFT_KEY);
    } catch { /* storage off or full: the draft is a safety net, not the only copy */ }
  }, 700);
}

function readDraft() {
  try {
    const d = JSON.parse(localStorage.getItem(DRAFT_KEY) || "null");
    if (!d?.doc || !d.doc.name || Date.now() - (d.at || 0) > 7 * 864e5) return null;
    return d.doc;
  } catch { return null; }
}

function forgetDraft() {
  clearTimeout(draftTimer);
  try { localStorage.removeItem(DRAFT_KEY); } catch { /* nothing to forget */ }
}

function saveDraftNow() {                 // leaving the editor before the debounce fired
  clearTimeout(draftTimer);
  try {
    if (E.doc?.dirty) localStorage.setItem(DRAFT_KEY, JSON.stringify({ doc: E.doc, at: Date.now() }));
  } catch { /* storage off or full */ }
}

function draftBanner() {
  const d = readDraft();
  if (!d) return "";
  return `<div class="banner" id="st-draft"><span>You have unsaved changes to <b>${esc(d.name)}</b>.</span>
    <div class="btn-row"><button class="btn" data-draft="resume">Resume</button><button class="btn quiet" data-draft="discard">Discard</button></div></div>`;
}

function wireDraft() {
  const d = readDraft();
  if (!d) return;
  box().querySelector('[data-draft="resume"]')?.addEventListener("click", () => {
    openEditor(d);                    // E.doc is set straight away
    saveDraftNow();                   // keep the safety net until it's actually saved
  });
  box().querySelector('[data-draft="discard"]')?.addEventListener("click", () => { forgetDraft(); renderHome(false); });
}

// ------------------------------------------------------------ entry points
export function initStudio() {
  box().addEventListener("click", onClick);
  box().addEventListener("dragover", (e) => {
    if ([...(e.dataTransfer?.types || [])].includes("Files")) e.preventDefault();
  });
  box().addEventListener("drop", (e) => {
    const file = e.dataTransfer?.files?.[0];
    if (file) { e.preventDefault(); importFile(file); }
  });
  box().addEventListener("keydown", (e) => {          // layer rows are list items: Enter or Space picks one
    if ((e.key === "Enter" || e.key === " ") && e.target.matches("li[data-sel]")) { e.preventDefault(); e.target.click(); }
  });
  // One listener on the shared sheet, so opening a sheet twice never runs an action twice.
  $("#sheet").addEventListener("click", onSheetClick);
  document.addEventListener("keydown", onKey);
  window.addEventListener("resize", () => { E.painted = -1; });      // the preview canvas changed size: repaint it
  on("state", () => { followLights(); if (!box().closest(".view").hidden) paintProblems(); });
  on("zones", () => { if (E.runner) restartRunner(); });
}

export function openStudio(req) {
  pending = req;
  if (location.hash === "#studio/edit") return renderStudio("edit");
  location.hash = "#studio/edit";
}

function onKey(e) {
  if (!E.doc) return;
  const typing = e.target?.matches?.("input, textarea, select, [contenteditable=true]") || e.target?.closest?.(".CodeMirror");
  if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "s") {
    e.preventDefault();
    save();
    return;
  }
  if ($("#sheet").open) return;         // a dialog owns the keyboard while it is up
  if (typing) return;
  if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "z") {
    e.preventDefault();
    histGo(e.shiftKey ? 1 : -1);
  } else if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "y") {
    e.preventDefault();
    histGo(1);
  } else if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "d" && typeof E.sel === "number") {
    e.preventDefault();
    dupLayer();
  } else if ((e.key === "Delete" || e.key === "Backspace") && typeof E.sel === "number") {
    e.preventDefault();
    delLayer();
  } else if (e.key === "Escape" && typeof E.sel === "number") {
    E.sel = "effect"; rerenderSides();
  }
}

// called by main.js's router: #studio, #studio/new, #studio/community
export async function renderStudio(arg) {
  if (!H.info) H.info = (await runtimeAssets()).layerInfo;
  Object.assign(MASTER, H.info.master || {});
  if (pending) {
    const req = pending;
    pending = null;
    if (E.doc) closeEditor(true);
    if (req.builtin) return remixBuiltin(req.builtin, req.params);
    await loadLibrary();
    const eff = H.lib.effects.find((x) => x.fx_id === req.fx_id);
    if (eff) return openEditor(docFromEffect(eff, { remix: eff.origin !== "mine" }));
  }
  if (E.doc && arg === "edit") return;
  if (E.doc && arg !== "edit") closeEditor();
  H.view = arg === "community" ? "community" : "home";
  if (H.view === "community") return renderCommunity();
  await loadLibrary();
  renderHome(arg === "new");
}

export function leaveStudio() {
  if (E.lights) setLights(false);
  stopStills();
  closeEditor(true);
}

// Lighting's "Create" and "Remix" buttons
export function remixBuiltin(effectId, params) {
  const make = REMIX[effectId];
  if (!make) return;
  const name = S.effects.find((e) => e.id === effectId)?.name || effectId;
  openEditor(newDoc({ name: `My ${name.toLowerCase()}`, layers: make(params), remix_of: effectId }));
}
export const canRemix = (effectId) => !!REMIX[effectId] || effectId.startsWith("fx:");

async function loadLibrary() {
  try { H.lib = await get("/api/studio/effects"); } catch (e) { fail(e); H.lib = { effects: [] }; }
}

// ------------------------------------------------------------ documents
function newDoc(d) {
  return normalizeDoc({ fx_id: null, name: "Untitled effect", description: "", tags: [], remix_of: "", mode: "layers",
                        layers: [], master: structuredClone(MASTER), expose: {}, source: "", values: {},
                        ...structuredClone(d), dirty: !d.fx_id });
}

function normalizeDoc(doc) {
  doc.master = { ...structuredClone(MASTER), ...(doc.master || {}) };
  doc.master.palette = Array.isArray(doc.master.palette) && doc.master.palette.length >= 2
    ? doc.master.palette.slice(0, 4) : structuredClone(MASTER.palette);
  doc.master.speed = Number(doc.master.speed ?? 100);
  doc.master.brightness = Number(doc.master.brightness ?? 100);
  doc.master.react = Number(doc.master.react ?? 50);
  doc.expose = doc.expose && typeof doc.expose === "object" ? doc.expose : {};
  doc.tags = Array.isArray(doc.tags) ? doc.tags : [];
  doc.values = doc.values && typeof doc.values === "object" ? doc.values : {};
  const seen = new Set();
  doc.layers = (Array.isArray(doc.layers) ? doc.layers : []).map((layer, i) => {
    // A v1 layer has no id; its old control ids were l<index>_<setting>, so keep that as its id. Layers made from
    // now on get a random id, and a reorder no longer moves anyone's controls.
    let id = layer.id;
    if (!id || !/^[a-z0-9][a-z0-9-]{0,15}$/.test(id) || seen.has(id)) id = String(i);
    while (seen.has(id)) id = uid();
    seen.add(id);
    return { ...layer, id, enabled: layer.enabled !== false, opacity: layer.opacity ?? 100,
             blend: layer.blend || "normal", mask: layer.mask || "all", palette: !!layer.palette,
             react: { what: layer.react?.what === "speed" ? "speed" : "strength", amount: Number(layer.react?.amount) || 0 },
             params: layer.params || {}, expose: layer.expose || {} };
  });
  return doc;
}

function docFromEffect(eff, { remix = false } = {}) {
  const d = { fx_id: remix ? null : eff.fx_id, name: remix ? `${eff.name} remix` : eff.name,
              description: eff.description || "", tags: eff.tags || [], remix_of: remix ? eff.id || eff.remix_of : eff.remix_of || "" };
  if (eff.kind === "layers") return newDoc({ ...d, mode: "layers", layers: eff.layers || [],
                                             master: eff.master || structuredClone(MASTER), expose: eff.expose || {} });
  return newDoc({ ...d, mode: "code", source: remix ? retitle(eff.source, d.name, d.remix_of) : eff.source });
}

// a copied code effect: new title, credit the original, drop the author (it's yours now)
function retitle(source, name, remixOf) {
  let s = source.replace(/<title>[\s\S]*?<\/title>/i, `<title>${esc(name)}</title>`).replace(/<meta name="author"[^>]*>\n?/i, "");
  if (remixOf && !/name="remix-of"/i.test(s)) s = s.replace(/(<meta name="airglow-effect"[^>]*>)/i, `$1\n<meta name="remix-of" content="${esc(remixOf)}">`);
  return s;
}

// ------------------------------------------------------------ the file: reading and writing
// Code effects may declare their controls with airglow.params({...}); the Studio writes them into the file's meta
// tags when it saves, so the app (and the lights) read them like any other control.
function specFromFile(p) {
  if (p.type === "number") return { id: p.id, label: p.label, type: "range", min: p.min, max: p.max, step: p.step || 1, default: p.default, unit: "" };
  if (p.type === "color") return { id: p.id, label: p.label, type: "color", default: p.default };
  if (p.type === "colors") return { id: p.id, label: p.label, type: "colors", default: p.default, min: 2, max: 4 };
  if (p.type === "boolean") return { id: p.id, label: p.label, type: "toggle", default: p.default };
  const options = (p.options || []).map((o) => ({ value: o, label: o }));
  return { id: p.id, label: p.label, type: "choice", default: p.default, options };
}

// A small, strict reader for the object literal in airglow.params({...}): keys, strings, numbers, booleans, arrays.
function parseLiteral(src, i = 0) {
  const skip = () => { while (i < src.length && /\s/.test(src[i])) i++; };
  function text(q) {
    let out = ""; i++;
    while (i < src.length && src[i] !== q) {
      if (src[i] === "\\") { i++; out += src[i++]; } else out += src[i++];
    }
    i++;
    return out;
  }
  function value() {
    skip();
    const c = src[i];
    if (c === "{" || c === "[") {
      const arr = c === "[", out = arr ? [] : {};
      i++; skip();
      const close = arr ? "]" : "}";
      while (i < src.length && src[i] !== close) {
        skip();
        if (arr) out.push(value());
        else {
          let key;
          if (src[i] === '"' || src[i] === "'") key = text(src[i]);
          else { key = ""; while (i < src.length && /[A-Za-z0-9_$]/.test(src[i])) key += src[i++]; }
          skip();
          if (src[i] !== ":") throw new Error("bad object");
          i++;
          out[key] = value();
        }
        skip();
        if (src[i] === ",") i++;
        skip();
      }
      i++;
      return out;
    }
    if (c === '"' || c === "'") return text(c);
    const n = /^-?\d+(?:\.\d+)?/.exec(src.slice(i));
    if (n) { i += n[0].length; return Number(n[0]); }
    for (const [word, v] of [["true", true], ["false", false], ["null", null]]) {
      if (src.startsWith(word, i)) { i += word.length; return v; }
    }
    throw new Error("can't read the value");
  }
  return value();
}

function declaredParams(code) {
  const at = code.search(/airglow\.params\s*\(/);
  if (at < 0) return null;
  let i = code.indexOf("(", at) + 1;
  while (i < code.length && /\s/.test(code[i])) i++;
  if (code[i] !== "{") return null;
  try {
    const raw = parseLiteral(code, i);
    const out = [];
    for (const [id, s] of Object.entries(raw)) {
      const label = String(s?.label || id).slice(0, 32);
      const type = s?.type === "color" ? "color" : s?.type === "colors" ? "colors" : s?.type === "boolean" ? "boolean" : s?.type === "select" ? "select" : "number";
      if (type === "number") out.push({ id, label, type, min: +s.min, max: +s.max, step: +(s.step || 1), default: +s.default });
      else if (type === "color") out.push({ id, label, type, default: String(s.default || "#ffffff").toLowerCase() });
      else if (type === "colors") out.push({ id, label, type, default: (Array.isArray(s.default) ? s.default : String(s.default || "").split(",")).map((c) => String(c).trim()).filter(Boolean) });
      else if (type === "boolean") out.push({ id, label, type, default: s.default === true });
      else out.push({ id, label, type, options: (Array.isArray(s.options) ? s.options : []).map(String), default: String(s.default || (s.options || [])[0] || "") });
    }
    return out;
  } catch { return null; }
}

function metaParams(d) {
  return [...d.querySelectorAll("meta[property]")].map((m) => {
    const a = (k) => m.getAttribute(k);
    const id = a("property"), label = a("label") || id, type = a("type");
    if (type === "number") return { id, label, type, min: +a("min"), max: +a("max"), step: +(a("step") || 1), default: +a("default") };
    if (type === "color") return { id, label, type, default: (a("default") || "#ffffff").toLowerCase() };
    if (type === "colors") return { id, label, type, default: (a("default") || "").split(",").map((s) => s.trim()).filter(Boolean) };
    if (type === "boolean") return { id, label, type, default: a("default") === "true" };
    const options = (a("options") || "").split(",").map((s) => s.trim()).filter(Boolean);
    return { id, label, type: "select", options, default: a("default") || options[0] };
  });
}

// read a code effect file in the page (DOMParser runs nothing): its controls, defaults and script
function parseSource(source) {
  const d = new DOMParser().parseFromString(source, "text/html");
  const meta = metaParams(d);
  const script = [...d.querySelectorAll("script")].find((s) => !s.type || s.type === "text/javascript" || s.type === "module");
  const layersEl = d.querySelector('script[type="application/json"]#layers');
  let layers = null;
  try { layers = layersEl ? JSON.parse(layersEl.textContent) : null; } catch { layers = null; }
  const declared = meta.length ? null : declaredParams(script?.textContent || "");
  const fileParams = meta.length ? meta : (declared || []);
  return { name: d.querySelector("title")?.textContent.trim() || "", code: script?.textContent || "", layers, fileParams,
           params: fileParams.map(specFromFile), declared: !!declared,
           audio: /audio/.test(d.querySelector('meta[name="permissions"]')?.getAttribute("content") || "") };
}

function metaLine(p) {
  let a = `property="${esc(p.id)}" label="${esc(p.label)}" type="${p.type}"`;
  if (p.type === "number") a += ` min="${p.min}" max="${p.max}" step="${p.step || 1}" default="${p.default}"`;
  else if (p.type === "colors") a += ` default="${esc((p.default || []).join(","))}"`;
  else if (p.type === "boolean") a += ` default="${p.default ? "true" : "false"}"`;
  else if (p.type === "select") a += ` options="${esc((p.options || []).join(","))}" default="${esc(p.default)}"`;
  else a += ` default="${esc(p.default)}"`;
  return `<meta ${a}>`;
}

function injectControls(source, params) {
  let s = source.replace(/^<meta property="[^"]*"[^\n]*>\n?/gm, "");
  if (!params.length) return s;
  const block = params.map(metaLine).join("\n");
  const at = s.search(/<script[\s>]/i);
  if (at >= 0) return s.slice(0, at) + block + "\n" + s.slice(at);
  return s + "\n" + block + "\n";
}

const codeSource = (doc) => {
  const p = parseSource(doc.source);
  let s = p.declared ? injectControls(doc.source, p.fileParams) : doc.source;
  // An effect that reads the music needs permissions=audio in the file, or the lights hand it null.
  if (!/<meta[^>]*\bname=["']?permissions/i.test(s) && /frame\.audio|ctx\.audio|audio\.bands/.test(s)) {
    s = s.replace(/(<meta[^>]*\bname=["']?airglow-effect["']?[^>]*>)/i, `$1\n<meta name="permissions" content="audio">`);
  }
  return s;
};

function usesAudio(doc) {
  if (!doc) return false;
  if (doc.mode === "layers") return doc.layers.some((l) => l.enabled !== false && ((H.info?.types[l.type]?.audio) || (l.react?.amount || 0) > 0));
  return /frame\.audio|ctx\.audio|audio\.bands/.test(doc.source);
}

// ------------------------------------------------------------ layers as the files and the runner see them
function layerConfig(doc = E.doc) {
  const layers = doc.layers.map((l) => (E.solo && l.id !== E.solo ? { ...l, enabled: false } : l));
  return { v: 2, layers, master: doc.master, expose: doc.expose };
}

function runnable(doc) {
  if (doc.mode === "layers") return { kind: "layers", layers: layerConfig(doc), params: E.userParams || {} };
  const p = parseSource(doc.source);
  const values = Object.fromEntries(p.params.map((s) => [s.id, doc.values[s.id] ?? s.default]));
  return { kind: "code", code: p.code, params: values };
}

const usesAudioDoc = (doc) => usesAudio(doc);

// the parts the server turns into an effect file (builder), or the file itself (code)
function payload(doc) {
  if (doc.mode === "code") return { source: codeSource(doc) };
  return { name: doc.name, description: doc.description, tags: doc.tags, remix_of: doc.remix_of,
           layers: doc.layers, master: doc.master, expose: doc.expose };
}

// ------------------------------------------------------------ undo and redo
function snapDoc(doc = E.doc) {
  if (!doc) return "null";
  return JSON.stringify({ name: doc.name, description: doc.description, tags: doc.tags, remix_of: doc.remix_of,
                          mode: doc.mode, layers: doc.layers, master: doc.master, expose: doc.expose,
                          source: doc.source, values: doc.values });
}

function histReset() {
  HIST.stack = [snapDoc()];
  HIST.i = 0;
  HIST.at = 0;
}

function histPush() {
  if (!E.doc || E.applying) return;
  const snap = snapDoc(), now = performance.now();
  if (HIST.stack[HIST.i] === snap) return;
  // A slider drag makes dozens of changes: they become one step. The first snapshot is never replaced.
  if (HIST.i > 0 && HIST.i === HIST.stack.length - 1 && now - HIST.at < 500) {
    HIST.stack[HIST.i] = snap;
  } else {
    HIST.stack = HIST.stack.slice(0, HIST.i + 1);
    HIST.stack.push(snap);
    if (HIST.stack.length > 60) HIST.stack.shift();
    HIST.i = HIST.stack.length - 1;
  }
  HIST.at = now;
}

async function histGo(dir) {
  if (!E.doc) return;
  const next = HIST.i + dir;
  if (next < 0 || next >= HIST.stack.length) return;
  HIST.i = next;
  const snap = JSON.parse(HIST.stack[next]);
  const fx = E.doc.fx_id;
  E.applying = true;
  Object.assign(E.doc, snap);
  E.doc.fx_id = fx;
  normalizeDoc(E.doc);
  E.applying = false;
  E.solo = null;
  if (typeof E.sel === "number") E.sel = Math.min(E.sel, Math.max(0, E.doc.layers.length - 1));
  if (E.doc.mode === "code") {
    renderEditor();
    await restartRunner();
    E.doc.dirty = true;
    setSavedLabel();
  } else {
    rerenderSides();
    changed(true, { hist: false });
  }
}

// ------------------------------------------------------------ home
function cardHTML(eff, acts) {
  const by = eff.origin === "community" ? `by ${esc(eff.author)}` : eff.kind === "code" ? "Code" : `${eff.layers?.length || 0} layer${eff.layers?.length === 1 ? "" : "s"}`;
  const music = eff.permissions?.includes("audio") ? `<i class="st-badge">Music</i>` : "";
  return `<div class="st-card" data-card="${esc(eff.fx_id || eff.id)}"><canvas class="st-live" aria-hidden="true"></canvas>
    <div class="st-cap"><strong>${esc(eff.name)}</strong><span>${by} ${music}</span></div>
    <div class="st-acts">${acts}</div></div>`;
}

function renderHome(pickStart) {
  const effects = H.lib?.effects || [];
  const mine = effects.filter((e) => e.origin === "mine"), community = effects.filter((e) => e.origin === "community");
  const using = S.state?.scene?.effect;
  const rig = litZones().length;
  setHTML(box(), `<div class="st-home">
    <div class="page-head"><div><h1>Studio</h1>
      <p class="lede">Make your own effects by stacking layers, or write one in code. It plays on your own case while you build it, and Share sends it to everyone's gallery.</p></div>
      <div class="btn-row"><button class="btn" data-st="learn">Learn</button><a class="btn" href="#studio/community">Community effects</a>
      <button class="btn" data-st="surprise">Surprise me</button><button class="btn primary" data-st="new">New effect</button></div></div>
    ${draftBanner()}
    ${rig ? "" : `<div class="banner"><span>No lights are set up yet, so previews use a sample case. <a href="#devices">Set up your lights</a> to see your own.</span></div>`}
    <section class="st-sec${pickStart ? " flash" : ""}" id="st-start"><h2 class="st-h">Start from</h2>
      <div class="st-grid">${LAYER_TEMPLATES.map((t) => `<button class="st-card st-tpl" data-card="${t.id}" data-tpl="${t.id}"><canvas class="st-live" aria-hidden="true"></canvas>
        <div class="st-cap"><strong>${esc(t.name)}</strong><span>${esc(t.description)}</span></div></button>`).join("")}
        ${CODE_TEMPLATES.map((t) => `<button class="st-card st-tpl" data-card="code-${t.id}" data-code-tpl="${t.id}"><canvas class="st-live" aria-hidden="true"></canvas>
        <div class="st-cap"><strong>${esc(t.name)} <i class="st-badge">Code</i></strong><span>${esc(t.description)}</span></div></button>`).join("")}</div></section>
    <section class="st-sec"><h2 class="st-h">Your effects</h2>
      ${mine.length ? `<div class="st-grid">${mine.map((e) => cardHTML(e, `
        <button class="btn${using === e.fx_id ? " on" : ""}" data-use="${esc(e.fx_id)}">${using === e.fx_id ? "On your lights" : "Use"}</button>
        <button class="btn" data-edit="${esc(e.fx_id)}">Edit</button>
        <button class="icon-btn" data-more="${esc(e.fx_id)}" aria-label="More for ${esc(e.name)}"><svg><use href="#i-more"/></svg></button>`)).join("")}</div>`
        : `<p class="hint">Nothing yet. Pick a starting point above: it takes a few minutes, and every change shows on your lights right away.</p>`}</section>
    ${community.length ? `<section class="st-sec"><h2 class="st-h">From the community</h2><div class="st-grid">${community.map((e) => cardHTML(e, `
        <button class="btn${using === e.fx_id ? " on" : ""}" data-use="${esc(e.fx_id)}">${using === e.fx_id ? "On your lights" : "Use"}</button>
        <button class="btn" data-remix="${esc(e.fx_id)}">Remix</button>
        <button class="icon-btn" data-more="${esc(e.fx_id)}" aria-label="More for ${esc(e.name)}"><svg><use href="#i-more"/></svg></button>`)).join("")}</div></section>` : ""}
    <section class="st-sec"><div class="btn-row"><button class="btn quiet" data-st="import">Import an effect file</button>
      <span class="hint">An effect is one HTML file. Drop one anywhere on this page to open it.</span>
      <input type="file" id="st-import" accept=".html,text/html" hidden></div></section>
  </div>`);
  if (pickStart) $("#st-start")?.scrollIntoView({ block: "start" });
  wireDraft();
  $("#st-import")?.addEventListener("change", (e) => { const f = e.target.files?.[0]; e.target.value = ""; if (f) importFile(f); });
  queueStills([
    ...LAYER_TEMPLATES.map((t) => ({ key: t.id, eff: { kind: "layers", layers: { v: 2, layers: t.layers, master: t.master || {}, expose: t.expose || {} }, params: {} } })),
    ...CODE_TEMPLATES.map((t) => ({ key: `code-${t.id}`, eff: { kind: "code", code: t.code, params: Object.fromEntries(t.params.map((p) => [p.id, p.default])) } })),
    ...effects.map((e) => ({ key: e.fx_id, eff: effectRunnable(e) })),
  ]);
}

function effectRunnable(e) {
  const params = Object.fromEntries((e.schema || []).map((s) => [s.id, s.default]));
  if (e.kind === "layers") return { kind: "layers", layers: { v: 2, layers: e.layers || [], master: e.master || {}, expose: e.expose || {} }, params };
  return { kind: "code", code: e.code, params };
}

// ------------------------------------------------------------ the preview rig: real lights, or the sample case
const rigZones = () => (litZones().length ? litZones() : sampleZones());

// ------------------------------------------------------------ live cards: a still for each, live while pointed at
const STILL = { queue: [], items: [], running: false, live: null, gen: 0 };

function stopStills() { STILL.queue = []; STILL.items = []; STILL.gen++; STILL.live?.stop(); STILL.live = null; }

function queueStills(items) { stopStills(); wireHover(); items.forEach(addStill); }

function addStill(item) { STILL.items.push(item); STILL.queue.push(item); pump(); }

// one effect at a time: start it, take one frame, stop it
async function pump() {
  if (STILL.running) return;
  STILL.running = true;
  const gen = STILL.gen;
  while (STILL.queue.length && gen === STILL.gen) {
    const it = STILL.queue.shift();
    const c = cardCanvas(it.key);
    if (c) await still(c, it.eff, gen).catch(() => {});
  }
  STILL.running = false;
  if (STILL.queue.length) pump();
}

function wireHover() {
  box().onpointerover = (e) => {
    const card = e.target.closest("[data-card]");
    const key = card?.dataset.card;
    if (STILL.live?.key === key) return;
    if (!key) {                                   // off the cards but still in the Studio: stop the preview
      STILL.live?.stop();
      STILL.live = null;
      return;
    }
    STILL.live?.stop();
    STILL.live = null;
    const item = STILL.items.find((i) => i.key === key);
    if (item) STILL.live = playOn(card.querySelector(".st-live"), item.eff, key);
  };
  box().onpointerleave = () => { STILL.live?.stop(); STILL.live = null; };
}

const cardCanvas = (key) => $(`[data-card="${CSS.escape(key)}"] .st-live`, box());

function still(canvas, eff, gen) {
  return new Promise((resolve) => {
    const zones = rigZones();
    const { points, slices } = ledPoints(zones);
    let done = false;
    const finish = () => { if (done) return; done = true; runner.destroy(); resolve(); };
    const runner = new EffectRunner({
      onReady: () => runner.tick(2.0, 1 / 30, testAudio(2.0)),
      onFrame: (colors) => { if (gen === STILL.gen) paintRig(canvas, zoneFrames(colors, slices), { pad: 10, zones }); finish(); },
      onStopped: finish, onError: finish,
    });
    runner.load(eff, points);
    setTimeout(finish, 2500);
  });
}

function playOn(canvas, eff, key) {
  const zones = rigZones();
  const { points, slices } = ledPoints(zones);
  let raf = 0, t0 = performance.now(), last = t0;
  const runner = new EffectRunner({ onFrame: (colors) => paintRig(canvas, zoneFrames(colors, slices), { pad: 10, zones }) });
  runner.load(eff, points);
  const loop = (now) => { const t = (now - t0) / 1000 + 2; runner.tick(t, (now - last) / 1000, testAudio(t)); last = now; raf = requestAnimationFrame(loop); };
  raf = requestAnimationFrame(loop);
  return { key, stop: () => { cancelAnimationFrame(raf); runner.destroy(); } };
}

// ------------------------------------------------------------ community
async function renderCommunity() {
  setHTML(box(), `<div class="st-home"><div class="page-head"><div><h1>Community effects</h1>
    <p class="lede">Effects people made in the Studio and shared. Point at one to see it on your own case.</p></div>
    <div class="btn-row"><button class="btn" data-st="learn">Learn</button><a class="btn" href="#studio">Back to the Studio</a></div></div>
    <p class="hint">Loading the gallery from GitHub…</p></div>`);
  try {
    H.gallery = await get("/api/studio/gallery");
    for (const r of H.gallery.revoked_now || []) toast(`${r.name} was taken down from the gallery and is off now${r.reason ? `: ${r.reason}` : ""}.`, true);
  } catch (e) {
    setHTML(box(), `<div class="st-home"><div class="page-head"><div><h1>Community effects</h1>
      <p class="lede">The gallery didn't load: ${esc(e.message)}</p></div><div class="btn-row"><a class="btn" href="#studio">Back</a>
      <button class="btn primary" data-st="community-retry">Try again</button></div></div></div>`);
    return;
  }
  paintCommunity();
}

function paintCommunity() {
  const g = H.gallery;
  const q = H.search.toLowerCase();
  const tags = [...new Set(g.effects.flatMap((e) => e.tags))].sort();
  const items = g.effects.filter((e) => (!H.tag || (H.tag === "music" ? e.audio : e.tags.includes(H.tag)))
    && (!q || `${e.name} ${e.author} ${e.description} ${e.tags.join(" ")}`.toLowerCase().includes(q)));
  setHTML(box(), `<div class="st-home"><div class="page-head"><div><h1>Community effects</h1>
    <p class="lede">${g.effects.length} effects people made in the Studio and shared. Point at one to see it on your own case.</p></div>
    <div class="btn-row"><button class="btn" data-st="learn">Learn</button><a class="btn" href="#studio">Back to the Studio</a></div></div>
    <div class="st-filter"><input class="text" id="st-search" placeholder="Search names, authors, tags" value="${esc(H.search)}" spellcheck="false">
      <div class="chips"><button class="chip-btn" data-tag="" aria-pressed="${!H.tag}">All</button><button class="chip-btn" data-tag="music" aria-pressed="${H.tag === "music"}">Music</button>
      ${tags.map((t) => `<button class="chip-btn" data-tag="${esc(t)}" aria-pressed="${H.tag === t}">${esc(t)}</button>`).join("")}</div></div>
    ${items.length ? `<div class="st-grid">${items.map((e) => `<div class="st-card" data-card="${esc(e.id)}"><canvas class="st-live" aria-hidden="true"></canvas>
      <div class="st-cap"><strong>${esc(e.name)}${e.featured ? ` <span class="tag new">Featured</span>` : ""}</strong><span>by ${esc(e.author)}${e.audio ? ` <i class="st-badge">Music</i>` : ""}</span>
      ${e.description ? `<p class="st-desc">${esc(e.description)}</p>` : ""}</div>
      <div class="st-acts">${e.installed && !e.update ? `<button class="btn" disabled>Installed</button>` : `<button class="btn primary" data-install="${esc(e.id)}">${e.update ? "Update" : "Install"}</button>`}
        <button class="btn" data-gremix="${esc(e.id)}">Remix</button>
        <button class="btn quiet" data-gdet="${esc(e.id)}">How it's made</button></div></div>`).join("")}</div>`
      : `<p class="hint">${g.effects.length ? "Nothing matches." : "No shared effects yet. Make one and press Share: yours could be the first."}</p>`}
  </div>`);
  $("#st-search")?.addEventListener("input", (e) => { H.search = e.target.value; clearTimeout(H.t); H.t = setTimeout(() => { paintCommunity(); $("#st-search").focus(); }, 250); });
  // stills need each effect's file: fetched as cards come into view
  queueStills([]);
  H.io?.disconnect();                 // every repaint (a search keystroke) used to leave an observer watching old cards
  const io = (H.io = new IntersectionObserver(async (entries) => {
    for (const en of entries) {
      if (!en.isIntersecting) continue;
      io.unobserve(en.target);
      const id = en.target.dataset.card;
      try {
        const { effect } = await get(`/api/studio/gallery/effect?id=${encodeURIComponent(id)}`);
        addStill({ key: id, eff: effectRunnable(effect) });
      } catch { /* the card stays dark */ }
    }
  }));
  $$(".st-card", box()).forEach((c) => io.observe(c));
}

// ------------------------------------------------------------ the editor
async function openEditor(doc) {
  stopStills();
  E.doc = normalizeDoc(doc);
  E.sel = E.doc.mode === "layers" && E.doc.layers.length ? E.doc.layers.length - 1 : "effect";
  E.logs = [];
  E.problem = null;
  E.layersCopy = null;
  E.solo = null;
  E.userParams = null;
  E.paused = false;
  E.scale = 1;
  E.overlay = false;
  E.t = 0;
  E.strobeHz = 0;
  E.errLine = null;
  Object.assign(SW, { last: 0, dir: 0, peaks: [], warned: 0 });
  if (location.hash !== "#studio/edit") history.replaceState(null, "", "#studio/edit");
  // A brand-new effect that follows the music is easier to build with the test beat already on.
  if (!E.doc.fx_id && !E.beat && usesAudio(E.doc)) E.beat = true;
  // Where Revert goes back to: this, or the last time Save was pressed. Autosave does not move it.
  E.saved = snapDoc();
  histReset();
  renderEditor();
  await restartRunner();
  E.last = performance.now();
  cancelAnimationFrame(E.raf);
  E.raf = requestAnimationFrame(loop);
  setSavedLabel();
}

function closeEditor(keepHash) {
  clearTimeout(E.auto);
  saveDraftNow();
  cancelAnimationFrame(E.raf);
  E.runner?.destroy();
  E.runner = null;
  clearInterval(E.watch);
  E.watch = null;
  E.doc = null;
  E.saved = null;
  if (!keepHash && location.hash === "#studio/edit") history.replaceState(null, "", "#studio");
}

function renderEditor() {
  const d = E.doc;
  setHTML(box(), `<div class="st-edit ${d.mode === "code" ? "is-code" : ""}">
    <header class="st-top">
      <a class="icon-btn" href="#studio" aria-label="Back to the Studio"><svg><use href="#i-back"/></svg></a>
      <input class="st-name" id="st-name" value="${esc(d.name)}" aria-label="Effect name" maxlength="40" spellcheck="false">
      <div class="seg" role="radiogroup" aria-label="How you edit it"><button role="radio" data-mode="layers" aria-checked="${d.mode === "layers"}">Layers</button><button role="radio" data-mode="code" aria-checked="${d.mode === "code"}">Code</button></div>
      <span class="spacer"></span>
      <span class="st-saved" id="st-saved" aria-live="polite">${d.dirty ? "Not saved" : "Saved"}</span>
      <button class="btn" data-st="revert" title="Back to where this session started, or your last Save" ${E.saved && snapDoc() !== E.saved ? "" : "disabled"}>Revert</button>
      <button class="btn" data-st="check">Check</button>
      <button class="btn${d.dirty ? " primary" : ""}" data-st="save">Save</button>
      <button class="btn" data-st="use">Use on my lights</button>
      <button class="btn" data-st="share">Share</button>
    </header>
    <aside class="st-left" id="st-left">${d.mode === "layers" ? layersHTML() : `<div class="st-code" id="st-code"></div>`}</aside>
    <section class="st-mid">
      <div class="st-stagebar">
        <label class="st-sw"><span class="switch"><input type="checkbox" id="st-lights"${E.lights ? " checked" : ""}><span></span></span> Also on my lights</label>
        <label class="st-sw"><span class="switch"><input type="checkbox" id="st-beat"${E.beat ? " checked" : ""}><span></span></span> Test beat</label>
        <div class="st-dbg" role="group" aria-label="Preview controls">
          <button class="btn quiet" data-dbg="pause" aria-pressed="${E.paused}" title="Pause the preview">${E.paused ? "Play" : "Pause"}</button>
          <button class="btn quiet" data-dbg="slow" aria-pressed="${E.scale !== 1}" title="Slow motion">${E.scale === 1 ? "1×" : "¼×"}</button>
          <button class="icon-btn" data-dbg="restart" title="Start the clock again" aria-label="Start the clock again"><svg><use href="#i-rotate"/></svg></button>
          <button class="icon-btn" data-dbg="overlay" aria-pressed="${E.overlay}" title="Show the coordinates and LED numbers" aria-label="Show the coordinates and LED numbers"><svg><use href="#i-sliders"/></svg></button>
          <button class="icon-btn" data-dbg="snap" title="Save the preview as a picture" aria-label="Save the preview as a picture"><svg><use href="#i-down"/></svg></button>
        </div>
        <span class="spacer"></span><span class="st-meter" id="st-meter"></span>
      </div>
      <div class="st-stage"><canvas id="st-canvas" aria-label="Your effect on your case"></canvas>
        ${litZones().length ? "" : `<p class="st-sample">Sample case · <a href="#devices">set up your lights</a></p>`}</div>
      <div class="st-console" id="st-console" aria-live="polite"></div>
    </section>
    <aside class="st-right" id="st-right">${inspectorHTML()}</aside>
  </div>`);
  wireEditor();
  if (d.mode === "code") mountCode();
  paintConsole();
  setSavedLabel();
}

// ---- left: layers, top layer first (it's drawn last)
function layersHTML() {
  const d = E.doc, T = H.info.types;
  const items = d.layers.map((l, i) => ({ l, i })).reverse();
  return `<div class="st-layers">
    <button class="st-layer st-eff" data-sel="effect" aria-pressed="${E.sel === "effect"}"><b>${esc(d.name)}</b><span>Palette, knobs, description</span></button>
    <ol class="st-list" id="st-list">${items.map(({ l, i }) => `<li class="st-layer" data-sel="${i}" aria-pressed="${E.sel === i}" draggable="true" role="button" tabindex="0">
      <span class="st-chip" style="background:${chipColor(l)}"></span>
      <span class="st-lname"><b>${esc(T[l.type]?.name || l.type)}${E.solo === l.id ? ` <i class="st-badge">Solo</i>` : ""}</b><span>${esc(layerLine(l))}</span></span>
      <button class="icon-btn" data-toggle="${i}" aria-label="${l.enabled === false ? "Show" : "Hide"} layer" aria-pressed="${l.enabled !== false}"><svg><use href="#i-${l.enabled === false ? "eye-off" : "eye"}"/></svg></button></li>`).join("")}</ol>
    <div class="st-add"><button class="btn" data-st="add" aria-expanded="false">Add a layer</button>
      <div class="st-menu" id="st-menu" hidden>${["Colour", "Motion", "Music"].map((g) => `<div class="st-mgroup"><span>${g}</span>
        ${Object.entries(T).filter(([, t]) => t.group === g).map(([id, t]) => `<button data-add="${id}"><b>${esc(t.name)}</b><span>${esc(t.description)}</span></button>`).join("")}</div>`).join("")}</div></div>
    <p class="hint">Layers higher up draw over the ones below. Drag to reorder. ${d.layers.length} of ${MAX_LAYERS} layers.</p></div>`;
}

function chipColor(l) {
  if (l.palette) return `linear-gradient(90deg, ${(E.doc.master.palette || []).join(", ")})`;
  const p = { ...(H.info.types[l.type]?.params || []).reduce((o, s) => ({ ...o, [s.id]: s.default }), {}), ...l.params };
  if (p.colors) return `linear-gradient(90deg, ${p.colors.join(", ")})`;
  if (p.color) return p.color;
  return "linear-gradient(90deg, #ff2fb4, #ffb21a, #22e06b, #00b4ff, #8a3dff)";
}

function layerLine(l) {
  const bits = [];
  if (l.palette) bits.push("palette");
  if ((l.opacity ?? 100) < 100) bits.push(`${l.opacity}%`);
  if (l.blend && l.blend !== "normal") bits.push(BLENDS[l.blend].toLowerCase());
  if (l.mask && l.mask !== "all") bits.push(`only ${AREAS[l.mask].toLowerCase()}`);
  if ((l.react?.amount || 0) > 0) bits.push("follows the music");
  return bits.join(", ") || "Whole case";
}

// ---- right: the selected layer, or the effect's details
const MASTER_SPECS = [
  { id: "palette", label: "Palette", type: "colors", default: MASTER.palette, min: 2, max: 4 },
  { id: "speed", label: "Speed, all layers", type: "range", default: 100, min: 0, max: 200, step: 5, unit: "%" },
  { id: "brightness", label: "Brightness", type: "range", default: 100, min: 0, max: 100, step: 1, unit: "%" },
  { id: "react", label: "Music reactivity", type: "range", default: 50, min: 0, max: 100, step: 1, unit: "%" },
];
const GENERAL_SPECS = [
  { id: "opacity", label: "Strength", type: "range", min: 0, max: 100, step: 1, unit: "%", default: 100 },
  { id: "blend", label: "Mix with the layers below", type: "choice", default: "normal", options: Object.entries(BLENDS).map(([value, label]) => ({ value, label })) },
  { id: "mask", label: "Where", type: "choice", default: "all", options: Object.entries(AREAS).map(([value, label]) => ({ value, label })) },
];

function exposeRowHtml(scope, key) {
  const on = scope === "m" ? !!E.doc.expose?.[key] : !!E.doc.layers?.[E.sel]?.expose?.[key];
  return `<label class="st-expose"><input type="checkbox" data-${scope === "m" ? "mexpose" : "expose"}="${key}"${on ? " checked" : ""}> Show as a control</label>`;
}

function masterHTML() {
  const d = E.doc;
  const exposed = Object.entries(d.expose || {});
  const palCount = d.layers.filter((l) => l.palette).length;
  const hint = palCount
    ? `${palCount} layer${palCount === 1 ? "" : "s"} take${palCount === 1 ? "s" : ""} colour from this palette. Speed and brightness apply to the whole effect.`
    : "No layer uses the palette yet. In a layer with a colour, tick “Take its colour from the effect palette”, and this changes all of them at once. Speed and brightness apply to the whole effect.";
  return `<div class="ins-block"><div class="ins-title"><h2>Effect</h2></div>
    <div class="field"><label class="lab" for="st-desc"><span>Description</span></label><textarea class="text" id="st-desc" rows="3" maxlength="200" placeholder="What does it look like?">${esc(d.description)}</textarea></div>
    <div class="field"><label class="lab" for="st-tags"><span>Tags</span></label><input class="text" id="st-tags" value="${esc(d.tags.join(", "))}" placeholder="ambient, warm, music" spellcheck="false"></div>
    ${d.remix_of ? `<p class="hint">A remix of ${esc(d.remix_of)}. Sharing credits the original.</p>` : ""}
    <div class="sub-head">Everything at once</div>
    <div id="st-master">${MASTER_SPECS.map((s) => fieldHTML(s, d.master[s.id], "mm", { reset: true }) + exposeRowHtml("m", s.id)).join("")}</div>
    <p class="hint">${hint}</p>
    <div class="sub-head">Controls people can change</div>
    ${exposed.length || d.layers.some((l) => Object.keys(l.expose || {}).length) ? `<ul class="st-exposed">
      ${exposed.map(([k, label]) => `<li><input class="text" data-expose-label="m:${k}" value="${esc(label)}" maxlength="32" aria-label="Control name"><span>whole effect</span></li>`).join("")}
      ${d.layers.flatMap((l, i) => Object.entries(l.expose || {}).map(([k, label]) => `<li><input class="text" data-expose-label="l:${i}:${k}" value="${esc(label)}" maxlength="32" aria-label="Control name"><span>${esc(H.info.types[l.type]?.name || l.type)}</span></li>`)).join("")}
    </ul>` : `<p class="hint">None yet. Tick “Show as a control” under a setting, and it appears in everyone's effect panel.</p>`}
    <div class="btn-row"><button class="btn" data-st="userview">See it as a user</button><button class="btn quiet" data-st="export">Export .html</button></div>
  </div>`;
}

function inspectorHTML() {
  const d = E.doc;
  if (d.mode === "code") return codeInspectorHTML();
  if (E.sel === "effect") return masterHTML();
  const l = d.layers[E.sel], t = H.info.types[l.type];
  const vals = { ...Object.fromEntries(t.params.map((s) => [s.id, s.default])), ...l.params };
  const hasColour = t.params.some((s) => s.id === "color" || s.id === "colors");
  return `<div class="ins-block"><div class="ins-title"><h2>${esc(t.name)}</h2></div>
    <div class="btn-row st-layer-acts">
      <button class="icon-btn" data-st="up" aria-label="Move layer up" title="Move up"><svg><use href="#i-up"/></svg></button>
      <button class="icon-btn" data-st="down" aria-label="Move layer down" title="Move down"><svg><use href="#i-down"/></svg></button>
      <button class="btn quiet" data-st="solo" aria-pressed="${E.solo === l.id}">Solo</button>
      <button class="icon-btn" data-st="dup" aria-label="Duplicate layer" title="Duplicate"><svg><use href="#i-copy"/></svg></button>
      <button class="icon-btn" data-st="del" aria-label="Delete layer" title="Delete"><svg><use href="#i-trash"/></svg></button></div>
    <p class="ins-desc">${esc(t.description)}${t.audio ? " It follows what's playing; switch on Test beat to build it without music." : ""}</p>
    <div class="ins-block" id="st-params">${t.params.map((s) => fieldHTML(s, vals[s.id], "lp", { reset: true }) + exposeRowHtml("l", s.id)).join("")}</div>
    <div class="ins-block" id="st-react">
      <div class="sub-head">Follow the music</div>
      ${fieldHTML({ id: "react", label: "React to music", type: "range", min: 0, max: 100, step: 1, unit: "%", default: 0 }, l.react?.amount || 0, "lr", { reset: true })}
      ${fieldHTML({ id: "what", label: "What reacts", type: "choice", default: "strength", options: [{ value: "strength", label: "Strength" }, { value: "speed", label: "Speed" }] }, l.react?.what || "strength", "lw")}
      <p class="hint">${(l.react?.amount || 0) > 0 ? "It moves with the music; Test beat is on so you can see it." : "At 0 it ignores the music."}</p>
    </div>
    ${hasColour ? `<label class="st-expose"><input type="checkbox" data-layer-palette="1"${l.palette ? " checked" : ""}> Take its colour from the effect palette</label>` : ""}
    <div class="rule-line"></div>
    <div class="ins-block" id="st-general">${GENERAL_SPECS.map((s) => fieldHTML(s, l[s.id] ?? s.default, "lg", { reset: true })).join("")}</div></div>`;
}

function codeInspectorHTML() {
  const p = parseSource(E.doc.source);
  const vals = Object.fromEntries(p.params.map((s) => [s.id, E.doc.values[s.id] ?? s.default]));
  return `<div class="ins-block"><div class="ins-title"><h2>Controls</h2></div>
    ${p.params.length ? `<div class="ins-block" id="st-cparams">${p.params.map((s) => fieldHTML(s, vals[s.id], "cp", { reset: true })).join("")}</div>`
      : `<p class="hint">Add controls in code with <code>airglow.params({ speed: { label: "Speed", type: "number", min: 0, max: 5, default: 1 } })</code>, or with <code>&lt;meta property="speed" ...&gt;</code> tags.</p>`}
    ${p.declared ? `<p class="hint">These are written into the file when you save; the <code>airglow.params(...)</code> call stays next to your code.</p>` : ""}
    <div class="rule-line"></div>
    <div class="sub-head">Effect API</div>
    <ul class="st-api"><li><code>render(ctx, frame)</code> draw on <code>ctx.canvas</code> (320 × 200)</li>
      <li><code>point(led, frame)</code> or answer per LED: <code>[r, g, b]</code></li>
      <li><code>led.local</code>, <code>led.fan</code>, <code>led.area</code>, <code>ctx.rig</code></li>
      <li><code>ctx.params</code> your controls, <code>console.log()</code> prints below the preview</li>
      <li><code>frame.time</code>, <code>frame.dt</code> in seconds; <code>frame.audio</code> <code>{level, bass, beat, bands[64]}</code></li></ul>
    <div class="btn-row"><button class="btn" data-st="learn">Guide and recipes</button><button class="btn" data-st="editor">Open in my editor</button></div>
    <p class="hint" id="st-editor-note">Opens the file in VS Code (or its folder); every save reloads the preview.</p></div>`;
}

function wireEditor() {
  const d = E.doc;
  $("#st-name").addEventListener("input", (e) => {
    d.name = e.target.value;
    if (d.mode === "code") {
      d.source = d.source.replace(/<title>[\s\S]*?<\/title>/i, `<title>${esc(d.name)}</title>`);
      if (E.cm && E.cm.getValue() !== d.source) E.cm.setValue(d.source);
    }
    const eff = $(".st-eff b"); if (eff) eff.textContent = d.name;
    changed(false);
  });
  $("#st-lights").addEventListener("change", (e) => setLights(e.target.checked));
  $("#st-beat").addEventListener("change", (e) => { E.beat = e.target.checked; });
  $("#st-import")?.addEventListener("change", (e) => { const f = e.target.files?.[0]; e.target.value = ""; if (f) importFile(f); });
  wireInspector();
  wireDrag();
}

function wireInspector() {
  const d = E.doc;
  if (d.mode === "code") {
    const p = parseSource(d.source);
    const root = $("#st-cparams");
    if (root) wireFields(root, { params: p.params }, Object.fromEntries(p.params.map((s) => [s.id, d.values[s.id] ?? s.default])), (vals) => {
      d.values = vals;
      E.runner?.setParams(vals);
    });
    return;
  }
  if (E.sel === "effect") {
    $("#st-desc")?.addEventListener("input", (e) => { d.description = e.target.value; changed(false); });
    $("#st-tags")?.addEventListener("input", (e) => { d.tags = e.target.value.split(",").map((t) => t.trim().toLowerCase().replace(/[^a-z0-9-]/g, "")).filter(Boolean).slice(0, 5); changed(false); });
    wireFields($("#st-master"), { params: MASTER_SPECS }, { palette: d.master.palette, speed: d.master.speed, brightness: d.master.brightness, react: d.master.react }, (v) => {
      Object.assign(d.master, v);
      changed(true);
      // a palette change repaints every chip that takes its colour from it
      $$("#st-list li[data-sel]").forEach((li) => {
        const layer = d.layers[+li.dataset.sel];
        if (layer) li.querySelector(".st-chip").style.background = chipColor(layer);
      });
    });
    $$("[data-mexpose]").forEach((cb) => cb.addEventListener("change", () => {
      const k = cb.dataset.mexpose;
      if (cb.checked) d.expose[k] = MASTER_LABELS[k];
      else delete d.expose[k];
      changed(false);
      rerenderSides();
    }));
    $$("[data-expose-label]").forEach((inp) => inp.addEventListener("input", () => {
      const [scope, a, b] = inp.dataset.exposeLabel.split(":");
      const label = inp.value.trim().slice(0, 32);
      if (scope === "m") d.expose[a] = label || MASTER_LABELS[a];
      else {
        const layer = d.layers[+a];
        const spec = H.info.types[layer.type]?.params.find((s) => s.id === b);
        layer.expose[b] = label || spec?.label || b;
      }
      changed(false);
    }));
    return;
  }
  const l = d.layers[E.sel], t = H.info.types[l.type];
  const vals = { ...Object.fromEntries(t.params.map((s) => [s.id, s.default])), ...l.params };
  wireFields($("#st-params"), { params: t.params }, vals, (v) => { l.params = v; changed(true); refreshLayerRow(); });
  wireFields($("#st-react"), { params: [
    { id: "react", label: "React to music", type: "range", min: 0, max: 100, step: 1, unit: "%", default: 0 },
    { id: "what", label: "What reacts", type: "choice", default: "strength", options: [{ value: "strength", label: "Strength" }, { value: "speed", label: "Speed" }] }] },
    { react: l.react?.amount || 0, what: l.react?.what || "strength" }, (v) => {
      l.react = { what: v.what, amount: v.react };
      if (v.react > 0 && !E.beat) { E.beat = true; const cb = $("#st-beat"); if (cb) cb.checked = true; toast("Test beat is on, so you can see it without music"); }
      changed(true);
      refreshLayerRow();
    });
  const general = GENERAL_SPECS;
  wireFields($("#st-general"), { params: general }, { opacity: l.opacity ?? 100, blend: l.blend || "normal", mask: l.mask || "all" }, (v) => {
    Object.assign(l, v); changed(true); refreshLayerRow();
  });
  $$("[data-expose]").forEach((cb) => cb.addEventListener("change", () => {
    const k = cb.dataset.expose, spec = t.params.find((s) => s.id === k);
    l.expose ||= {};
    if (cb.checked) {
      const taken = new Set([...Object.values(E.doc.expose || {}), ...E.doc.layers.flatMap((x) => Object.values(x.expose || {}))]);
      l.expose[k] = taken.has(spec.label) ? `${t.name} ${spec.label.toLowerCase()}` : spec.label;
    } else delete l.expose[k];
    changed(false);
    rerenderSides();
  }));
  $("[data-layer-palette]")?.addEventListener("change", (e) => { l.palette = e.target.checked; changed(true); refreshLayerRow(); rerenderSides(); });
}

function refreshLayerRow() {
  const li = $(`#st-list [data-sel="${E.sel}"]`);
  const l = E.doc?.layers?.[E.sel];
  if (!li || !l) return;
  li.querySelector(".st-chip").style.background = chipColor(l);
  li.querySelector(".st-lname span").textContent = layerLine(l);
}

function wireDrag() {
  const list = $("#st-list");
  if (!list) return;
  let from = null;
  list.addEventListener("dragstart", (e) => { from = +e.target.closest("[data-sel]")?.dataset.sel; e.dataTransfer.effectAllowed = "move"; });
  list.addEventListener("dragover", (e) => { if (from !== null) e.preventDefault(); });
  list.addEventListener("drop", (e) => {
    e.preventDefault();
    const to = +e.target.closest("[data-sel]")?.dataset.sel;
    if (from === null || Number.isNaN(to) || to === from) return;
    const [l] = E.doc.layers.splice(from, 1);
    E.doc.layers.splice(to, 0, l);
    E.sel = to; from = null;
    rerenderSides(); changed(true);
  });
}

function rerenderSides() {
  if (E.doc.mode === "layers") { $("#st-left").innerHTML = layersHTML(); wireDrag(); }
  $("#st-right").innerHTML = inspectorHTML();
  $$("#st-right input[type=range]").forEach(rangeFill);
  wireInspector();
}

function moveLayer(dir) {
  if (typeof E.sel !== "number") return;
  const to = E.sel + dir;
  if (to < 0 || to >= E.doc.layers.length) return;
  const [l] = E.doc.layers.splice(E.sel, 1);
  E.doc.layers.splice(to, 0, l);
  E.sel = to;
  rerenderSides(); changed(true);
}

function dupLayer() {
  if (typeof E.sel !== "number") return;
  if (E.doc.layers.length >= MAX_LAYERS) return toast(`${MAX_LAYERS} layers is the most an effect can have.`, true);
  const copy = structuredClone(E.doc.layers[E.sel]);
  copy.id = uid(); copy.expose = {};
  E.doc.layers.splice(E.sel + 1, 0, copy);
  E.sel++;
  rerenderSides(); changed(true);
}

function delLayer() {
  if (typeof E.sel !== "number") return;
  if (E.doc.layers.length === 1) return toast("An effect needs at least one layer.", true);
  const doc = E.doc, index = E.sel;
  const removed = E.doc.layers.splice(E.sel, 1)[0];
  E.sel = Math.min(E.sel, E.doc.layers.length - 1);
  if (E.solo === removed.id) E.solo = null;
  rerenderSides(); changed(true);
  toast("Layer deleted", { action: { label: "Undo", onClick: () => {
    if (E.doc !== doc) return;
    E.doc.layers.splice(index, 0, removed);
    E.sel = index;
    rerenderSides(); changed(true);
  } } });
}

function toggleSolo() {
  if (typeof E.sel !== "number") return;
  const id = E.doc.layers[E.sel].id;
  E.solo = E.solo === id ? null : id;
  rerenderSides(); changed(true);
}

// ------------------------------------------------------------ changes: preview, lights, saved state
const pushLayers = coalesce(() => E.runner?.setLayers(layerConfig()), 30);

function changed(visual, { hist = true, auto = true } = {}) {
  if (!E.doc) return;
  E.doc.dirty = true;
  setSavedLabel();
  if (visual && E.doc.mode === "layers") pushLayers();
  rememberDraft();
  if (hist) histPush();
  if (auto) scheduleSave();
  syncLights();
}

function setSavedLabel() {
  const s = $("#st-saved");
  if (s) s.textContent = E.doc?.dirty ? "Not saved" : "Saved";
  const b = $('[data-st="save"]');
  b?.classList.toggle("primary", !!E.doc?.dirty);
  const rev = $('[data-st="revert"]');
  if (rev) rev.disabled = !(E.saved && E.doc && snapDoc() !== E.saved);
}

function scheduleSave() {
  clearTimeout(E.auto);
  if (!E.doc?.fx_id) return;
  E.auto = setTimeout(() => quietSave(), 1500);
}

async function quietSave() {
  if (!E.doc?.fx_id || !E.doc.dirty) return;
  return save({ quiet: true });
}

const syncLights = coalesce(async () => {
  if (!E.lights || !E.doc) return;
  try { await post("/api/studio/draft", { ...payload(E.doc), values: E.doc.values, lights: true }); E.problem = null; }
  catch (e) { E.problem = e.message; paintConsole(); }
}, 350);

// picking a profile (the tray, say) ends what the Studio plays on your lights: the switch follows
function followLights() {
  const playing = S.state?.trial?.kind === "studio";
  if (E.lights && playing) E.lightsSeen = true;
  else if (E.lights && E.lightsSeen && !playing) {
    E.lights = E.lightsSeen = false;
    const cb = $("#st-lights"); if (cb) cb.checked = false;
  }
}

async function setLights(on) {
  E.lights = on; E.lightsSeen = false;
  const cb = $("#st-lights"); if (cb) cb.checked = on;
  try {
    if (on) { await post("/api/studio/draft", { ...payload(E.doc), values: E.doc.values, lights: true }); toast("Playing on your lights"); }
    else await post("/api/studio/draft", { lights: false });
    emit("refresh");
  } catch (e) { E.lights = false; if (cb) cb.checked = false; fail(e); }
}

async function restartRunner() {
  E.runner?.destroy();
  E.backdrop = null;
  E.painted = -1;                  // a new runner: draw its first frame even if the counter happens to match
  const d = E.doc;
  if (!d) return;
  const zones = rigZones();
  const { points, slices } = ledPoints(zones);
  E.slices = slices;
  const gen = ++E.gen;
  E.runner = new EffectRunner({
    onFrame: (colors, ms) => {
      if (gen !== E.gen) return;
      E.frames = zoneFrames(colors, E.slices); E.ms = E.ms * 0.9 + ms * 0.1; E.frameCount++; E.framesSeen++;
      measure(colors);
    },
    onBackdrop: (bmp) => { if (gen === E.gen) { E.backdrop?.close?.(); E.backdrop = bmp; } else bmp.close?.(); },
    onLog: (t) => { E.logs.push({ t }); E.logs = E.logs.slice(-40); paintConsole(); },
    onError: (err) => {
      if (gen !== E.gen) return;
      if (err.line) { E.errLine = err.line; markError(err.line); }
      E.logs.push({ err: `${err.message}${err.line ? ` (line ${err.line})` : ""}`, line: err.line || null });
      E.logs = E.logs.slice(-40);
      paintConsole();
    },
    onStopped: (why) => {
      if (gen !== E.gen || why === "stopped" || why === "error") return;
      E.logs.push({ err: why === "hung" ? "It stopped answering for 3 seconds (an endless loop?), so it was stopped." : "Each frame took longer than 20 ms, so it was stopped. Draw less, or at a lower resolution." });
      paintConsole();
    },
  });
  await E.runner.load(runnable(d), points);
  if (E.userParams) E.runner.setParams(E.userParams);
}

function loop(now) {
  if (!E.doc) return;
  if (now - E.last < 15) { E.raf = requestAnimationFrame(loop); return; }      // 60 fps is plenty on any monitor
  const real = Math.min(0.25, Math.max(0.001, (now - E.last) / 1000));
  E.last = now;
  if (!E.paused) {
    E.t += real * E.scale;
    const audio = E.beat ? testAudio(E.t) : null;
    E.runner?.tick(E.t, real * E.scale, audio, E.doc?.mode === "code" && E.framesSeen % 4 === 0);
  }
  // Repaint only when the effect sent a new frame: the canvas is expensive (one glow per LED), and a still effect
  // was being drawn 60 times a second on top of that.
  if (E.painted !== E.framesSeen) {
    paintStage();
    E.painted = E.framesSeen;
  }
  if (now - E.fpsAt > 1000) {
    E.fps = E.frameCount; E.frameCount = 0; E.fpsAt = now;
    const m = $("#st-meter");
    if (m) m.innerHTML = `${E.fps} fps · <b class="${E.ms > 15 ? "warm" : ""}">${E.ms.toFixed(1)} ms</b> a frame · ${rigZones().reduce((n, z) => n + z.positions.length, 0)} LEDs${E.ms > 15 ? " · close to the 20 ms limit" : ""}`;
  }
  E.raf = requestAnimationFrame(loop);
}

function paintStage() {
  const c = $("#st-canvas");
  if (!c) return;
  paintRig(c, E.frames, { fit: "stage", backdrop: E.backdrop, pad: 26, maxScale: 4, zones: rigZones(), overlay: E.overlay ? drawOverlay : null });
}

// The x/y grid and every eighth LED's number, drawn on the same mapping as the rig.
function drawOverlay(ctx, { ox, oy, s, u, zones }) {
  const X = (x) => ox + x * u * s, Y = (y) => oy + y * u * s;
  ctx.strokeStyle = "rgba(255,255,255,.14)";
  ctx.lineWidth = 1;
  ctx.setLineDash([3, 5]);
  for (let x = 0; x <= 1.6 + 1e-6; x += 0.4) { ctx.beginPath(); ctx.moveTo(X(x), Y(0)); ctx.lineTo(X(x), Y(1)); ctx.stroke(); }
  for (let y = 0; y <= 1 + 1e-6; y += 0.25) { ctx.beginPath(); ctx.moveTo(X(0), Y(y)); ctx.lineTo(X(1.6), Y(y)); ctx.stroke(); }
  ctx.setLineDash([]);
  ctx.font = "10px ui-monospace, Consolas, monospace";
  ctx.fillStyle = "rgba(255,255,255,.55)";
  for (let x = 0; x <= 1.6 + 1e-6; x += 0.4) ctx.fillText(x.toFixed(1), X(x) + 2, Y(1) - 3);
  for (let y = 0; y <= 1 + 1e-6; y += 0.5) ctx.fillText(y.toFixed(1), X(0) + 2, Y(y) - 3);
  let n = 0;
  for (const z of zones) {
    for (const [x, y] of z.positions) {
      if (n % 8 === 0) { ctx.fillStyle = "rgba(255,255,255,.5)"; ctx.fillText(String(n), X(x) + 3, Y(y) - 3); }
      n++;
    }
    const cx = z.positions.reduce((a, p) => a + p[0], 0) / z.positions.length;
    const cy = z.positions.reduce((a, p) => a + p[1], 0) / z.positions.length;
    ctx.fillStyle = "rgba(201,162,95,.9)";
    ctx.fillText(z.zone?.name || z.key, X(cx) + 4, Y(cy) - 6);
  }
}

// Full-brightness strobing is against the gallery's rules; measure it while the effect plays.
function measure(colors) {
  const n = colors.length / 3;
  if (!n) return;
  let lum = 0;
  for (let i = 0; i < colors.length; i += 3) lum += (0.2126 * colors[i] + 0.7152 * colors[i + 1] + 0.0722 * colors[i + 2]) / 255;
  lum /= n;
  const now = performance.now() / 1000;
  const d = lum - SW.last;
  SW.last = lum;
  if (Math.abs(d) < 0.06) return;
  const dir = Math.sign(d);
  if (SW.dir && dir !== SW.dir) SW.peaks.push(now);
  SW.dir = dir;
  SW.peaks = SW.peaks.filter((t) => now - t <= 3);
  E.strobeHz = +(SW.peaks.length / 6).toFixed(1);
  if (E.strobeHz > 3.2 && now - SW.warned > 12) {
    SW.warned = now;
    E.logs.push({ err: `It flashes about ${E.strobeHz} times a second. The gallery allows at most 3, so slow it down or lower the contrast.` });
    E.logs = E.logs.slice(-40);
    paintConsole();
  }
}

function paintConsole() {
  const c = $("#st-console");
  if (!c) return;
  // the lights only matter here while they play this draft, or this effect
  const host = (S.state?.studio?.problems || []).filter((p) => (p.effect === "fx:draft" && E.lights) || (E.doc?.fx_id && p.effect === E.doc.fx_id));
  const lines = [...host.map((p) => ({ err: `On your lights: ${p.text}${p.message ? ` ${p.message}` : ""}` })),
    ...(E.problem ? [{ err: E.problem }] : []), ...E.logs.slice(-8)];
  c.hidden = !lines.length && E.doc?.mode !== "code";
  setHTML(c, lines.length ? lines.map((l) => `<div class="${l.err ? "st-err" : ""}">${esc(l.err || l.t)}${l.line ? ` <button class="st-jump" data-jump="${l.line}">jump to line ${l.line}</button>` : ""}</div>`).join("")
    : `<div class="hint">console.log() output and errors show here.</div>`);
}

function paintProblems() { if (E.doc) paintConsole(); }

function jumpToLine(line) {
  if (!E.cm || !line) return;
  E.cm.setCursor({ line: Math.max(0, line - 1), ch: 0 });
  E.cm.focus();
}

function markError(line) {
  if (!E.cm) return;
  if (E.errMark != null) E.cm.removeLineClass(E.errMark, "background", "cm-error-line");
  E.errMark = line ? Math.max(0, line - 1) : null;
  if (E.errMark != null) E.cm.addLineClass(E.errMark, "background", "cm-error-line");
}

// ------------------------------------------------------------ code mode
const CM_FILES = ["codemirror.min.js", "mode-xml.min.js", "mode-javascript.min.js", "mode-css.min.js", "mode-htmlmixed.min.js",
  "addon-matchbrackets.min.js", "addon-closebrackets.min.js", "addon-closetag.min.js", "addon-active-line.min.js"];
let cmLoaded = null;
function loadCodeMirror() {
  cmLoaded ||= (async () => {
    const css = document.createElement("link");
    css.rel = "stylesheet"; css.href = "/vendor/codemirror/codemirror.min.css";
    document.head.append(css);
    for (const f of CM_FILES) {
      await new Promise((res, rej) => { const s = document.createElement("script"); s.src = `/vendor/codemirror/${f}`; s.onload = res; s.onerror = rej; document.head.append(s); });
    }
  })();
  return cmLoaded;
}

async function mountCode() {
  await loadCodeMirror();
  const host = $("#st-code");
  if (!host || !E.doc) return;
  E.cm = window.CodeMirror(host, { value: E.doc.source, mode: "htmlmixed", lineNumbers: true, indentUnit: 2, tabSize: 2,
    matchBrackets: true, autoCloseBrackets: true, autoCloseTags: true, styleActiveLine: true, lineWrapping: false });
  let timer = 0;
  E.cm.on("change", () => {
    E.doc.source = E.cm.getValue();
    changed(false);
    clearTimeout(timer);
    timer = setTimeout(codeChanged, 450);
  });
}

async function codeChanged() {
  if (!E.doc || E.doc.mode !== "code") return;
  E.logs = [];
  E.errLine = null;
  markError(null);
  const p = parseSource(E.doc.source);
  if (p.name && p.name !== E.doc.name) { E.doc.name = p.name; const n = $("#st-name"); if (n) n.value = p.name; }
  $("#st-right").innerHTML = inspectorHTML();
  $$("#st-right input[type=range]").forEach(rangeFill);
  wireInspector();
  await restartRunner();
  try {
    const r = await post("/api/studio/check", { source: codeSource(E.doc) });
    E.problem = null;
    if (r.flags.length) E.logs.push({ t: `Reviewer note: it ${r.flags.join("; ")}.` });
  } catch (e) { E.problem = e.message; }
  paintConsole();
}

async function toCode() {
  const d = E.doc;
  try {
    const built = await post("/api/studio/build", payload(d));
    const cfg = { v: 2, layers: d.layers, master: d.master, expose: d.expose };
    const code = `// Made from layers in the Studio. AirglowLayers draws them (it comes with every effect): change the list,
// or add your own code after it, e.g. point(led, frame) { ... }.
const EFFECT = ${JSON.stringify(cfg)};
airglow.effect(AirglowLayers.effect(EFFECT));`;
    const r = await post("/api/studio/build", { name: d.name, description: d.description, tags: d.tags, remix_of: d.remix_of, code, params: built.effect.params });
    E.layersCopy = { layers: structuredClone(d.layers), master: structuredClone(d.master), expose: structuredClone(d.expose) };
    d.mode = "code"; d.source = r.source; d.values = {};
    renderEditor();
    await restartRunner();
    changed(false);
  } catch (e) { fail(e); }
}

// Code -> layers works after reopening too: the generated block is read back.
function jsonAt(source, marker) {
  const i = source.indexOf(marker);
  if (i < 0) return null;
  const starts = [source.indexOf("{", i), source.indexOf("[", i)].filter((n) => n >= 0);
  if (!starts.length) return null;
  const start = Math.min(...starts);
  let depth = 0, quote = null, esc = false;
  for (let k = start; k < source.length; k++) {
    const ch = source[k];
    if (quote) { if (esc) esc = false; else if (ch === "\\") esc = true; else if (ch === quote) quote = null; continue; }
    if (ch === '"' || ch === "'") { quote = ch; continue; }
    if (ch === "{" || ch === "[") depth++;
    else if (ch === "}" || ch === "]") {
      depth--;
      if (depth === 0) { try { return JSON.parse(source.slice(start, k + 1)); } catch { return null; } }
    }
  }
  return null;
}

function parseLayersConfig(source) {
  const cfg = jsonAt(source, "const EFFECT");
  if (cfg && Array.isArray(cfg.layers)) return cfg;
  const layers = jsonAt(source, "const LAYERS");
  if (Array.isArray(layers)) return { layers, master: structuredClone(MASTER), expose: {} };
  return null;
}

function toLayers() {
  if (!E.layersCopy) {
    const cfg = parseLayersConfig(E.doc.source);
    if (!cfg) { toast("This effect was written in code, so it has no layers to go back to.", true); renderEditor(); return; }
    E.layersCopy = cfg;
  }
  if (!confirm("Go back to the layers? Changes you made in the code are dropped.")) { renderEditor(); return; }
  clearInterval(E.watch);                         // the editor's file watcher belongs to Code mode
  E.watch = null;
  E.doc.mode = "layers";
  E.doc.layers = E.layersCopy.layers || [];
  E.doc.master = E.layersCopy.master || structuredClone(MASTER);
  E.doc.expose = E.layersCopy.expose || {};
  E.layersCopy = null;
  normalizeDoc(E.doc);
  E.sel = Math.max(0, E.doc.layers.length - 1);
  renderEditor();
  restartRunner();
  changed(true);
}

async function openInEditor() {
  try {
    const r = await post("/api/studio/editor", { source: E.doc.mode === "code" ? codeSource(E.doc) : payload(E.doc) });
    const note = $("#st-editor-note");
    if (note) note.textContent = `Opened ${r.path} in ${r.editor}. Save it there and the preview reloads.`;
    clearInterval(E.watch);
    let seen = r.mtime;
    E.watch = setInterval(async () => {
      if (!E.doc) return clearInterval(E.watch);
      try {
        const f = await get(`/api/studio/editor?file=${encodeURIComponent(r.file)}`);
        if (f.mtime !== seen) { seen = f.mtime; E.doc.source = f.source; E.cm?.setValue(f.source); codeChanged(); }
      } catch { /* file moved; keep the last version */ }
    }, 700);
  } catch (e) { fail(e); }
}

// ------------------------------------------------------------ save, use, share, check
const saves = new Map();
async function save({ quiet = false } = {}) {
  const d = E.doc;
  if (!d) return null;
  const key = d.fx_id || d, queue = saves.get(key) || { tail: null };
  const before = snapDoc(d), previous = queue.tail;
  saves.set(key, queue);
  const writing = (async () => {
    if (previous) await previous;
    try {
      // A queued first save must reuse the ID created by the preceding write.
      const r = await post("/api/studio/save", { ...payload(JSON.parse(before)), fx_id: d.fx_id });
      d.fx_id = r.effect.fx_id;
      // Reopening the new effect must join the same queue, including its already queued first saves.
      if (key !== d.fx_id) saves.delete(key);
      saves.set(d.fx_id, queue);
      d.dirty = snapDoc(d) !== before || queue.tail !== writing;
      if (E.doc === d) {
        if (!d.dirty) forgetDraft();
        if (!quiet) E.saved = before;          // Revert goes back to this deliberate save
        setSavedLabel();
      }
      if (!quiet) {
        S.effects = await get("/api/effects");
        emit("refresh");
      }
      return r.effect;
    } catch (e) { if (!quiet) fail(e); return null; }
  })();
  queue.tail = writing;
  try { return await writing; }
  finally {
    if (queue.tail === writing) {
      saves.delete(key);
      saves.delete(d.fx_id);
    }
  }
}

async function revert() {
  if (!E.saved || !E.doc) return toast("Nothing to go back to yet.", true);
  if (snapDoc() === E.saved) return toast("That is already how it was when you started.", true);
  if (!confirm("Go back to where this session started (or your last Save)? Changes since then are dropped.")) return;
  const snap = JSON.parse(E.saved), fx = E.doc.fx_id;
  E.applying = true;
  Object.assign(E.doc, snap);
  E.doc.fx_id = fx;
  normalizeDoc(E.doc);
  E.applying = false;
  E.sel = typeof E.sel === "number" ? Math.min(E.sel, E.doc.layers.length - 1) : "effect";
  renderEditor();
  await restartRunner();
  // Autosave may have written later edits, so the file has to be brought back too; marking it dirty does that.
  E.doc.dirty = true;
  histPush();
  setSavedLabel();
  rememberDraft();
  scheduleSave();
}

async function useIt() {
  const eff = E.doc.dirty || !E.doc.fx_id ? await save() : { fx_id: E.doc.fx_id };
  if (!eff) return;
  try {
    E.lights = false; const cb = $("#st-lights"); if (cb) cb.checked = false;
    const r = await post("/api/studio/use", { fx_id: eff.fx_id, values: E.doc.values });
    S.state.scene = r.scene;
    toast(`${E.doc.name} is on your lights`); emit("refresh");
  } catch (e) { fail(e); }
}

async function shareIt(fxId) {
  let eff = null;
  if (E.doc && !fxId) { eff = E.doc.dirty || !E.doc.fx_id ? await save() : null; fxId = eff?.fx_id || E.doc.fx_id; if (!fxId) return; }
  const e = (H.lib?.effects || []).find((x) => x.fx_id === fxId) || E.doc || {};
  const desc = e.description || "";
  openSheet(`<form class="body" id="sh-form"><h2>Share ${esc(e.name)}</h2>
    <p class="lede" style="margin:0">GitHub opens in your browser with the effect filled in. Press <b>Propose changes</b> there and it goes to the community gallery. Once it's checked and approved, everyone can use it.</p>
    <div class="field"><label class="lab" for="sh-login"><span>Your GitHub user name</span></label><input class="text" id="sh-login" required value="${esc(remember("airglow.github"))}" spellcheck="false" autocomplete="username"></div>
    <div class="field"><label class="lab" for="sh-d"><span>Description</span></label><textarea class="text" id="sh-d" rows="2" maxlength="200" required placeholder="What does it look like?">${esc(desc)}</textarea></div>
    <div class="field"><label class="lab" for="sh-tags"><span>Tags</span></label><input class="text" id="sh-tags" value="${esc((e.tags || []).join(", "))}" placeholder="ambient, warm, music" spellcheck="false"></div>
    <div class="field"><label class="lab" for="sh-lic"><span>Licence</span></label><select class="select" id="sh-lic">${LICENSES.slice(0, 5).map((l) => `<option${l === "MIT" ? " selected" : ""}>${l}</option>`).join("")}</select></div>
    <label class="check"><input type="checkbox" id="sh-ok" required> I made this (or remixed something open), and it may be shared under this licence.</label>
    <p class="hint">Shared: the effect itself, your GitHub name and the text above. Nothing about your PC.</p>
    <div class="actions"><button type="button" class="btn quiet" data-close>Cancel</button><span class="spacer"></span><button class="btn primary">Open GitHub</button></div></form>`);
  $("#sh-form").addEventListener("submit", async (ev) => {
    ev.preventDefault();
    const login = $("#sh-login").value.trim().replace(/^@/, "");
    remember("airglow.github", login);
    try {
      const r = await post("/api/studio/share", { fx_id: fxId, author: login, description: $("#sh-d").value, open: false,
        tags: $("#sh-tags").value.split(",").map((t) => t.trim().toLowerCase()).filter(Boolean).slice(0, 5), license: $("#sh-lic").value });
      let copied = false;
      if (!r.filled) { try { await navigator.clipboard.writeText(r.source); copied = true; } catch { copied = false; } }
      await post("/api/studio/open-share", { url: r.url });
      openSheet(`<div class="body"><h2>Almost there</h2>
        ${r.filled ? `<p class="lede" style="margin:0">GitHub is open in your browser with <span class="path">${esc(r.path)}</span> filled in. Press <b>Propose changes</b>, then <b>Create pull request</b>.</p>`
          : `<p class="lede" style="margin:0">GitHub is open in your browser with the file name filled in. ${copied ? "The effect is on your clipboard:" : "Copy the effect below,"} click into the empty file there, press <b>Ctrl+V</b>, then <b>Propose changes</b> and <b>Create pull request</b>.</p>
            ${copied ? "" : `<textarea class="text" rows="6" readonly>${esc(r.source)}</textarea>`}`}
        ${r.flags.length ? `<p class="hint">The reviewer will ask about this: it ${esc(r.flags.join("; "))}.</p>` : ""}
        <p class="hint">Didn't open? <a href="${esc(r.url)}" target="_blank" rel="noopener">Open it here</a>.</p>
        <div class="actions"><span class="spacer"></span><button class="btn primary" data-close>Done</button></div></div>`);
    } catch (err) { fail(err); }
  });
}

// The repo's checks, in the app: metadata, size, reviewer scan, music permission, strobing and frame time. Share
// comes through here first, and the file that will be sent is shown before it goes.
async function checkSheet() {
  if (!E.doc) return;
  openSheet(`<div class="body"><h2>Ready to share?</h2>
    <p class="hint" id="st-check-msg">Checking…</p><div id="st-check-list" class="st-checks"></div>
    <div class="actions"><span class="spacer"></span><button class="btn quiet" data-close>Close</button><button class="btn primary" id="st-check-share" hidden>Share</button></div></div>`);
  try {
    const body = E.doc.mode === "code" ? { source: codeSource(E.doc) } : payload(E.doc);
    const built = await post("/api/studio/build", body);       // the exact file, too
    const r = await post("/api/studio/check", { source: built.source });
    const eff = r.effect, flags = r.flags || [];
    const params = eff.params || [];
    const wants = usesAudioDoc(E.doc), has = (eff.permissions || []).includes("audio");
    const rows = [
      { ok: !!E.doc.name.trim(), text: E.doc.name.trim() ? `Named “${E.doc.name.trim()}”` : "Give it a name" },
      { ok: !!E.doc.description.trim(), text: E.doc.description.trim() ? "Has a description" : "Add a description: people see it in the gallery" },
      { ok: params.length > 0, text: params.length ? `${params.length} control${params.length === 1 ? "" : "s"} people can turn` : "No controls yet. It still works, but nobody can tune it" },
      { ok: !wants || has, text: wants ? (has ? "Uses the music, with permission" : "It reads the music but the file doesn't ask for it. Save again") : "No music needed" },
      { ok: eff.bytes <= 32000, text: eff.bytes < 1000 ? `${eff.bytes} bytes of 32 KB` : `${Math.round(eff.bytes / 1000)} KB of 32 KB` },
      { ok: !flags.length, text: flags.length ? `Reviewer note: it ${flags.join("; ")}` : "No reviewer flags" },
      { ok: E.strobeHz <= 3.2, text: E.strobeHz > 3.2 ? `Flashes about ${E.strobeHz} times a second (the limit is 3)` : "Not strobing faster than 3 times a second" },
      { ok: E.ms <= 18, text: E.ms > 18 ? `${E.ms.toFixed(1)} ms a frame, close to the 20 ms limit` : `${E.ms.toFixed(1)} ms a frame` },
    ];
    setHTML($("#st-check-list"), rows.map((x) => `<div class="st-check ${x.ok ? "ok" : "bad"}"><span aria-hidden="true">${x.ok ? "✓" : "!"}</span>${esc(x.text)}</div>`).join("")
      + `<details class="st-file"><summary>The file that will be shared (${eff.bytes} bytes)</summary><pre class="st-pre">${esc(built.source)}</pre></details>`
      + `<p class="hint">Licence: you choose it when you press Share. Layer and control limits are checked by the file format; the frame time and strobing are measured while it plays here.</p>`);
    const msg = $("#st-check-msg");
    msg.textContent = rows.every((x) => x.ok) ? "Everything checks out." : "Some things to fix first, but you can still share.";
    const btn = $("#st-check-share");
    btn.hidden = false;
    btn.addEventListener("click", () => { closeSheet(); shareIt(); });
  } catch (e) { $("#st-check-msg").textContent = e.message; }
}

function download(name, text, type = "text/html") {
  const a = document.createElement("a");
  a.href = URL.createObjectURL(new Blob([text], { type }));
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 5000);
}

async function exportIt() {
  if (!E.doc) return;
  try {
    const r = E.doc.mode === "code"
      ? await post("/api/studio/build", { source: codeSource(E.doc) })
      : await post("/api/studio/build", payload(E.doc));
    download(`${r.effect.id || slug(E.doc.name)}.html`, r.source);
    toast("Saved the effect file to your downloads");
  } catch (e) { fail(e); }
}

async function importFile(file) {
  if (!file) return;
  if (file.size > 40000) return toast("That file is too big for an effect (32 KB at most).", true);
  try {
    const text = await file.text();
    const r = await post("/api/studio/check", { source: text });
    if (!r?.effect) throw new Error("That isn't an Airglow effect");
    const eff = r.effect;
    if (E.doc) closeEditor(true);
    openEditor(eff.kind === "layers"
      ? newDoc({ name: eff.name, description: eff.description, tags: eff.tags, remix_of: eff.remix_of || "",
                 layers: eff.layers, master: eff.master, expose: eff.expose })
      : newDoc({ name: eff.name, description: eff.description, tags: eff.tags, remix_of: eff.remix_of || "",
                 mode: "code", source: text }));
    toast(`Opened ${eff.name}`);
  } catch (e) { fail(e); }
}

function snapshotPNG() {
  const c = $("#st-canvas");
  if (!c || !E.doc) return;
  c.toBlob((b) => { if (b) download(`${slug(E.doc.name)}.png`, b, "image/png"); });
}

// ------------------------------------------------------------ sheets: more, guide, gallery detail
function moreSheet(eff) {
  if (!eff) return;
  sheetEffect = eff;
  const mine = eff.origin === "mine";
  openSheet(`<div class="body"><h2>${esc(eff.name)}</h2>
    ${eff.description ? `<p class="lede" style="margin:0">${esc(eff.description)}</p>` : ""}
    <div class="btn-row">
      ${mine ? `<button class="btn" data-m="share">Share</button>` : `<button class="btn" data-m="rollback">Go back to the previous version</button>`}
      <button class="btn" data-m="remix">Remix</button>
      <button class="btn danger" data-m="delete">${mine ? "Delete" : "Uninstall"}</button></div>
    <div class="actions"><span class="spacer"></span><button class="btn quiet" data-close>Close</button></div></div>`);
}

async function onSheetClick(e) {
  const copy = e.target.closest("[data-copy]");
  if (copy) {
    const source = document.getElementById(copy.dataset.copy);
    if (source) {
      try { await navigator.clipboard.writeText(source.textContent); toast("Copied"); }
      catch { toast("Couldn't copy: select the text instead.", true); }
    }
    return;
  }
  const gd = e.target.closest("[data-gd]")?.dataset.gd;
  if (gd && sheetGallery) {
    const eff = sheetGallery;
    if (gd === "remix") { closeSheet(); if (E.doc) closeEditor(true); openEditor(docFromEffect(eff, { remix: true })); return; }
    if (gd === "install") {
      try { await post("/api/studio/install", { id: eff.id }); S.effects = await get("/api/effects"); toast("Installed. It's in your effects now."); closeSheet(); H.gallery = await get("/api/studio/gallery"); if (H.view === "community") paintCommunity(); }
      catch (err) { fail(err); }
      return;
    }
  }
  const m = e.target.closest("[data-m]")?.dataset.m;
  if (!m || !sheetEffect) return;
  const eff = sheetEffect;
  if (m === "share") return shareIt(eff.fx_id);
  if (m === "remix") { closeSheet(); if (E.doc) closeEditor(true); return openEditor(docFromEffect(eff, { remix: true })); }
  try {
    if (m === "delete") {
      if (!confirm(eff.origin === "mine" ? `Delete ${eff.name}? This can't be undone.` : `Uninstall ${eff.name}?`)) return;
      await post("/api/studio/delete", { fx_id: eff.fx_id });
      toast(eff.origin === "mine" ? "Deleted" : "Uninstalled");
    } else if (m === "rollback") {
      await post("/api/studio/rollback", { fx_id: eff.fx_id }); toast("Back to the previous version");
    }
    closeSheet(); S.effects = await get("/api/effects"); await loadLibrary(); renderHome();
  } catch (err) { fail(err); }
}

function learnSheet() {
  const recipes = [
    { id: "rec-chase", title: "A chase around every fan", code: `airglow.effect({
  point(led, frame) {
    const head = (frame.time * 0.4) % 1;
    const d = (led.local - head + 1) % 1;      // led.local runs 0..1 around each fan
    const k = Math.max(0, 1 - d / 0.3) ** 2;
    return [255 * k, 120 * k, 255];
  },
});` },
    { id: "rec-bars", title: "Bars from the music", code: `airglow.effect({
  render(ctx, frame) {
    const g = ctx.canvas.getContext("2d");
    g.fillStyle = "#05060a"; g.fillRect(0, 0, ctx.width, ctx.height);
    const bands = frame.audio ? frame.audio.bands : [];
    g.fillStyle = "#00e0c8";
    for (let i = 0; i < 16; i++) {
      const h = 180 * (bands[i * 4] || 0);
      g.fillRect(i * 20 + 2, 200 - h, 16, h);
    }
  },
});` },
    { id: "rec-params", title: "Controls next to the code", code: `airglow.params({
  speed: { label: "Speed", type: "number", min: 0.1, max: 3, step: 0.1, default: 1 },
  color: { label: "Colour", type: "color", default: "#2f5bff" },
});

airglow.effect({
  point(led, frame) {
    const v = 0.5 + 0.5 * Math.sin(led.x * 6 - frame.time * ctx.params.speed * 2);
    return [0, 180 * v, 255 * v];
  },
});` },
  ];
  openSheet(`<div class="body st-learn"><h2>Make an effect</h2>
    <p class="lede" style="margin:0">The Studio is for making one effect and seeing it on your own case. Layers cover most looks; code covers the rest. It is all one HTML file that runs sandboxed.</p>
    <div class="sub-head">The short path</div>
    <ol class="st-steps">
      <li>Pick a starting point, or <b>Surprise me</b>.</li>
      <li>Stack layers. Every change is live; <b>Also on my lights</b> puts it on the hardware.</li>
      <li>Tick <b>Show as a control</b> under anything people should be able to turn. The palette and the master speed are the two worth exposing most.</li>
      <li>Press <b>Check</b>, then <b>Share</b>. GitHub opens with the file; press Propose changes.</li>
    </ol>
    <div class="sub-head">What the file may contain</div>
    <ul class="st-api"><li>At most 32 KB, 12 layers and 16 controls.</li>
      <li>Builder effects: the layer stack, a palette, master speed, brightness and reactivity, and which of them are controls.</li>
      <li>Code effects: <code>airglow.effect({ init, render, point, frame, onParamChange, dispose })</code>, plus <code>airglow.params({...})</code> for controls.</li>
      <li>Music needs <code>permissions=audio</code>; the Studio adds it for music layers and for layers that react to the beat.</li>
      <li>Tags are lowercase; the licence and description are required to share.</li></ul>
    <div class="sub-head">Recipes</div>
    ${recipes.map((r) => `<div class="st-recipe"><div class="st-recipe-head"><b>${esc(r.title)}</b><button class="btn quiet" data-copy="${r.id}">Copy</button></div>
      <pre class="st-pre" id="${r.id}">${esc(r.code)}</pre></div>`).join("")}
    <p class="hint">A type file for editors is in <code>docs/effect-api.d.ts</code>. The full format is in <code>docs/EFFECT-STUDIO.md</code>.</p>
    <div class="actions"><span class="spacer"></span><button class="btn primary" data-close>Done</button></div></div>`);
}

async function galleryDetail(id) {
  try {
    const { effect } = await get(`/api/studio/gallery/effect?id=${encodeURIComponent(id)}`);
    sheetGallery = { ...effect, id };
    const controls = (effect.schema || []).map((s) => `<li><b>${esc(s.label)}</b> <span>${esc(s.type)}</span></li>`).join("");
    const layers = effect.kind === "layers" && effect.layers
      ? `<ul class="st-api">${effect.layers.map((l) => `<li>${esc(H.info.types[l.type]?.name || l.type)}${l.palette ? " · palette" : ""}${(l.react?.amount || 0) > 0 ? " · follows the music" : ""}</li>`).join("")}</ul>`
      : `<p class="hint">Written in code. Remix it to read and change the script.</p>`;
    openSheet(`<div class="body"><h2>${esc(effect.name)}</h2>
      <p class="lede" style="margin:0">${esc(effect.description || "No description.")}</p>
      <p class="hint">by ${esc(effect.author || "unknown")}${effect.remix_of ? ` · a remix of ${esc(effect.remix_of)}` : ""}${(effect.permissions || []).includes("audio") ? " · uses the music" : ""}</p>
      <div class="sub-head">Controls</div>${controls ? `<ul class="st-api">${controls}</ul>` : `<p class="hint">No controls: its look is fixed.</p>`}
      <div class="sub-head">How it's made</div>${layers}
      <div class="actions"><button class="btn" data-gd="install">Install</button><button class="btn" data-gd="remix">Remix it</button><span class="spacer"></span><button class="btn quiet" data-close>Close</button></div></div>`);
  } catch (e) { fail(e); }
}

// ------------------------------------------------------------ clicks
async function onClick(e) {
  const t = e.target.closest("button, a, [data-sel]");
  if (!t) return;
  const ds = t.dataset;
  if (ds.jump) return jumpToLine(+ds.jump);
  if (ds.st === "new") { $("#st-start")?.scrollIntoView({ behavior: "smooth" }); $("#st-start")?.classList.add("flash"); return; }
  if (ds.st === "community-retry") return renderCommunity();
  if (ds.st === "learn") return learnSheet();
  if (ds.st === "surprise") {
    const s = surprise();
    return openEditor(newDoc({ name: "Untitled effect", layers: s.layers, master: s.master, expose: s.expose }));
  }
  if (ds.st === "import") { $("#st-import")?.click(); return; }
  if (ds.tpl) { const tp = LAYER_TEMPLATES.find((x) => x.id === ds.tpl); return openEditor(newDoc({ name: tp.id === "blank" ? "Untitled effect" : `My ${tp.name.toLowerCase()}`, layers: structuredClone(tp.layers), master: tp.master, expose: tp.expose || {} })); }
  if (ds.codeTpl) {
    const tp = CODE_TEMPLATES.find((x) => x.id === ds.codeTpl);
    try {
      const r = await post("/api/studio/build", { name: `My ${tp.name.toLowerCase()} effect`, code: tp.code, params: tp.params });
      return openEditor(newDoc({ name: `My ${tp.name.toLowerCase()} effect`, mode: "code", source: r.source }));
    } catch (err) { return fail(err); }
  }
  const lib = H.lib?.effects || [];
  if (ds.edit) { const eff = lib.find((x) => x.fx_id === ds.edit); if (eff) openEditor(docFromEffect(eff)); return; }
  if (ds.remix) { const eff = lib.find((x) => x.fx_id === ds.remix); if (eff) openEditor(docFromEffect(eff, { remix: true })); return; }
  if (ds.use) {
    try { const r = await post("/api/studio/use", { fx_id: ds.use }); S.state.scene = r.scene; toast("On your lights"); emit("refresh"); renderHome(); }
    catch (err) { fail(err); }
    return;
  }
  if (ds.more) return moreSheet(lib.find((x) => x.fx_id === ds.more));
  if (ds.gdet) return galleryDetail(ds.gdet);
  if (ds.tag !== undefined && H.view === "community") { H.tag = ds.tag; return paintCommunity(); }
  if (ds.install) {
    t.disabled = true;
    try { await post("/api/studio/install", { id: ds.install }); toast("Installed. It's in your effects now."); S.effects = await get("/api/effects"); H.gallery = await get("/api/studio/gallery"); paintCommunity(); }
    catch (err) { t.disabled = false; fail(err); }
    return;
  }
  if (ds.gremix) {
    try { const { effect } = await get(`/api/studio/gallery/effect?id=${encodeURIComponent(ds.gremix)}`); openEditor(docFromEffect({ ...effect, id: ds.gremix }, { remix: true })); }
    catch (err) { fail(err); }
    return;
  }
  if (!E.doc) return;
  // ---- editor
  if (ds.dbg) {
    if (ds.dbg === "pause") { E.paused = !E.paused; E.painted = -1; t.textContent = E.paused ? "Play" : "Pause"; }
    else if (ds.dbg === "slow") { E.scale = E.scale === 1 ? 0.25 : 1; }
    else if (ds.dbg === "restart") { E.t = 0; E.last = performance.now(); E.logs.push({ t: "Clock back to 0 s." }); paintConsole(); }
    else if (ds.dbg === "overlay") { E.overlay = !E.overlay; E.painted = -1; }
    else if (ds.dbg === "snap") return snapshotPNG();
    if (ds.dbg === "pause" || ds.dbg === "overlay") t.setAttribute("aria-pressed", String(ds.dbg === "pause" ? E.paused : E.overlay));
    if (ds.dbg === "slow") { t.textContent = E.scale === 1 ? "1×" : "¼×"; t.setAttribute("aria-pressed", String(E.scale !== 1)); }
    return;
  }
  if (ds.mode && ds.mode !== E.doc.mode) return ds.mode === "code" ? toCode() : toLayers();
  if (ds.sel !== undefined && !e.target.closest("[data-toggle]")) {
    E.sel = ds.sel === "effect" ? "effect" : +ds.sel;
    $$("#st-left [data-sel]").forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.sel === String(E.sel))));
    $("#st-right").innerHTML = inspectorHTML();
    $$("#st-right input[type=range]").forEach(rangeFill);
    return wireInspector();
  }
  if (ds.toggle !== undefined) { const l = E.doc.layers[+ds.toggle]; l.enabled = l.enabled === false; rerenderSides(); return changed(true); }
  if (ds.st === "add") { const m = $("#st-menu"); m.hidden = !m.hidden; t.setAttribute("aria-expanded", String(!m.hidden)); return; }
  if (ds.add) {
    const spec = H.info.types[ds.add];
    if (E.doc.layers.length >= MAX_LAYERS) return toast(`${MAX_LAYERS} layers is the most an effect can have.`, true);
    E.doc.layers.push({ id: uid(), type: ds.add, enabled: true, opacity: 100,
      blend: E.doc.layers.length && !["solid", "gradient", "rainbow", "plasma", "cycle"].includes(ds.add) ? "add" : "normal",
      mask: "all", palette: false, react: { what: "strength", amount: 0 },
      params: Object.fromEntries(spec.params.map((s) => [s.id, structuredClone(s.default)])), expose: {} });
    E.sel = E.doc.layers.length - 1;
    if (spec.audio && !E.beat) { E.beat = true; const cb = $("#st-beat"); if (cb) cb.checked = true; toast("Test beat is on, so you can see it without music"); }
    rerenderSides(); return changed(true);
  }
  if (ds.st === "del") return delLayer();
  if (ds.st === "dup") return dupLayer();
  if (ds.st === "solo") return toggleSolo();
  if (ds.st === "up") return moveLayer(1);
  if (ds.st === "down") return moveLayer(-1);
  if (ds.st === "save") { if (await save()) toast("Saved. It's in your effects, and in Lighting's effect list."); return; }
  if (ds.st === "revert") return revert();
  if (ds.st === "check") return checkSheet();
  if (ds.st === "export") return exportIt();
  if (ds.st === "use") return useIt();
  if (ds.st === "share") return checkSheet();     // the check comes first; its own Share button opens GitHub
  if (ds.st === "editor") return openInEditor();
  if (ds.st === "userview") return userPanel();
}

// Preview the effect panel people get in Lighting, live on the stage.
async function userPanel() {
  try {
    const r = await post("/api/studio/build", payload(E.doc));
    const schema = r.effect.schema || [];
    if (!schema.length) return toast("No controls yet. Tick “Show as a control” under a setting first.", true);
    const vals = Object.fromEntries(schema.map((s) => [s.id, E.doc.values?.[s.id] ?? s.default]));
    openSheet(`<div class="body"><h2>${esc(E.doc.name)} as a user</h2>
      <p class="lede" style="margin:0">This is the panel people get in Lighting. Moving it here only previews it; the effect is unchanged.</p>
      <div id="st-user" class="ins-block"></div>
      <p class="hint">Values are per person. Exposed settings start at the values you set.</p>
      <div class="actions"><span class="spacer"></span><button class="btn primary" data-close>Done</button></div></div>`);
    const root = $("#st-user");
    root.innerHTML = schema.map((s) => fieldHTML(s, vals[s.id], "uv")).join("");
    wireFields(root, { params: schema }, vals, (v) => { E.userParams = v; E.runner?.setParams(v); });
    E.userParams = vals;
    E.runner?.setParams(vals);
    const sheet = $("#sheet");
    const done = () => {
      sheet.removeEventListener("close", done);
      E.userParams = null;
      if (E.doc) restartRunner();
    };
    sheet.addEventListener("close", done);
  } catch (e) { fail(e); }
}
