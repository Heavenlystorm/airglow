// The detail view a reading opens into: the figure large, its history as one wide line, the time span to choose,
// and what it did in that span. It grows out of the tile (or pin) it was opened from and keeps updating live.

import { esc, on, rangeFill, setHTML, toast, unitF } from "./core.js";
import { R, fmt, fmtText, level, range, sensorName, thresholds, value } from "./readings.js";
import { createLightGraph, lightTokens, paint, rgb } from "./light-graph.js";
import { countTo, reduced, uiScale } from "./motion.js";
import { liveRange, trendOf } from "./overview-sensors.js";
import { agoText, savedSpan, saveSpan, spanButtons } from "./overview-pulse.js";

const KEY = "airglow.focus.span", SPRING = "cubic-bezier(.16, 1, .3, 1)";
const F = { dialog: null, graph: null, open: null, span: 300, openedAt: 0, closing: false, held: null, selected: null };
const clock = new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit", second: "2-digit" });
const timeText = (seconds) => clock.format(seconds * 1000);

// A cursor steps through actual sample times. A missing paired reading stays missing.
export function historyWindow(series, sensors, end, span) {
  const byTime = new Map();
  const stats = sensors.map((id, index) => {
    let low = null, peak = null, sum = 0, count = 0;
    for (const point of series[id] || []) {
      const [time, reading] = point;
      if (!Number.isFinite(time) || time < end - span || time > end || !Number.isFinite(reading)) continue;
      if (!byTime.has(time)) byTime.set(time, { time, values: Array(sensors.length).fill(null) });
      byTime.get(time).values[index] = reading;
      if (!low || reading < low[1]) low = point;
      if (!peak || reading > peak[1]) peak = point;
      sum += reading;
      count++;
    }
    return { low, peak, count, average: count ? sum / count : null };
  });
  return { end, span, stats, rows: [...byTime.values()].sort((a, b) => a.time - b.time) };
}

function holdWindow(end) {
  F.held = { end, series: Object.fromEntries(F.open.sensors.map((id) =>
    [id, (R.series[id] || []).map(([time, reading]) => [time, reading])])) };
  F.dialog.querySelector(".light-tip").hidden = true;
  F.graph?.hover(null);
}

function selectSample(index) {
  const row = F.open.window.rows[index];
  if (!row) return;
  if (!F.held) holdWindow(F.open.window.end);
  F.selected = row.time;
  F.dialog.querySelector(".focus-copy-fallback").hidden = true;
  render();
}

function snapshotText() {
  const { sensors, window: { end, span, rows, stats } } = F.open;
  const iso = (time) => new Date(time * 1000).toISOString();
  const lines = [`Airglow · ${span / 60} min sensor history`, `${iso(end - span)} – ${iso(end)}`];
  const selected = rows.find((row) => row.time === F.selected);
  sensors.forEach((id, index) => {
    const meta = R.meta[id], { count, low, peak, average } = stats[index];
    lines.push("", sensorName(id), `${count} recorded sample${count === 1 ? "" : "s"}`);
    if (count) {
      lines.push(`Low: ${fmtText(meta, low[1])} at ${iso(low[0])}`, `Average: ${fmtText(meta, average)}`,
        `Peak: ${fmtText(meta, peak[1])} at ${iso(peak[0])}`);
    }
    if (selected) lines.push(`Selected: ${fmtText(meta, selected.values[index])} at ${iso(selected.time)}`);
  });
  return lines.join("\n");
}

async function copySnapshot() {
  const open = F.open, text = snapshotText();
  const fallback = F.dialog.querySelector(".focus-copy-fallback");
  try {
    await navigator.clipboard.writeText(text);
    if (F.open !== open) return;
    fallback.hidden = true;
    toast("Sensor summary copied");
  } catch {
    if (F.open !== open) return;
    fallback.hidden = false;
    fallback.value = text;
    fallback.focus();
    fallback.select();
    toast("Press Ctrl+C to copy the selected summary");
  }
}

