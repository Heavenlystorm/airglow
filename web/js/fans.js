// Fans screen: temperature/power readouts with history, the curve editor, and fan speeds.

import { ghost } from "./motion.js";
import { $, $$, S, cap, clamp, curveLabel, emit, esc, fail, fmtTemp, on, post, rangeFill, setHTML, tempLevel, toUnit, toast, unitF } from "./core.js";

const HISTORY_SECONDS = 600;
const num = (v) => `<span class="num" data-ghost="${ghost(v)}">${v}</span>`;

// ------------------------------------------------------------ readouts
function spark(points, min, max, color) {
  if (points.length < 2) return "";
  const now = Date.now() / 1000;
  const xy = points.map(([t, v]) => [((t - (now - HISTORY_SECONDS)) / HISTORY_SECONDS) * 300, 44 - ((clamp(v, min, max) - min) / (max - min)) * 40]);
  const d = "M" + xy.map(([x, y]) => `${x.toFixed(1)},${y.toFixed(1)}`).join("L");
  return `<svg viewBox="0 0 300 44" preserveAspectRatio="none" aria-hidden="true"><path d="${d}L${xy.at(-1)[0]},44L${xy[0][0]},44Z" fill="${color}" fill-opacity=".08"/><path d="${d}" fill="none" stroke="${color}" stroke-width="1.5" vector-effect="non-scaling-stroke"/></svg>`;
}

const shortCpu = (n) => (n || "Processor").replace(/^AMD\s+|^Intel\(R\)\s+Core\(TM\)\s+/i, "");
const shortGpu = (n) => (n || "Graphics card").replace(/^NVIDIA\s+GeForce\s+|^AMD\s+Radeon\s+/i, "");
const lineColor = (lvl) => (lvl === "hot" ? "#ef5a48" : lvl === "warm" ? "#e39540" : "#b9b5aa");

function renderReadouts() {
  const s = S.state.sensors, h = S.history;
  const pts = (k) => h.filter((p) => p[k] != null).map((p) => [p.t, p[k]]);
  const tempBig = (c, lvl) => `<div class="big ${lvl}">${num(c == null ? "–" : Math.round(toUnit(c)))}<span class="u">°${unitF() ? "F" : "C"}</span></div>`;
  const cpuLvl = tempLevel(s.cpu_temp, 80, 90), gpuLvl = tempLevel(s.gpu_temp, 75, 85);
  const peak = Math.max(150, ...h.map((p) => (p.cpu_w ?? 0) + (p.gpu_w ?? 0)));
  const total = h.filter((p) => p.cpu_w != null && p.gpu_w != null).map((p) => [p.t, p.cpu_w + p.gpu_w]);
  let html = `
    <div class="ro"><div class="head"><span>${esc(shortCpu(s.cpu_name))}</span><span>${s.cpu_power != null ? Math.round(s.cpu_power) + " W" : ""}</span></div>
      ${tempBig(s.cpu_temp, cpuLvl)}${spark(pts("cpu"), 20, 100, lineColor(cpuLvl))}</div>
    <div class="ro"><div class="head"><span>${esc(shortGpu(s.gpu_name))}</span><span>${s.gpu_power != null ? Math.round(s.gpu_power) + " W" : ""}</span></div>
      ${tempBig(s.gpu_temp, gpuLvl)}${spark(pts("gpu"), 20, 100, lineColor(gpuLvl))}</div>
    ${s.liquid_temp != null ? `<div class="ro"><div class="head"><span>Coolant</span><span>liquid cooler</span></div>
      ${tempBig(s.liquid_temp, tempLevel(s.liquid_temp, 40, 50))}</div>` : ""}
    <div class="ro"><div class="head"><span>Power draw</span><span>processor and graphics</span></div>
      <div class="big">${num(s.total_power == null ? "–" : Math.round(s.total_power))}<span class="u">W</span></div>${spark(total, 0, Math.ceil(peak * 1.15 / 50) * 50, "#b9b5aa")}</div>`;
  if (!s.online) {
    html += `<p class="ro note">Temperatures come from LibreHardwareMonitor, which isn't responding. Open it from the Start menu, and turn on Options, Remote Web Server, Run.</p>`;
  }
  setHTML($("#readouts"), html);
}

