// Overview: a grid of tiles you arrange yourself. Sensor tiles (number + graph, gauge, big graph, bar) and control
// tiles (lighting, presets, fans). Tiles are built once and updated in place, so numbers count and graphs morph
// instead of flashing, and dragging never fights the 2-second refresh.

import { $, $$, S, clamp, curveLabel, effectName, emit, esc, fail, on, post, rangeFill, sceneColor, sceneSwatch, setHTML, toast } from "./core.js";
import { R, defaultStyle, ensureHistory, fmt, fmtText, level, loadCatalog, range, resample, sensorName, toDisplay, value } from "./readings.js";
import { EASE, SPRING, countTo, flip, reduced } from "./motion.js";
import { paintMini } from "./stage.js";
import { closeSheet, openSheet } from "./sheet.js";

const COLS = 4;
const ROW_H = 152, GAP = 12;
const STYLES = [
  { id: "spark", name: "Number" }, { id: "ring", name: "Gauge" }, { id: "graph", name: "Graph" }, { id: "bar", name: "Bar" },
];
const CONTROL = {
  lighting: { name: "Lighting", desc: "A live miniature of your rig, with on/off and brightness.", w: 2, h: 2 },
  presets: { name: "Presets", desc: "One click to switch lighting and fans together.", w: 2, h: 1 },
  fans: { name: "Case fans", desc: "Fan speed and the curve they follow.", w: 1, h: 1 },
};

const O = { layout: null, editing: false, els: new Map(), drag: null };
const grid = () => $("#ov-grid");
const visible = () => !$('[data-view="overview"]').hidden;

// ------------------------------------------------------------ persistence
let saveTimer;
function save() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(async () => {
    try { O.layout = await post("/api/overview", O.layout); S.overview = O.layout; emit("pins"); }
    catch (e) { fail(e); }
  }, 350);
}

const tileById = (id) => O.layout.tiles.find((t) => t.id === id);
const newId = (base) => {
  const b = base.replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 20) || "tile";
  let i = 1, id = b;
  while (tileById(id)) id = `${b}-${++i}`;
  return id;
};

// ------------------------------------------------------------ building tiles
function shell(t) {
  const el = document.createElement("article");
  el.className = `tile tile-${t.type}`;
  el.dataset.id = t.id;
  el.innerHTML = `<div class="tile-inner"></div>
    <div class="tile-tools"><button class="icon-btn" data-act="edit" title="Tile settings"><svg><use href="#i-sliders"/></svg></button>
      <button class="icon-btn" data-act="remove" title="Remove tile"><svg><use href="#i-close"/></svg></button></div>
    <span class="tile-resize" data-act="resize" title="Drag to resize" aria-hidden="true"></span>`;
  if (t.type !== "sensor") el.querySelector('[data-act="edit"]').remove();
  return el;
}

function sizeTile(el, t) {
  el.style.gridColumn = `span ${t.w}`;
  el.style.gridRow = `span ${t.h}`;
  el.dataset.w = t.w; el.dataset.h = t.h;
}

// Sensor tile markup per style. Values and paths are filled in by update().
function sensorMarkup(t) {
  const two = t.sensors.length > 1;
  const head = `<header class="tile-head"><span class="tile-name"></span><span class="tile-src"></span></header>`;
  if (t.style === "ring") {
    return `${head}<div class="readout"><span class="num">–</span><span class="unit"></span></div><div class="g-cap"></div><div class="gauge"><svg viewBox="0 0 120 120" aria-hidden="true">
        <path class="g-track" d="${arc(60, 60, 48, 135, 405)}" pathLength="100"/>
        <path class="g-fill" d="${arc(60, 60, 48, 135, 405)}" pathLength="100" stroke-dasharray="0 100"/>
        <text class="g-lo" x="26" y="114" text-anchor="middle"></text><text class="g-hi" x="94" y="114" text-anchor="middle"></text></svg>
      <div class="g-read"><span class="num">–</span><span class="unit"></span></div></div>`;
  }
  if (t.style === "bar") {
    return `${head}<div class="readout"><span class="num">–</span><span class="unit"></span><span class="of"></span></div>
      <div class="hbar"><i class="fill"></i><b class="peak" title="Highest in the last 10 minutes"></b></div>
      <div class="bar-foot"><span class="lo"></span><span class="peak-txt"></span></div>`;
  }
  if (t.style === "graph") {
    return `${head}<div class="legend">${t.sensors.map((_, i) => `<span class="lg lg-${i}"><i></i><span class="lg-name"></span><b class="num">–</b><span class="unit"></span></span>`).join("")}</div>
      <div class="chart"><svg viewBox="0 0 400 100" preserveAspectRatio="none" aria-hidden="true">
        <line class="gl" x1="0" x2="400" y1="0.5" y2="0.5"/><line class="gl" x1="0" x2="400" y1="50" y2="50"/><line class="gl" x1="0" x2="400" y1="99.5" y2="99.5"/>
        ${t.sensors.map((_, i) => `<path class="area a-${i}"/><path class="line l-${i}"/>`).join("")}</svg>
        <span class="ax ax-hi"></span><span class="ax ax-mid"></span><span class="ax-time">10 min</span></div>`;
  }
  return `${head}<div class="readout"><span class="num">–</span><span class="unit"></span>${two ? "" : ""}</div>
    <div class="minmax"></div>
    <svg class="spark" viewBox="0 0 300 60" preserveAspectRatio="none" aria-hidden="true"><path class="area"/><path class="line"/></svg>`;
}

