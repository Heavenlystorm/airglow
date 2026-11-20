// The Overview's pulse: the pinned readings as one wide line of light beside the workspace title. It shows a short
// span by default, so the lines visibly flow; the span is a per-PC preference, not part of the saved workspace.

import { esc, setHTML } from "./core.js";
import { R, ensureHistory, fmt, fmtText, level, range, sensorName, value } from "./readings.js";
import { createLightGraph, lightTokens, paint } from "./light-graph.js";
import { countTo } from "./motion.js";
import { liveRange } from "./overview-sensors.js";

export const SPANS = [[60, "1 min"], [300, "5 min"], [600, "10 min"]];
const KEY = "airglow.pulse.span";
const P = { el: null, graph: null, key: null, pins: [], span: 60, open: null };

export function savedSpan(key = KEY, fallback = 60) {
  try {
    const stored = Number(localStorage.getItem(key));
    return SPANS.some(([seconds]) => seconds === stored) ? stored : fallback;
  } catch { return fallback; }                       // storage off: the default span
}
export function saveSpan(seconds, key = KEY) {
  try { localStorage.setItem(key, String(seconds)); } catch { /* storage off: the choice lasts until the window closes */ }
}
export const spanButtons = (current) => SPANS.map(([seconds, label]) =>
  `<button type="button" role="radio" data-span="${seconds}" aria-checked="${seconds === current}">${label}</button>`).join("");
export const agoText = (seconds) => seconds < 4 ? "now" : seconds < 90 ? `${Math.round(seconds)} s ago` : `${Math.round(seconds / 60)} min ago`;

export function initPulse({ after, open }) {
  if (!after || P.el) return;
  P.open = open;
  P.span = savedSpan();
  const el = P.el = document.createElement("section");
  el.className = "ov-pulse"; el.id = "ov-pulse";
  el.setAttribute("aria-label", "Pinned readings");
  el.innerHTML = `<div class="pulse-pins"></div>
    <div class="pulse-plot"><div class="light-host"></div><output class="light-tip" aria-hidden="true" hidden></output>
      <p class="pulse-empty">Pin a reading from a tile's settings and it flows here.</p></div>
    <div class="seg pulse-span" role="radiogroup" aria-label="Time shown in the pulse">${spanButtons(P.span)}</div>`;
  after.after(el);
  try { P.graph = createLightGraph(el.querySelector(".light-host"), { pad: [12, 16, 2, 0] }); } catch { P.graph = null; }

  el.addEventListener("click", (e) => {
    const span = e.target.closest("[data-span]"), pin = e.target.closest("[data-pin]");
    if (span) {
      P.span = Number(span.dataset.span);
      saveSpan(P.span);
      for (const button of el.querySelectorAll("[data-span]")) button.setAttribute("aria-checked", String(button === span));
      draw();
    } else if (pin) P.open?.(pin.dataset.pin, pin, pin.style.getPropertyValue("--pin"));
  });
  const plot = el.querySelector(".pulse-plot"), tip = plot.querySelector(".light-tip");
  const leave = () => { P.graph?.hover(null); tip.hidden = true; };
  plot.addEventListener("pointerleave", leave);
  plot.addEventListener("pointermove", (e) => {
    const box = P.graph?.canvas.getBoundingClientRect();
    if (!box?.width || e.pointerType === "touch") return leave();
    const x = (e.clientX - box.left) * (P.graph.canvas.clientWidth / box.width), read = P.graph.probe(x);
    if (!read || read.values.every((v) => v == null)) return leave();
    P.graph.hover(x);
    setHTML(tip, `${P.pins.map((id, i) => `<span class="tip-${i}">${esc(R.meta[id]?.short || sensorName(id))} <b>${esc(fmtText(R.meta[id], read.values[i]))}</b></span>`).join("")}<i>${agoText(read.ago)}</i>`);
    tip.hidden = false;
    tip.style.setProperty("--tx", `${x.toFixed(1)}px`);
    tip.style.setProperty("--ty", "0px");
  });
}

// Readings in the same unit share one scale, so two temperatures can be compared; different units each fill the band.
function scales(pins, from) {
  const byUnit = new Map();
  for (const id of pins) {
    const meta = R.meta[id], unit = meta?.unit || id;
    const [lo, hi] = liveRange(meta, R.series[id], from, range(id, meta)), shared = byUnit.get(unit);
    byUnit.set(unit, shared ? [Math.min(shared[0], lo), Math.max(shared[1], hi)] : [lo, hi]);
  }
  return (id) => byUnit.get(R.meta[id]?.unit || id);
}

function draw() {
  const el = P.el, pins = P.pins;
  const tokens = lightTokens(el), palette = [tokens.ink, tokens.second, tokens.third, tokens.fourth];
  const scale = scales(pins, Date.now() / 1000 - P.span);
  for (const button of el.querySelectorAll("[data-pin]")) {
    const id = button.dataset.pin, meta = R.meta[id], v = value(id), f = fmt(meta, v);
    button.style.setProperty("--pin", paint(palette[pins.indexOf(id) % palette.length]));
    button.dataset.level = level(id, meta, v);
    countTo(button.querySelector(".num"), f.num, { roll: true });
    setHTML(button.querySelector(".unit"), esc(f.unit));
  }
  P.graph?.update({ window: P.span, glow: tokens.glow, curve: tokens.curve, text: tokens.text, fade: .12,
    series: pins.map((id, i) => {
      const [lo, hi] = scale(id);
      return { points: R.series[id] || [], lo, hi, color: palette[i % palette.length], fill: i ? .1 : .22, width: 2.1 };
    }) });
}

export function renderPulse(pins, { editing = false } = {}) {
  const el = P.el;
  if (!el) return;
  el.hidden = editing;                    // Customize needs the row for its own instructions
  if (editing) return;
  const key = `${pins.join("|")}|${R.loaded ? 1 : 0}`;
  if (P.key !== key) {
    P.key = key; P.pins = [...pins];
    ensureHistory(pins).catch(() => {});
    setHTML(el.querySelector(".pulse-pins"), pins.map((id) => {
      const name = R.meta[id]?.short || sensorName(id);
      return `<button type="button" class="pulse-pin" data-pin="${esc(id)}" title="${esc(R.meta[id]?.name || name)}: open detail"><i aria-hidden="true"></i><span class="pulse-name">${esc(name)}</span><b class="num">–</b><span class="unit"></span></button>`;
    }).join(""));
    el.dataset.empty = String(!pins.length);
  }
  draw();
}