// ------------------------------------------------------------ curve editor
const ED = { W: 900, H: 400, L: 52, R: 884, T: 16, B: 362, tMin: 20, tMax: 100 };
const ex = (t) => ED.L + ((t - ED.tMin) / (ED.tMax - ED.tMin)) * (ED.R - ED.L);
const ey = (d) => ED.B - (d / 100) * (ED.B - ED.T);
const E = { curve: null, draft: null, sel: null, drag: null, drawn: "" };
const saved = () => S.curves.curves[E.curve];
const dirty = () => E.draft && JSON.stringify(E.draft) !== JSON.stringify(saved());

function interp(points, t) {
  const p = [...points].sort((a, b) => a[0] - b[0]);
  if (t <= p[0][0]) return p[0][1];
  for (let i = 1; i < p.length; i++) if (t <= p[i][0]) return p[i - 1][1] + (p[i][1] - p[i - 1][1]) * (t - p[i - 1][0]) / (p[i][0] - p[i - 1][0]);
  return p.at(-1)[1];
}

function renderEditor() {
  if (!S.curves || !S.state) return;
  const F = S.state.fans;
  // a profile can bring its own curve: then none of the named ones is on, and the editor keeps the last one shown
  const named = F.base_curve in S.curves.curves ? F.base_curve : null;
  if (!E.curve || (named && E.curve !== named && !dirty())) { E.curve = named || E.curve || "balanced"; E.draft = null; }   // follow tray/profile changes
  if (!E.draft) E.draft = saved().map((p) => p.slice());
  const { min_duty: minDuty, safety_temp: safety } = S.curves;
  const src = S.settings?.settings?.temp_source;
  const tracking = src === "cpu" ? "the processor" : src === "gpu" ? "the graphics card"
    : src === "liquid" ? (S.state.sensors.liquid_temp != null ? "the coolant" : "the hotter part until a coolant reading comes through")
    : "whichever part is hotter";
  // Rebuilding the whole graph every two seconds threw away the focus of a point you had picked with the keyboard.
  // Only draw when something it shows has changed, and put the focus back when it does.
  const key = JSON.stringify([E.curve, E.draft, E.sel, E.drag ? E.drag.i : null, F.curve, F.temp, named, minDuty,
                               safety, dirty(), S.settings?.settings?.temp_source, S.state.sensors.liquid_temp]);
  if (key === E.drawn) return;
  E.drawn = key;
  const focusPt = document.activeElement?.matches?.("#editor .pt") ? +document.activeElement.dataset.i : null;
  setHTML($("#curve-seg"), Object.keys(S.curves.curves).map((c) =>
    `<button role="radio" data-c="${c}" aria-checked="${!!named && c === E.curve}">${curveLabel(c)}</button>`).join(""));
  const pts = E.draft;
  const line = (arr) => [[ED.tMin, arr[0][1]], ...arr, [ED.tMax, arr.at(-1)[1]]].map(([t, d]) => `${ex(t).toFixed(1)},${ey(d).toFixed(1)}`).join(" ");
  let g = `<g class="grid">`;
  for (let t = 20; t <= 100; t += 10) g += `<line class="${t % 50 === 0 ? "major" : ""}" x1="${ex(t)}" x2="${ex(t)}" y1="${ED.T}" y2="${ED.B}"/>`;
  for (let d = 0; d <= 100; d += 10) g += `<line class="${d % 50 === 0 ? "major" : ""}" x1="${ED.L}" x2="${ED.R}" y1="${ey(d)}" y2="${ey(d)}"/>`;
  g += `</g><g class="axis">`;
  for (let t = 20; t <= 100; t += 10) g += `<text x="${ex(t)}" y="${ED.B + 22}" text-anchor="middle">${Math.round(toUnit(t))}°</text>`;
  for (let d = 0; d <= 100; d += 20) g += `<text x="${ED.L - 10}" y="${ey(d) + 4}" text-anchor="end">${d}%</text>`;
  g += `</g><rect class="zone-hot" x="${ex(safety)}" y="${ED.T}" width="${ED.R - ex(safety)}" height="${ED.B - ED.T}"/>
    <text class="zone-label" x="${ex(safety) + 8}" y="${ED.T + 18}">Always full speed</text>
    <rect class="zone-floor" x="${ED.L}" y="${ey(minDuty)}" width="${ED.R - ED.L}" height="${ED.B - ey(minDuty)}"/>
    <text class="zone-label" x="${ED.L + 8}" y="${ED.B - 8}">Below ${minDuty}% some fans stall, so curves stay above it</text>`;
  if (dirty()) g += `<polyline class="ghost" points="${line(saved())}"/>`;
  g += `<polygon class="curve-fill" points="${ex(ED.tMin)},${ED.B} ${line(pts)} ${ex(ED.tMax)},${ED.B}"/><polyline class="curve-line" points="${line(pts)}"/>`;
  if (F.temp != null && E.curve === F.curve) {
    const t = clamp(F.temp, ED.tMin, ED.tMax), d = F.temp >= safety ? 100 : interp(pts, t);
    const right = ex(t) < ED.R - 150;
    g += `<line class="now-line" x1="${ex(t)}" x2="${ex(t)}" y1="${ED.T}" y2="${ED.B}"/><circle class="now-dot" cx="${ex(t)}" cy="${ey(d)}" r="5"/>
      <text class="now-label" x="${ex(t) + (right ? 10 : -10)}" y="${ey(d) - 12}" text-anchor="${right ? "start" : "end"}">Now ${fmtTemp(F.temp)}, ${Math.round(d)}%</text>`;
  }
  pts.forEach(([t, d], i) => {
    g += `<circle class="pt${E.sel === i ? " sel" : ""}" data-i="${i}" cx="${ex(t)}" cy="${ey(d)}" r="8" tabindex="0" role="slider" aria-label="Point ${i + 1}: ${Math.round(toUnit(t))} degrees, ${d} percent" aria-valuenow="${d}" aria-valuemin="${minDuty}" aria-valuemax="100"/>`;
  });
  if (E.drag) g += `<text class="tip" x="${ex(pts[E.drag.i][0])}" y="${ey(pts[E.drag.i][1]) - 16}" text-anchor="middle">${fmtTemp(pts[E.drag.i][0])}, ${pts[E.drag.i][1]}%</text>`;
  setHTML($("#editor"), g);
  if (focusPt != null) $(`#editor .pt[data-i="${focusPt}"]`)?.focus({ preventScroll: true });
  $("#curve-save").disabled = !dirty();
  $("#curve-discard").disabled = !dirty();
  $("#curve-lede").textContent = !(F.curve in S.curves.curves)
    ? `The fans follow ${F.curve === "trial" ? "the curve of the profile you're trying" : `${curveLabel(F.curve)}'s own curve`}. Pick a curve here to switch back to one of yours.`
    : F.curve !== F.base_curve
    ? `An automation has switched the fans to ${curveLabel(F.curve)} for now. ${curveLabel(F.base_curve)} comes back when it stops.`
    : `Fans that Airglow drives follow this curve, tracking ${tracking}.`;
  $("#editor-hint").textContent = dirty() ? "Unsaved changes. The dashed line is the saved curve." : "Drag points, double-click to add one, Delete removes the selected point, arrow keys fine-tune it.";
}