function controlMarkup(t) {
  if (t.type === "lighting") {
    return `<a class="mini" href="#lighting" title="Open Lighting"><canvas aria-label="Live preview of your lighting"></canvas><span class="mini-empty" hidden>No lighting connected</span></a>
      <footer class="lt-foot"><div class="lt-what"><b class="lt-effect"></b><span class="lt-sub"></span></div>
        <input type="range" class="lt-bri" min="0" max="100" aria-label="Brightness">
        <label class="switch" title="Lights on or off"><input type="checkbox" class="lt-on"><span></span></label></footer>`;
  }
  if (t.type === "presets") {
    return `<header class="tile-head"><span class="tile-name">Presets</span><a class="tile-src link" href="#lighting">Edit</a></header><div class="chips pcards"></div>`;
  }
  return `<header class="tile-head"><span class="tile-name">Case fans</span><span class="tile-src fan-curve"></span></header>
    <div class="readout"><span class="num">–</span><span class="unit">%</span></div><div class="fan-sub"></div>
    <div class="seg fan-seg" role="radiogroup" aria-label="Fan curve"></div>`;
}

function mount(t) {
  let el = O.els.get(t.id);
  const sig = t.type === "sensor" ? `${t.style}|${t.sensors.length}` : t.type;
  if (!el) { el = shell(t); O.els.set(t.id, el); }
  if (el._sig !== sig) {
    el._sig = sig;
    el.className = `tile tile-${t.type}${t.type === "sensor" ? ` s-${t.style}` : ""}`;
    el.querySelector(".tile-inner").innerHTML = t.type === "sensor" ? sensorMarkup(t) : controlMarkup(t);
    el._paths = null;
    if (t.type === "lighting") wireLighting(el);
  }
  sizeTile(el, t);
  return el;
}

// ------------------------------------------------------------ geometry helpers
function arc(cx, cy, r, a0, a1) {
  const p = (a) => [cx + r * Math.cos(a * Math.PI / 180), cy + r * Math.sin(a * Math.PI / 180)];
  const [x0, y0] = p(a0), [x1, y1] = p(a1);
  return `M${x0.toFixed(2)} ${y0.toFixed(2)}A${r} ${r} 0 ${a1 - a0 > 180 ? 1 : 0} 1 ${x1.toFixed(2)} ${y1.toFixed(2)}`;
}

// Path through resampled points; gaps (no data yet) are skipped. Same point count every time => smooth morphing.
function seriesPath(vals, lo, hi, w, h, pad = 2) {
  const n = vals.length;
  let d = "", area = "", first = null, last = null;
  vals.forEach((v, i) => {
    if (v == null) return;
    const x = (i / (n - 1)) * w, y = h - pad - ((clamp(v, lo, hi) - lo) / (hi - lo || 1)) * (h - pad * 2);
    d += `${d ? "L" : "M"}${x.toFixed(1)} ${y.toFixed(1)}`;
    if (first == null) first = x;
    last = x;
  });
  if (!d) return { d: "", area: "" };
  area = `${d}L${last.toFixed(1)} ${h}L${first.toFixed(1)} ${h}Z`;
  return { d, area };
}

function setPath(el, d) {
  if (!el) return;
  if (el._d === d) return;
  const same = el._d && el._d.split(/[ML]/).length === d.split(/[ML]/).length;
  el._d = d;
  // morph with CSS when the shape has the same structure, otherwise swap instantly
  if (same && !reduced()) el.style.d = `path("${d}")`; else { el.style.transition = "none"; el.style.d = `path("${d}")`; el.offsetWidth; el.style.transition = ""; }
}

// ------------------------------------------------------------ updating
function unavailable(id) {
  const meta = R.meta[id];
  const needsLhm = !/^(sys\/|app\/|auto\/(ram_load|ram_used|net_|disk_))/.test(id);
  if (needsLhm && !S.state?.sensors?.online) return "Needs LibreHardwareMonitor";
  if (!meta) return "Not found on this PC";
  return "No reading on this PC";
}

function source(meta) {
  if (!meta) return "";
  const s = meta.source || (meta.group !== "Quick picks" && meta.group !== "Windows" ? meta.group : "");
  return (s || "").replace(/^AMD |^Intel\(R\) Core\(TM\) |^NVIDIA GeForce |^NVIDIA |\(TM\)|\(R\)/g, "").trim();
}

