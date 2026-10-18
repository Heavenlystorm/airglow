// The Studio: make lighting effects by stacking layers (or in code), see them on your own rig as you go, and share
// them with the community. docs/EFFECT-STUDIO.md has the design; effects run sandboxed through fxrun.js.

import { $, $$, S, coalesce, emit, esc, fail, get, on, post, rangeFill, setHTML, toast } from "./core.js";
import { fieldHTML, wireFields } from "./lighting.js";
import { litZones, paintRig } from "./stage.js";
import { EffectRunner, ledPoints, runtimeAssets, testAudio, zoneFrames } from "./fxrun.js";
import { CODE_TEMPLATES, LAYER_TEMPLATES, REMIX } from "./studio-start.js";
import { closeSheet, openSheet } from "./sheet.js";

const AREAS = { all: "Whole case", front: "Front", top: "Top", rear: "Rear", bottom: "Bottom", side: "Side panel",
  cooler: "CPU cooler", ram: "Memory", board: "Motherboard", gpu: "Graphics card", desk: "Desk" };
const BLENDS = { normal: "Cover", add: "Add light", screen: "Brighten", multiply: "Darken" };
const LICENSES = ["MIT", "Apache-2.0", "BSD-3-Clause", "MPL-2.0", "GPL-3.0-or-later", "CC0 is not allowed"];
const remember = (k, v) => { try { if (v === undefined) return localStorage.getItem(k) || ""; localStorage.setItem(k, v); } catch { return ""; } return v; };

const H = { lib: null, gallery: null, info: null, view: "home", search: "", tag: "" };   // home and community
const E = { doc: null, runner: null, slices: [], sel: "effect", lights: false, beat: false, raf: 0, t0: 0, last: 0,
            backdrop: null, frames: {}, ms: 0, fps: 0, frameCount: 0, fpsAt: 0, problem: null, cm: null, logs: [],
            layersCopy: null, watch: null, gen: 0 };

const box = () => $("#studio");
let pending = null;            // something Lighting asked the Studio to open ("Remix in the Studio", "Edit in ...")

// ------------------------------------------------------------ entry points
export function initStudio() {
  box().addEventListener("click", onClick);
  box().addEventListener("keydown", (e) => {          // layer rows are list items: Enter or Space picks one
    if ((e.key === "Enter" || e.key === " ") && e.target.matches("li[data-sel]")) { e.preventDefault(); e.target.click(); }
  });
  on("state", () => { followLights(); if (!box().closest(".view").hidden) paintProblems(); });
  on("zones", () => { if (E.runner) restartRunner(); });
  on("studio:open", (req) => { pending = req; if (location.hash === "#studio/edit") renderStudio("edit"); else location.hash = "#studio/edit"; });
}

// called by main.js's router: #studio, #studio/new, #studio/community
export async function renderStudio(arg) {
  if (!H.info) H.info = (await runtimeAssets()).layerInfo;
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
  return { fx_id: null, name: "Untitled effect", description: "", tags: [], remix_of: "", mode: "layers",
           layers: [], source: "", values: {}, ...structuredClone(d), dirty: !d.fx_id };
}

function docFromEffect(eff, { remix = false } = {}) {
  const d = { fx_id: remix ? null : eff.fx_id, name: remix ? `${eff.name} remix` : eff.name,
              description: eff.description || "", tags: eff.tags || [], remix_of: remix ? eff.id || eff.remix_of : eff.remix_of || "" };
  if (eff.kind === "layers") return newDoc({ ...d, mode: "layers", layers: eff.layers });
  return newDoc({ ...d, mode: "code", source: remix ? retitle(eff.source, d.name, d.remix_of) : eff.source });
}

// a copied code effect: new title, credit the original, drop the author (it's yours now)
function retitle(source, name, remixOf) {
  let s = source.replace(/<title>[\s\S]*?<\/title>/i, `<title>${esc(name)}</title>`).replace(/<meta name="author"[^>]*>\n?/i, "");
  if (remixOf && !/name="remix-of"/i.test(s)) s = s.replace(/(<meta name="airglow-effect"[^>]*>)/i, `$1\n<meta name="remix-of" content="${esc(remixOf)}">`);
  return s;
}