function svgPoint(e) {
  const svg = $("#editor"), r = svg.getBoundingClientRect();
  return [(e.clientX - r.left) * (ED.W / r.width), (e.clientY - r.top) * (ED.H / r.height)];
}
const fromSvg = ([x, y]) => [Math.round(ED.tMin + ((x - ED.L) / (ED.R - ED.L)) * (ED.tMax - ED.tMin)), Math.round(((ED.B - y) / (ED.B - ED.T)) * 100)];
function movePoint(i, t, d) {
  const p = E.draft, lo = i > 0 ? p[i - 1][0] + 1 : ED.tMin, hi = i < p.length - 1 ? p[i + 1][0] - 1 : ED.tMax;
  p[i] = [clamp(t, lo, hi), clamp(d, S.curves.min_duty, 100)];
}

function wireEditor() {
  const svg = $("#editor");
  svg.addEventListener("pointerdown", (e) => {
    const c = e.target.closest(".pt");
    if (!c) { E.sel = null; renderEditor(); return; }
    E.drag = { i: +c.dataset.i }; E.sel = E.drag.i;
    svg.setPointerCapture(e.pointerId); renderEditor();
  });
  svg.addEventListener("pointermove", (e) => { if (E.drag) { movePoint(E.drag.i, ...fromSvg(svgPoint(e))); renderEditor(); } });
  const end = () => { if (E.drag) { E.drag = null; renderEditor(); $(`#editor .pt[data-i="${E.sel}"]`)?.focus(); } };
  svg.addEventListener("pointerup", end);
  svg.addEventListener("pointercancel", end);
  svg.addEventListener("dblclick", (e) => {
    if (e.target.closest(".pt")) return;
    const [t, d] = fromSvg(svgPoint(e));
    if (E.draft.length >= 8) return toast("A curve can have up to 8 points", true);
    if (t < ED.tMin || t > ED.tMax || E.draft.some((p) => p[0] === t)) return;
    E.draft.push([t, clamp(d, S.curves.min_duty, 100)]);
    E.draft.sort((a, b) => a[0] - b[0]);
    E.sel = E.draft.findIndex((p) => p[0] === t);
    renderEditor();
  });
  svg.addEventListener("keydown", (e) => {
    const c = e.target.closest(".pt");
    if (!c) return;
    const i = +c.dataset.i, step = e.shiftKey ? 5 : 1, [t, d] = E.draft[i];
    const moves = { ArrowUp: [0, step], ArrowDown: [0, -step], ArrowRight: [step, 0], ArrowLeft: [-step, 0] };
    if (moves[e.key]) { e.preventDefault(); movePoint(i, t + moves[e.key][0], d + moves[e.key][1]); E.sel = i; }
    else if ((e.key === "Delete" || e.key === "Backspace") && E.draft.length > 2) { e.preventDefault(); E.draft.splice(i, 1); E.sel = null; }
    else return;
    renderEditor();
    if (E.sel != null) $(`#editor .pt[data-i="${i}"]`)?.focus();
  });
  $("#curve-seg").addEventListener("click", async (e) => {
    const c = e.target.closest("[data-c]")?.dataset.c;
    if (!c || (c === E.curve && c === S.state.fans.base_curve)) return;
    if (dirty() && !confirm(`Discard unsaved changes to the ${curveLabel(E.curve)} curve?`)) return;
    E.curve = c; E.draft = null; E.sel = null;
    try { await post("/api/fan_curve", { name: c }); S.state.fans.base_curve = c; toast(`Fans now follow the ${curveLabel(c)} curve`); emit("refresh"); }
    catch (err) { fail(err); }
    renderEditor();
  });
  $("#curve-discard").addEventListener("click", () => { E.draft = null; E.sel = null; renderEditor(); });
  $("#curve-reset").addEventListener("click", () => { E.draft = S.curves.defaults[E.curve].map((p) => p.slice()); E.sel = null; renderEditor(); if (dirty()) toast("Default curve loaded. Save to keep it."); });
  $("#curve-save").addEventListener("click", async () => {
    try { const r = await post(`/api/curves/${E.curve}`, { points: E.draft }); S.curves.curves[E.curve] = r.points; E.draft = null; renderEditor(); toast(`${curveLabel(E.curve)} curve saved`); }
    catch (err) { fail(err); }
  });
}

