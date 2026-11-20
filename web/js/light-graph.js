// Living-light graphs: the canvas renderers behind the Overview's lines and gauges.
//
// Readings arrive every two seconds. A graph draws a little behind the clock (LAG), so the head of each line is
// always between two real readings and glides along the curve instead of jumping when the next one lands.
// Colour, glow strength and curve style come from CSS custom properties (see overview-light.css), so every interface
// design keeps its own character. Forced-colours mode falls back to the SVG paths the tiles still carry.

import { reduced } from "./motion.js";

export const LAG = 2.4;            // seconds behind the clock: a little more than one sampling period
const MAX_GAP = 10;                // a longer silence is an outage and breaks the line (as in readings.js)
const TAU = Math.PI * 2;
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
const lerp = (a, b, f) => a + (b - a) * f;
const easeOut = (k) => 1 - (1 - k) ** 3;

// ------------------------------------------------------------ geometry (pure; tools/test-light-graph.mjs)
// Runs of consecutive readings inside [from, to]. One reading on each side of the window is kept, so a line enters
// and leaves at the edge instead of starting inside the plot.
export function splitRuns(points, from, to, maxGap = MAX_GAP) {
  const runs = [];
  let run = null;
  for (let i = 0; i < (points?.length || 0); i++) {
    const [t, v] = points[i], next = points[i + 1];
    if (next && next[0] < from) continue;
    if (!Number.isFinite(v)) { run = null; continue; }
    if (run && t - run.at(-1)[0] > maxGap) run = null;
    if (!run) runs.push(run = []);
    run.push(points[i]);
    if (t > to) break;
  }
  return runs;
}

// Fritsch–Carlson tangents: a smooth curve through every reading that never overshoots one.
export function tangents(xs, ys) {
  const n = xs.length, m = new Array(n).fill(0);
  if (n < 2) return m;
  const h = [], d = [];
  for (let i = 0; i < n - 1; i++) {
    h[i] = xs[i + 1] - xs[i];
    d[i] = h[i] > 0 ? (ys[i + 1] - ys[i]) / h[i] : 0;
  }
  m[0] = d[0]; m[n - 1] = d[n - 2];
  for (let i = 1; i < n - 1; i++) {
    if (d[i - 1] * d[i] <= 0) continue;
    const a = 2 * h[i] + h[i - 1], b = h[i] + 2 * h[i - 1];
    m[i] = (a + b) / (a / d[i - 1] + b / d[i]);
  }
  return m;
}

// The value a run shows at time t, on the same curve the renderer draws. null outside the run.
export function valueAt(run, t, curve = "smooth") {
  if (!run?.length || t < run[0][0] || t > run.at(-1)[0]) return null;
  let i = 0;
  while (i < run.length - 2 && run[i + 1][0] <= t) i++;
  if (run.length === 1) return run[0][1];
  const [t0, v0] = run[i], [t1, v1] = run[i + 1], h = t1 - t0;
  if (h <= 0) return v1;
  const u = clamp((t - t0) / h, 0, 1);
  if (curve === "step") return u >= 1 ? v1 : v0;
  if (curve === "linear") return lerp(v0, v1, u);
  const m = tangents(run.map((p) => p[0]), run.map((p) => p[1]));
  const u2 = u * u, u3 = u2 * u;
  return (2 * u3 - 3 * u2 + 1) * v0 + (u3 - 2 * u2 + u) * h * m[i] + (-2 * u3 + 3 * u2) * v1 + (u3 - u2) * h * m[i + 1];
}