function updateSensor(el, t) {
  const id = t.sensors[0], meta = R.meta[id], v = value(id);
  const name = t.label || meta?.name || sensorName(id);
  setHTML(el.querySelector(".tile-name"), esc(name));
  setHTML(el.querySelector(".tile-src"), esc(t.style === "graph" ? "" : source(meta)));
  const missing = v == null && t.style !== "graph";
  el.classList.toggle("missing", missing);
  let note = el.querySelector(".tile-note");
  if (missing) {
    if (!note) { note = document.createElement("p"); note.className = "tile-note"; el.querySelector(".tile-inner").append(note); }
    note.textContent = unavailable(id);
  } else note?.remove();

  const lvl = level(id, meta, v);
  el.dataset.level = lvl;
  const f = fmt(meta, v);
  const [lo, hi] = range(id, meta);

  if (t.style === "spark") {
    countTo(el.querySelector(".num"), f.num);
    setHTML(el.querySelector(".unit"), esc(f.unit));
    const pts = resample(R.series[id]);
    const vals = pts.filter((x) => x != null);
    // scale the sparkline to its own recent range so small changes stay visible
    let a = Math.min(...vals), b = Math.max(...vals);
    if (meta?.unit === "%") { a = Math.min(a, 0); b = Math.max(b, 10); }
    const span = Math.max(b - a, meta?.unit === "°C" ? 8 : meta?.unit === "W" ? 20 : (b || 1) * 0.2);
    const mid = (a + b) / 2;
    const p = vals.length ? seriesPath(pts, mid - span * 0.6, mid + span * 0.6, 300, 60, 4) : { d: "", area: "" };
    setPath(el.querySelector(".spark .line"), p.d);
    setPath(el.querySelector(".spark .area"), p.area);
    const lo10 = vals.length ? fmt(meta, Math.min(...vals)) : null, hi10 = vals.length ? fmt(meta, Math.max(...vals)) : null;
    setHTML(el.querySelector(".minmax"), lo10 && vals.length > 3 ? `<span>${esc(lo10.num)}–${esc(hi10.num)}${esc(hi10.unit === "°C" || hi10.unit === "°F" ? "°" : " " + hi10.unit)}</span> over 10 min` : "");
  } else if (t.style === "ring") {
    el.querySelectorAll(".num").forEach((n) => countTo(n, f.num));
    el.querySelectorAll(".unit").forEach((u) => setHTML(u, esc(f.unit)));
    const k = v == null ? 0 : clamp((v - lo) / (hi - lo || 1), 0, 1);
    setHTML(el.querySelector(".g-cap"), meta?.unit === "%" ? "" : `of ${esc(fmtText(meta, hi))}`);
    el.querySelector(".g-fill").style.strokeDasharray = `${(k * 100).toFixed(2)} 100`;
    el.querySelector(".g-lo").textContent = fmtText(meta, lo);
    el.querySelector(".g-hi").textContent = fmtText(meta, hi);
  } else if (t.style === "bar") {
    countTo(el.querySelector(".num"), f.num);
    setHTML(el.querySelector(".unit"), esc(f.unit));
    setHTML(el.querySelector(".of"), meta?.max ? `of ${esc(fmtText(meta, meta.max))}` : "");
    const k = v == null ? 0 : clamp((v - lo) / (hi - lo || 1), 0, 1);
    el.querySelector(".hbar .fill").style.width = `${(k * 100).toFixed(1)}%`;
    const vals = (R.series[id] || []).map((p) => p[1]);
    const peak = vals.length ? Math.max(...vals) : null;
    const pk = peak == null ? null : clamp((peak - lo) / (hi - lo || 1), 0, 1);
    const peakEl = el.querySelector(".hbar .peak");
    peakEl.hidden = pk == null;
    if (pk != null) peakEl.style.left = `${(pk * 100).toFixed(1)}%`;
    setHTML(el.querySelector(".bar-foot .lo"), meta?.max ? "" : esc(`${fmtText(meta, lo)} to ${fmtText(meta, hi)}`));
    setHTML(el.querySelector(".peak-txt"), peak == null ? "" : `peak ${esc(fmtText(meta, peak))}`);
  } else if (t.style === "graph") {
    const metas = t.sensors.map((i) => R.meta[i]);
    // one scale for both lines when they share a unit, so they can be compared
    let [glo, ghi] = range(id, meta);
    if (t.sensors[1] && metas[1]?.unit === meta?.unit) {
      const [l2, h2] = range(t.sensors[1], metas[1]);
      glo = Math.min(glo, l2); ghi = Math.max(ghi, h2);
    }
    t.sensors.forEach((sid, i) => {
      const m = metas[i], sv = value(sid), sf = fmt(m, sv);
      const lg = el.querySelector(`.lg-${i}`);
      setHTML(lg.querySelector(".lg-name"), esc(m?.short && m.short !== m?.name ? m.short : (m?.name || sensorName(sid))));
      countTo(lg.querySelector(".num"), sf.num);
      setHTML(lg.querySelector(".unit"), esc(sf.unit));
      const [l, h] = i === 0 || metas[1]?.unit === meta?.unit ? [glo, ghi] : range(sid, m);
      const p = seriesPath(resample(R.series[sid]), l, h, 400, 100, 3);
      setPath(el.querySelector(`.l-${i}`), p.d);
      setPath(el.querySelector(`.a-${i}`), p.area);
    });
    setHTML(el.querySelector(".ax-hi"), esc(fmtText(meta, toDisplay(meta, ghi) === ghi ? ghi : ghi)));
    setHTML(el.querySelector(".ax-mid"), esc(fmtText(meta, (glo + ghi) / 2)));
    if (!t.label) setHTML(el.querySelector(".tile-name"), esc(t.sensors.length > 1 ? `${metas[0]?.short || ""} and ${metas[1]?.short || ""}`.trim() : name));
  }
}

