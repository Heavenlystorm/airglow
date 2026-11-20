// The detail view a reading opens into: the figure large, its history as one wide line, the time span to choose,
// and what it did in that span. It grows out of the tile (or pin) it was opened from and keeps updating live.

import { esc, on, setHTML, unitF } from "./core.js";
import { R, fmt, fmtText, level, range, sensorName, thresholds, value } from "./readings.js";
import { createLightGraph, lightTokens, paint, rgb } from "./light-graph.js";
import { countTo, reduced, uiScale } from "./motion.js";
import { liveRange, trendOf } from "./overview-sensors.js";
import { agoText, savedSpan, saveSpan, spanButtons } from "./overview-pulse.js";

const KEY = "airglow.focus.span", SPRING = "cubic-bezier(.16, 1, .3, 1)";
const F = { dialog: null, graph: null, open: null, span: 300, openedAt: 0, closing: false };

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
    if (!span) return;
    F.span = Number(span.dataset.span);
    saveSpan(F.span, KEY);
    for (const button of dialog.querySelectorAll("[data-span]")) button.setAttribute("aria-checked", String(button === span));
    render();
  });
  // A double click on a tile used to open its settings, and still does: the first click only got here first.
  dialog.addEventListener("dblclick", () => {
    if (performance.now() - F.openedAt < 700 && F.open?.onSettings) closeFocus(F.open.onSettings, true);
  });
  dialog.addEventListener("pointermove", scrub);
  // Ctrl+1…7 and links still change the section; the view belongs to the Overview and leaves with it.
  window.addEventListener("hashchange", () => closeFocus(null, true));
  on("state", render);
  on("catalog", render);
}

function scrub(e) {
  const plot = F.dialog?.querySelector(".focus-plot"), tip = plot?.querySelector(".light-tip");
  if (!plot || !F.graph) return;
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
  const [id, second] = o.sensors, meta = R.meta[id], v = value(id), f = fmt(meta, v);
  const now = Date.now() / 1000, from = now - F.span, points = R.series[id] || [];
  const seen = points.filter((p) => p[0] >= from && Number.isFinite(p[1])).map((p) => p[1]);
  const limits = thresholds(id, meta), lvl = level(id, meta, v);
  dialog.dataset.level = lvl;
  dialog.querySelector("#focus-title").textContent = o.label || meta?.name || sensorName(id);
  dialog.querySelector(".focus-source").textContent = R.stale ? "Waiting for Airglow to reconnect" : source(meta);
  countTo(dialog.querySelector(".focus-read .num"), f.num, { roll: true });
  setHTML(dialog.querySelector(".focus-read .unit"), esc(f.unit));

  const moving = R.stale ? null : trendOf(meta, points, now), drift = dialog.querySelector(".focus-trend");
  drift.dataset.trend = moving?.state || "";
  drift.textContent = trendText(meta, moving);

  const tokens = lightTokens(dialog);
  let [lo, hi] = liveRange(meta, points, from, range(id, meta));
  const second_ = second ? R.meta[second] : null, shared = second && second_?.unit === meta?.unit;
  let [lo2, hi2] = second ? liveRange(second_, R.series[second], from, range(second, second_)) : [0, 1];
  if (shared) { lo = lo2 = Math.min(lo, lo2); hi = hi2 = Math.max(hi, hi2); }
  const series = [{ points, lo, hi, color: tokens.ink, thresholds: limits, warm: tokens.warm, hot: tokens.hot, fill: .3, width: 2.3 }];
  if (second) series.push({ points: R.series[second] || [], lo: lo2, hi: hi2, color: tokens.second, dash: [6, 5], fill: .08, width: 2 });
  F.graph?.update({ window: F.span, glow: tokens.glow, curve: tokens.curve, text: tokens.text, track: tokens.track, fade: .5, series,
    guides: [lo, (lo + hi) / 2, hi], marks: limits ? [{ value: limits[0], color: tokens.warm }, { value: limits[1], color: tokens.hot }] : [] });
  dialog.querySelector(".focus-hi").textContent = fmtText(meta, hi);
  dialog.querySelector(".focus-mid").textContent = fmtText(meta, (lo + hi) / 2);
  dialog.querySelector(".focus-lo").textContent = fmtText(meta, lo);
  dialog.querySelector(".focus-then").textContent = `${F.span / 60} min ago`;

  const stat = (label, text, tone = "") => `<div${tone ? ` data-tone="${tone}"` : ""}><dt>${label}</dt><dd>${esc(text)}</dd></div>`;
  const mean = seen.length ? seen.reduce((a, b) => a + b, 0) / seen.length : null;
  setHTML(dialog.querySelector(".focus-stats"), `${stat("Low", seen.length ? fmtText(meta, Math.min(...seen)) : "–")}
    ${stat("Average", seen.length ? fmtText(meta, mean) : "–")}${stat("Peak", seen.length ? fmtText(meta, Math.max(...seen)) : "–")}
    ${limits ? stat("Airglow warning", fmtText(meta, limits[0]), "warm") + stat("Airglow high", fmtText(meta, limits[1]), "hot") : ""}
    ${second ? stat(second_?.name || sensorName(second), fmtText(second_, value(second)), "second") : ""}`);
}

export function openFocus({ sensors, label = "", from = null, color = "", onSettings = null }) {
  if (!sensors?.length || !globalThis.HTMLDialogElement) return;
  if (!F.dialog) build();
  const dialog = F.dialog;
  if (dialog.open) return;
  F.open = { sensors, label, onSettings };
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
    <dl class="focus-stats"></dl></article>`;
  dialog.showModal();
  F.openedAt = performance.now();
  dialog.querySelector(".focus-plot").addEventListener("pointerleave", () => scrub(null));
  try { F.graph = createLightGraph(dialog.querySelector(".light-host"), { fps: 60, pad: [14, 18, 2, 0] }); } catch { F.graph = null; }
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