function source(meta) {
  return [meta?.source || (meta?.group !== "Quick picks" ? meta?.group : ""), meta?.sensor_name].filter(Boolean).join(" · ");
}

// "Rising 4.0° a minute". A temperature difference converts by scale only: 1 °C of change is 1.8 °F of change.
export function trendText(meta, moving) {
  if (!moving) return "";
  if (moving.state === "steady") return "Steady over the last minute";
  const amount = Math.abs(moving.perMinute);
  const change = meta?.unit === "°C" ? `${(amount * (unitF() ? 1.8 : 1)).toFixed(1)}°` : fmtText(meta, amount);
  return `${moving.state === "rising" ? "Rising" : "Falling"} ${change} a minute`;
}

function build() {
  const dialog = F.dialog = document.createElement("dialog");
  dialog.className = "light-focus"; dialog.id = "light-focus";
  dialog.setAttribute("aria-labelledby", "focus-title");
  document.body.append(dialog);
  dialog.addEventListener("cancel", (e) => { e.preventDefault(); closeFocus(); });
  dialog.addEventListener("click", (e) => {
    const span = e.target.closest("[data-span]");
    if (e.target === dialog || e.target.closest("[data-close]")) return closeFocus();
    if (e.target.closest("[data-settings]")) return closeFocus(F.open?.onSettings);
    if (e.target.closest("[data-copy-summary]")) return void copySnapshot();
    if (e.target.closest("[data-hold]")) {
      if (!F.held) return selectSample(F.open.window.rows.length - 1);
      F.held = null;
      F.selected = null;
      dialog.querySelector(".focus-copy-fallback").hidden = true;
      return render();
    }
    const jump = e.target.closest("[data-jump]");
    if (jump) {
      const report = F.open.window;
      const point = report.stats[0][jump.dataset.jump];
      if (point) {
        selectSample(report.rows.findIndex((row) => row.time === point[0]));
        dialog.querySelector(".focus-cursor").focus({ preventScroll: true });
      }
      return;
    }
    if (!span) return;
    F.span = Number(span.dataset.span);
    dialog.querySelector(".focus-copy-fallback").hidden = true;
    saveSpan(F.span, KEY);
    for (const button of dialog.querySelectorAll("[data-span]")) button.setAttribute("aria-checked", String(button === span));
    render();
  });
  dialog.addEventListener("input", (e) => {
    if (e.target.matches(".focus-cursor")) selectSample(Number(e.target.value));
  });
  dialog.addEventListener("click", (e) => {
    if (!F.held || !e.target.closest(".focus-plot") || !F.graph) return;
    const canvas = F.graph.canvas, box = canvas.getBoundingClientRect();
    if (!box.width || e.clientX < box.left || e.clientX > box.right || e.clientY < box.top || e.clientY > box.bottom) return;
    const { t } = F.graph.probe((e.clientX - box.left) * canvas.clientWidth / box.width);
    const rows = F.open.window.rows;
    if (rows.length) selectSample(rows.reduce((best, row, index) =>
      Math.abs(row.time - t) < Math.abs(rows[best].time - t) ? index : best, 0));
  });
  // A double click on a tile used to open its settings, and still does: the first click only got here first.
  dialog.addEventListener("dblclick", (e) => {
    if (e.target.closest("button, input, textarea")) return;
    if (performance.now() - F.openedAt < 700 && F.open?.onSettings) closeFocus(F.open.onSettings, true);
  });
  dialog.addEventListener("pointermove", scrub);
  // Ctrl+1…7 and links still change the section; the view belongs to the Overview and leaves with it.
  window.addEventListener("hashchange", () => closeFocus(null, true));
  on("state", () => { if (!F.held) render(); });
  on("catalog", render);
}