function updateLighting(el) {
  const sc = S.state.scene, eff = S.state.effective;
  const preset = S.presets.find((p) => p.id === S.state.active_preset);
  setHTML(el.querySelector(".lt-effect"), esc(eff.on ? effectName(eff.effect) : "Lights off"));
  const o = S.state.override;
  const sub = o?.locked ? "Off while the PC is locked" : o?.lighting ? "An automation is in control" : preset ? `${preset.name} preset` : "Your own settings";
  setHTML(el.querySelector(".lt-sub"), esc(sub));
  const bri = el.querySelector(".lt-bri"), on = el.querySelector(".lt-on");
  if (document.activeElement !== bri && Date.now() - (bri._t || 0) > 1500) { bri.value = sc.brightness; rangeFill(bri); }
  on.checked = sc.on;
  el.style.setProperty("--scene", sceneColor(eff));
  const any = (S.zones?.zones || []).some((z) => z.device.online && z.config.enabled);
  el.querySelector(".mini-empty").hidden = any;
}

function wireLighting(el) {
  const bri = el.querySelector(".lt-bri");
  let timer;
  bri.addEventListener("input", () => {
    rangeFill(bri); bri._t = Date.now();
    clearTimeout(timer);
    timer = setTimeout(() => post("/api/scene", { brightness: +bri.value }).then((r) => { S.state.scene = r.scene; }).catch(fail), 60);
  });
  el.querySelector(".lt-on").addEventListener("change", async (e) => {
    try { const r = await post("/api/scene", { on: e.target.checked }); S.state.scene = r.scene; emit("refresh"); }
    catch (err) { fail(err); }
  });
}

function updatePresets(el) {
  const active = S.state.active_preset;
  setHTML(el.querySelector(".chips"), S.presets.length
    ? S.presets.map((p) => `<button class="pcard" data-preset="${esc(p.id)}" aria-pressed="${p.id === active}" style="--pc:${sceneColor(p)};--ps:${sceneSwatch(p)}">
        <span class="pswatch"></span><span class="pname">${esc(p.name)}</span><span class="pmeta">${esc(effectName(p.effect))}</span></button>`).join("")
    : `<p class="hint">No presets yet. Save one from the Lighting tab.</p>`);
}

function updateFans(el) {
  const F = S.state.fans;
  el.classList.toggle("missing", !F.hub_online);
  countTo(el.querySelector(".num"), F.duty == null ? "–" : String(F.duty));
  const rpms = F.ports.map((p) => p.rpm).filter((r) => r > 0);
  setHTML(el.querySelector(".fan-sub"), F.hub_online
    ? (rpms.length ? `${Math.round(rpms.reduce((a, b) => a + b, 0) / rpms.length).toLocaleString()} rpm across ${rpms.length} fan${rpms.length === 1 ? "" : "s"}` : "Fans stopped")
    : "No NZXT hub connected");
  setHTML(el.querySelector(".fan-curve"), F.curve !== F.base_curve ? `${esc(curveLabel(F.curve))}, by automation` : esc(curveLabel(F.curve)));
  setHTML(el.querySelector(".fan-seg"), Object.keys(S.curves?.curves || {}).map((c) =>
    `<button role="radio" data-curve="${c}" aria-checked="${c === F.base_curve}">${curveLabel(c)}</button>`).join(""));
}

function update(el, t) {
  if (t.type === "sensor") updateSensor(el, t);
  else if (t.type === "lighting") updateLighting(el);
  else if (t.type === "presets") updatePresets(el);
  else if (t.type === "fans") updateFans(el);
}