// read a code effect file in the page (DOMParser runs nothing): its controls, defaults and script
function parseSource(source) {
  const d = new DOMParser().parseFromString(source, "text/html");
  const params = [...d.querySelectorAll("meta[property]")].map((m) => {
    const a = (k) => m.getAttribute(k);
    const id = a("property"), label = a("label") || id, type = a("type");
    if (type === "number") return { id, label, type: "range", min: +a("min"), max: +a("max"), step: +(a("step") || 1), default: +a("default"), unit: "" };
    if (type === "color") return { id, label, type: "color", default: (a("default") || "#ffffff").toLowerCase() };
    if (type === "boolean") return { id, label, type: "toggle", default: a("default") === "true" };
    const opts = (a("options") || "").split(",").map((o) => o.trim()).filter(Boolean);
    return { id, label, type: "choice", default: a("default") || opts[0], options: opts.map((o) => ({ value: o, label: o })) };
  });
  const script = [...d.querySelectorAll("script")].find((s) => !s.type || s.type === "text/javascript" || s.type === "module");
  const layersEl = d.querySelector('script[type="application/json"]#layers');
  let layers = null;
  try { layers = layersEl ? JSON.parse(layersEl.textContent) : null; } catch { layers = null; }
  return { name: d.querySelector("title")?.textContent.trim() || "", code: script?.textContent || "", layers, params,
           audio: /audio/.test(d.querySelector('meta[name="permissions"]')?.getAttribute("content") || "") };
}

// what the runner gets for the current document
function runnable(doc) {
  if (doc.mode === "layers") return { kind: "layers", layers: doc.layers, params: {} };
  const p = parseSource(doc.source);
  const values = Object.fromEntries(p.params.map((s) => [s.id, doc.values[s.id] ?? s.default]));
  return { kind: "code", code: p.code, params: values };
}

const usesAudio = (doc) => doc.mode === "layers"
  ? doc.layers.some((l) => l.enabled !== false && H.info.types[l.type]?.audio)
  : parseSource(doc.source).audio || /frame\.audio/.test(doc.source);

// the parts the server turns into an effect file (builder), or the file itself (code)
function payload(doc) {
  if (doc.mode === "code") return { source: doc.source };
  return { name: doc.name, description: doc.description, tags: doc.tags, remix_of: doc.remix_of, layers: doc.layers };
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
      <div class="btn-row"><a class="btn" href="#studio/community">Community effects</a><button class="btn primary" data-st="new">New effect</button></div></div>
    ${rig ? "" : `<div class="banner"><span>No lights are set up yet, so previews have nothing to light. Set them up in Devices first.</span></div>`}
    <section class="st-sec${pickStart ? " flash" : ""}" id="st-start"><h2 class="st-h">Start from</h2>
      <div class="st-grid">${LAYER_TEMPLATES.map((t) => `<button class="st-card st-tpl" data-tpl="${t.id}"><canvas class="st-live" aria-hidden="true"></canvas>
        <div class="st-cap"><strong>${esc(t.name)}</strong><span>${esc(t.description)}</span></div></button>`).join("")}
        ${CODE_TEMPLATES.map((t) => `<button class="st-card st-tpl st-code" data-code-tpl="${t.id}"><span class="st-codeglyph" aria-hidden="true">{ }</span>
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
  </div>`);
  if (pickStart) $("#st-start")?.scrollIntoView({ block: "start" });
  queueStills([...LAYER_TEMPLATES.map((t) => ({ key: t.id, eff: { kind: "layers", layers: t.layers, params: {} } })),
               ...effects.map((e) => ({ key: e.fx_id, eff: effectRunnable(e) }))]);
}

function effectRunnable(e) {
  const params = Object.fromEntries((e.schema || []).map((s) => [s.id, s.default]));
  return e.kind === "layers" ? { kind: "layers", layers: e.layers, params } : { kind: "code", code: e.code, params };
}

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
    const card = e.target.closest("[data-card], [data-tpl]");
    const key = card?.dataset.card || card?.dataset.tpl;
    if (!key || STILL.live?.key === key) return;
    STILL.live?.stop();
    STILL.live = null;
    const item = STILL.items.find((i) => i.key === key);
    if (item) STILL.live = playOn(card.querySelector(".st-live"), item.eff, key);
  };
  box().onpointerleave = () => { STILL.live?.stop(); STILL.live = null; };
}

function cardCanvas(key) {
  return $(`[data-card="${CSS.escape(key)}"] .st-live, [data-tpl="${CSS.escape(key)}"] .st-live`, box());
}

