// Overview: a grid of tiles you arrange yourself. Sensor tiles (number + graph, gauge, big graph, bar) and control
// tiles (lighting, presets, fans). Tiles are built once and updated in place, so readings change without the tile
// flashing, and dragging never fights the 2-second refresh.

import { $, $$, S, clamp, curveLabel, effectName, emit, esc, fail, on, post, rangeFill, sceneColor, sceneSwatch, setHTML, toast } from "./core.js";
import { R, ensureHistory, fmt, fmtText, level, loadCatalog, range, resample, sensorName, thresholds, value } from "./readings.js";
import { EASE, SPRING, countTo, flip, reduced } from "./motion.js";
import { createOverviewRig } from "./overview-rig.js";
import { profileArtwork, fanInstrument } from "./overview-instruments.js";
import { sensorFamily, instrumentModel } from "./overview-sensors.js";
import { closeSheet, openSheet } from "./sheet.js";
import { createLayoutHistory, createWorkspace, deleteWorkspace, exportWorkspace, importWorkspace, initializeWorkspaces, renameWorkspace, restoreOverview, starterWorkspace, switchWorkspace, syncWorkspace } from "./overview-workspaces.js";

const COLS = 4;
const STYLES = [
  { id: "auto", name: "Instrument" },
  { id: "spark", name: "Number" }, { id: "ring", name: "Gauge" }, { id: "graph", name: "Graph" }, { id: "bar", name: "Bar" },
];
const CONTROL = {
  lighting: { name: "Lighting", desc: "A live miniature of your rig, with on/off and brightness.", w: 2, h: 2 },
  presets: { name: "Profiles", desc: "One click to switch lighting and fans together.", w: 2, h: 1 },
  fans: { name: "Case fans", desc: "Fan speed and the curve they follow.", w: 1, h: 1 },
  control: { name: "Control status", desc: "Completed work, connection health and recent control changes.", w: 2, h: 1 },
};

const O = { layout: null, editing: false, els: new Map(), drag: null, history: null, saveState: "saved" };
let overviewFrames = {};
const grid = () => $("#ov-grid");
const visible = () => !$('[data-view="overview"]').hidden;
let measureRev = 0;                    // tile text is measured for overflow only when this changes (resize, font)
window.addEventListener("resize", () => { measureRev++; });
document.fonts?.ready.then(() => { measureRev++; if (visible()) renderOverview(); });

// ------------------------------------------------------------ persistence
let saveTimer, saveRevision = 0, saving = null, resetting = false, savedLayout = null;
function save(group = null, record = true) {
  if (resetting) return;
  syncWorkspace(O.layout);
  if (record) O.history?.record(O.layout, group);
  O.saveState = "saving";
  renderWorkspaceToolbar();
  saveRevision++;
  clearTimeout(saveTimer);
  saveTimer = setTimeout(flushOverview, 350);
}

function flushOverview() {
  saveTimer = null;
  if (saving || resetting) return saving;      // the in-flight save will pick up edits when it finishes
  const revision = saveRevision;
  saving = (async () => {
    try {
      const next = await post("/api/overview", structuredClone(O.layout));
      savedLayout = JSON.stringify(next);
      if (revision !== saveRevision) return;      // this response predates edits still waiting to be saved
      // Keep tile references held by an open sheet, a drag or an undo toast intact.
      restoreOverview(O.layout, next);
      S.overview = O.layout;
      O.saveState = "saved";
      renderWorkspaceToolbar();
      emit("pins");
    } catch (e) {
      if (revision === saveRevision) { O.saveState = "unsaved"; renderWorkspaceToolbar(); }
      fail(e);
    }
    finally {
      saving = null;
      if (!resetting && revision !== saveRevision && !saveTimer) saveTimer = setTimeout(flushOverview, 0);
    }
  })();
  return saving;
}

async function resetOverview() {
  if (resetting) return null;
  resetting = true;
  renderWorkspaceToolbar();
  saveRevision++;
  clearTimeout(saveTimer);
  saveTimer = null;
  try {
    await saving;                        // finish the older write before issuing the reset at the server
    // A new/renamed workspace can still be inside the debounce window. Persist that collection
    // before resetting its current layout, or the server would reset an older active workspace.
    if (O.layout.workspaces?.length && JSON.stringify(O.layout) !== savedLayout) {
      syncWorkspace(O.layout);
      const retained = await post("/api/overview", structuredClone(O.layout));
      savedLayout = JSON.stringify(retained);
    }
    const layout = await post("/api/overview/reset");
    savedLayout = JSON.stringify(layout);
    O.layout = layout; S.overview = layout;
    O.history?.record(layout);
    O.saveState = "saved";
    return layout;
  } catch (error) {
    resetting = false;
    save();                             // a failed reset must not lose an unsaved edit
    throw error;
  } finally { resetting = false; renderWorkspaceToolbar(); }
}

// ------------------------------------------------------------ personal workspaces
function renderWorkspaceToolbar() {
  if (!O.history) return;
  const toolbar = $("#ov-workspaces");
  if (!toolbar) return;
  const select = $("#ov-workspace");
  const options = `${O.layout.active_workspace ? "" : '<option value="">Current layout</option>'}${O.layout.workspaces.map((w) => `<option value="${esc(w.id)}">${esc(w.name)}</option>`).join("")}`;
  setHTML(select, options);
  select.value = O.layout.active_workspace || "";
  select.disabled = resetting;
  $("#ov-workspace-manage").disabled = resetting;
  $("#ov-undo").disabled = resetting || !O.history.canUndo;
  $("#ov-redo").disabled = resetting || !O.history.canRedo;
  const status = $("#ov-workspace-status");
  status.textContent = { saving: "Saving…", saved: "Saved", unsaved: "Not saved" }[O.saveState];
  status.dataset.state = O.saveState;
}

function changedWorkspace() {
  S.overview = O.layout;
  save();
  renderOverview();
  paintLightingTiles();
  emit("pins");
  ensureHistory([...new Set(O.layout.tiles.flatMap((t) => t.sensors || []))]).catch(() => {});
}

function workspaceChange(change) {
  if (resetting || O.drag) return;
  try { change(); closeSheet(); changedWorkspace(); }
  catch (error) { fail(error); }
}