// ------------------------------------------------------------ render
export function renderOverview() {
  if (!O.layout || !S.state) return;
  const g = grid();
  const wanted = O.layout.tiles.map((t) => mount(t));
  // drop removed tiles, keep the order of the layout
  for (const [id, el] of O.els) if (!tileById(id)) { el.remove(); O.els.delete(id); }
  if (!O.drag) wanted.forEach((el, i) => { if (g.children[i] !== el) g.insertBefore(el, g.children[i] || null); });
  let add = $("#ov-add");
  if (O.editing && !add) {
    add = document.createElement("button");
    add.id = "ov-add"; add.className = "tile tile-add";
    add.innerHTML = `<svg aria-hidden="true"><use href="#i-plus"/></svg><span>Add a tile</span>`;
    add.addEventListener("click", addTileSheet);
  }
  if (O.editing) g.append(add); else add?.remove();
  for (const t of O.layout.tiles) update(O.els.get(t.id), t);
  g.classList.toggle("editing", O.editing);
  $$(".tile", g).forEach((el) => { el.tabIndex = O.editing && !el.classList.contains("tile-add") ? 0 : -1; });
}

// Live preview frames arrive ~15 times a second while the Overview is open.
function paintLightingTiles() {
  if (!visible()) return;
  for (const t of O.layout?.tiles || []) {
    if (t.type !== "lighting") continue;
    const c = O.els.get(t.id)?.querySelector("canvas");
    if (c) paintMini(c);
  }
}

// ------------------------------------------------------------ editing: move, resize, keyboard
function setEditing(on) {
  O.editing = on;
  $("#ov-edit").setAttribute("aria-pressed", String(on));
  $("#ov-edit").innerHTML = on ? "Done" : `<svg aria-hidden="true"><use href="#i-move"/></svg>Customize`;
  $("#ov-editbar").hidden = !on;
  flip(grid(), () => renderOverview());
}

function cell() {
  const w = grid().clientWidth;
  return { cw: (w - GAP * (COLS - 1)) / COLS, rh: ROW_H };
}

function onPointerDown(e) {
  if (!O.editing || e.button !== 0) return;
  const el = e.target.closest(".tile:not(.tile-add)");
  if (!el || e.target.closest("button:not(.pcard), input, a.link")) return;
  const t = tileById(el.dataset.id);
  const r = el.getBoundingClientRect();
  e.preventDefault();
  if (e.target.closest('[data-act="resize"]')) {
    O.drag = { mode: "resize", el, t, x0: e.clientX, y0: e.clientY, w0: t.w, h0: t.h };
  } else {
    O.drag = { mode: "move", el, t, ox: e.clientX - r.left, oy: e.clientY - r.top, x0: e.clientX, y0: e.clientY, started: false, natural: r };
  }
  grid().setPointerCapture(e.pointerId);
}

function naturalRect(el) {
  const tr = el.style.transform;
  el.style.transform = "none";
  const r = el.getBoundingClientRect();
  el.style.transform = tr;
  return r;
}

function onPointerMove(e) {
  const d = O.drag;
  if (!d) return;
  if (d.mode === "resize") {
    const { cw, rh } = cell();
    const w = clamp(Math.round(d.w0 + (e.clientX - d.x0) / (cw + GAP)), 1, COLS);
    const h = clamp(Math.round(d.h0 + (e.clientY - d.y0) / (rh + GAP)), 1, 3);
    if (w !== d.t.w || h !== d.t.h) {
      flip(grid(), () => { d.t.w = w; d.t.h = h; sizeTile(d.el, d.t); });
      update(d.el, d.t);
      d.changed = true;
    }
    return;
  }
  if (!d.started) {
    if (Math.hypot(e.clientX - d.x0, e.clientY - d.y0) < 5) return;
    d.started = true;
    d.el.classList.add("dragging");
  }
  d.el.style.transform = `translate(${e.clientX - d.ox - d.natural.left}px, ${e.clientY - d.oy - d.natural.top}px)`;
  // which tile is under the pointer?
  const over = $$(".tile:not(.tile-add)", grid()).find((x) => {
    if (x === d.el) return false;
    const r = x.getBoundingClientRect();
    return e.clientX > r.left && e.clientX < r.right && e.clientY > r.top && e.clientY < r.bottom;
  });
  if (!over || over === d.lastOver) { if (!over) d.lastOver = null; return; }
  d.lastOver = over;
  const tiles = O.layout.tiles;
  const from = tiles.indexOf(d.t), to = tiles.findIndex((x) => x.id === over.dataset.id);
  flip(grid(), () => {
    tiles.splice(from, 1);
    tiles.splice(to, 0, d.t);
    grid().insertBefore(d.el, from < to ? over.nextSibling : over);
  }, { skip: d.el, duration: 300 });
  d.natural = naturalRect(d.el);
  d.el.style.transform = `translate(${e.clientX - d.ox - d.natural.left}px, ${e.clientY - d.oy - d.natural.top}px)`;
  d.changed = true;
}

function onPointerUp() {
  const d = O.drag;
  O.drag = null;
  if (!d) return;
  if (d.mode === "move" && d.started) {
    const from = d.el.style.transform;
    d.el.style.transform = "";
    d.el.classList.remove("dragging");
    if (!reduced()) d.el.animate([{ transform: from }, { transform: "none" }], { duration: 320, easing: SPRING });
  }
  if (d.changed) save();
  if (d.started || d.changed) O.justDragged = performance.now();
}

