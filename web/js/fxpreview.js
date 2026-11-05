// Screen previews exchange colours with a separate memory-watched process; effect code never runs in this window.
import { post } from "./core.js";

export class PreviewRunner {
  constructor(handlers = {}) {
    this.h = handlers;
    this.ready = false;
    this.pending = {};
    this.busy = false;
  }

  async load(effect, points) {
    this.effect = structuredClone(effect);
    this.points = structuredClone(points);
    try {
      const { id } = await post("/api/studio/preview/load", { effect, points });
      this.id = id;
      if (this.destroyed) { this.destroy(); return; }
      this.ready = true;
      this.h.onReady?.();
    } catch (error) { this.failed(error); }
  }

  tick(time, dt, audio = null, backdrop = false) {
    if (!this.ready || this.busy) return false;
    this.busy = true;
    const updates = this.pending;
    this.pending = {};
    this.draw({ id: this.id, time, dt, audio, backdrop, updates });
    return true;
  }

  async draw(body) {
    const started = performance.now();
    let restored = false;
    try {
      let frame;
      do {
        frame = await post("/api/studio/preview/tick", body);
        if (this.destroyed) return;
        if (frame.problem) {
          const problem = frame.problem;
          if (problem.reason === "stopped" && !restored) {
            // Hidden or paused previews let their lease expire; restore them when frames are wanted again.
            restored = true;
            await this.load(this.effect, this.points);
            if (!this.ready || this.destroyed) return;
            body.id = this.id; body.updates = {};
            continue;
          }
          this.h.onError?.({ message: problem.message || problem.text || problem.reason, line: problem.line });
          this.destroy();
          this.h.onStopped?.(problem.reason);
          return;
        }
        if (frame.colors !== null) break;
        if (performance.now() - started > 15000) throw new Error("The preview process didn't start");
        body.updates = {};
        await new Promise(resolve => { this.retry = setTimeout(resolve, 50); this.wakeRetry = resolve; });
      } while (!this.destroyed);
      if (this.destroyed) return;
      for (const line of frame.logs || []) this.h.onLog?.(line);
      this.h.onFrame?.(Uint8Array.from(atob(frame.colors), c => c.charCodeAt(0)), frame.ms || 0);
      if (frame.backdrop && this.h.onBackdrop) {
        const blob = new Blob([Uint8Array.from(atob(frame.backdrop), c => c.charCodeAt(0))], { type: "image/jpeg" });
        const bitmap = await createImageBitmap(blob);
        if (this.destroyed) bitmap.close(); else this.h.onBackdrop(bitmap);
      }
    } catch (error) { this.failed(error); }
    finally { this.busy = false; }
  }

  setParams(params) {
    Object.assign(this.effect.params ||= {}, params);
    this.pending.params = { ...(this.pending.params || {}), ...params };
  }
  setLayers(layers) { this.effect.layers = layers; this.pending.layers = layers; }
  setPoints(points) { this.points = points; this.pending.points = points; }

  failed(error) {
    if (this.destroyed) return;
    this.destroy();
    this.h.onError?.({ message: String(error.message || error), line: null });
    this.h.onStopped?.("error");
  }

  destroy() {
    this.destroyed = true;
    this.ready = false;
    clearTimeout(this.retry);
    this.wakeRetry?.();
    if (this.id) {
      post("/api/studio/preview/stop", { id: this.id }).catch(() => {});
      this.id = null;
    }
  }
}