function historyStep(direction) {
  if (resetting || O.drag) return;
  const next = O.history?.[direction]();
  if (!next) return;
  closeSheet();
  restoreOverview(O.layout, next);
  S.overview = O.layout;
  save(null, false);
  renderOverview();
  paintLightingTiles();
  emit("pins");
  ensureHistory([...new Set(O.layout.tiles.flatMap((t) => t.sensors || []))]).catch(() => {});
}

function workspaceNameSheet(rename = false) {
  const active = O.layout.workspaces.find((w) => w.id === O.layout.active_workspace);
  openSheet(`<form class="body" id="ov-workspace-form"><h2>${rename ? "Rename workspace" : "New workspace"}</h2>
    <label class="field"><span class="lab">Name</span><input class="text" id="ov-workspace-name" maxlength="40" required autocomplete="off" value="${esc(rename ? active?.name || "" : "")}" placeholder="Evening, focus, play…"></label>
    ${rename ? "" : `<label class="field"><span class="lab">Start with</span><select class="select" id="ov-workspace-starter"><option value="current">A copy of this layout</option><option value="showcase">Showcase · lighting canvas</option><option value="monitoring">Monitoring · readings first</option><option value="balanced">Balanced · light and performance</option></select></label>`}
    <p class="hint">Workspaces save your tiles and pinned readings.</p><p class="hint" id="ov-workspace-error" role="alert" hidden></p>
    <div class="actions"><button class="btn quiet" type="button" data-close>Cancel</button><span class="spacer"></span><button class="btn primary">${rename ? "Rename" : "Create workspace"}</button></div></form>`);
  $("#ov-workspace-name").focus();
  $("#ov-workspace-name").select();
  $("#ov-workspace-form").addEventListener("submit", (event) => {
    event.preventDefault();
    if (resetting) return;
    try {
      const name = $("#ov-workspace-name").value;
      if (rename) renameWorkspace(O.layout, active?.id, name);
      else createWorkspace(O.layout, name, starterWorkspace(O.layout, $("#ov-workspace-starter").value));
      closeSheet(); changedWorkspace();
    } catch (error) {
      $("#ov-workspace-error").textContent = error.message;
      $("#ov-workspace-error").hidden = false;
    }
  });
}

function workspaceSheet() {
  const active = O.layout.workspaces.find((w) => w.id === O.layout.active_workspace);
  openSheet(`<div class="body" id="ov-workspace-sheet"><h2>Your workspaces</h2>
    <p class="hint">A different arrangement for each way you use Airglow. Each keeps its own tiles and pinned readings.</p>
    <div class="workspace-list">${O.layout.workspaces.map((w) => `<button class="workspace-card" data-workspace="${esc(w.id)}" aria-pressed="${w.id === active?.id}"><span class="workspace-glyph" aria-hidden="true">${w.tiles.slice(0, 8).map((t) => `<i class="${t.type === "lighting" ? "is-rig" : ""}"></i>`).join("")}</span><span><b>${esc(w.name)}</b><small>${w.tiles.length} tiles · ${w.pins.length} pinned</small></span><span aria-hidden="true">${w.id === active?.id ? "✓" : "→"}</span></button>`).join("")}</div>
    <div class="btn-row workspace-actions"><button class="btn primary" id="ov-workspace-new"${O.layout.workspaces.length >= 8 ? " disabled" : ""}>New workspace</button><button class="btn" id="ov-workspace-rename"${active ? "" : " disabled"}>Rename</button><button class="btn danger" id="ov-workspace-delete"${active && O.layout.workspaces.length > 1 ? "" : " disabled"}>Delete</button></div>
    <div class="actions"><button class="btn quiet" id="ov-workspace-export">Export layout</button><button class="btn quiet" id="ov-workspace-import"${O.layout.workspaces.length >= 8 ? " disabled" : ""}>Import layout</button><span class="spacer"></span><button class="btn" data-close>Done</button></div>
    <input type="file" id="ov-workspace-file" accept=".json,application/json" hidden></div>`);
  $("#ov-workspace-sheet").addEventListener("click", (e) => {
    const id = e.target.closest("[data-workspace]")?.dataset.workspace;
    if (id) workspaceChange(() => switchWorkspace(O.layout, id));
  });
  $("#ov-workspace-new").addEventListener("click", () => workspaceNameSheet());
  $("#ov-workspace-rename").addEventListener("click", () => workspaceNameSheet(true));
  $("#ov-workspace-delete").addEventListener("click", () => {
    workspaceChange(() => deleteWorkspace(O.layout, active?.id));
    toast("Workspace deleted. Layout Undo brings it back.");
  });
  $("#ov-workspace-export").addEventListener("click", () => {
    const data = exportWorkspace(O.layout);
    const url = URL.createObjectURL(new Blob([JSON.stringify(data, null, 2)], { type: "application/json" }));
    const link = document.createElement("a"); link.href = url;
    link.download = `airglow-${O.layout.active_workspace || "workspace"}.json`;
    link.click(); setTimeout(() => URL.revokeObjectURL(url), 1000);
  });
  $("#ov-workspace-import").addEventListener("click", () => $("#ov-workspace-file").click());
  $("#ov-workspace-file").addEventListener("change", async (event) => {
    const file = event.target.files?.[0];
    if (!file) return;
    try {
      if (file.size > 100000) throw new Error("That workspace file is too large.");
      const imported = importWorkspace(await file.text());
      workspaceChange(() => createWorkspace(O.layout, imported.name, imported));
    } catch (error) { fail(error); }
  });
}