// ------------------------------------------------------------ fan speeds
function renderPorts() {
  const F = S.state.fans, s = S.state.sensors;
  const channels = F.channels || [];
  // name the controller above its ports once there's more than one source of fans on the page
  const named = new Set(F.ports.map((q) => q.device)).size > 1 || channels.length > 0;
  setHTML($("#ports"), F.hub_online
    ? (F.ports.length ? F.ports.map((p, i) => `${named && p.device !== F.ports[i - 1]?.device ? `<li class="ports-dev">${esc(p.device)}</li>` : ""}<li><span>Fan ${p.port}</span><span class="bar"><i style="width:${p.duty}%"></i></span><span class="rpm">${p.rpm.toLocaleString()} rpm</span></li>`).join("")
                      : `<li class="empty-note">No fans are spinning on the fan controller.</li>`)
    : channels.length ? ""
    : `<li class="empty-note">No fan controller is connected directly. Motherboard headers and cooler pumps show up below once LibreHardwareMonitor runs.</li>`);
  // fans and pumps Airglow drives through LHM are listed below with their own controls; don't list them twice
  const listed = new Set(channels.map((c) => c.fan_id).filter(Boolean));
  setHTML($("#other-fans"), (s.fans || []).filter((f) => !listed.has(f.id)).map((f) => `<li><span>${esc(cap(f.name))}</span><span>${f.rpm.toLocaleString()} rpm</span></li>`).join(""));
  renderChannels(channels, s.online);
}

