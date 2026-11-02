// A single queue owns the controller's Studio trial. Closing an editor invalidates queued draft writes.
export function createLivePreview({ post, onChange = () => {}, onResult = () => {} }) {
  const state = { wanted: false, confirmed: false, pending: false, error: "" };
  let tail = Promise.resolve(), generation = 0, count = 0;
  function send(on, payload, token) {
    count++; state.pending = true; onChange(state);
    const task = tail.then(async () => {
      if (on && (token !== generation || !state.wanted)) return;
      try {
        const result = await post("/api/studio/draft", on ? { ...payload, lights: true } : { lights: false });
        state.confirmed = !!result.lights; state.error = "";
        onResult(result);
      } catch (e) {
        state.error = `${on ? "Draft preview could not update" : "Live preview could not stop"}: ${e.message}. ${on ? "Try the preview switch again." : "Retry Stop preview to restore your scene."}`;
        if (on && !state.confirmed) {
          state.wanted = false;
          // The start may have reached the controller before its reply was lost. Cleanup is idempotent;
          // replaying the failed start would instead revive an obsolete draft after navigation.
          try {
            const result = await post("/api/studio/draft", { lights: false });
            state.confirmed = !!result.lights; onResult(result);
          } catch (cleanup) {
            state.error += ` Cleanup could not be confirmed: ${cleanup.message}. Retry Stop preview.`;
          }
        }
      } finally { onChange(state); }
    }).finally(() => { state.pending = --count > 0; onChange(state); });
    tail = task.catch(() => {});
    return task;
  }
  return {
    state,
    set(on, payload = {}) {
      state.wanted = !!on; generation++;
      return send(!!on, structuredClone(payload), generation);
    },
    sync(payload) { return state.wanted ? send(true, structuredClone(payload), generation) : Promise.resolve(); },
    observe(current) {
      if (state.pending) return;
      const was = state.confirmed;
      state.confirmed = current?.trial?.kind === "studio";
      if (was && !state.confirmed) { state.wanted = false; generation++; state.error = ""; }
      onChange(state);
    },
  };
}