function onKey(e) {
  if (!O.editing) return;
  const el = e.target.closest?.(".tile:not(.tile-add)");
  if (!el || e.target !== el) return;
  const t = tileById(el.dataset.id), tiles = O.layout.tiles, i = tiles.indexOf(t);
  let changed = true;
  if (e.shiftKey && e.key === "ArrowRight") t.w = Math.min(COLS, t.w + 1);
  else if (e.shiftKey && e.key === "ArrowLeft") t.w = Math.max(1, t.w - 1);
  else if (e.shiftKey && e.key === "ArrowDown") t.h = Math.min(3, t.h + 1);
  else if (e.shiftKey && e.key === "ArrowUp") t.h = Math.max(1, t.h - 1);
  else if ((e.key === "ArrowRight" || e.key === "ArrowDown") && i < tiles.length - 1) { tiles.splice(i, 1); tiles.splice(i + 1, 0, t); }
  else if ((e.key === "ArrowLeft" || e.key === "ArrowUp") && i > 0) { tiles.splice(i, 1); tiles.splice(i - 1, 0, t); }
  else if (e.key === "Delete" || e.key === "Backspace") { removeTile(t.id); e.preventDefault(); return; }
  else if (e.key === "Enter" && t.type === "sensor") { tileSheet(t.id); e.preventDefault(); return; }
  else changed = false;
  if (!changed) return;
  e.preventDefault();
  flip(grid(), () => renderOverview());
  el.focus();
  save();
}

function removeTile(id) {
  const t = tileById(id);
  flip(grid(), () => { O.layout.tiles = O.layout.tiles.filter((x) => x.id !== id); renderOverview(); });
  save();
  toast(`${t.label || (t.type === "sensor" ? sensorName(t.sensors[0]) : CONTROL[t.type].name)} removed`);
}

async function addTile(tile) {
  if (tile.type === "sensor") {
    await ensureHistory(tile.sensors).catch(() => {});
  }
  flip(grid(), () => { O.layout.tiles.push(tile); renderOverview(); });
  save();
  const el = O.els.get(tile.id);
  el?.scrollIntoView({ block: "nearest", behavior: reduced() ? "auto" : "smooth" });
  el?.classList.add("just-added");
  setTimeout(() => el?.classList.remove("just-added"), 1200);
}

// ------------------------------------------------------------ sheets: picker, add, tile settings
function pickerHTML(selected) {
  return `<input class="text search" id="pk-q" type="search" placeholder="Search readings, for example memory or fan" autocomplete="off" spellcheck="false">
    <div class="picker" id="pk-list">${pickerRows(selected, "")}</div>`;
}

// Everyday words people search with, mapped onto the words sensors use.
const ALIASES = { memory: "ram", graphics: "gpu vram video", processor: "cpu", drive: "ssd disk nvme storage", gpu: "graphics",
  temperature: "temp heat", throughput: "network speed", fan: "rpm cooling", download: "network internet", upload: "network internet",
  power: "watts wattage", load: "usage utilization", clock: "frequency speed", data: "space usage" };