// ------------------------------------------------------------ motherboard headers and coolers (through LHM)
const MODES = [["", "Leave alone"], ["curve", "Follow the curve"], ["steady", "Steady speed"]];
let chanKey = "";

function channelRow(c) {
  const mode = c.mode || "";
  const steady = c.steady ?? (c.pump ? 70 : 50);
  return `<li class="chan" data-chan="${esc(c.id)}">
    <div class="chan-name"><strong>${esc(c.name)}${c.pump ? ` <span class="tag">Pump</span>` : ""}</strong><span>${esc(c.device)}</span>
      <button type="button" class="text-link" data-chan-all title="Give every fan below the same setting">Use for all</button></div>
    <span class="bar" title="What it runs at now"><i></i></span><span class="rpm"></span>
    <select class="select" data-mode aria-label="How ${esc(c.name)} runs">${MODES.map(([v, l]) => `<option value="${v}"${v === mode ? " selected" : ""}>${l}</option>`).join("")}</select>
    <span class="chan-steady"${mode === "steady" ? "" : " hidden"}><input type="range" min="${S.curves?.min_duty ?? 20}" max="100" step="5" value="${steady}" data-steady aria-label="Steady speed"><output>${steady}%</output></span>
    <p class="chan-note hint"></p></li>`;
}

function renderChannels(channels, lhmOnline) {
  const box = $("#channels");
  if (!lhmOnline && !channels.length) {
    setHTML(box, `<h2>Motherboard and cooler fans</h2><p class="hint">Airglow can drive the board's fan headers and the pumps and fans of many liquid coolers through LibreHardwareMonitor, which isn't responding. Set it up in Settings, Helper programs.</p>`);
    chanKey = "";
    return;
  }
  if (!channels.length) {
    setHTML(box, `<h2>Motherboard and cooler fans</h2><p class="hint">LibreHardwareMonitor doesn't list any fan it can drive on this PC. Some boards only allow it with the fan headers set to manual or "full speed" in the BIOS.</p>`);
    chanKey = "";
    return;
  }
  // rebuild the rows only when what they are changes, never under a control you're using
  const key = JSON.stringify(channels.map((c) => [c.id, c.name, c.device, c.pump, c.mode, c.steady]));
  if (key !== chanKey && !box.contains(document.activeElement)) {
    chanKey = key;
    setHTML(box, `<h2>Motherboard and cooler fans</h2>
      <p class="hint">Through LibreHardwareMonitor. Fans left alone keep your BIOS or cooler curve. Pumps usually run best at a steady speed. When Airglow quits, they all go back to their own curves.</p>
      <ul class="chan-list">${channels.map(channelRow).join("")}</ul>`);
    $$("#channels input[type=range]").forEach(rangeFill);
  }
  for (const c of channels) {
    const row = box.querySelector(`[data-chan="${CSS.escape(c.id)}"]`);
    if (!row) continue;
    const now = c.target ?? c.duty;
    row.querySelector(".bar i").style.width = `${clamp(now ?? 0, 0, 100)}%`;
    row.querySelector(".rpm").textContent = c.rpm != null ? `${c.rpm.toLocaleString()} rpm` : now != null ? `${Math.round(now)}%` : "";
    row.querySelector(".chan-note").textContent = c.error ? `LibreHardwareMonitor won't change it: ${c.error}`
      : c.pump && c.mode === "curve" ? "A pump following the curve slows down when things are cool. Most run best at a steady 60% or more." : "";
    row.querySelector(".chan-note").classList.toggle("warn", !!c.error || (c.pump && c.mode === "curve"));
  }
}