function initWorkspaceToolbar() {
  const toolbar = document.createElement("div");
  toolbar.id = "ov-workspaces"; toolbar.className = "ov-workspaces";
  toolbar.innerHTML = `<label class="workspace-picker"><span>Workspace</span><select class="select" id="ov-workspace" aria-label="Overview workspace"></select></label>
    <button class="chip-btn" id="ov-workspace-manage" title="Create, rename or share a workspace" aria-label="Manage workspaces">Workspaces</button>
    <div class="workspace-history" role="group" aria-label="Layout history"><button class="icon-btn" id="ov-undo" aria-label="Undo layout edit" title="Undo layout edit (Ctrl+Z)">↶</button><button class="icon-btn" id="ov-redo" aria-label="Redo layout edit" title="Redo layout edit (Ctrl+Shift+Z)">↷</button></div><span id="ov-workspace-status" class="workspace-status" role="status"></span>`;
  $(".ov-bar").prepend(toolbar);
  $("#ov-workspace").addEventListener("change", (event) => {
    if (event.target.value) workspaceChange(() => switchWorkspace(O.layout, event.target.value));
  });
  $("#ov-workspace-manage").addEventListener("click", workspaceSheet);
  $("#ov-undo").addEventListener("click", () => historyStep("undo"));
  $("#ov-redo").addEventListener("click", () => historyStep("redo"));
  document.addEventListener("keydown", (event) => {
    if (!visible() || $("#sheet").open || !(event.ctrlKey || event.metaKey) || event.altKey || event.target.closest?.("input, textarea, select, [contenteditable]")) return;
    const key = event.key.toLowerCase();
    if (key !== "z" && key !== "y") return;
    event.preventDefault(); historyStep(key === "y" || event.shiftKey ? "redo" : "undo");
  });
  renderWorkspaceToolbar();
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
    <div class="tile-tools"><button class="icon-btn" data-act="edit" title="Tile settings" aria-label="Tile settings"><svg><use href="#i-sliders"/></svg></button>
      <button class="icon-btn" data-act="remove" title="Remove tile" aria-label="Remove tile"><svg><use href="#i-close"/></svg></button></div>
    <span class="tile-resize" data-act="resize" title="Drag to resize" aria-hidden="true"></span>`;
  return el;
}

function sizeTile(el, t) {
  el.style.gridColumn = `span ${t.w}`;
  el.style.gridRow = `span ${t.h}`;
  el.dataset.w = t.w; el.dataset.h = t.h;
  el.dataset.emphasis = t.appearance?.emphasis || "standard";
  if (t.appearance?.accent) el.style.setProperty("--tile-accent", t.appearance.accent);
  else el.style.removeProperty("--tile-accent");
}

// Sensor tile markup per style. Values and paths are filled in by update().
function sensorMarkup(t) {
  const two = t.sensors.length > 1;
  const head = `<header class="tile-head"><span class="tile-name"></span><span class="tile-src"></span></header>`;
  if (t.style === "auto") return `${head}<div class="readout"><span class="num">–</span><span class="unit"></span></div>
    <div class="instrument-meter"><div class="instrument-track"><i></i><b class="threshold-tick warning"></b><b class="threshold-tick critical"></b></div><div class="instrument-scale"><span class="scale-lo"></span><span class="scale-hi"></span></div></div>
    <div class="instrument-history"><div class="history-caption"><span>10 min</span><span class="history-range"></span></div><svg viewBox="0 0 400 100" preserveAspectRatio="none" aria-hidden="true"><path class="area"/><path class="line"/></svg><span class="history-empty">Collecting history</span></div>
    <div class="instrument-detail"></div><div class="instrument-compare"></div>`;
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
        <span class="ax ax-hi"></span><span class="ax ax-mid"></span><span class="ax ax-hi ax-right" hidden></span><span class="ax ax-mid ax-right" hidden></span><span class="ax-time">10 min</span></div>`;
  }
  return `${head}<div class="readout"><span class="num">–</span><span class="unit"></span>${two ? "" : ""}</div>
    <div class="minmax"></div>
    <svg class="spark" viewBox="0 0 300 60" preserveAspectRatio="none" aria-hidden="true"><path class="area"/><path class="line"/></svg>`;
}

function controlMarkup(t) {
  if (t.type === "lighting") {
    return `<div class="canvas-heading"><span>Lighting canvas</span><a class="link" href="#lighting" title="Open Lighting">Open Lighting ↗</a></div><div class="overview-rig-host"></div>
      <footer class="lt-foot"><div class="lt-what"><b class="lt-effect"></b><span class="lt-sub"></span></div>
        <input type="range" class="lt-bri" min="0" max="100" aria-label="Brightness">
        <label class="switch" title="Lights on or off"><input type="checkbox" class="lt-on"><span></span></label></footer>`;
  }
  if (t.type === "presets") {
    return `<header class="tile-head"><span class="tile-name">Profiles</span><a class="tile-src link" href="#lighting">Edit</a></header><div class="chips pcards"></div>`;
  }
  if (t.type === "control") return `<header class="tile-head"><span class="tile-name">Control status</span></header>
    <div class="control-tile-state"></div><p class="control-tile-detail hint"></p><button class="btn quiet" data-control-center>Open control center</button>`;
  return `<header class="tile-head"><span class="tile-name">Case fans</span><span class="tile-src fan-curve"></span></header>
    <div class="readout"><span class="num">–</span><span class="unit">%</span></div><div class="fan-sub"></div><div class="fan-instrument"></div>
    <div class="seg fan-seg" role="radiogroup" aria-label="Fan curve"></div>`;
}