function still(canvas, eff, gen) {
  return new Promise((resolve) => {
    const { points, slices } = ledPoints(litZones());
    let done = false;
    const finish = () => { if (done) return; done = true; r.destroy(); resolve(); };
    const r = new EffectRunner({
      onReady: () => r.tick(2.0, 1 / 30, testAudio(2.0)),
      onFrame: (colors) => { if (gen === STILL.gen) paintRig(canvas, zoneFrames(colors, slices), { pad: 10 }); finish(); },
      onStopped: finish, onError: finish,
    });
    r.load(eff, points);
    setTimeout(finish, 2500);
  });
}

function playOn(canvas, eff, key) {
  const { points, slices } = ledPoints(litZones());
  let raf = 0, t0 = performance.now(), last = t0;
  const r = new EffectRunner({ onFrame: (colors) => paintRig(canvas, zoneFrames(colors, slices), { pad: 10 }) });
  r.load(eff, points);
  const loop = (now) => { const t = (now - t0) / 1000 + 2; r.tick(t, (now - last) / 1000, testAudio(t)); last = now; raf = requestAnimationFrame(loop); };
  raf = requestAnimationFrame(loop);
  return { key, stop: () => { cancelAnimationFrame(raf); r.destroy(); } };
}

// ------------------------------------------------------------ community
async function renderCommunity() {
  setHTML(box(), `<div class="st-home"><div class="page-head"><div><h1>Community effects</h1>
    <p class="lede">Effects people made in the Studio and shared. Point at one to see it on your own case.</p></div>
    <div class="btn-row"><a class="btn" href="#studio">Back to the Studio</a></div></div><p class="hint">Loading the gallery from GitHub…</p></div>`);
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
    <div class="btn-row"><a class="btn" href="#studio">Back to the Studio</a></div></div>
    <div class="st-filter"><input class="text" id="st-search" placeholder="Search names, authors, tags" value="${esc(H.search)}" spellcheck="false">
      <div class="chips"><button class="chip-btn" data-tag="" aria-pressed="${!H.tag}">All</button><button class="chip-btn" data-tag="music" aria-pressed="${H.tag === "music"}">Music</button>
      ${tags.map((t) => `<button class="chip-btn" data-tag="${esc(t)}" aria-pressed="${H.tag === t}">${esc(t)}</button>`).join("")}</div></div>
    ${items.length ? `<div class="st-grid">${items.map((e) => `<div class="st-card" data-card="${esc(e.id)}"><canvas class="st-live" aria-hidden="true"></canvas>
      <div class="st-cap"><strong>${esc(e.name)}${e.featured ? ` <span class="tag new">Featured</span>` : ""}</strong><span>by ${esc(e.author)}${e.audio ? ` <i class="st-badge">Music</i>` : ""}</span>
      ${e.description ? `<p class="st-desc">${esc(e.description)}</p>` : ""}</div>
      <div class="st-acts">${e.installed && !e.update ? `<button class="btn" disabled>Installed</button>` : `<button class="btn primary" data-install="${esc(e.id)}">${e.update ? "Update" : "Install"}</button>`}
        <button class="btn" data-gremix="${esc(e.id)}">Remix</button></div></div>`).join("")}</div>`
      : `<p class="hint">${g.effects.length ? "Nothing matches." : "No shared effects yet. Make one and press Share: yours could be the first."}</p>`}
  </div>`);
  $("#st-search")?.addEventListener("input", (e) => { H.search = e.target.value; clearTimeout(H.t); H.t = setTimeout(() => { paintCommunity(); $("#st-search").focus(); }, 250); });
  // stills need each effect's file: fetched as cards come into view
  queueStills([]);
  const io = new IntersectionObserver(async (entries) => {
    for (const en of entries) {
      if (!en.isIntersecting) continue;
      io.unobserve(en.target);
      const id = en.target.dataset.card;
      try {
        const { effect } = await get(`/api/studio/gallery/effect?id=${encodeURIComponent(id)}`);
        addStill({ key: id, eff: effectRunnable(effect) });
      } catch { /* the card stays dark */ }
    }
  });
  $$(".st-card", box()).forEach((c) => io.observe(c));
}

// ------------------------------------------------------------ the editor
async function openEditor(doc) {
  stopStills();
  E.doc = doc;
  E.sel = doc.mode === "layers" && doc.layers.length ? doc.layers.length - 1 : "effect";
  E.logs = [];
  E.problem = null;
  E.layersCopy = null;
  if (location.hash !== "#studio/edit") history.replaceState(null, "", "#studio/edit");
  renderEditor();
  await restartRunner();
  E.t0 = E.last = performance.now();
  cancelAnimationFrame(E.raf);
  E.raf = requestAnimationFrame(loop);
}

function closeEditor(keepHash) {
  cancelAnimationFrame(E.raf);
  E.runner?.destroy();
  E.runner = null;
  clearInterval(E.watch);
  E.watch = null;
  E.doc = null;
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
      <span class="st-saved" id="st-saved">${d.dirty ? "Not saved" : "Saved"}</span>
      <button class="btn" data-st="save">Save</button>
      <button class="btn" data-st="use">Use on my lights</button>
      <button class="btn primary" data-st="share">Share</button>
    </header>
    <aside class="st-left" id="st-left">${d.mode === "layers" ? layersHTML() : `<div class="st-code" id="st-code"></div>`}</aside>
    <section class="st-mid">
      <div class="st-stagebar">
        <label class="st-sw"><span class="switch"><input type="checkbox" id="st-lights"${E.lights ? " checked" : ""}><span></span></span> Also on my lights</label>
        <label class="st-sw"><span class="switch"><input type="checkbox" id="st-beat"${E.beat ? " checked" : ""}><span></span></span> Test beat</label>
        <span class="spacer"></span><span class="st-meter" id="st-meter"></span>
      </div>
      <div class="st-stage"><canvas id="st-canvas" aria-label="Your effect on your case"></canvas>${litZones().length ? "" : `<p class="st-empty">Set up your lights in Devices to see the effect on your case.</p>`}</div>
      <div class="st-console" id="st-console" aria-live="polite"></div>
    </section>
    <aside class="st-right" id="st-right">${inspectorHTML()}</aside>
  </div>`);
  wireEditor();
  if (d.mode === "code") mountCode();
  paintConsole();
}

