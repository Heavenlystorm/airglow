// Runs a Studio effect safely from any page: the Studio preview, gallery cards, and the effect host that drives the
// real lights. The effect lives in a sandboxed iframe (web/runtime/frame.html) with a worker inside; this side
// only ever receives LED colours (and, for the Studio, a picture of the canvas).

const W = 320, H = 200, UNIT = 200;          // stage canvas units (1.6 x 1.0) -> effect canvas pixels
let assets = null;

// worker.js, layers.js and the layer defaults, fetched once and handed to each frame (it can't fetch anything)
export function runtimeAssets() {
  assets ||= Promise.all([
    fetch("/runtime/worker.js").then((r) => r.text()),
    fetch("/runtime/layers.js").then((r) => r.text()),
    fetch("/runtime/layers.json").then((r) => r.json()),
  ]).then(([runtime, layersLib, layerInfo]) => {
    const defaults = {};
    for (const [id, t] of Object.entries(layerInfo.types)) defaults[id] = Object.fromEntries(t.params.map((p) => [p.id, p.default]));
    return { runtime, layersLib, layerInfo, defaults };
  });
  return assets;
}

// LED positions of the given zones (from /api/zones) in effect canvas pixels, with the area each zone sits in.
// Returns {points, slices: [{key, from, count}]} so colours can be cut back into per-zone frames.
export function ledPoints(zones) {
  const points = [], slices = [];
  for (const z of zones) {
    slices.push({ key: z.key, from: points.length, count: z.positions.length });
    for (const [x, y] of z.positions) points.push([x * UNIT, y * UNIT, z.spot || null]);
  }
  return { points, slices };
}

// colours for all points -> {zone key: "rrggbb..."} (what the stage's paint functions take)
export function zoneFrames(colors, slices) {
  const out = {};
  for (const s of slices) {
    let hex = "";
    for (let i = s.from * 3, end = (s.from + s.count) * 3; i < end; i++) hex += (colors[i] | 0).toString(16).padStart(2, "0");
    out[s.key] = hex;
  }
  return out;
}

// "Test beat": a steady 120 BPM kick and a sweep through the bands, so music layers can be built in silence
export function testAudio(time) {
  const beatLen = 0.5, phase = time % beatLen;
  const kick = Math.exp(-phase * 9);
  const bands = new Float32Array(64);
  const sweep = (time * 0.35) % 1;
  for (let i = 0; i < 64; i++) {
    const x = i / 63;
    bands[i] = Math.min(1, 0.85 * kick * Math.max(0, 1 - x * 2.2) + 0.9 * Math.exp(-((x - sweep) ** 2) / 0.004) + 0.08 * Math.random());
  }
  return { level: 0.25 + 0.5 * kick, bass: kick, beat: phase < 1 / 30, bands: Array.from(bands) };
}

export class EffectRunner {
  // handlers: {onFrame(colors, ms), onBackdrop(bitmap), onLog(text), onError({message, line}), onStopped(reason), onReady()}
  constructor(handlers = {}) {
    this.h = handlers;
    this.ready = false;
    this.busy = 0;                    // when the frame being drawn was asked for
    this.frame = document.createElement("iframe");
    this.frame.setAttribute("sandbox", "allow-scripts");
    this.frame.setAttribute("aria-hidden", "true");
    this.frame.tabIndex = -1;
    this.frame.style.cssText = "position:absolute;width:0;height:0;border:0;visibility:hidden";
    this._onMessage = (e) => this._message(e);
    window.addEventListener("message", this._onMessage);
    this._hello = new Promise((res) => { this._helloRes = res; });
  }

  // effect: {kind: "layers"|"code", layers?, code?, params: {id: value}}; points from ledPoints()
  async load(effect, points) {
    const a = await runtimeAssets();
    this.frame.src = "/runtime/frame.html";
    document.body.appendChild(this.frame);
    await this._hello;
    this._post({ type: "load", runtime: a.runtime, layersLib: a.layersLib, defaults: a.defaults, kind: effect.kind,
                 code: effect.code || "", layers: effect.layers || [], params: effect.params || {}, points });
  }

  tick(time, dt, audio = null, backdrop = false) {
    if (!this.ready) return false;
    // still waiting for the last frame: skip, but keep knocking now and then so the frame can tell a stuck effect
    if (this.busy && performance.now() - this.busy < 400) return false;
    this.busy = performance.now();
    this._post({ type: "tick", time, dt, audio, backdrop });
    return true;
  }

  setParams(params) { this._post({ type: "params", params }); }
  async setLayers(layers) { this._post({ type: "layers", layers, defaults: (await runtimeAssets()).defaults }); }
  setPoints(points) { this._post({ type: "points", points }); }

  destroy() {
    window.removeEventListener("message", this._onMessage);
    try { this._post({ type: "stop" }); } catch { /* already gone */ }
    this.frame.remove();
    this.ready = false;
  }

  _post(msg) { this.frame.contentWindow?.postMessage(msg, "*"); }

  _message(e) {
    if (e.source !== this.frame.contentWindow || !e.data || typeof e.data.type !== "string") return;
    const d = e.data;
    if (d.type === "hello") this._helloRes();
    else if (d.type === "ready") { this.ready = true; this.h.onReady?.(); }
    else if (d.type === "frame" && d.colors instanceof Uint8Array) {
      this.busy = 0;
      this.h.onFrame?.(d.colors, +d.ms || 0);
      if (d.backdrop instanceof ImageBitmap) this.h.onBackdrop?.(d.backdrop);
    } else if (d.type === "log") this.h.onLog?.(String(d.text).slice(0, 500));
    else if (d.type === "error") this.h.onError?.({ message: String(d.message).slice(0, 300), line: Number.isInteger(d.line) ? d.line : null });
    else if (d.type === "stopped") { this.ready = false; this.h.onStopped?.(String(d.reason)); }
  }
}

export const CANVAS = { W, H };