function mount(t) {
  let el = O.els.get(t.id);
  if (el && el._type !== t.type) { el._rig?.dispose(); el.remove(); O.els.delete(t.id); el = null; }
  const sig = t.type === "sensor" ? `${t.style}|${t.sensors.length}` : t.type;
  if (!el) { el = shell(t); el._type = t.type; O.els.set(t.id, el); }
  if (el._sig !== sig) {
    el._rig?.dispose(); el._rig = null;
    el._sig = sig;
    el.className = `tile tile-${t.type}${t.type === "sensor" ? ` s-${t.style}` : ""}`;
    el.querySelector(".tile-inner").innerHTML = t.type === "sensor" ? sensorMarkup(t) : controlMarkup(t);
    el._paths = null;
    if (t.type === "lighting") {
      wireLighting(el);
      el._rig = createOverviewRig(el.querySelector(".overview-rig-host"), {
        isVisible: () => visible() && !document.hidden && el.isConnected,
        onSelect: (key) => { if (!O.editing && key) location.hash = `#lighting/${encodeURIComponent(key)}`; },
        onViewChange: (view) => { const tile = tileById(t.id); if (!tile || O.editing) return; tile.view = view; save(); updateLighting(el, tile); },
      });
    }
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

// Close each area separately so outages remain visible in both the line and fill.
function seriesPath(vals, lo, hi, w, h, pad = 2) {
  const n = vals.length;
  let d = "", area = "", segment = "", first = null, last = null;
  const finish = () => {
    if (segment) area += `${segment}L${last.toFixed(1)} ${h}L${first.toFixed(1)} ${h}Z`;
    segment = ""; first = last = null;
  };
  vals.forEach((v, i) => {
    if (!Number.isFinite(v)) { finish(); return; }
    const x = (i / (n - 1)) * w, y = h - pad - ((clamp(v, lo, hi) - lo) / (hi - lo || 1)) * (h - pad * 2);
    const point = `${segment ? "L" : "M"}${x.toFixed(1)} ${y.toFixed(1)}`;
    d += point; segment += point;
    if (first == null) first = x;
    last = x;
  });
  finish();
  return { d, area };
}

function setPath(el, d) {
  if (!el) return;
  if (el._d === d) return;
  el._d = d;
  el.style.d = `path("${d}")`;
}

// ------------------------------------------------------------ updating
function unavailable(id) {
  if (R.stale) return "Waiting for Airglow to reconnect";
  const meta = R.meta[id];
  const needsLhm = meta?.needs_lhm ?? !/^(sys\/|app\/|auto\/(ram_load|ram_used|net_|disk_))/.test(id);
  if (needsLhm && !S.state?.sensors?.online) return "Needs LibreHardwareMonitor";
  if (!meta) return "Not found on this PC";
  return "No reading on this PC";
}

function source(meta) {
  if (!meta) return "";
  const s = meta.source || (meta.group !== "Quick picks" && meta.group !== "Windows" ? meta.group : "");
  return (s || "").replace(/^AMD |^Intel\(R\) Core\(TM\) |^NVIDIA GeForce |^NVIDIA |\(TM\)|\(R\)| \d+-Core Processor$/g, "").trim();
}

function updateSensor(el, t) {
  const id = t.sensors[0], meta = R.meta[id], v = value(id);
  el.dataset.family = sensorFamily(meta, id);
  const name = t.label || meta?.name || sensorName(id);
  setHTML(el.querySelector(".tile-name"), esc(name));
  const src = el.querySelector(".tile-src"), srcText = t.style === "graph" ? "" : source(meta);
  setHTML(src, esc(srcText));
  // the title wins: the source is shortened first, and when too little of it is left it moves to the title's tooltip.
  // Measuring (scrollWidth) makes the browser lay the grid out again, so only do it when the words changed or the
  // window was resized, not on every two-second update of every tile.
  const nameEl = el.querySelector(".tile-name");
  const fitKey = `${measureRev}|${name}|${srcText}`;
  if (nameEl._fitKey !== fitKey) {
    nameEl._fitKey = fitKey;
    src.hidden = false;
    src.hidden = !!srcText && (nameEl.scrollWidth > nameEl.clientWidth || src.clientWidth < 64);
    nameEl.title = [name, srcText, meta?.sensor_name].filter(Boolean).join(" · ");
    src.title = !src.hidden && src.scrollWidth > src.clientWidth ? srcText : "";
  }
  const missing = t.sensors.every((sid) => value(sid) == null);
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

  if (t.style === "auto") {
    updateInstrument(el, t, meta, v, lo, hi);
  } else if (t.style === "spark") {
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
    const samples = (R.series[id] || []).map((p) => p[1]).filter(Number.isFinite);
    setHTML(el.querySelector(".minmax"), samples.length > 3 ? `<span>${esc(fmtText(meta, Math.min(...samples)))}–${esc(fmtText(meta, Math.max(...samples)))}</span> over 10 min` : "");
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
    const vals = (R.series[id] || []).map((p) => p[1]).filter(Number.isFinite);
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
      const pts = resample(R.series[sid]);
      const p = pts.filter((x) => x != null).length ? seriesPath(pts, l, h, 400, 100, 3) : { d: "", area: "" };
      setPath(el.querySelector(`.l-${i}`), p.d);
      setPath(el.querySelector(`.a-${i}`), p.area);
    });
    setHTML(el.querySelector(".ax-hi"), esc(fmtText(meta, ghi)));
    setHTML(el.querySelector(".ax-mid"), esc(fmtText(meta, (glo + ghi) / 2)));
    const differentUnits = metas.length > 1 && metas[1]?.unit !== meta?.unit;
    el.querySelector(".chart").classList.toggle("mixed-units", differentUnits);
    const [rlo, rhi] = range(t.sensors[1], metas[1]);
    for (const [selector, v] of [[".ax-hi.ax-right", rhi], [".ax-mid.ax-right", (rlo + rhi) / 2]]) {
      const axis = el.querySelector(selector); axis.hidden = !differentUnits;
      setHTML(axis, differentUnits ? esc(fmtText(metas[1], v)) : "");
    }
    if (!t.label) setHTML(el.querySelector(".tile-name"), esc(t.sensors.length > 1 ? `${metas[0]?.short || ""} and ${metas[1]?.short || ""}`.trim() : name));
  }
}

function updateInstrument(el, t, meta, v, lo, hi) {
  const id = t.sensors[0], f = fmt(meta, v), limits = thresholds(id, meta);
  const pts = resample(R.series[id]), samples = (R.series[id] || []).map(p => p[1]);
  const model = instrumentModel({value: v, lo, hi, samples, thresholds: limits});
  countTo(el.querySelector(".readout .num"), f.num);
  setHTML(el.querySelector(".readout .unit"), esc(f.unit));
  el.querySelector(".instrument-track i").style.width = `${model.fraction * 100}%`;
  for (const name of ["warning", "critical"]) {
    const tick = el.querySelector(`.threshold-tick.${name}`);
    tick.hidden = model[name] == null;
    if (model[name] != null) tick.style.left = `${model[name] * 100}%`;
  }
  setHTML(el.querySelector(".scale-lo"), esc(fmtText(meta, lo)));
  setHTML(el.querySelector(".scale-hi"), esc(fmtText(meta, hi)));
  const p = seriesPath(pts, lo, hi, 400, 100, 3);
  setPath(el.querySelector(".instrument-history .line"), p.d);
  setPath(el.querySelector(".instrument-history .area"), p.area);
  el.querySelector(".history-empty").hidden = pts.some(Number.isFinite);
  setHTML(el.querySelector(".history-range"), esc(`${fmtText(meta, lo)} – ${fmtText(meta, hi)}`));
  const stats = model.stats;
  setHTML(el.querySelector(".instrument-detail"), `${stats ? `<dl><div><dt>Low · 10 min</dt><dd>${esc(fmtText(meta, stats.min))}</dd></div><div><dt>Average</dt><dd>${esc(fmtText(meta, stats.mean))}</dd></div><div><dt>Peak</dt><dd>${esc(fmtText(meta, stats.max))}</dd></div></dl>` : ""}${limits ? `<p class="threshold-caption">Airglow warning ${esc(fmtText(meta, limits[0]))} · high ${esc(fmtText(meta, limits[1]))}</p>` : ""}`);
  const second = t.sensors[1], sm = R.meta[second];
  if (second) {
    const sv = value(second), [sl, sh] = range(second, sm);
    const shared = sm?.unit === meta?.unit;
    const comparison = seriesPath(resample(R.series[second]), shared ? Math.min(lo, sl) : sl, shared ? Math.max(hi, sh) : sh, 400, 80, 3);
    setHTML(el.querySelector(".instrument-compare"), `<div><span>${esc(sm?.name || sensorName(second))}</span><b>${esc(fmtText(sm, sv))}</b></div><svg viewBox="0 0 400 80" preserveAspectRatio="none" aria-hidden="true"><path class="line" d="${comparison.d}"/></svg><small>10 min · ${esc(fmtText(sm, shared ? Math.min(lo, sl) : sl))} – ${esc(fmtText(sm, shared ? Math.max(hi, sh) : sh))}</small>`);
  } else setHTML(el.querySelector(".instrument-compare"), "");
  el.dataset.compare = String(!!second);
}