async function saveChannel(row, patch) {
  const c = (S.state.fans.channels || []).find((x) => x.id === row.dataset.chan);
  const body = { id: row.dataset.chan, mode: patch.mode ?? c?.mode ?? "", duty: patch.duty ?? +row.querySelector("[data-steady]").value };
  try {
    const r = await post("/api/fans/channel", body);
    S.state.fans.channels = r.channels;
    chanKey = "";
    renderPorts();
    toast(!body.mode ? `${c?.name || "That fan"} keeps its own curve again` : body.mode === "curve" ? `${c?.name || "That fan"} follows the fan curve` : `${c?.name || "That fan"} holds ${body.duty}%`);
  } catch (err) { fail(err); chanKey = ""; renderPorts(); }
}

export function renderFans() {
  if (!S.state) return;
  renderReadouts();
  renderPorts();
  if (!E.drag) renderEditor();          // dragging redraws it itself; a focused point keeps its focus
}

export function initFans() {
  wireEditor();
  const box = $("#channels");
  box.addEventListener("change", (e) => {
    const row = e.target.closest("[data-chan]");
    if (!row) return;
    if (e.target.matches("[data-mode]")) {
      row.querySelector(".chan-steady").hidden = e.target.value !== "steady";
      saveChannel(row, { mode: e.target.value });
    } else if (e.target.matches("[data-steady]")) saveChannel(row, { mode: "steady", duty: +e.target.value });
  });
  box.addEventListener("input", (e) => {
    if (!e.target.matches("[data-steady]")) return;
    rangeFill(e.target);
    e.target.nextElementSibling.textContent = `${e.target.value}%`;
  });
  // "Use for all": the same setting for every header and cooler Airglow drives, one call each.
  box.addEventListener("click", async (e) => {
    const b = e.target.closest("[data-chan-all]");
    if (!b) return;
    const row = b.closest("[data-chan]");
    const src = (S.state.fans.channels || []).find((c) => c.id === row.dataset.chan);
    if (!src) return;
    const mode = src.mode || "";
    const duty = +(row.querySelector("[data-steady]")?.value ?? 50);
    const others = (S.state.fans.channels || []).filter((c) => c.id !== src.id);
    let failed = null;
    for (const c of others) {
      try {
        const r = await post("/api/fans/channel", { id: c.id, mode, duty });
        S.state.fans.channels = r.channels;
      } catch (err) { failed = err; break; }
    }
    chanKey = "";
    renderPorts();
    const what = !mode ? "keeps its own curve" : mode === "curve" ? "follows the fan curve" : `holds ${duty}%`;
    if (failed) toast(`${src.name}'s setting was applied until ${failed.message}`, true);
    else toast(`All ${others.length + 1} fans now ${what}`);
  });
  on("state", () => { if (!$('[data-view="fans"]').hidden) renderFans(); });
  window.addEventListener("beforeunload", (e) => { if (dirty()) e.preventDefault(); });
}