// Trace a run up to xEnd. Returns where the line stops: the head.
function trace(path, xs, ys, curve, xEnd) {
  const m = curve === "smooth" ? tangents(xs, ys) : null;
  let hx = xs[0], hy = ys[0];
  path.moveTo(hx, hy);
  for (let i = 0; i < xs.length - 1; i++) {
    const x0 = xs[i], y0 = ys[i], x1 = xs[i + 1], y1 = ys[i + 1], h = x1 - x0;
    if (x0 >= xEnd) break;
    if (h <= 0) continue;
    const u = Math.min(1, (xEnd - x0) / h);
    if (curve === "step") {
      path.lineTo(x0 + h * u, y0);
      if (u === 1) path.lineTo(x1, y1);
      hx = x0 + h * u; hy = u === 1 ? y1 : y0;
    } else if (curve === "linear") {
      hx = x0 + h * u; hy = lerp(y0, y1, u);
      path.lineTo(hx, hy);
    } else {
      let ax = x0 + h / 3, ay = y0 + m[i] * h / 3, bx = x1 - h / 3, by = y1 - m[i + 1] * h / 3;
      hx = x1; hy = y1;
      if (u < 1) {                 // cut the segment at the head (de Casteljau)
        const px = lerp(x0, ax, u), py = lerp(y0, ay, u), qx = lerp(ax, bx, u), qy = lerp(ay, by, u);
        const rx = lerp(bx, x1, u), ry = lerp(by, y1, u);
        const sx = lerp(px, qx, u), sy = lerp(py, qy, u), tx = lerp(qx, rx, u), ty = lerp(qy, ry, u);
        hx = lerp(sx, tx, u); hy = lerp(sy, ty, u);
        ax = px; ay = py; bx = sx; by = sy;
      }
      path.bezierCurveTo(ax, ay, bx, by, hx, hy);
    }
  }
  return [hx, hy];
}

// ------------------------------------------------------------ colour
const colors = new Map();
let probe = null;
// Any CSS colour as [r, g, b, a]. Hex and rgb() are parsed directly; anything else is painted and read back.
export function rgb(css) {
  css = String(css || "").trim();
  if (!css) return [0, 0, 0, 0];
  let c = colors.get(css);
  if (c) return c;
  const hex = /^#([\da-f]{3}|[\da-f]{6})$/i.exec(css), fn = /^rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)(?:[,/\s]+([\d.]+))?\s*\)$/.exec(css);
  if (hex) {
    const s = hex[1].length === 3 ? [...hex[1]].map((x) => x + x).join("") : hex[1];
    c = [0, 2, 4].map((i) => parseInt(s.slice(i, i + 2), 16)).concat(1);
  } else if (fn) {
    c = [+fn[1], +fn[2], +fn[3], fn[4] == null ? 1 : +fn[4]];
  } else {
    probe ||= Object.assign(document.createElement("canvas"), { width: 1, height: 1 }).getContext("2d", { willReadFrequently: true });
    probe.clearRect(0, 0, 1, 1);
    probe.fillStyle = "#000"; probe.fillStyle = css;
    probe.fillRect(0, 0, 1, 1);
    const d = probe.getImageData(0, 0, 1, 1).data;
    c = [d[0], d[1], d[2], d[3] / 255];
  }
  if (colors.size > 256) colors.clear();
  colors.set(css, c);
  return c;
}
export const paint = (c, a = 1) => `rgba(${Math.round(c[0])},${Math.round(c[1])},${Math.round(c[2])},${(a * (c[3] ?? 1)).toFixed(3)})`;
export const blend = (a, b, f) => [lerp(a[0], b[0], f), lerp(a[1], b[1], f), lerp(a[2], b[2], f), lerp(a[3] ?? 1, b[3] ?? 1, f)];
const WHITE = [255, 255, 255, 1];