// ---- left: layers, top layer first (it's drawn last)
function layersHTML() {
  const d = E.doc, T = H.info.types;
  const items = d.layers.map((l, i) => ({ l, i })).reverse();
  return `<div class="st-layers">
    <button class="st-layer st-eff" data-sel="effect" aria-pressed="${E.sel === "effect"}"><b>${esc(d.name)}</b><span>Name, description, controls</span></button>
    <ol class="st-list" id="st-list">${items.map(({ l, i }) => `<li class="st-layer" data-sel="${i}" aria-pressed="${E.sel === i}" draggable="true" role="button" tabindex="0">
      <span class="st-chip" style="background:${chipColor(l)}"></span>
      <span class="st-lname"><b>${esc(T[l.type]?.name || l.type)}</b><span>${esc(layerLine(l))}</span></span>
      <button class="icon-btn" data-toggle="${i}" aria-label="${l.enabled === false ? "Show" : "Hide"} layer" aria-pressed="${l.enabled !== false}"><svg><use href="#i-${l.enabled === false ? "eye-off" : "eye"}"/></svg></button></li>`).join("")}</ol>
    <div class="st-add"><button class="btn" data-st="add" aria-expanded="false">Add a layer</button>
      <div class="st-menu" id="st-menu" hidden>${["Colour", "Motion", "Music"].map((g) => `<div class="st-mgroup"><span>${g}</span>
        ${Object.entries(T).filter(([, t]) => t.group === g).map(([id, t]) => `<button data-add="${id}"><b>${esc(t.name)}</b><span>${esc(t.description)}</span></button>`).join("")}</div>`).join("")}</div></div>
    <p class="hint">Layers higher up draw over the ones below. Drag to reorder.</p></div>`;
}

function chipColor(l) {
  const p = { ...(H.info.types[l.type]?.params || []).reduce((o, s) => ({ ...o, [s.id]: s.default }), {}), ...l.params };
  if (p.colors) return `linear-gradient(90deg, ${p.colors.join(", ")})`;
  if (p.color) return p.color;
  return "linear-gradient(90deg, #ff2fb4, #ffb21a, #22e06b, #00b4ff, #8a3dff)";
}

function layerLine(l) {
  const bits = [];
  if ((l.opacity ?? 100) < 100) bits.push(`${l.opacity}%`);
  if (l.blend && l.blend !== "normal") bits.push(BLENDS[l.blend].toLowerCase());
  if (l.mask && l.mask !== "all") bits.push(`only ${AREAS[l.mask].toLowerCase()}`);
  return bits.join(", ") || "Whole case";
}