function updateLighting(el, t) {
  const sc = S.state.scene, eff = S.state.effective;
  const preset = S.presets.find((p) => p.id === S.state.active_preset);
  setHTML(el.querySelector(".lt-effect"), esc(eff.on ? effectName(eff.effect) : "Lights off"));
  const o = S.state.override;
  const sub = o?.locked ? "Off while the PC is locked" : o?.lighting ? "An automation is in control" : preset ? `${preset.name} profile` : "Your own settings";
  setHTML(el.querySelector(".lt-sub"), esc(sub));
  const bri = el.querySelector(".lt-bri"), on = el.querySelector(".lt-on");
  if (document.activeElement !== bri && Date.now() - (bri._t || 0) > 1500) { bri.value = sc.brightness; rangeFill(bri); }
  on.checked = sc.on;
  el.style.setProperty("--scene", sceneColor(eff));
  el._rig?.update({ zones: S.zones?.zones || [], frames: overviewFrames, view: t?.view || "2d" });
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
        <span class="pswatch">${profileArtwork(p)}</span><span class="pname">${esc(p.name)}</span><span class="pmeta">${esc(effectName(p.effect))}${p.id === active ? " · On" : ""}</span></button>`).join("")
    : `<p class="hint">No profiles yet. Save one from the Lighting tab.</p>`);
}

function fanSummary(fans) {
  const managed = [...(fans.ports || []), ...(fans.channels || []).filter((c) => ["curve", "steady"].includes(c.mode) && !c.pump)];
  const duties = managed.map((c) => c.duty).filter(Number.isFinite), rpms = managed.map((c) => c.rpm).filter(Number.isFinite);
  const average = (values) => values.length ? Math.round(values.reduce((a, b) => a + b, 0) / values.length) : null;
  return { duty: average(duties), rpm: average(rpms), reporting: rpms.length, count: managed.length,
    error: managed.some((c) => !!c.error) };
}

function updateFans(el) {
  const F = S.state.fans, reported = fanSummary(F);
  el.classList.toggle("missing", R.stale || reported.duty == null);
  countTo(el.querySelector(".num"), R.stale || reported.duty == null ? "–" : String(reported.duty));
  el.querySelector(".readout").title = "Average reported speed of Airglow-controlled fans";
  setHTML(el.querySelector(".fan-sub"), R.stale ? "Waiting for Airglow to reconnect" : reported.error ? "Fan control needs attention (Fans tab)"
    : reported.rpm != null ? `${reported.rpm.toLocaleString()} rpm average · ${reported.reporting} reporting`
    : reported.count || F.hub_online ? "No RPM reading yet" : "No controlled fans detected (Fans tab)");
  setHTML(el.querySelector(".fan-curve"), F.curve !== F.base_curve ? `${esc(curveLabel(F.curve))}, by automation` : esc(curveLabel(F.curve)));
  setHTML(el.querySelector(".fan-seg"), Object.keys(S.curves?.curves || {}).map((c) =>
    `<button role="radio" data-curve="${c}" aria-checked="${c === F.base_curve}">${curveLabel(c)}</button>`).join(""));
  const curves = {...S.curves, curves: {...S.curves?.curves}};
  for (const p of S.presets) if (p.fan_points) curves.curves[`profile:${p.id}`] = p.fan_points;
  setHTML(el.querySelector(".fan-instrument"), fanInstrument(F, curves, {stale: R.stale, tempUnit: S.state.temp_unit}));
}

function update(el, t) {
  if (t.type === "sensor") updateSensor(el, t);
  else if (t.type === "lighting") updateLighting(el, t);
  else if (t.type === "presets") updatePresets(el);
  else if (t.type === "fans") updateFans(el);
  else if (t.type === "control") {
    const c = S.state.control, counts = c?.counts || {};
    const labels = { healthy: "Working normally", failed: "Output needs attention", stale: "Work is taking longer", disconnected: "A source is disconnected", pending: "Waiting for completed work" };
    el.querySelector(".control-tile-state").textContent = R.stale ? "Waiting for Airglow" : labels[c?.state] || "Waiting for status";
    el.querySelector(".control-tile-detail").textContent = R.stale ? "Connection lost. Status will update after reconnecting." : c
      ? `${counts.healthy || 0} healthy · ${counts.pending || 0} pending · ${(counts.failed || 0) + (counts.stale || 0) + (counts.disconnected || 0)} need attention`
      : "Status will appear when the next reading arrives.";
  }
}

// ------------------------------------------------------------ render
export function renderOverview() {
  if (!O.layout || !S.state) return;
  const g = grid();
  const wanted = O.layout.tiles.map((t) => mount(t));
  // drop removed tiles, keep the order of the layout
  for (const [id, el] of O.els) if (!tileById(id)) { el._rig?.dispose(); el.remove(); O.els.delete(id); }
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
  $$(".tile", g).forEach((el) => { el.tabIndex = O.editing ? 0 : -1; });
}

// Live preview frames arrive ~15 times a second while the Overview is open.
function paintLightingTiles() {
  if (!visible() || document.hidden) return;
  for (const t of O.layout?.tiles || []) {
    if (t.type !== "lighting") continue;
    O.els.get(t.id)?._rig?.update({zones: S.zones?.zones || [], frames: overviewFrames, view: t.view || "2d"});
  }
}

// ------------------------------------------------------------ editing: move, resize, keyboard
function setEditing(on) {
  if (resetting) return;
  O.editing = on;
  $("#ov-edit").setAttribute("aria-pressed", String(on));
  $("#ov-edit").innerHTML = on ? "Done" : `<svg aria-hidden="true"><use href="#i-move"/></svg>Customize`;
  $("#ov-editbar").hidden = !on;
  flip(grid(), () => renderOverview());
}

function cell() {
  const g = grid(), style = getComputedStyle(g);
  const cols = Math.min(COLS, style.gridTemplateColumns.split(" ").length);
  const w = g.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
  const columnGap = parseFloat(style.columnGap), rowGap = parseFloat(style.rowGap);
  return { cw: (w - columnGap * (cols - 1)) / cols, rh: parseFloat(style.gridAutoRows), cols, columnGap, rowGap };
}

// Pointer coordinates and rectangles include CSS zoom; grid measurements and transforms do not.
// Measuring the rendered/layout ratio also handles a zoom inherited from any ancestor.
function viewportScale() {
  const g = grid();
  const scale = g.offsetWidth > 0 ? g.getBoundingClientRect().width / g.offsetWidth : 1;
  return Number.isFinite(scale) && scale > 0 ? scale : 1;
}

function onPointerDown(e) {
  if (resetting || !O.editing || e.button !== 0) return;
  const el = e.target.closest(".tile:not(.tile-add)");
  if (!el || e.target.closest("button:not(.pcard), input, select, a.link")) return;
  const t = tileById(el.dataset.id);
  const r = el.getBoundingClientRect();
  e.preventDefault();
  if (e.target.closest('[data-act="resize"]')) {
    O.drag = { mode: "resize", el, t, x0: e.clientX, y0: e.clientY, w0: Math.min(t.w, cell().cols), savedW0: t.w, h0: t.h };
  } else {
    O.drag = { mode: "move", el, t, ox: e.clientX - r.left, oy: e.clientY - r.top, x0: e.clientX, y0: e.clientY, started: false, natural: r };
  }
  grid().setPointerCapture(e.pointerId);
  showSnapGuide(el, t);
}

// The guide marks the actual grid slot, independently of the dragged tile's transform.
function showSnapGuide(el, t) {
  if (!document.createElement || !el.getBoundingClientRect) return;
  let guide = $("#ov-snap-guide");
  if (!guide) { guide = document.createElement("div"); guide.id = "ov-snap-guide"; guide.setAttribute("aria-hidden", "true"); grid().append(guide); }
  const r = naturalRect(el), g = grid().getBoundingClientRect(), scale = viewportScale();
  Object.assign(guide.style, {left:`${(r.left-g.left)/scale}px`,top:`${(r.top-g.top)/scale}px`,width:`${r.width/scale}px`,height:`${r.height/scale}px`});
  guide.textContent = `${Math.min(t.w, cell().cols)} × ${t.h}`;
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
  const scale = viewportScale();
  if (d.mode === "resize") {
    const { cw, rh, cols, columnGap, rowGap } = cell();
    const visibleW = clamp(Math.round(d.w0 + (e.clientX - d.x0) / ((cw + columnGap) * scale)), 1, cols);
    // A narrower viewport only clamps presentation; vertical resizing must keep the desktop span.
    const w = visibleW === d.w0 ? d.savedW0 : visibleW;
    const h = clamp(Math.round(d.h0 + (e.clientY - d.y0) / ((rh + rowGap) * scale)), 1, 3);
    if (w !== d.t.w || h !== d.t.h) {
      flip(grid(), () => { d.t.w = w; d.t.h = h; sizeTile(d.el, d.t); });
      update(d.el, d.t);
      showSnapGuide(d.el, d.t);
      d.changed = true;
    }
    return;
  }
  if (!d.started) {
    if (Math.hypot(e.clientX - d.x0, e.clientY - d.y0) < 5) return;
    d.started = true;
    d.el.classList.add("dragging");
  }
  d.el.style.transform = `translate(${(e.clientX - d.ox - d.natural.left) / scale}px, ${(e.clientY - d.oy - d.natural.top) / scale}px)`;
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
  d.el.style.transform = `translate(${(e.clientX - d.ox - d.natural.left) / scale}px, ${(e.clientY - d.oy - d.natural.top) / scale}px)`;
  d.changed = true;
  showSnapGuide(d.el, d.t);
}

function onPointerUp() {
  const d = O.drag;
  O.drag = null;
  $("#ov-snap-guide")?.remove?.();
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
  if (resetting || !O.editing) return;
  const el = e.target.closest?.(".tile:not(.tile-add)");
  if (!el || e.target !== el) return;
  const t = tileById(el.dataset.id), tiles = O.layout.tiles, i = tiles.indexOf(t);
  let changed = true;
  if (e.shiftKey && e.key === "ArrowRight") { if (t.w < cell().cols) t.w++; else changed = false; }
  else if (e.shiftKey && e.key === "ArrowLeft") t.w = Math.max(1, Math.min(cell().cols, t.w) - 1);
  else if (e.shiftKey && e.key === "ArrowDown") t.h = Math.min(3, t.h + 1);
  else if (e.shiftKey && e.key === "ArrowUp") t.h = Math.max(1, t.h - 1);
  else if ((e.key === "ArrowRight" || e.key === "ArrowDown") && i < tiles.length - 1) { tiles.splice(i, 1); tiles.splice(i + 1, 0, t); }
  else if ((e.key === "ArrowLeft" || e.key === "ArrowUp") && i > 0) { tiles.splice(i, 1); tiles.splice(i - 1, 0, t); }
  else if (e.key === "Delete" || e.key === "Backspace") { removeTile(t.id); e.preventDefault(); return; }
  else if (e.key === "Enter") { tileSheet(t.id); e.preventDefault(); return; }
  else changed = false;
  if (!changed) return;
  e.preventDefault();
  flip(grid(), () => renderOverview());
  el.focus();
  save();
}

function removeTile(id) {
  if (resetting) return;
  const t = tileById(id);
  if (!t) return;
  const label = t.label || (t.type === "sensor" ? sensorName(t.sensors[0]) : CONTROL[t.type].name);
  flip(grid(), () => { O.layout.tiles = O.layout.tiles.filter((x) => x.id !== id); renderOverview(); });
  save();
  const removalRevision = saveRevision;
  toast(`${label} removed`, {
    action: {
      label: "Undo",
      onClick: () => {
        if (saveRevision === removalRevision) historyStep("undo");
        else toast("Use layout Undo to step back through your recent edits.");
      },
    },
  });
}

async function addTile(tile) {
  if (resetting) return;
  const workspace = O.layout.active_workspace, layout = O.layout;
  if (tile.type === "sensor") {
    await ensureHistory(tile.sensors).catch(() => {});
  }
  if (resetting || O.layout !== layout || O.layout.active_workspace !== workspace) return;
  flip(grid(), () => { O.layout.tiles.push(tile); renderOverview(); });
  save();
  const el = O.els.get(tile.id);
  el?.scrollIntoView({ block: "nearest", behavior: reduced() ? "auto" : "smooth" });
  el?.classList.add("just-added");
  setTimeout(() => el?.classList.remove("just-added"), 1200);
}

// ------------------------------------------------------------ sheets: picker, add, tile settings
function pickerHTML(selected) {
  return `<input class="text search" id="pk-q" type="search" placeholder="Search readings, for example memory or fan" aria-label="Search readings" autocomplete="off" spellcheck="false">
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
    addTile({ id: newId(m?.short || sid.split("/").pop()), type: "sensor", sensors: [sid], style: "auto", w: 1, h: 1 });
  }, null);
}

