// The effect host page (web/runtime/host.html): runs Studio effects for the real lights. Airglow sends what to run
// and a frame clock over /api/fx/host/stream; colours go back in one POST per frame. Everything is driven by those
// events rather than timers, since this window is hidden and hidden pages get their timers slowed down.

import { EffectRunner } from "./fxrun.js";

const token = new URLSearchParams(location.hash.slice(1)).get("token") || "";
const runners = new Map();                 // instance id -> EffectRunner
const sent = new Map();                    // instance id -> the colours last sent
const ALIVE_MS = 1000;                     // post at least this often: Airglow restarts a host that goes quiet
let out = fresh(), waiting = 0, lastPost = 0;

function fresh() { return { frames: new Map(), problems: {}, logs: [] }; }

function same(a, b) {
  if (!b || a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
  return true;
}

// frames as one binary body: per instance, its id's length (1 byte), the id, the colours' length (2 bytes, little
// endian) and the colours. No JSON or base64 30 times a second.
function pack(frames) {
  let size = 0;
  for (const [id, c] of frames) size += 3 + id.length + c.length;
  const body = new Uint8Array(size);
  let o = 0;
  for (const [id, c] of frames) {
    body[o++] = id.length;
    for (let i = 0; i < id.length; i++) body[o++] = id.charCodeAt(i);
    body[o++] = c.length & 0xff; body[o++] = c.length >> 8;
    body.set(c, o); o += c.length;
  }
  return body;
}

function post(type, body) {
  lastPost = performance.now();
  // read every reply: 30 unread responses a second pile up in WebView2 until the host is killed for memory
  fetch("/api/fx/host/frames", { method: "POST", headers: { "Content-Type": type, "X-Airglow": "1", "X-Airglow-Token": token }, body })
    .then((r) => r.arrayBuffer())
    .catch(() => { /* Airglow is restarting; it starts this host again if needed */ });
}

function flush() {
  const { frames, problems, logs } = out;
  out = fresh();
  for (const [id, c] of frames) {                // an effect that didn't change (a still frame, a slow fade) isn't sent
    if (same(c, sent.get(id))) frames.delete(id);
    else sent.set(id, c);
  }
  if (Object.keys(problems).length || logs.length) post("application/json", JSON.stringify({ token, problems, logs }));
  if (frames.size) post("application/octet-stream", pack(frames));
  else if (runners.size && performance.now() - lastPost > ALIVE_MS) post("application/octet-stream", new Uint8Array(0));
}

function unload(id) {
  runners.get(id)?.destroy();
  runners.delete(id);
  sent.delete(id);
}

function load(e) {
  unload(e.id);
  const r = new EffectRunner({
    onFrame: (colors) => { out.frames.set(e.id, colors); if (--waiting <= 0) flush(); },
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
on("points", (e) => { sent.delete(e.id); runners.get(e.id)?.setPoints(e.points); });
on("params", (e) => runners.get(e.id)?.setParams(e.params));
on("tick", (t) => {
  flush();                                   // whatever arrived for the last frame
  waiting = 0;
  for (const r of runners.values()) if (r.tick(t.time, t.dt, t.audio)) waiting++;
});