// ---- right: the selected layer, or the effect's details
function inspectorHTML() {
  const d = E.doc;
  if (d.mode === "code") return codeInspectorHTML();
  if (E.sel === "effect") {
    const exposed = d.layers.flatMap((l, i) => Object.entries(l.expose || {}).map(([k, label]) => ({ i, k, label, t: H.info.types[l.type] })));
    return `<div class="ins-block"><div class="ins-title"><h2>Effect</h2></div>
      <div class="field"><label class="lab" for="st-desc"><span>Description</span></label><textarea class="text" id="st-desc" rows="3" maxlength="200" placeholder="What does it look like?">${esc(d.description)}</textarea></div>
      <div class="field"><label class="lab" for="st-tags"><span>Tags</span></label><input class="text" id="st-tags" value="${esc(d.tags.join(", "))}" placeholder="ambient, warm, music" spellcheck="false"></div>
      ${d.remix_of ? `<p class="hint">A remix of ${esc(d.remix_of)}. Sharing credits the original.</p>` : ""}
      <div class="sub-head">Controls people can change</div>
      ${exposed.length ? `<ul class="st-exposed">${exposed.map((x) => `<li><input class="text" data-expose-label="${x.i}:${x.k}" value="${esc(x.label)}" maxlength="32" aria-label="Control name"><span>${esc(x.t.name)} layer</span></li>`).join("")}</ul>`
        : `<p class="hint">None yet. In a layer, tick "Show as a control" under a setting, and it appears in everyone's effect panel.</p>`}
      </div>`;
  }
  const l = d.layers[E.sel], t = H.info.types[l.type];
  const vals = { ...Object.fromEntries(t.params.map((s) => [s.id, s.default])), ...l.params };
  const general = [
    { id: "opacity", label: "Strength", type: "range", min: 0, max: 100, step: 1, unit: "%", default: 100 },
    { id: "blend", label: "Mix with the layers below", type: "choice", default: "normal", options: Object.entries(BLENDS).map(([value, label]) => ({ value, label })) },
    { id: "mask", label: "Where", type: "choice", default: "all", options: Object.entries(AREAS).map(([value, label]) => ({ value, label })) }];
  return `<div class="ins-block"><div class="ins-title"><h2>${esc(t.name)}</h2>
      <span class="btn-row"><button class="icon-btn" data-st="dup" aria-label="Duplicate layer" title="Duplicate"><svg><use href="#i-copy"/></svg></button>
      <button class="icon-btn" data-st="del" aria-label="Delete layer" title="Delete"><svg><use href="#i-trash"/></svg></button></span></div>
    <p class="ins-desc">${esc(t.description)}${t.audio ? " It follows what's playing; switch on Test beat to build it without music." : ""}</p>
    <div class="ins-block" id="st-params">${t.params.map((s) => fieldHTML(s, vals[s.id], "lp") + (s.type === "colors" ? "" :
      `<label class="st-expose"><input type="checkbox" data-expose="${s.id}"${l.expose?.[s.id] ? " checked" : ""}> Show as a control</label>`)).join("")}</div>
    <div class="rule-line"></div>
    <div class="ins-block" id="st-general">${general.map((s) => fieldHTML(s, l[s.id] ?? s.default, "lg")).join("")}</div></div>`;
}

function codeInspectorHTML() {
  const p = parseSource(E.doc.source);
  const vals = Object.fromEntries(p.params.map((s) => [s.id, E.doc.values[s.id] ?? s.default]));
  return `<div class="ins-block"><div class="ins-title"><h2>Controls</h2></div>
    ${p.params.length ? `<div class="ins-block" id="st-cparams">${p.params.map((s) => fieldHTML(s, vals[s.id], "cp")).join("")}</div>`
      : `<p class="hint">Add controls with &lt;meta property="speed" label="Speed" type="number" min="0" max="5" default="1"&gt; at the top. Types: number, color, boolean, select.</p>`}
    <div class="rule-line"></div>
    <div class="sub-head">Effect API</div>
    <ul class="st-api"><li><code>render(ctx, frame)</code> draw on <code>ctx.canvas</code> (320 × 200)</li>
      <li><code>point(led, frame)</code> or answer per LED: <code>[r, g, b]</code></li>
      <li><code>ctx.params</code> your controls, <code>ctx.log()</code> prints below the preview</li>
      <li><code>frame.time</code>, <code>frame.dt</code> in seconds; <code>frame.audio</code> <code>{level, bass, beat, bands[64]}</code></li></ul>
    <button class="btn" data-st="editor">Open in my editor</button>
    <p class="hint" id="st-editor-note">Opens the file in VS Code (or its folder); every save reloads the preview.</p></div>`;
}

function wireEditor() {
  const d = E.doc;
  $("#st-name").addEventListener("input", (e) => {
    d.name = e.target.value;
    if (d.mode === "code") { d.source = d.source.replace(/<title>[\s\S]*?<\/title>/i, `<title>${esc(d.name)}</title>`); E.cm?.setValue(d.source); }
    const eff = $(".st-eff b"); if (eff) eff.textContent = d.name;
    changed(false);
  });
  $("#st-lights").addEventListener("change", (e) => setLights(e.target.checked));
  $("#st-beat").addEventListener("change", (e) => { E.beat = e.target.checked; });
  wireInspector();
  wireDrag();
}

function wireInspector() {
  const d = E.doc;
  if (d.mode === "code") {
    const p = parseSource(d.source);
    const root = $("#st-cparams");
    if (root) wireFields(root, { params: p.params }, Object.fromEntries(p.params.map((s) => [s.id, d.values[s.id] ?? s.default])), (vals) => {
      d.values = vals; E.runner?.setParams(vals); syncLights();
    });
    return;
  }
  if (E.sel === "effect") {
    $("#st-desc")?.addEventListener("input", (e) => { d.description = e.target.value; changed(false); });
    $("#st-tags")?.addEventListener("input", (e) => { d.tags = e.target.value.split(",").map((t) => t.trim().toLowerCase().replace(/[^a-z0-9-]/g, "")).filter(Boolean).slice(0, 5); changed(false); });
    $$("[data-expose-label]").forEach((inp) => inp.addEventListener("input", () => {
      const [i, k] = inp.dataset.exposeLabel.split(":");
      d.layers[+i].expose[k] = inp.value.trim() || k;
      changed(false);
    }));
    return;
  }
  const l = d.layers[E.sel], t = H.info.types[l.type];
  const vals = { ...Object.fromEntries(t.params.map((s) => [s.id, s.default])), ...l.params };
  wireFields($("#st-params"), { params: t.params }, vals, (v) => { l.params = v; changed(true); refreshLayerRow(); });
  const general = [{ id: "opacity", type: "range", unit: "%" }, { id: "blend", type: "choice" }, { id: "mask", type: "choice" }];
  wireFields($("#st-general"), { params: general }, { opacity: l.opacity ?? 100, blend: l.blend || "normal", mask: l.mask || "all" }, (v) => {
    Object.assign(l, v); changed(true); refreshLayerRow();
  });
  $$("[data-expose]").forEach((cb) => cb.addEventListener("change", () => {
    const k = cb.dataset.expose, spec = t.params.find((s) => s.id === k);
    l.expose ||= {};
    if (cb.checked) {
      const taken = new Set(d.layers.flatMap((x) => Object.values(x.expose || {})));
      l.expose[k] = taken.has(spec.label) ? `${t.name} ${spec.label.toLowerCase()}` : spec.label;
    } else delete l.expose[k];
    changed(false);
  }));
}

function refreshLayerRow() {
  const li = $(`#st-list [data-sel="${E.sel}"]`);
  const l = E.doc.layers[E.sel];
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
    renumberExpose();
    rerenderSides(); changed(true);
  });
}