function scrub(e) {
  const plot = F.dialog?.querySelector(".focus-plot"), tip = plot?.querySelector(".light-tip");
  if (!plot || !F.graph || F.held) return;
  const box = F.graph.canvas.getBoundingClientRect();
  const inside = e && e.pointerType !== "touch" && box.width && e.clientX >= box.left && e.clientX <= box.right && e.clientY >= box.top && e.clientY <= box.bottom;
  const x = inside ? (e.clientX - box.left) * (F.graph.canvas.clientWidth / box.width) : 0;
  const read = inside ? F.graph.probe(x) : null;
  if (!read || read.values.every((v) => v == null)) { F.graph.hover(null); tip.hidden = true; return; }
  F.graph.hover(x);
  const { sensors } = F.open;
  setHTML(tip, `${sensors.map((id, i) => sensors.length > 1
    ? `<span class="tip-${i}">${esc(R.meta[id]?.short || sensorName(id))} <b>${esc(fmtText(R.meta[id], read.values[i]))}</b></span>`
    : `<b>${esc(fmtText(R.meta[id], read.values[i]))}</b>`).join("")}<i>${agoText(read.ago)}</i>`);
  tip.hidden = false;
  tip.style.setProperty("--tx", `${x.toFixed(1)}px`);
  tip.style.setProperty("--ty", "0px");
}

function render() {
  const o = F.open, dialog = F.dialog;
  if (!o || !dialog?.open) return;
  const [id, second] = o.sensors, meta = R.meta[id], held = F.held;
  const now = held?.end ?? Date.now() / 1000, from = now - F.span;
  const history = held?.series ?? R.series, points = history[id] || [];
  const report = o.window = held?.report?.span === F.span ? held.report : historyWindow(history, o.sensors, now, F.span);
  if (held) held.report = report;
  const { rows } = report;
  const selected = held ? rows.find((row) => row.time === F.selected) || rows.at(-1) : null;
  F.selected = selected?.time ?? null;
  const v = held ? selected?.values[0] : value(id), f = fmt(meta, v);
  const limits = thresholds(id, meta), lvl = level(id, meta, v);
  dialog.dataset.level = lvl;
  dialog.querySelector("#focus-title").textContent = o.label || meta?.name || sensorName(id);
  dialog.querySelector(".focus-source").textContent = !held && R.stale ? "Waiting for Airglow to reconnect" : source(meta);
  const number = dialog.querySelector(".focus-read .num");
  if (held) number.textContent = f.num;
  else countTo(number, f.num, { roll: true });
  setHTML(dialog.querySelector(".focus-read .unit"), esc(f.unit));

  const moving = held || R.stale ? null : trendOf(meta, points, now), drift = dialog.querySelector(".focus-trend");
  drift.dataset.trend = moving?.state || "";
  drift.textContent = trendText(meta, moving);

  const tokens = lightTokens(dialog);
  let [lo, hi] = liveRange(meta, points, from, range(id, meta, points));
  const second_ = second ? R.meta[second] : null, shared = second && second_?.unit === meta?.unit;
  const otherPoints = history[second] || [];
  let [lo2, hi2] = second ? liveRange(second_, otherPoints, from, range(second, second_, otherPoints)) : [0, 1];
  if (shared) { lo = lo2 = Math.min(lo, lo2); hi = hi2 = Math.max(hi, hi2); }
  const series = [{ points, lo, hi, color: tokens.ink, thresholds: limits, warm: tokens.warm, hot: tokens.hot, fill: .3, width: 2.3 }];
  if (second) series.push({ points: otherPoints, lo: lo2, hi: hi2, color: tokens.second, dash: [6, 5], fill: .08, width: 2 });
  F.graph?.update({ window: F.span, fixedAt: held?.end, cursor: selected, glow: tokens.glow, curve: tokens.curve, text: tokens.text, track: tokens.track, fade: .5, series,
    guides: [lo, (lo + hi) / 2, hi], marks: limits ? [{ value: limits[0], color: tokens.warm }, { value: limits[1], color: tokens.hot }] : [] });
  dialog.querySelector(".focus-hi").textContent = fmtText(meta, hi);
  dialog.querySelector(".focus-mid").textContent = fmtText(meta, (lo + hi) / 2);
  dialog.querySelector(".focus-lo").textContent = fmtText(meta, lo);
  dialog.querySelector(".focus-then").textContent = held ? timeText(from) : `${F.span / 60} min ago`;
  dialog.querySelector(".focus-now").textContent = held ? timeText(now) : "now";

  const hold = dialog.querySelector("[data-hold]"), cursor = dialog.querySelector(".focus-cursor");
  const index = selected ? rows.indexOf(selected) : -1;
  hold.textContent = held ? "Resume live" : "Pause history";
  hold.disabled = !held && !rows.length;
  cursor.hidden = !held || !selected;
  cursor.max = Math.max(0, rows.length - 1);
  cursor.value = Math.max(0, index);
  if (rows.length > 1) rangeFill(cursor);
  else cursor.style.setProperty("--fill", "0%");
  if (selected) cursor.setAttribute("aria-valuetext", [timeText(selected.time),
    ...o.sensors.map((sid, i) => `${sensorName(sid)} ${fmtText(R.meta[sid], selected.values[i])}`)].join(", "));
  const note = selected ? `Sample ${index + 1} of ${rows.length} · ${timeText(selected.time)}`
    : rows.length ? `${rows.length} sample${rows.length === 1 ? "" : "s"} in ${F.span / 60} min` : "No samples in this window";
  dialog.querySelector(".focus-sample-info").textContent = note;
  dialog.querySelector("[data-copy-summary]").disabled = !rows.length;

  const { low, average, peak } = report.stats[0];
  const stats = [
    { label: "Low", reading: low?.[1], jump: "low" },
    { label: "Average", reading: average },
    { label: "Peak", reading: peak?.[1], jump: "peak" },
  ];
  if (limits) stats.push({ label: "Airglow warning", reading: limits[0], tone: "warm" },
    { label: "Airglow high", reading: limits[1], tone: "hot" });
  if (second) stats.push({ label: second_?.name || sensorName(second), reading: held ? selected?.values[1] : value(second), tone: "second" });
  renderStats(dialog.querySelector(".focus-stats"), stats, meta, second_);
}