const STYLE_ICONS = {
  auto: `<svg viewBox="0 0 40 28"><text x="2" y="14" class="si-num">42</text><path d="M2 23H38M2 19V25M12 19V25M22 19V25M32 19V25" class="si-line"/></svg>`,
  spark: `<svg viewBox="0 0 40 28"><text x="2" y="15" class="si-num">42</text><path d="M2 24 L10 21 L17 23 L24 17 L31 19 L38 13" class="si-line"/></svg>`,
  ring: `<svg viewBox="0 0 40 28"><path d="M11 24 A11 11 0 1 1 29 24" class="si-track"/><path d="M11 24 A11 11 0 0 1 26 5.6" class="si-arc"/></svg>`,
  graph: `<svg viewBox="0 0 40 28"><path d="M2 5H38M2 14H38M2 23H38" class="si-grid"/><path d="M2 20 L8 16 L14 18 L20 10 L26 13 L32 6 L38 9" class="si-line"/></svg>`,
  bar: `<svg viewBox="0 0 40 28"><rect x="2" y="12" width="36" height="5" rx="2.5" class="si-track-f"/><rect x="2" y="12" width="22" height="5" rx="2.5" class="si-fill"/></svg>`,
};

function appearanceMarkup(t) {
  return `<fieldset class="tile-appearance"><legend>Presentation</legend>
    <label class="field"><span class="lab">Emphasis</span><select class="select" id="ts-emphasis">${[["standard","Standard"],["focus","Feature tile"],["quiet","Quiet"]].map(([v,label])=>`<option value="${v}"${(t.appearance?.emphasis || "standard")===v ? " selected" : ""}>${label}</option>`).join("")}</select></label>
    <div class="field"><span class="lab">Accent</span><div class="tile-accent-options"><button type="button" class="chip-btn" id="ts-accent-theme" aria-pressed="${!t.appearance?.accent}">Follow theme</button>${["#86c8c1","#b4baff","#e7ad83","#c4c67c","#df93ae"].map(c=>`<button type="button" class="tile-accent-swatch" data-accent="${c}" style="--swatch:${c}" aria-label="Accent ${c}" aria-pressed="${t.appearance?.accent===c}"></button>`).join("")}<label class="custom-tile-accent">Custom<input type="color" id="ts-accent-custom" value="${esc(t.appearance?.accent || "#86c8c1")}" aria-label="Custom tile accent"></label></div></div>
    <p class="hint">Accents style the tile. Lighting colors and temperature alerts keep their meaning.</p></fieldset>`;
}