// controls are named after their layer's position (l<index>_<setting>); keep labels with their layer when it moves
function renumberExpose() { /* expose lives on the layer itself, so ids follow on save; nothing to move */ }

function rerenderSides() {
  if (E.doc.mode === "layers") $("#st-left").innerHTML = layersHTML();
  $("#st-right").innerHTML = inspectorHTML();
  $$("#st-right input[type=range]").forEach(rangeFill);
  wireInspector();
  wireDrag();
}

// ------------------------------------------------------------ changes: preview, lights, saved state
const pushLayers = coalesce(() => E.runner?.setLayers(E.doc.layers), 30);

function changed(visual) {
  E.doc.dirty = true;
  const s = $("#st-saved"); if (s) s.textContent = "Not saved";
  if (visual && E.doc.mode === "layers") pushLayers();
  syncLights();
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
  const d = E.doc;
  if (!d) return;
  const { points, slices } = ledPoints(litZones());
  E.slices = slices;
  const gen = ++E.gen;
  E.runner = new EffectRunner({
    onFrame: (colors, ms) => {
      if (gen !== E.gen) return;
      E.frames = zoneFrames(colors, E.slices); E.ms = E.ms * 0.9 + ms * 0.1; E.frameCount++;
    },
    onBackdrop: (bmp) => { if (gen === E.gen) { E.backdrop?.close?.(); E.backdrop = bmp; } else bmp.close?.(); },
    onLog: (t) => { E.logs.push({ t }); E.logs = E.logs.slice(-40); paintConsole(); },
    onError: (err) => { if (gen === E.gen) { E.logs.push({ err: `${err.message}${err.line ? ` (line ${err.line} of the script)` : ""}` }); paintConsole(); } },
    onStopped: (why) => {
      if (gen !== E.gen || why === "stopped" || why === "error") return;
      E.logs.push({ err: why === "hung" ? "It stopped answering for 3 seconds (an endless loop?), so it was stopped." : "Each frame took longer than 20 ms, so it was stopped. Draw less, or at a lower resolution." });
      paintConsole();
    },
  });
  await E.runner.load(runnable(d), points);
}