function renderStats(list, stats, primaryMeta, secondMeta) {
  // Change only values as readings arrive, so the jump buttons keep keyboard focus.
  setHTML(list, stats.map(({ label, tone, jump }) => {
    const button = jump ? `<button type="button" data-jump="${jump}"><span></span><span aria-hidden="true">↗</span></button>` : "<span></span>";
    return `<div${tone ? ` data-tone="${tone}"` : ""}><dt>${esc(label)}</dt><dd>${button}</dd></div>`;
  }).join(""));
  list.querySelectorAll("dd").forEach((el, index) => {
    const { reading, tone, jump } = stats[index], text = fmtText(tone === "second" ? secondMeta : primaryMeta, reading);
    el.querySelector("span").textContent = text;
    if (jump) {
      const button = el.querySelector("button");
      button.disabled = !Number.isFinite(reading);
      button.title = `Inspect the ${jump === "low" ? "lowest" : "highest"} sample`;
      button.setAttribute("aria-label", `${button.title}, ${text}`);
    }
  });
}

export function openFocus({ sensors, label = "", from = null, color = "", onSettings = null }) {
  if (!sensors?.length || !globalThis.HTMLDialogElement) return;
  if (!F.dialog) build();
  const dialog = F.dialog;
  if (dialog.open) return;
  F.open = { sensors, label, onSettings };
  F.held = null;
  F.selected = null;
  F.span = savedSpan(KEY, 300);
  F.closing = false;
  // The view keeps the colour of whatever opened it: a tile's accent, or the pin's line in the pulse.
  const inherited = from ? getComputedStyle(from) : null;
  for (const name of ["--light-ink", "--light-2", "--light-warm", "--light-hot"]) {
    const given = name === "--light-ink" && color ? color : inherited?.getPropertyValue(name).trim();
    if (given && rgb(given)[3] > 0) dialog.style.setProperty(name, paint(rgb(given)));
    else dialog.style.removeProperty(name);
  }
  dialog.innerHTML = `<article class="focus-panel">
    <header class="focus-head"><div class="focus-title"><h2 id="focus-title"></h2><p class="focus-source"></p></div>
      <div class="seg focus-span" role="radiogroup" aria-label="Time shown">${spanButtons(F.span)}</div>
      ${onSettings ? `<button type="button" class="btn quiet" data-settings>Tile settings</button>` : ""}
      <button type="button" class="icon-btn" data-close aria-label="Close" title="Close (Esc)"><svg aria-hidden="true"><use href="#i-close"/></svg></button></header>
    <div class="focus-read"><span class="num">–</span><span class="unit"></span><span class="focus-trend" role="status"></span></div>
    <div class="focus-plot"><div class="light-host"></div><output class="light-tip" aria-hidden="true" hidden></output>
      <span class="focus-axis focus-hi"></span><span class="focus-axis focus-mid"></span><span class="focus-axis focus-lo"></span>
      <span class="focus-time focus-then"></span><span class="focus-time focus-now">now</span></div>
    <div class="focus-inspection">
      <span class="focus-sample-info"></span>
      <input class="focus-cursor" type="range" min="0" step="1" aria-label="Inspect recorded sample" hidden>
      <div class="btn-row"><button type="button" class="btn quiet" data-hold>Pause history</button>
        <button type="button" class="btn quiet" data-copy-summary><svg aria-hidden="true"><use href="#i-copy"/></svg>Copy summary</button></div>
      <textarea class="focus-copy-fallback" aria-label="Sensor summary to copy" rows="3" readonly hidden></textarea>
    </div>
    <dl class="focus-stats"></dl></article>`;
  dialog.showModal();
  F.openedAt = performance.now();
  dialog.querySelector(".focus-plot").addEventListener("pointerleave", () => scrub(null));
  try { F.graph = createLightGraph(dialog.querySelector(".light-host"), { glide: 20, pad: [14, 18, 2, 0] }); } catch { F.graph = null; }
  render();
  dialog.querySelector("[data-close]").focus({ preventScroll: true });

  const panel = dialog.querySelector(".focus-panel");
  if (reduced() || !from?.getBoundingClientRect || !panel.animate) return;
  const a = from.getBoundingClientRect(), b = panel.getBoundingClientRect(), zoom = uiScale();
  if (!a.width || !b.width) return;
  panel.animate([{ transformOrigin: "top left", opacity: .35, transform: `translate(${(a.left - b.left) / zoom}px, ${(a.top - b.top) / zoom}px) scale(${a.width / b.width}, ${a.height / b.height})` },
    { transformOrigin: "top left", opacity: 1, transform: "none" }], { duration: 480, easing: SPRING });
  [...panel.children].forEach((child, i) => child.animate([{ opacity: 0, transform: "translateY(10px)" }, { opacity: 1, transform: "none" }],
    { duration: 380, delay: 110 + i * 45, easing: SPRING, fill: "backwards" }));
}

export function closeFocus(then = null, instant = false) {
  const dialog = F.dialog;
  if (!dialog?.open || F.closing) return;
  const finish = () => {
    F.closing = false;
    F.graph?.dispose(); F.graph = null;
    F.open = null;
    F.held = null;
    F.selected = null;
    dialog.close();
    dialog.replaceChildren();
    if (typeof then === "function") then();
  };
  const panel = dialog.querySelector(".focus-panel");
  if (instant || reduced() || !panel?.animate) return finish();
  F.closing = true;
  dialog.dataset.closing = "true";
  panel.animate([{ opacity: 1, transform: "none" }, { opacity: 0, transform: "translateY(8px) scale(.985)" }], { duration: 170, easing: "ease-in" })
    .finished.then(() => { delete dialog.dataset.closing; finish(); }, finish);
}