function pickerRows(selected, q) {
  const words = q.toLowerCase().split(/\s+/).filter(Boolean);
  const groups = new Map();
  for (const m of Object.values(R.meta)) {
    let hay = `${m.name} ${m.group} ${m.source || ""} ${m.kind} ${m.hw || ""}`.toLowerCase();
    hay += " " + Object.entries(ALIASES).filter(([k]) => hay.includes(k)).map(([, v]) => v).join(" ");
    const hayWords = hay.split(/[^a-z0-9#+]+/);
    if (words.some((w) => !hayWords.some((h) => h.startsWith(w)))) continue;
    if (m.group !== "Quick picks" && m.value == null) continue;
    if (!groups.has(m.group)) groups.set(m.group, []);
    groups.get(m.group).push(m);
  }
  if (!groups.size) return `<p class="hint" style="padding:18px 4px">Nothing matches “${esc(q)}”.</p>`;
  let html = "";
  for (const [g, list] of groups) {
    const nice = g === "Quick picks" ? "Common" : g === "Windows" ? "From Windows (works without LibreHardwareMonitor)" : g;
    html += `<section><h3>${esc(nice)}</h3>${list.map((m) => `<button type="button" class="pick" data-pick="${esc(m.id)}" aria-pressed="${m.id === selected}"${m.value == null ? ' data-empty="1"' : ""}>
      <span class="pk-name">${esc(m.name)}${m.group === "Quick picks" && m.source ? `<small>${esc(source(m))}</small>` : ""}</span>
      <span class="pk-val">${m.value == null ? "not on this PC" : esc(fmtText(m, m.value))}</span></button>`).join("")}</section>`;
  }
  return html;
}

function wirePicker(onPick, selected) {
  const q = $("#pk-q");
  q.addEventListener("input", () => { $("#pk-list").innerHTML = pickerRows(selected, q.value.trim()); });
  $("#pk-list").addEventListener("click", (e) => {
    const b = e.target.closest("[data-pick]");
    if (b) onPick(b.dataset.pick);
  });
  setTimeout(() => q.focus(), 30);
}

async function addTileSheet() {
  await loadCatalog().catch(() => {});
  const has = new Set(O.layout.tiles.map((t) => t.type));
  const controls = Object.entries(CONTROL).filter(([k]) => !has.has(k));
  openSheet(`<div class="body wide"><h2>Add a tile</h2>
    ${controls.length ? `<div class="ctl-cards">${controls.map(([k, c]) => `<button type="button" class="ctl-card" data-ctl="${k}"><strong>${c.name}</strong><span>${c.desc}</span></button>`).join("")}</div>
      <h3 class="sub-head">Or a reading</h3>` : ""}
    ${pickerHTML(null)}
    <div class="actions"><span class="spacer"></span><button type="button" class="btn quiet" data-close>Cancel</button></div></div>`);
  $$("[data-ctl]").forEach((b) => b.addEventListener("click", () => {
    const k = b.dataset.ctl;
    closeSheet();
    addTile({ id: newId(k), type: k, w: CONTROL[k].w, h: CONTROL[k].h });
  }));
  wirePicker((sid) => {
    closeSheet();
    const m = R.meta[sid];
    addTile({ id: newId(m?.short || sid.split("/").pop()), type: "sensor", sensors: [sid], style: defaultStyle(m), w: 1, h: 1 });
  }, null);
}

const STYLE_ICONS = {
  spark: `<svg viewBox="0 0 40 28"><text x="2" y="15" class="si-num">42</text><path d="M2 24 L10 21 L17 23 L24 17 L31 19 L38 13" class="si-line"/></svg>`,
  ring: `<svg viewBox="0 0 40 28"><path d="M11 24 A11 11 0 1 1 29 24" class="si-track"/><path d="M11 24 A11 11 0 0 1 26 5.6" class="si-arc"/></svg>`,
  graph: `<svg viewBox="0 0 40 28"><path d="M2 5H38M2 14H38M2 23H38" class="si-grid"/><path d="M2 20 L8 16 L14 18 L20 10 L26 13 L32 6 L38 9" class="si-line"/></svg>`,
  bar: `<svg viewBox="0 0 40 28"><rect x="2" y="12" width="36" height="5" rx="2.5" class="si-track-f"/><rect x="2" y="12" width="22" height="5" rx="2.5" class="si-fill"/></svg>`,
};

function tileSheet(id, mode = "main", slot = 0) {
  const t = tileById(id);
  if (!t) return;
  if (mode === "pick") {
    openSheet(`<div class="body wide"><h2>${slot ? "Second reading" : "Choose a reading"}</h2>${pickerHTML(t.sensors[slot])}
      <div class="actions"><button type="button" class="btn quiet" id="pk-back">Back</button><span class="spacer"></span></div></div>`);
    $("#pk-back").addEventListener("click", () => tileSheet(id));
    wirePicker(async (sid) => {
      t.sensors[slot] = sid;
      await ensureHistory([sid]).catch(() => {});
      renderOverview(); save(); tileSheet(id);
    }, t.sensors[slot]);
    return;
  }
  const pins = O.layout.pins;
  const m0 = R.meta[t.sensors[0]], m1 = t.sensors[1] ? R.meta[t.sensors[1]] : null;
  const pinned = pins.includes(t.sensors[0]);
  openSheet(`<form class="body" id="ts-form"><h2>Tile settings</h2>
    <div class="ts-row"><span class="ts-lab">Reading</span><div class="ts-val"><b>${esc(m0?.name || sensorName(t.sensors[0]))}</b><small>${esc(source(m0) || m0?.group || "")}</small></div><button type="button" class="btn" data-pick-slot="0">Change</button></div>
    ${t.style === "graph" ? `<div class="ts-row"><span class="ts-lab">Compare with</span>${m1 || t.sensors[1]
      ? `<div class="ts-val"><b>${esc(m1?.name || sensorName(t.sensors[1]))}</b><small>${esc(source(m1) || m1?.group || "")}</small></div><div class="btn-row"><button type="button" class="btn" data-pick-slot="1">Change</button><button type="button" class="icon-btn" id="ts-drop2" title="Remove the second reading"><svg><use href="#i-close"/></svg></button></div>`
      : `<div class="ts-val"><span class="hint">Nothing. Graphs can show two readings, for example processor and graphics power.</span></div><button type="button" class="btn" data-pick-slot="1">Add</button>`}</div>` : ""}
    <div class="field"><span class="lab">Style</span><div class="styles" role="radiogroup">${STYLES.map((s) => `<button type="button" role="radio" class="style-btn" data-style="${s.id}" aria-checked="${t.style === s.id}">${STYLE_ICONS[s.id]}<span>${s.name}</span></button>`).join("")}</div></div>
    <label class="field"><span class="lab">Name</span><input class="text" id="ts-label" maxlength="40" value="${esc(t.label || "")}" placeholder="${esc(m0?.name || "")}" autocomplete="off"></label>
    <label class="ts-pin"><span class="switch"><input type="checkbox" id="ts-pin"${pinned ? " checked" : ""}${!pinned && pins.length >= 4 ? " disabled" : ""}><span></span></span>
      <span><b>Show in the top bar and tray</b><small>${!pinned && pins.length >= 4 ? "Up to four readings can be pinned. Unpin one first." : "Keeps this reading in view on every tab, and in the tray icon's tooltip."}</small></span></label>
    <div class="actions"><button type="button" class="btn danger" id="ts-remove">Remove tile</button><span class="spacer"></span><button class="btn primary">Done</button></div></form>`);
  const live = () => { renderOverview(); save(); };
  $$("[data-pick-slot]").forEach((b) => b.addEventListener("click", () => tileSheet(id, "pick", +b.dataset.pickSlot)));
  $("#ts-drop2")?.addEventListener("click", () => { t.sensors = t.sensors.slice(0, 1); live(); tileSheet(id); });
  $$("[data-style]").forEach((b) => b.addEventListener("click", () => {
    t.style = b.dataset.style;
    if (t.style !== "graph") t.sensors = t.sensors.slice(0, 1);
    if (t.style === "graph" && t.w < 2) t.w = 2;
    flip(grid(), live);
    tileSheet(id);
  }));
  $("#ts-label").addEventListener("input", (e) => { const v = e.target.value.trim(); if (v) t.label = v; else delete t.label; live(); });
  $("#ts-pin").addEventListener("change", (e) => {
    O.layout.pins = e.target.checked ? [...pins, t.sensors[0]].slice(0, 4) : pins.filter((p) => p !== t.sensors[0]);
    live();
  });
  $("#ts-remove").addEventListener("click", () => { closeSheet(); removeTile(id); });
  $("#ts-form").addEventListener("submit", (e) => { e.preventDefault(); closeSheet(); });
}

// ------------------------------------------------------------ init
export async function initOverview(layout) {
  O.layout = layout;
  S.overview = layout;
  const g = grid();
  g.addEventListener("pointerdown", onPointerDown);
  g.addEventListener("pointermove", onPointerMove);
  g.addEventListener("pointerup", onPointerUp);
  g.addEventListener("pointercancel", onPointerUp);
  g.addEventListener("keydown", onKey);
  g.addEventListener("click", async (e) => {
    const act = e.target.closest("[data-act]")?.dataset.act;
    const tile = e.target.closest(".tile")?.dataset.id;
    if (act === "remove") return removeTile(tile);
    if (act === "edit") return tileSheet(tile);
    if (O.editing) {
      if (performance.now() - (O.justDragged || 0) < 300) return;
      if (e.target.closest(".tile-sensor") && !e.target.closest("button")) tileSheet(tile);
      if (e.target.closest("a")) e.preventDefault();
      return;
    }
    const p = e.target.closest("[data-preset]");
    if (p) {
      const preset = S.presets.find((x) => x.id === p.dataset.preset);
      try {
        const r = await post(`/api/presets/${encodeURIComponent(preset.id)}/apply`);
        const failed = r.steps.filter((s) => !s.ok);
        if (failed.length) toast(`${preset.name} is on, but ${failed[0].step} didn't apply: ${failed[0].error}`, true);
        else toast(`${preset.name} is on`);
        emit("refresh");
      } catch (err) { fail(err); }
    }
    const c = e.target.closest("[data-curve]");
    if (c) {
      try { await post("/api/fan_curve", { name: c.dataset.curve }); toast(`Fans now follow the ${curveLabel(c.dataset.curve)} curve`); emit("refresh"); }
      catch (err) { fail(err); }
    }
  });
  g.addEventListener("dblclick", (e) => {
    const el = e.target.closest(".tile-sensor");
    if (el && !O.editing) tileSheet(el.dataset.id);
  });
  $("#ov-edit").addEventListener("click", () => setEditing(!O.editing));
  $("#ov-reset").addEventListener("click", async () => {
    if (!confirm("Put the Overview back to how it was at first? Your tiles and pins will be replaced.")) return;
    try {
      const l = await post("/api/overview/reset");
      O.layout = l; S.overview = l;
      await ensureHistory([...new Set(l.tiles.flatMap((t) => t.sensors || []))]).catch(() => {});
      flip(grid(), () => renderOverview());
      emit("pins");
    } catch (err) { fail(err); }
  });
  document.addEventListener("keydown", (e) => { if (e.key === "Escape" && O.editing && !$("#sheet").open) setEditing(false); });
  on("state", () => { if (visible()) renderOverview(); });
  await ensureHistory([...new Set(layout.tiles.flatMap((t) => t.sensors || []))]).catch(() => {});
  renderOverview();
}

export function onFrames() { paintLightingTiles(); }
export const overviewLayout = () => O.layout;