function loop(now) {
  if (!E.doc) return;
  if (now - E.last < 15) { E.raf = requestAnimationFrame(loop); return; }      // 60 fps is plenty on any monitor
  const t = (now - E.t0) / 1000, dt = Math.min(0.25, (now - E.last) / 1000);
  E.last = now;
  const audio = E.beat ? testAudio(t) : null;
  E.runner?.tick(t, dt, audio, E.frameCount % 2 === 0);
  const c = $("#st-canvas");
  if (c) paintRig(c, E.frames, { fit: "stage", backdrop: E.backdrop, pad: 26, maxScale: 4 });
  if (now - E.fpsAt > 1000) {
    E.fps = E.frameCount; E.frameCount = 0; E.fpsAt = now;
    const m = $("#st-meter");
    if (m) m.innerHTML = `${E.fps} fps · <b class="${E.ms > 15 ? "warm" : ""}">${E.ms.toFixed(1)} ms</b> a frame${E.ms > 15 ? " · close to the 20 ms limit" : ""}`;
  }
  E.raf = requestAnimationFrame(loop);
}

function paintConsole() {
  const c = $("#st-console");
  if (!c) return;
  // the lights only matter here while they play this draft, or this effect
  const host = (S.state?.studio?.problems || []).filter((p) => (p.effect === "fx:draft" && E.lights) || (E.doc?.fx_id && p.effect === E.doc.fx_id));
  const lines = [...host.map((p) => ({ err: `On your lights: ${p.text}${p.message ? ` ${p.message}` : ""}` })),
    ...(E.problem ? [{ err: E.problem }] : []), ...E.logs.slice(-8)];
  c.hidden = !lines.length && E.doc?.mode !== "code";
  setHTML(c, lines.length ? lines.map((l) => `<div class="${l.err ? "st-err" : ""}">${esc(l.err || l.t)}</div>`).join("")
    : `<div class="hint">ctx.log() output and errors show here.</div>`);
}

function paintProblems() { if (E.doc) paintConsole(); }

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
  if (!E.doc) return;
  E.logs = [];
  const p = parseSource(E.doc.source);
  if (p.name && p.name !== E.doc.name) { E.doc.name = p.name; $("#st-name").value = p.name; }
  $("#st-right").innerHTML = inspectorHTML();
  $$("#st-right input[type=range]").forEach(rangeFill);
  wireInspector();
  await restartRunner();
  try {
    const r = await post("/api/studio/check", { source: E.doc.source });
    E.problem = null;
    if (r.flags.length) E.logs.push({ t: `Note for the gallery review: it ${r.flags.join("; ")}.` });
  } catch (e) { E.problem = e.message; }
  paintConsole();
}

async function toCode() {
  const d = E.doc;
  try {
    const built = await post("/api/studio/build", payload(d));
    const layers = d.layers.map((l) => JSON.stringify(l)).join(",\n  ");
    const code = `// Made from layers in the Studio. AirglowLayers draws them (it comes with every effect): change the list,
// or add your own code, e.g. airglow.effect({ ...AirglowLayers.effect(LAYERS), point(led, frame) { ... } }).
const LAYERS = [\n  ${layers}\n];\nairglow.effect(AirglowLayers.effect(LAYERS));`;
    const r = await post("/api/studio/build", { name: d.name, description: d.description, tags: d.tags, remix_of: d.remix_of, code, params: built.effect.params });
    E.layersCopy = structuredClone(d.layers);
    d.mode = "code"; d.source = r.source; d.values = {};
    renderEditor();
    await restartRunner();
  } catch (e) { fail(e); }
}

function toLayers() {
  if (!E.layersCopy) { toast("This effect was written in code, so it has no layers to go back to.", true); renderEditor(); return; }
  if (!confirm("Go back to the layers? Changes you made in the code are dropped.")) { renderEditor(); return; }
  E.doc.mode = "layers"; E.doc.layers = E.layersCopy; E.layersCopy = null;
  renderEditor(); restartRunner(); changed(true);
}

