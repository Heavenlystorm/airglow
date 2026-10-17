// The effect host page (web/runtime/host.html): runs Studio effects for the real lights. Airglow sends what to run
// and a frame clock over /api/fx/host/stream; colours go back in one POST per frame. Everything is driven by those
// events rather than timers, since this window is hidden and hidden pages get their timers slowed down.

import { EffectRunner } from "./fxrun.js";

const token = new URLSearchParams(location.hash.slice(1)).get("token") || "";
const runners = new Map();                 // instance id -> EffectRunner
let out = fresh(), waiting = 0;

function fresh() { return { frames: {}, problems: {}, logs: [] }; }

function b64(bytes) {
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

function flush() {
  if (!Object.keys(out.frames).length && !Object.keys(out.problems).length && !out.logs.length) return;
  const body = JSON.stringify({ token, ...out });
  out = fresh();
  // read every reply: 30 unread responses a second pile up in WebView2 until the host is killed for memory
  fetch("/api/fx/host/frames", { method: "POST", headers: { "Content-Type": "application/json", "X-Airglow": "1" }, body })
    .then((r) => r.arrayBuffer())
    .catch(() => { /* Airglow is restarting; it starts this host again if needed */ });
}

function unload(id) {
  runners.get(id)?.destroy();
  runners.delete(id);
}

function load(e) {
  unload(e.id);
  const r = new EffectRunner({
    onFrame: (colors) => { out.frames[e.id] = b64(colors); if (--waiting <= 0) flush(); },
    onError: (err) => { out.problems[e.id] = { reason: "error", message: err.message, line: err.line }; },
    onStopped: (why) => {
      if (why === "stopped") return;
      out.problems[e.id] ||= { reason: why };
      runners.delete(e.id);
      flush();
    },
    onLog: (text) => { if (out.logs.length < 20) out.logs.push(text); },
  });
  runners.set(e.id, r);
  r.load({ kind: e.kind, code: e.code, layers: e.layers, params: e.params }, e.points);
}

const es = new EventSource(`/api/fx/host/stream?token=${encodeURIComponent(token)}`);
const on = (type, fn) => es.addEventListener(type, (ev) => { try { fn(JSON.parse(ev.data)); } catch (err) { console.error(err); } });
on("load", load);
on("unload", (e) => unload(e.id));
on("points", (e) => runners.get(e.id)?.setPoints(e.points));
on("params", (e) => runners.get(e.id)?.setParams(e.params));
on("tick", (t) => {
  flush();                                   // whatever arrived for the last frame
  waiting = 0;
  for (const r of runners.values()) if (r.tick(t.time, t.dt, t.audio)) waiting++;
});