function wireAppearance(t, live) {
  const change = (next) => { t.appearance = {emphasis: "standard", accent: null, ...t.appearance, ...next}; live(); };
  $("#ts-emphasis").addEventListener("change", e=>change({emphasis:e.target.value}));
  const accent = value => {
    change({accent:value});
    $("#ts-accent-theme").setAttribute("aria-pressed", String(!value));
    $$("[data-accent]").forEach(b=>b.setAttribute("aria-pressed", String(b.dataset.accent===value)));
    if (value) $("#ts-accent-custom").value = value;
  };
  $("#ts-accent-theme").addEventListener("click", ()=>accent(null));
  $$("[data-accent]").forEach(b=>b.addEventListener("click", ()=>accent(b.dataset.accent)));
  $("#ts-accent-custom").addEventListener("input", e=>accent(e.target.value));
  $("#ts-accent-custom").addEventListener("change", ()=>O.history?.endGroup());
}

function tileSheet(id, mode = "main", slot = 0) {
  if (resetting) return;
  O.history?.endGroup(); // A reopened sheet starts a new edit, including after Escape.
  const t = tileById(id);
  if (!t) return;
  if (t.type !== "sensor") {
    openSheet(`<form class="body" id="ts-form"><h2>${esc(CONTROL[t.type].name)} tile</h2>${appearanceMarkup(t)}
      <div class="actions"><button type="button" class="btn danger" id="ts-remove">Remove tile</button><span class="spacer"></span><button class="btn primary">Done</button></div></form>`);
    wireAppearance(t, ()=>{renderOverview();save(`appearance-${id}`);});
    $("#ts-remove").addEventListener("click", ()=>{closeSheet();removeTile(id);});
    $("#ts-form").addEventListener("submit", e=>{e.preventDefault();O.history?.endGroup();closeSheet();});
    return;
  }
  if (mode === "pick") {
    openSheet(`<div class="body wide"><h2>${slot ? "Second reading" : "Choose a reading"}</h2>${pickerHTML(t.sensors[slot])}
      <div class="actions"><button type="button" class="btn quiet" id="pk-back">Back</button><span class="spacer"></span></div></div>`);
    $("#pk-back").addEventListener("click", () => tileSheet(id));
    wirePicker(async (sid) => {
      const workspace = O.layout.active_workspace;
      t.sensors[slot] = sid;
      renderOverview(); save();
      await ensureHistory([sid]).catch(() => {});
      if (O.layout.active_workspace === workspace && tileById(id) === t && $("#sheet").open) tileSheet(id);
    }, t.sensors[slot]);
    return;
  }
  const pins = O.layout.pins;
  const m0 = R.meta[t.sensors[0]], m1 = t.sensors[1] ? R.meta[t.sensors[1]] : null;
  const pinned = pins.includes(t.sensors[0]);
  openSheet(`<form class="body" id="ts-form"><h2>Tile settings</h2>
    <div class="ts-row"><span class="ts-lab">Reading</span><div class="ts-val"><b>${esc(m0?.name || sensorName(t.sensors[0]))}</b><small>${esc(source(m0) || m0?.group || "")}</small></div><button type="button" class="btn" data-pick-slot="0">Change</button></div>
    ${["auto", "graph"].includes(t.style) ? `<div class="ts-row"><span class="ts-lab">Compare with</span>${m1 || t.sensors[1]
      ? `<div class="ts-val"><b>${esc(m1?.name || sensorName(t.sensors[1]))}</b><small>${esc(source(m1) || m1?.group || "")}</small></div><div class="btn-row"><button type="button" class="btn" data-pick-slot="1">Change</button><button type="button" class="icon-btn" id="ts-drop2" title="Remove the second reading"><svg><use href="#i-close"/></svg></button></div>`
      : `<div class="ts-val"><span class="hint">Add a second reading. Instruments show the comparison when the tile is tall enough.</span></div><button type="button" class="btn" data-pick-slot="1">Add</button>`}</div>` : ""}
    <div class="field"><span class="lab">Style</span><div class="styles" role="radiogroup">${STYLES.map((s) => `<button type="button" role="radio" class="style-btn" data-style="${s.id}" aria-checked="${t.style === s.id}">${STYLE_ICONS[s.id]}<span>${s.name}</span></button>`).join("")}</div></div>
    <label class="field"><span class="lab">Name</span><input class="text" id="ts-label" maxlength="40" value="${esc(t.label || "")}" placeholder="${esc(m0?.name || "")}" autocomplete="off"></label>
    ${appearanceMarkup(t)}
    <label class="ts-pin"><span class="switch"><input type="checkbox" id="ts-pin"${pinned ? " checked" : ""}${!pinned && pins.length >= 4 ? " disabled" : ""}><span></span></span>
      <span><b>Show in the top bar and tray</b><small>${!pinned && pins.length >= 4 ? "Up to four readings can be pinned. Unpin one first." : "Keeps this reading in view on every tab, and in the tray icon's tooltip."}</small></span></label>
    <div class="actions"><button type="button" class="btn danger" id="ts-remove">Remove tile</button><span class="spacer"></span><button class="btn primary">Done</button></div></form>`);
  const live = () => { renderOverview(); save(); };
  wireAppearance(t, ()=>{renderOverview();save(`appearance-${id}`);});
  $$("[data-pick-slot]").forEach((b) => b.addEventListener("click", () => tileSheet(id, "pick", +b.dataset.pickSlot)));
  $("#ts-drop2")?.addEventListener("click", () => { t.sensors = t.sensors.slice(0, 1); live(); tileSheet(id); });
  $$("[data-style]").forEach((b) => b.addEventListener("click", () => {
    t.style = b.dataset.style;
    if (!["auto", "graph"].includes(t.style)) t.sensors = t.sensors.slice(0, 1);
    if (t.style === "graph" && t.w < 2) t.w = 2;
    flip(grid(), live);
    tileSheet(id);
  }));
  $("#ts-label").addEventListener("input", (e) => {
    const v = e.target.value.trim(), tile = tileById(id) || t;
    if (v) tile.label = v; else delete tile.label;
    renderOverview(); save(`label-${id}`);
  });
  $("#ts-label").addEventListener("blur", () => O.history?.endGroup());
  $("#ts-pin").addEventListener("change", (e) => {
    const list = O.layout.pins, sid = t.sensors[0];
    O.layout.pins = e.target.checked ? [...list, sid].slice(0, 4) : list.filter((p) => p !== sid);
    live();
  });
  $("#ts-remove").addEventListener("click", () => { closeSheet(); removeTile(id); });
  $("#ts-form").addEventListener("submit", (e) => { e.preventDefault(); O.history?.endGroup(); closeSheet(); });
}

