// Saved endpoint snapshots and preview generations. This module never writes to the lighting controller.
export const BLEND_KEY = "airglow.studio.blend.v1";
export const BLEND_MARKER = "Airglow saved-effects blend v1";
export const isBlend = effect => typeof effect?.source === "string" && effect.source.includes(BLEND_MARKER);
export const blendable = effect => ["layers", "code"].includes(effect?.kind) && typeof effect.source === "string" && !isBlend(effect);

export function snapshotEffect(effect) {
  if (!blendable(effect)) throw new Error("Choose an original saved effect; blends cannot be nested.");
  return { fx_id: effect.fx_id, name: effect.name, source: effect.source, values: {} };
}

function validSnapshot(value) {
  return value && typeof value.name === "string" && value.name.length <= 40
    && typeof value.fx_id === "string" && value.fx_id.length <= 160
    && typeof value.source === "string" && new TextEncoder().encode(value.source).length <= 32000
    && value.source.includes('airglow-effect') && !value.source.includes(BLEND_MARKER)
    && value.values && typeof value.values === "object" && !Array.isArray(value.values);
}

export function createBlendStorage(storage) {
  return {
    read() {
      try {
        const raw = storage.getItem(BLEND_KEY) || "null";
        if (raw.length > 150000) return null;
        const value = JSON.parse(raw);
        if (value?.version !== 1 || !validSnapshot(value.a) || !validSnapshot(value.b)
          || value.a.source === value.b.source || typeof value.name !== "string" || value.name.length > 40
          || !Number.isFinite(value.amount) || value.amount < 0 || value.amount > 100) return null;
        return { a: value.a, b: value.b, name: value.name, amount: value.amount };
      } catch { return null; }
    },
    write(value) {
      try { storage.setItem(BLEND_KEY, JSON.stringify({ version: 1, ...value })); return true; }
      catch { return false; }
    },
  };
}

export function createBlendPreview({ build, createRunner, points, onFrame, onReady = () => {}, onError = () => {} }) {
  let generation = 0, runner = null, amount = 50, ended = false;
  return {
    async load(payload) {
      const token = ++generation;
      runner?.destroy(); runner = null;
      amount = payload.amount;
      try {
        const result = await build(structuredClone(payload));
        if (ended || token !== generation) return;
        let failed = false;
        const current = createRunner({
          onFrame: (...args) => { if (!ended && token === generation) onFrame(...args); },
          onReady: () => { if (!ended && token === generation) onReady(); },
          onError: error => { if (!ended && token === generation) { failed = true; onError(error.message || String(error)); } },
          onStopped: reason => { if (!ended && token === generation && !failed && reason !== "stopped") onError(`Preview stopped: ${reason}. Choose simpler effects or retry.`); },
        });
        runner = current;
        await current.load({ kind: "code", code: result.effect.code, params: { blend: amount } }, points);
        if (ended || token !== generation) current.destroy();
        else current.setParams({ blend: amount });
      } catch (error) { if (!ended && token === generation) onError(error.message || String(error)); }
    },
    setAmount(value) { amount = value; runner?.setParams({ blend: value }); },
    tick(time, dt, audio) { return runner?.tick(time, dt, audio, false); },
    destroy() { ended = true; generation++; runner?.destroy(); runner = null; },
  };
}