async function openInEditor() {
  try {
    const r = await post("/api/studio/editor", { source: E.doc.source });
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

// ------------------------------------------------------------ save, use, share
async function save() {
  const d = E.doc;
  try {
    const r = await post("/api/studio/save", { ...payload(d), fx_id: d.fx_id });
    d.fx_id = r.effect.fx_id; d.dirty = false;
    $("#st-saved").textContent = "Saved";
    S.effects = await get("/api/effects");
    emit("refresh");
    return r.effect;
  } catch (e) { fail(e); return null; }
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
      // GitHub only takes about 8 KB in an address: a longer effect goes on the clipboard, to paste on the page
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

// ------------------------------------------------------------ clicks
async function onClick(e) {
  const t = e.target.closest("button, a, [data-sel]");
  if (!t) return;
  const ds = t.dataset;
  if (ds.st === "new") { $("#st-start")?.scrollIntoView({ behavior: "smooth" }); $("#st-start")?.classList.add("flash"); return; }
  if (ds.st === "community-retry") return renderCommunity();
  if (ds.tpl) { const tp = LAYER_TEMPLATES.find((x) => x.id === ds.tpl); return openEditor(newDoc({ name: tp.id === "blank" ? "Untitled effect" : `My ${tp.name.toLowerCase()}`, layers: tp.layers })); }
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
    E.doc.layers.push({ type: ds.add, enabled: true, opacity: 100, blend: E.doc.layers.length && !["solid", "gradient", "rainbow", "plasma", "cycle"].includes(ds.add) ? "add" : "normal",
      mask: "all", params: Object.fromEntries(spec.params.map((s) => [s.id, structuredClone(s.default)])), expose: {} });
    E.sel = E.doc.layers.length - 1;
    if (spec.audio && !E.beat) { E.beat = true; const cb = $("#st-beat"); if (cb) cb.checked = true; toast("Test beat is on, so you can see it without music"); }
    rerenderSides(); return changed(true);
  }
  if (ds.st === "del" && typeof E.sel === "number") {
    if (E.doc.layers.length === 1) return toast("An effect needs at least one layer.", true);
    E.doc.layers.splice(E.sel, 1); E.sel = Math.min(E.sel, E.doc.layers.length - 1); rerenderSides(); return changed(true);
  }
  if (ds.st === "dup" && typeof E.sel === "number") {
    if (E.doc.layers.length >= 12) return toast("12 layers is the most an effect can have.", true);
    E.doc.layers.splice(E.sel + 1, 0, { ...structuredClone(E.doc.layers[E.sel]), expose: {} }); E.sel++; rerenderSides(); return changed(true);
  }
  if (ds.st === "save") { if (await save()) toast("Saved. It's in your effects, and in Lighting's effect list."); return; }
  if (ds.st === "use") return useIt();
  if (ds.st === "share") return shareIt();
  if (ds.st === "editor") return openInEditor();
}

function moreSheet(eff) {
  if (!eff) return;
  const mine = eff.origin === "mine";
  openSheet(`<div class="body"><h2>${esc(eff.name)}</h2>
    ${eff.description ? `<p class="lede" style="margin:0">${esc(eff.description)}</p>` : ""}
    <div class="btn-row">
      ${mine ? `<button class="btn" data-m="share">Share</button>` : `<button class="btn" data-m="rollback">Go back to the previous version</button>`}
      <button class="btn" data-m="remix">Remix</button>
      <button class="btn danger" data-m="delete">${mine ? "Delete" : "Uninstall"}</button></div>
    <div class="actions"><span class="spacer"></span><button class="btn quiet" data-close>Close</button></div></div>`);
  $("#sheet").addEventListener("click", async (ev) => {
    const m = ev.target.closest("[data-m]")?.dataset.m;
    if (!m) return;
    if (m === "share") return shareIt(eff.fx_id);
    if (m === "remix") { closeSheet(); return openEditor(docFromEffect(eff, { remix: true })); }
    try {
      if (m === "delete") {
        if (!confirm(mine ? `Delete ${eff.name}? This can't be undone.` : `Uninstall ${eff.name}?`)) return;
        await post("/api/studio/delete", { fx_id: eff.fx_id });
        toast(mine ? "Deleted" : "Uninstalled");
      } else if (m === "rollback") {
        await post("/api/studio/rollback", { fx_id: eff.fx_id }); toast("Back to the previous version");
      }
      closeSheet(); S.effects = await get("/api/effects"); await loadLibrary(); renderHome();
    } catch (err) { fail(err); }
  });
}