// The colours and character a design gives its graphs, read from the element that hosts one.
export function lightTokens(el) {
  const css = getComputedStyle(el), v = (name) => css.getPropertyValue(name).trim();
  const glow = parseFloat(v("--light-glow")), curve = v("--light-curve").replace(/["']/g, "");
  return {
    ink: rgb(v("--light-ink")), second: rgb(v("--light-2")), third: rgb(v("--light-3")), fourth: rgb(v("--light-4")),
    warm: rgb(v("--light-warm")), hot: rgb(v("--light-hot")), track: rgb(v("--light-track")), text: rgb(v("--light-text")),
    glow: Number.isFinite(glow) ? clamp(glow, 0, 1.5) : 1, curve: ["step", "linear"].includes(curve) ? curve : "smooth",
  };
}

// ------------------------------------------------------------ one shared clock
// Every canvas answers frame(ts) with the milliseconds until it next needs drawing (0: next frame, -1: at rest).
// The clock sleeps until the soonest of them, so a quiet Overview does not run a callback on every display refresh.
// All canvases draw on one shared beat. Each canvas that redraws makes the whole window composite again, so eight
// canvases drawing on eight different refreshes of a 240 Hz display cost eight times what one beat does.
export const BEAT = 1000 / 30;
const GLIDE_MIN = 2;                                // frames a second for the slowest line at rest
const active = new Set();
let raf = 0, timer = 0, beat = -Infinity, refresh = 1000 / 60, chained = -1;
// Wake just before the refresh the beat falls on. An animation-frame callback is a frame of work for the page even
// when nothing is drawn, so the timer aims for one callback a beat; the display's refresh interval is learned from
// callbacks that do follow each other.
function sleep(ms, ts) {
  const margin = clamp(refresh * .7, 2.5, 11);
  if (ms <= margin + 1.5) { chained = ts; raf = requestAnimationFrame(tick); }
  else timer = setTimeout(() => { timer = 0; raf = requestAnimationFrame(tick); }, ms - margin);
}
// Now and then a glint runs along every line at once.
let glintAt = -Infinity, nextGlint = 0;
const GLINT = 1500, RIPPLE = .5;                    // milliseconds a glint lasts; seconds the ring at the head does
function tick(ts) {
  raf = 0;
  if (chained >= 0 && ts - chained > 1) refresh = Math.min(refresh, ts - chained);
  chained = -1;
  if (ts - beat < BEAT - 3) return sleep(BEAT - (ts - beat), ts);
  beat = ts;
  if (ts >= nextGlint) { if (nextGlint) glintAt = ts; nextGlint = ts + 24000 + Math.random() * 20000; }
  let next = Infinity;
  for (const item of active) {
    const wait = item.frame(ts);
    if (wait >= 0 && wait < next) next = wait;
  }
  if (next === Infinity || document.hidden) return;
  sleep(Math.max(next, BEAT), ts);
}
function wake() {
  if (document.hidden) return;
  if (timer) { clearTimeout(timer); timer = 0; }
  if (!raf) raf = requestAnimationFrame(tick);
}
// While tiles are being rearranged the graphs hold still and redraw only for new readings, so moving
// a tile keeps the whole frame budget.
let calm = false;
export function setLightCalm(on) {
  if (calm === !!on) return;
  calm = !!on;
  for (const item of active) item.dirty = true;
  wake();
}
globalThis.document?.addEventListener?.("visibilitychange", wake);
globalThis.document?.addEventListener?.("appearancechange", () => { for (const item of active) item.dirty = true; wake(); });

// A canvas that fills its host at the display's real pixel size (interface zoom included) and knows whether it is
// on screen. `draw(ctx, width, height, seconds)` works in CSS pixels.
function surface(host, className) {
  const canvas = document.createElement("canvas");
  canvas.className = className;
  canvas.setAttribute("aria-hidden", "true");
  host.append(canvas);
  const s = { canvas, ctx: canvas.getContext("2d"), w: 0, h: 0, scale: 1, visible: false, dirty: true, onChange: null };
  const resize = new ResizeObserver(([entry]) => {
    const box = entry.devicePixelContentBoxSize?.[0], css = entry.contentRect;
    const pw = Math.round(box ? box.inlineSize : css.width * devicePixelRatio), ph = Math.round(box ? box.blockSize : css.height * devicePixelRatio);
    s.w = canvas.clientWidth; s.h = canvas.clientHeight;
    if (!pw || !ph || !s.w) return;
    // Very large tiles on 4K displays do not need more than ~3.4 million pixels each.
    const limit = Math.min(1, Math.sqrt(3.4e6 / (pw * ph)));
    canvas.width = Math.round(pw * limit); canvas.height = Math.round(ph * limit);
    s.scale = canvas.width / s.w;
    // setting a canvas size clears it: draw again now, in the same frame, or the tile shows a blank graph until the
    // next beat
    s.dirty = true; s.onResize?.(); s.onChange?.();
  });
  try { resize.observe(canvas, { box: "device-pixel-content-box" }); } catch { resize.observe(canvas); }
  const seen = new IntersectionObserver(([entry]) => { s.visible = entry.isIntersecting; s.dirty = true; s.onChange?.(); });
  seen.observe(canvas);
  s.dispose = () => { resize.disconnect(); seen.disconnect(); canvas.remove(); };
  return s;
}

// ------------------------------------------------------------ line graphs
/**
 * createLightGraph(host, { fps, glide, pad }) → { update(model), hover(x), probe(x), dispose() }
 * model: { window, series: [{ points, lo, hi, color, thresholds: [warm, hot]?, warm, hot, dash, fill }],
 *          glow, curve, fade, guides: [value], marks: [{ value, color }], track }
 */
export function createLightGraph(host, { fps = 30, glide = 6, pad = [10, 12, 2, 0], className = "light-canvas" } = {}) {
  const s = surface(host, className);
  const g = {
    model: null, series: [], reveal: 0, revealed: false, last: 0, hoverX: null, ripple: -1, rippleKey: 0,
    shimmer: -1, dirty: true, busyUntil: 0, clock: () => Date.now() / 1000,
  };
  const interval = 1000 / fps;
  const plot = () => ({ x: pad[3], y: pad[0], w: Math.max(1, s.w - pad[1] - pad[3]), h: Math.max(1, s.h - pad[0] - pad[2]) });
  const still = () => reduced();

  function draw(dt) {
    const { ctx } = s, m = g.model;
    ctx.setTransform(s.scale, 0, 0, s.scale, 0, 0);
    ctx.clearRect(0, 0, s.w, s.h);
    if (!m) return;
    const R = plot(), now = g.clock(), t1 = still() ? now : now - LAG, t0 = t1 - m.window;
    const glow = m.glow ?? 1, curve = m.curve || "smooth";
    const px = (t) => R.x + (t - t0) / m.window * R.w;
    const heads = [];
    let any = false;

    ctx.save();
    ctx.beginPath();
    ctx.rect(0, 0, R.x + R.w * easeOut(g.reveal) + (g.reveal >= 1 ? pad[1] : 0), s.h);
    ctx.clip();
    ctx.lineJoin = "round"; ctx.lineCap = "round";
    g.series.forEach((state, index) => {
      const def = m.series[index];
      // ranges and colours ease toward their targets, so a rescale or a warning never snaps
      const k = dt == null ? 1 : 1 - Math.exp(-dt / .22);
      state.lo = lerp(state.lo, def.lo, k); state.hi = lerp(state.hi, def.hi, k);
      state.color = blend(state.color, def.color, k);
      const span = state.hi - state.lo || 1;
      const py = (v) => R.y + R.h - (clamp(v, state.lo, state.hi) - state.lo) / span * R.h;
      let stroke = paint(state.color);
      if (def.thresholds && def.warm && def.hot) {
        const [warmAt, hotAt] = def.thresholds, at = (v) => clamp(1 - (v - state.lo) / span, 0, 1);
        const gradient = ctx.createLinearGradient(0, R.y, 0, R.y + R.h);
        const calm = at(warmAt - (hotAt - warmAt) * .6);
        for (const [stop, color] of [[at(hotAt), def.hot], [at(warmAt), def.warm], [calm, state.color]]) gradient.addColorStop(stop, paint(color));
        if (calm < 1) gradient.addColorStop(1, paint(state.color));
        stroke = gradient;
      }
      const runs = splitRuns(def.points, t0, t1);
      runs.forEach((run, r) => {
        const xs = run.map((p) => px(p[0])), ys = run.map((p) => py(p[1]));
        const path = new Path2D(), [hx, hy] = trace(path, xs, ys, curve, px(t1));
        if (hx < R.x - 1) return;
        any = true;
        if (run.length === 1 || hx - xs[0] < .5) {           // a lone reading is a point of light, not a line
          ctx.fillStyle = paint(state.color); ctx.beginPath(); ctx.arc(hx, hy, 1.6, 0, TAU); ctx.fill();
        } else {
          if (def.fill !== false) {
            const area = new Path2D(path);
            area.lineTo(hx, R.y + R.h + pad[2]); area.lineTo(Math.max(xs[0], R.x), R.y + R.h + pad[2]); area.closePath();
            const wash = ctx.createLinearGradient(0, R.y, 0, R.y + R.h + pad[2]);
            const strength = (def.fill ?? .3) * (index ? .45 : 1);
            wash.addColorStop(0, paint(state.color, strength)); wash.addColorStop(1, paint(state.color, 0));
            ctx.fillStyle = wash; ctx.fill(area);
          }
          if (glow > 0) {
            ctx.strokeStyle = stroke;
            for (const [width, alpha] of [[11, .05], [6.5, .1], [3.6, .2]]) {
              ctx.lineWidth = width; ctx.globalAlpha = alpha * glow; ctx.stroke(path);
            }
            ctx.globalAlpha = 1;
          }
          ctx.setLineDash(def.dash || []);
          ctx.lineWidth = def.width || 1.9; ctx.strokeStyle = stroke; ctx.stroke(path);
          ctx.setLineDash([]);
          if (glow > 0) {                                      // a paler core, like a lit tube
            ctx.lineWidth = .8; ctx.globalAlpha = .34 * glow; ctx.strokeStyle = "#fff"; ctx.stroke(path); ctx.globalAlpha = 1;
          }
          if (glow > 0 && g.shimmer >= 0) {
            const centre = R.x + R.w * (.1 + 1.05 * g.shimmer), band = Math.max(46, R.w * .16);
            const sweep = ctx.createLinearGradient(centre - band, 0, centre + band, 0);
            sweep.addColorStop(0, "rgba(255,255,255,0)"); sweep.addColorStop(.5, `rgba(255,255,255,${.5 * glow})`); sweep.addColorStop(1, "rgba(255,255,255,0)");
            ctx.globalCompositeOperation = "lighter"; ctx.lineWidth = (def.width || 1.9) + .8; ctx.strokeStyle = sweep; ctx.stroke(path);
            ctx.globalCompositeOperation = "source-over";
          }
        }
        // only the newest run has a live head, and only while its readings are current
        if (r === runs.length - 1 && t1 - run.at(-1)[0] < MAX_GAP) heads.push({ x: hx, y: hy, color: def.thresholds ? headColor(def, state, run, t1, curve) : state.color, index });
      });
    });
    // older readings fade toward the left edge: the line is a trail
    if (any && (m.fade ?? .3) < 1) {
      const trail = ctx.createLinearGradient(R.x, 0, R.x + R.w, 0);
      trail.addColorStop(0, `rgba(0,0,0,${m.fade ?? .3})`); trail.addColorStop(.55, "#000");
      ctx.globalCompositeOperation = "destination-in"; ctx.fillStyle = trail; ctx.fillRect(0, 0, s.w, s.h);
      ctx.globalCompositeOperation = "source-over";
    }
    ctx.restore();

    // guides and warning marks sit behind the light
    ctx.globalCompositeOperation = "destination-over";
    const first = g.series[0];
    if (first) {
      const span = first.hi - first.lo || 1, py = (v) => Math.round(R.y + R.h - (v - first.lo) / span * R.h) + .5;
      for (const mark of m.marks || []) {
        if (mark.value <= first.lo || mark.value >= first.hi) continue;
        ctx.strokeStyle = paint(mark.color, .5); ctx.lineWidth = 1; ctx.setLineDash([2, 5]);
        ctx.beginPath(); ctx.moveTo(R.x, py(mark.value)); ctx.lineTo(R.x + R.w, py(mark.value)); ctx.stroke();
      }
      ctx.setLineDash([]);
      for (const value of m.guides || []) {
        ctx.strokeStyle = paint(m.track || [255, 255, 255, .1], m.track ? 1 : .1); ctx.lineWidth = 1;
        ctx.beginPath(); ctx.moveTo(R.x, py(value)); ctx.lineTo(R.x + R.w, py(value)); ctx.stroke();
      }
    }
    ctx.globalCompositeOperation = "source-over";

    for (const head of m.head === false ? [] : heads) {
      if (glow > 0) {
        const halo = ctx.createRadialGradient(head.x, head.y, 0, head.x, head.y, 12);
        halo.addColorStop(0, paint(head.color, .55 * glow)); halo.addColorStop(1, paint(head.color, 0));
        ctx.fillStyle = halo; ctx.beginPath(); ctx.arc(head.x, head.y, 12, 0, TAU); ctx.fill();
        if (g.ripple >= 0 && head.index === 0) {
          ctx.strokeStyle = paint(head.color, (1 - g.ripple) ** 2 * .42 * glow); ctx.lineWidth = 1;
          ctx.beginPath(); ctx.arc(head.x, head.y, 3 + 11 * easeOut(g.ripple), 0, TAU); ctx.stroke();
        }
      }
      ctx.fillStyle = paint(glow > 0 ? blend(head.color, WHITE, .7) : head.color);
      ctx.beginPath(); ctx.arc(head.x, head.y, glow > 0 ? 2.6 : 2.2, 0, TAU); ctx.fill();
    }

    if (g.hoverX != null && any) {
      const x = clamp(g.hoverX, R.x, R.x + R.w), read = probeAt(x);
      const line = ctx.createLinearGradient(0, R.y, 0, R.y + R.h);
      const tint = m.text || [255, 255, 255, 1];
      line.addColorStop(0, paint(tint, 0)); line.addColorStop(.25, paint(tint, .5)); line.addColorStop(1, paint(tint, .16));
      ctx.strokeStyle = line; ctx.lineWidth = 1;
      ctx.beginPath(); ctx.moveTo(Math.round(x) + .5, R.y - 4); ctx.lineTo(Math.round(x) + .5, R.y + R.h); ctx.stroke();
      read.values.forEach((value, index) => {
        const state = g.series[index];
        if (value == null || !state) return;
        const y = R.y + R.h - (clamp(value, state.lo, state.hi) - state.lo) / (state.hi - state.lo || 1) * R.h;
        ctx.fillStyle = paint(m.ground || [0, 0, 0, 1]); ctx.beginPath(); ctx.arc(x, y, 4.6, 0, TAU); ctx.fill();
        ctx.strokeStyle = paint(state.color); ctx.lineWidth = 1.8; ctx.beginPath(); ctx.arc(x, y, 3.6, 0, TAU); ctx.stroke();
      });
    }
    // an attribute write restyles the tile, so only when the number changes
    if (s.canvas.dataset.series !== String(heads.length)) s.canvas.dataset.series = String(heads.length);
  }

  // The head takes the colour of the level it is at, so a line that climbs past a warning arrives lit in it.
  function headColor(def, state, run, t1, curve) {
    const value = valueAt(run, Math.min(t1, run.at(-1)[0]), curve) ?? run.at(-1)[1];
    return value >= def.thresholds[1] ? def.hot : value >= def.thresholds[0] ? def.warm : state.color;
  }

  function probeAt(x) {
    const m = g.model, R = plot(), now = g.clock(), t1 = still() ? now : now - LAG;
    const t = t1 - m.window + (clamp(x, R.x, R.x + R.w) - R.x) / R.w * m.window;
    return { t, ago: Math.max(0, now - t), values: m.series.map((def) => {
      for (const run of splitRuns(def.points, t1 - m.window, t1)) {
        const value = valueAt(run, t, m.curve || "smooth");
        if (value != null) return value;
      }
      return null;
    }) };
  }

  g.frame = (ts) => {
    if (!g.model || !s.visible || !s.w) return -1;
    if (still() || calm) {
      if (g.dirty || s.dirty) { g.reveal = 1; g.ripple = g.shimmer = -1; g.last = 0; draw(null); g.dirty = s.dirty = false; }
      return -1;
    }
    // Every frame drawn makes the window composite again, which costs far more than the drawing. So: full rate only
    // while the eye is on it (arriving, the ring at the head, a glint, the pointer), half rate while a range or
    // colour eases, and at rest only the slow glide: two steps for each pixel the head travels, at most `glide`
    // frames a second. A ten-minute line that moves a third of a pixel a second draws twice a second.
    // A graph with nothing to show yet has nothing to move.
    if (!g.revealed && g.hoverX == null) {
      if (g.dirty || s.dirty) { g.last = 0; draw(null); g.dirty = s.dirty = false; }
      return -1;
    }
    g.shimmer = ts - glintAt < GLINT ? (ts - glintAt) / GLINT : -1;
    const pace = () => (g.reveal < 1 || g.ripple >= 0 || g.shimmer >= 0 || g.hoverX != null) ? interval
      : ts < g.busyUntil ? Math.max(interval, BEAT * 2)
      : 1000 / clamp(plot().w / g.model.window * 2, GLIDE_MIN, glide);
    const since = ts - g.last;
    // due within half a beat counts as due: the clock only wakes on beats, and waking again for the rest is a frame
    if (since < pace() - BEAT / 2 && !s.dirty && !g.dirty) return pace() - since;
    const dt = g.last ? Math.min(.25, (ts - g.last) / 1000) : null;
    g.last = ts;
    if (g.reveal < 1) g.reveal = Math.min(1, g.reveal + (dt ?? 0) / .9);
    if (g.ripple >= 0) { g.ripple += (dt ?? 0) / RIPPLE; if (g.ripple >= 1) g.ripple = -1; }
    draw(dt);
    g.dirty = s.dirty = false;
    return pace();
  };
  s.onChange = wake;
  s.onResize = () => g.frame(performance.now());
  active.add(g);

  return {
    canvas: s.canvas,
    update(model) {
      const fresh = !g.model;
      g.model = model;
      // keep each line's eased range and colour; a changed number of lines starts over
      g.series = model.series.map((def, i) => (!fresh && g.series[i]) || { lo: def.lo, hi: def.hi, color: def.color });
      const has = model.series.some((def) => def.points?.some((p) => Number.isFinite(p[1])));
      if (has && !g.revealed) { g.revealed = true; g.reveal = 0; }
      if (!has) { g.revealed = false; g.reveal = 0; }
      const newest = model.series[0]?.points?.at(-1)?.[0] || 0;
      if (g.rippleKey && newest > g.rippleKey && !still()) g.ripple = 0;
      g.rippleKey = newest;
      g.dirty = true;
      // a range or colour that changed eases for a moment; an unchanged one needs no extra frames
      const eases = model.series.some((def, i) => {
        const state = g.series[i], span = Math.abs(def.hi - def.lo) || 1;
        return Math.abs(state.lo - def.lo) > span * .002 || Math.abs(state.hi - def.hi) > span * .002
          || def.color.some((v, c) => Math.abs(v - state.color[c]) > (c < 3 ? 2 : .02));
      });
      if (eases) g.busyUntil = performance.now() + 900;
      wake();
    },
    hover(x) { g.hoverX = x; g.dirty = true; if (still()) { s.dirty = true; } wake(); },
    probe: (x) => g.model ? probeAt(x) : null,
    plot,
    setClock(clock) { g.clock = clock; },
    dispose() { active.delete(g); s.dispose(); },
  };
}

// ------------------------------------------------------------ gauges
/**
 * createLightGauge(host) → { update({ fraction, range: [lo, hi]?, color, track, glow, width, missing }), dispose() }
 * A 270° arc on the tile's 120-unit gauge grid: a bright head, a dim tail and the last ten minutes as a thin outer arc.
 */
export function createLightGauge(host, { bleed = 14, className = "light-canvas light-gauge" } = {}) {
  const s = surface(host, className);
  const g = { model: null, shown: 0, speed: 0, color: null, last: 0, dirty: true, lo: 0, hi: 0, entered: false };
  const START = Math.PI * .75, SWEEP = Math.PI * 1.5;

  function draw() {
    const { ctx } = s, m = g.model;
    ctx.setTransform(s.scale, 0, 0, s.scale, 0, 0);
    ctx.clearRect(0, 0, s.w, s.h);
    if (!m) return;
    const size = Math.min(s.w, s.h) - bleed * 2, unit = size / 120, cx = s.w / 2, cy = s.h / 2;
    if (size <= 0) return;
    const r = 48 * unit, width = Math.max(2, (m.width || 7) * unit), glow = m.glow ?? 1;
    const k = clamp(g.shown, 0, 1), end = START + SWEEP * k, color = g.color || m.color;
    ctx.lineCap = m.cap || "round";
    ctx.strokeStyle = paint(m.track); ctx.lineWidth = width;
    ctx.beginPath(); ctx.arc(cx, cy, r, START, START + SWEEP); ctx.stroke();
    // five quiet marks at each quarter of the scale
    ctx.fillStyle = paint(m.text || m.track, .5);
    for (let i = 0; i <= 4; i++) {
      const a = START + SWEEP * i / 4, rr = r - width / 2 - 5 * unit;
      ctx.beginPath(); ctx.arc(cx + Math.cos(a) * rr, cy + Math.sin(a) * rr, Math.max(.8, 1.1 * unit), 0, TAU); ctx.fill();
    }
    if (m.range && g.hi > g.lo + .004) {
      ctx.strokeStyle = paint(color, .5); ctx.lineWidth = Math.max(1, 1.6 * unit); ctx.lineCap = "round";
      ctx.beginPath(); ctx.arc(cx, cy, r + width / 2 + 4 * unit, START + SWEEP * g.lo, START + SWEEP * g.hi); ctx.stroke();
      ctx.lineCap = m.cap || "round";
    }
    if (m.missing || k <= .002) return;
    let stroke = paint(color);
    if (glow > 0 && ctx.createConicGradient) {
      // the tail starts dim and brightens toward the head
      const sweep = ctx.createConicGradient(START - .06, cx, cy), reach = Math.max(.02, SWEEP * k / TAU);
      sweep.addColorStop(0, paint(color, .38)); sweep.addColorStop(Math.min(.98, reach), paint(color, 1));
      sweep.addColorStop(Math.min(.99, reach + .01), paint(color, 1)); sweep.addColorStop(1, paint(color, .38));
      stroke = sweep;
    }
    if (glow > 0) {
      ctx.strokeStyle = paint(color, 1);
      for (const [extra, alpha] of [[14, .06], [7, .12]]) {
        ctx.lineWidth = width + extra * unit; ctx.globalAlpha = alpha * glow;
        ctx.beginPath(); ctx.arc(cx, cy, r, Math.max(START, end - .9), end); ctx.stroke();
      }
      ctx.globalAlpha = 1;
    }
    ctx.strokeStyle = stroke; ctx.lineWidth = width;
    ctx.beginPath(); ctx.arc(cx, cy, r, START, end); ctx.stroke();
    if (glow > 0) {
      const hx = cx + Math.cos(end) * r, hy = cy + Math.sin(end) * r, reach = width * 1.9;
      const halo = ctx.createRadialGradient(hx, hy, 0, hx, hy, reach);
      halo.addColorStop(0, paint(blend(color, WHITE, .5), .7 * glow)); halo.addColorStop(1, paint(color, 0));
      ctx.fillStyle = halo; ctx.beginPath(); ctx.arc(hx, hy, reach, 0, TAU); ctx.fill();
      ctx.fillStyle = paint(blend(color, WHITE, .78)); ctx.beginPath(); ctx.arc(hx, hy, width * .26, 0, TAU); ctx.fill();
    }
    if (s.canvas.dataset.fraction !== k.toFixed(2)) s.canvas.dataset.fraction = k.toFixed(2);
  }

  g.frame = (ts) => {
    const m = g.model;
    if (!m || !s.visible || !s.w) return -1;
    const target = m.missing ? 0 : clamp(m.fraction, 0, 1);
    if (reduced() || calm) {
      if (g.dirty || s.dirty) { g.last = 0; g.shown = target; g.speed = 0; g.color = m.color; [g.lo, g.hi] = m.range || [0, 0]; draw(); g.dirty = s.dirty = false; }
      return -1;
    }
    const dt = g.last ? Math.min(.05, (ts - g.last) / 1000) : .016;
    g.last = ts;
    // a lightly underdamped spring: the needle arrives with a small, quick settle
    g.speed += ((target - g.shown) * 120 - g.speed * 17) * dt;
    g.shown += g.speed * dt;
    const ease = 1 - Math.exp(-dt / .2);
    g.color = g.color ? blend(g.color, m.color, ease) : m.color;
    const [lo, hi] = m.range || [0, 0];
    g.lo = lerp(g.lo, lo, ease); g.hi = lerp(g.hi, hi, ease);
    // at rest once nothing is a visible step away: a thousandth of the arc is a third of a pixel on a large gauge
    const settled = Math.abs(target - g.shown) < .001 && Math.abs(g.speed) < .004 && Math.abs(g.lo - lo) < .003 && Math.abs(g.hi - hi) < .003
      && g.color.every((v, i) => Math.abs(v - m.color[i]) < (i < 3 ? 2 : .02));
    if (settled) { g.shown = target; g.speed = 0; g.color = m.color; g.lo = lo; g.hi = hi; }
    draw();
    g.dirty = s.dirty = false;
    if (settled) g.last = 0;
    return settled ? -1 : 0;
  };
  s.onChange = wake;
  s.onResize = () => g.frame(performance.now());
  active.add(g);

  return {
    canvas: s.canvas,
    update(model) { g.model = model; g.dirty = true; wake(); },
    dispose() { active.delete(g); s.dispose(); },
  };
}
