// The effect runtime, inside a Web Worker. The frame page (frame.js) builds the worker from this file, the layer
// renderer and the effect's own script. Effects get no DOM, no network (the page's CSP blocks every connection)
// and nothing of Airglow's; they only ever answer "which colour is each LED".
//
// An effect calls airglow.effect({...}) with:
//   init(ctx)                         once, before the first frame
//   render(ctx, frame)                draw on ctx.canvas (320 x 200) every frame; LEDs take the colour under them
//   point(led, frame) -> [r, g, b]    or answer per LED instead (0..255); led = {x, y (0..1), px, py, area, index}
//   frame(frame)                      optional, once per frame before point()
//   onParamChange(id, value, ctx)     optional
//   dispose(ctx)                      optional
// frame = {time, dt (seconds), audio: {level, bass, beat, bands: Float32Array(64)} or null}
// Everything but `airglow` stays inside this function, so an effect's own names never clash with the runtime's.
(() => {
"use strict";
const W = 320, H = 200;
const airglow = (() => {
  let def = null;
  return { effect(d) { def = d; }, get _def() { return def; } };
})();
self.airglow = airglow;

const R = {
  def: null, canvas: null, ctx: null, sampler: null, points: null, n: 0, logs: 0, logReset: 0,
};

function post(msg, transfer) { self.postMessage(msg, transfer || []); }

function log(...args) {
  const now = Date.now();
  if (now - R.logReset > 1000) { R.logReset = now; R.logs = 0; }
  if (++R.logs > 20) return;                                     // at most 20 lines a second
  post({ type: "log", text: args.map((a) => (typeof a === "string" ? a : (() => { try { return JSON.stringify(a); } catch { return String(a); } })())).join(" ").slice(0, 500) });
}

function setPoints(list) {
  // list: [[px, py, area], ...] in canvas pixels
  R.n = list.length;
  R.points = list.map(([px, py, area], index) => ({ x: px / W, y: py / H, px, py, area: area || null, index }));
}

function ensureCanvas() {
  if (R.canvas) return;
  R.canvas = new OffscreenCanvas(W, H);
  // Remember what the effect draws with. Asking for "2d" here instead would lock the canvas to 2D before an effect
  // that wants WebGL has had its first frame.
  try {
    const get = R.canvas.getContext.bind(R.canvas);
    R.canvas.getContext = (type, ...rest) => { R.drawing = type; return get(type, ...rest); };
  } catch { /* can't watch it: the colour read-back goes the long way round, which still works */ }
  R.ctx = { canvas: R.canvas, width: W, height: H, params: R.ctx?.params || {}, log };
}

function start(params, points) {
  R.def = airglow._def;
  if (!R.def || (typeof R.def.render !== "function" && typeof R.def.point !== "function")) {
    throw new Error("The effect needs to call airglow.effect({ render(ctx, frame) { ... } }) or give point(led, frame).");
  }
  ensureCanvas();
  R.ctx.params = { ...params };
  setPoints(points || []);
  R.def.init?.(R.ctx);
}

// colours at the LEDs: from the canvas (render) or asked per LED (point)
const READ = { direct: undefined };    // the effect canvas's own 2D context, when that's what it draws with
function sample(frame) {
  const out = new Uint8Array(R.n * 3);
  if (R.def.render) {
    R.def.render(R.ctx, frame);
    if (READ.direct === undefined) READ.direct = R.drawing === "2d" ? R.canvas.getContext("2d") : null;
    let img;
    if (READ.direct) {
      img = READ.direct.getImageData(0, 0, W, H).data;                // no copy through another canvas
    } else {
      if (!R.sampler) R.sampler = new OffscreenCanvas(W, H).getContext("2d", { willReadFrequently: true });
      R.sampler.clearRect(0, 0, W, H);
      R.sampler.drawImage(R.canvas, 0, 0);                          // works for 2d and webgl2 canvases alike
      img = R.sampler.getImageData(0, 0, W, H).data;
    }
    for (let i = 0; i < R.n; i++) {                                 // a 3 x 3 average: smoother on thin details
      const p = R.points[i], cx = Math.round(p.px), cy = Math.round(p.py);
      let r = 0, g = 0, b = 0, k = 0;
      for (let dy = -1; dy <= 1; dy++) for (let dx = -1; dx <= 1; dx++) {
        const x = cx + dx, y = cy + dy;
        if (x < 0 || y < 0 || x >= W || y >= H) continue;
        const o = (y * W + x) * 4, a = img[o + 3] / 255;
        r += img[o] * a; g += img[o + 1] * a; b += img[o + 2] * a; k++;
      }
      out[i * 3] = r / k; out[i * 3 + 1] = g / k; out[i * 3 + 2] = b / k;
    }
  } else {
    R.def.frame?.(frame);
    for (let i = 0; i < R.n; i++) {
      const c = R.def.point(R.points[i], frame) || [0, 0, 0];
      out[i * 3] = c[0]; out[i * 3 + 1] = c[1]; out[i * 3 + 2] = c[2];
    }
  }
  return out;
}

// what the Studio shows behind the LEDs: the canvas, or a coarse grid of point colours
async function backdrop(frame) {
  if (R.def.render) return createImageBitmap(R.canvas);
  const gw = 80, gh = 50, img = new ImageData(gw, gh);
  for (let y = 0; y < gh; y++) for (let x = 0; x < gw; x++) {
    const c = R.def.point({ x: (x + 0.5) / gw, y: (y + 0.5) / gh, px: (x + 0.5) * W / gw, py: (y + 0.5) * H / gh, area: null, index: 100000 + y * gw + x }, frame) || [0, 0, 0];
    const o = (y * gw + x) * 4;
    img.data[o] = c[0]; img.data[o + 1] = c[1]; img.data[o + 2] = c[2]; img.data[o + 3] = 255;
  }
  return createImageBitmap(img);
}

self.onmessage = async (e) => {
  const m = e.data;
  try {
    if (m.type === "start") {
      start(m.params, m.points);
      post({ type: "ready" });
    } else if (m.type === "points") {
      setPoints(m.points);
    } else if (m.type === "layers" && typeof AirglowLayers !== "undefined") {
      // the Studio's builder changed a layer: swap the stack in place (no reload, so sliders feel live)
      R.def = AirglowLayers.effect(m.layers, m.defaults || {});
      R.def.init?.(R.ctx);
    } else if (m.type === "params") {
      for (const [id, v] of Object.entries(m.params)) {
        if (R.ctx.params[id] === v) continue;
        R.ctx.params[id] = v;
        R.def.onParamChange?.(id, v, R.ctx);
      }
    } else if (m.type === "tick") {
      const audio = m.audio ? { ...m.audio, bands: new Float32Array(m.audio.bands || 64) } : null;
      const frame = { time: m.time, dt: m.dt, audio };
      const t0 = performance.now();
      const colors = sample(frame);
      const ms = performance.now() - t0;
      const msg = { type: "frame", colors, ms };
      const transfer = [colors.buffer];
      if (m.backdrop) { msg.backdrop = await backdrop(frame); transfer.push(msg.backdrop); }
      post(msg, transfer);
    } else if (m.type === "stop") {
      R.def?.dispose?.(R.ctx);
    }
  } catch (err) {
    post({ type: "error", message: String(err?.message || err), stack: String(err?.stack || "") });
  }
};
})();
