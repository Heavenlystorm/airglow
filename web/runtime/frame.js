// Inside the sandboxed effect frame: builds the effect's worker and relays messages between it and the page that
// embeds this frame (the Studio, a gallery card, or the effect host that drives the real lights).
//
// page -> frame: load {runtime, layersLib, kind, code, layers, defaults, params, points}, tick {time, dt, audio,
//                backdrop}, params {params}, points {points}, stop
// frame -> page: ready, frame {colors, ms, backdrop?}, log {text}, error {message, line}, stopped {reason}
"use strict";
(() => {
  const HANG_MS = 3000;          // no answer to a frame for this long: the effect is stuck
  const SLOW_MS = 20, SLOW_RUN = 30;
  let worker = null, prefixLines = 0, pending = 0, slow = 0, stopped = false;

  const up = (msg, transfer) => parent.postMessage(msg, "*", transfer || []);

  function stop(reason) {
    if (stopped) return;
    stopped = true;
    worker?.terminate();
    worker = null;
    up({ type: "stopped", reason });
  }

  function load(m) {
    // the layer renderer comes with every effect: builder effects are drawn by it, code effects may use it too
    const body = m.kind === "layers"
      ? `airglow.effect(AirglowLayers.effect(${JSON.stringify(m.layers)}, ${JSON.stringify(m.defaults || {})}));\n`
      : String(m.code || "");
    const prefix = `${m.runtime}\n${m.layersLib}\n`;
    prefixLines = prefix.split("\n").length - 1;
    const url = URL.createObjectURL(new Blob([prefix, body], { type: "text/javascript" }));
    worker = new Worker(url);
    URL.revokeObjectURL(url);
    worker.onerror = (e) => {
      e.preventDefault();
      const line = e.lineno ? Math.max(1, e.lineno - prefixLines) : null;
      up({ type: "error", message: String(e.message || "The effect stopped with an error").replace(/^Uncaught /, ""), line });
      stop("error");
    };
    worker.onmessage = (e) => {
      const d = e.data;
      if (d.type === "frame") {
        pending = 0;
        slow = d.ms > SLOW_MS ? slow + 1 : 0;
        if (slow >= SLOW_RUN) return stop("slow");
        up(d, [d.colors.buffer, ...(d.backdrop ? [d.backdrop] : [])]);
      } else if (d.type === "error") {
        const m2 = /blob:[^\s)]+?:(\d+):\d+/.exec(d.stack || "");          // the top frame inside the effect's script
        const n = m2 && +m2[1];
        up({ type: "error", message: d.message, line: n ? Math.max(1, n - prefixLines) : null });
        stop("error");
      } else if (d.type === "log" || d.type === "ready") {
        up(d);
      }
    };
    worker.postMessage({ type: "start", params: m.params || {}, points: m.points || [] });
  }

  window.addEventListener("message", (e) => {
    if (e.source !== parent || !e.data || typeof e.data.type !== "string") return;
    const m = e.data;
    if (m.type === "load") {
      if (worker) return;
      try { load(m); } catch (err) { up({ type: "error", message: String(err.message || err) }); stop("error"); }
      return;
    }
    if (!worker) return;
    if (m.type === "tick") {
      const now = performance.now();
      if (pending) {                               // still busy with the last one: skip, unless it's stuck
        if (now - pending > HANG_MS) stop("hung");
        return;
      }
      pending = now;
      worker.postMessage({ type: "tick", time: +m.time, dt: +m.dt, audio: m.audio || null, backdrop: !!m.backdrop });
    } else if (m.type === "params" || m.type === "points" || m.type === "layers") {
      worker.postMessage(m);
    } else if (m.type === "stop") {
      worker.postMessage({ type: "stop" });
      stop("stopped");
    }
  });
  up({ type: "hello" });
})();