// ------------------------------------------------------------ init
export async function initOverview(layout) {
  savedLayout = JSON.stringify(layout);
  initializeWorkspaces(layout);
  O.layout = layout;
  O.history = createLayoutHistory(layout);
  S.overview = layout;
  initWorkspaceToolbar();
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
  $("#ov-addtile").addEventListener("click", addTileSheet);
  $("#ov-reset").addEventListener("click", async () => {
    if (!confirm("Reset this workspace's tiles and pins to the default? Other workspaces stay saved, and layout Undo can restore this one.")) return;
    try {
      const l = await resetOverview();
      if (!l) return;
      await ensureHistory([...new Set(l.tiles.flatMap((t) => t.sensors || []))]).catch(() => {});
      flip(grid(), () => renderOverview());
      emit("pins");
    } catch (err) { fail(err); }
  });
  document.addEventListener("keydown", (e) => { if (e.key === "Escape" && O.editing && !$("#sheet").open) setEditing(false); });
  on("state", () => { if (visible()) renderOverview(); });
  on("zones", () => { if (visible()) { renderOverview(); paintLightingTiles(); } });
  on("frames:live", (frame) => { overviewFrames = frame.zones || {}; paintLightingTiles(); });
  document.addEventListener("appearancechange", () => {
    measureRev++;
    if (visible() && !document.hidden) { renderOverview(); paintLightingTiles(); }
  });
  new MutationObserver(() => paintLightingTiles()).observe(document.documentElement, { attributes: true, attributeFilter: ["data-rig"] });
  await ensureHistory([...new Set(layout.tiles.flatMap((t) => t.sensors || []))]).catch(() => {});
  renderOverview();
}

export function onFrames() { paintLightingTiles(); }
export const overviewLayout = () => O.layout;
