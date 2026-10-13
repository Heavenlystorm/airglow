// Fans screen: temperature/power readouts with history, the curve editor, and fan speeds.

import { $, S, cap, clamp, curveLabel, emit, esc, fail, fmtTemp, on, post, setHTML, tempLevel, toUnit, toast, unitF } from "./core.js";

const HISTORY_SECONDS = 600;

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
const lineColor = (lvl) => (lvl === "hot" ? "#ff5d4f" : lvl === "warm" ? "#ffab40" : "#a8acb2");

function renderReadouts() {
  const s = S.state.sensors, h = S.history;
  const pts = (k) => h.filter((p) => p[k] != null).map((p) => [p.t, p[k]]);
  const tempBig = (c, lvl) => `<div class="big ${lvl}">${c == null ? "–" : Math.round(toUnit(c))}<span class="u">°${unitF() ? "F" : "C"}</span></div>`;
  const cpuLvl = tempLevel(s.cpu_temp, 80, 90), gpuLvl = tempLevel(s.gpu_temp, 75, 85);
  const peak = Math.max(150, ...h.map((p) => (p.cpu_w ?? 0) + (p.gpu_w ?? 0)));
  const total = h.filter((p) => p.cpu_w != null && p.gpu_w != null).map((p) => [p.t, p.cpu_w + p.gpu_w]);
  let html = `
    <div class="ro"><div class="head"><span>${esc(shortCpu(s.cpu_name))}</span><span>${s.cpu_power != null ? Math.round(s.cpu_power) + " W" : ""}</span></div>
      ${tempBig(s.cpu_temp, cpuLvl)}${spark(pts("cpu"), 20, 100, lineColor(cpuLvl))}</div>
    <div class="ro"><div class="head"><span>${esc(shortGpu(s.gpu_name))}</span><span>${s.gpu_power != null ? Math.round(s.gpu_power) + " W" : ""}</span></div>
      ${tempBig(s.gpu_temp, gpuLvl)}${spark(pts("gpu"), 20, 100, lineColor(gpuLvl))}</div>
    <div class="ro"><div class="head"><span>Power draw</span><span>processor and graphics</span></div>
      <div class="big">${s.total_power == null ? "–" : Math.round(s.total_power)}<span class="u">W</span></div>${spark(total, 0, Math.ceil(peak * 1.15 / 50) * 50, "#a8acb2")}</div>`;
  if (!s.online) {
    html += `<p class="ro note">Temperatures come from LibreHardwareMonitor, which isn't responding. Open it from the Start menu, and turn on Options, Remote Web Server, Run.</p>`;
  }
  setHTML($("#readouts"), html);
}

// ------------------------------------------------------------ curve editor
const ED = { W: 900, H: 400, L: 52, R: 884, T: 16, B: 362, tMin: 20, tMax: 100 };
const ex = (t) => ED.L + ((t - ED.tMin) / (ED.tMax - ED.tMin)) * (ED.R - ED.L);
const ey = (d) => ED.B - (d / 100) * (ED.B - ED.T);
const E = { curve: null, draft: null, sel: null, drag: null };
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
  if (!E.curve || (E.curve !== F.base_curve && !dirty())) { E.curve = F.base_curve; E.draft = null; }   // follow tray/preset changes
  if (!E.draft) E.draft = saved().map((p) => p.slice());
  setHTML($("#curve-seg"), Object.keys(S.curves.curves).map((c) =>
    `<button role="radio" data-c="${c}" aria-checked="${c === E.curve}">${curveLabel(c)}</button>`).join(""));
  const { min_duty: minDuty, safety_temp: safety } = S.curves;
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
  $("#curve-save").disabled = !dirty();
  $("#curve-discard").disabled = !dirty();
  const src = S.settings?.settings?.temp_source;
  const tracking = src === "cpu" ? "the processor" : src === "gpu" ? "the graphics card" : "whichever part is hotter";
  $("#curve-lede").textContent = F.curve !== F.base_curve
    ? `An automation has switched the fans to ${curveLabel(F.curve)} for now. ${curveLabel(F.base_curve)} comes back when it stops.`
    : `The NZXT fans follow this curve, tracking ${tracking}.`;
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
    if (!c || c === E.curve) return;
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
  setHTML($("#ports"), F.hub_online
    ? (F.ports.length ? F.ports.map((p) => `<li><span>${new Set(F.ports.map((q) => q.device)).size > 1 ? `${esc(p.device)}, fan` : "Fan"} ${p.port}</span><span class="bar"><i style="width:${p.duty}%"></i></span><span class="rpm">${p.rpm.toLocaleString()} rpm</span></li>`).join("")
                      : `<li class="empty-note">No fans are spinning on the NZXT hub.</li>`)
    : `<li class="empty-note">No NZXT hub connected, so there are no fans for Airglow to drive. Motherboard fans can follow FanControl profiles from your presets.</li>`);
  setHTML($("#other-fans"), (s.fans || []).map((f) => `<li><span>${esc(cap(f.name))}</span><span>${f.rpm.toLocaleString()} rpm</span></li>`).join(""));
}

export function renderFans() {
  if (!S.state) return;
  renderReadouts();
  renderPorts();
  if (!E.drag && !document.activeElement?.classList?.contains("pt")) renderEditor();
}

export function initFans() {
  wireEditor();
  on("state", () => { if (!$('[data-view="fans"]').hidden) renderFans(); });
  window.addEventListener("beforeunload", (e) => { if (dirty()) e.preventDefault(); });
}
