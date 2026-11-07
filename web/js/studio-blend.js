// Two saved effects, one independent screen preview. Saving never starts a hardware trial.
import { $, emit, esc, post, rangeFill, toast } from "./core.js";
import { openSheet, closeSheet } from "./sheet.js";
import { litZones, paintRig } from "./stage.js";
import { ledPoints, sampleZones, testAudio, zoneFrames } from "./fxrun.js";
import { PreviewRunner } from "./fxpreview.js";
import { blendable, snapshotEffect, createBlendStorage, createBlendPreview } from "./studio-blend-state.js";

let disposeCurrent = null;
export function closeBlend() { disposeCurrent?.(); }

export function openBlend(effects, { onSaved = () => {} } = {}) {
  closeBlend();
  const candidates = effects.filter(blendable);
  let storage;
  try { storage = createBlendStorage(localStorage); } catch { storage = createBlendStorage(null); }
  const restored = storage.read();
  if (!restored && candidates.length < 2) return toast("Save two different original effects to blend. Existing blends cannot be nested.", true);
  let state = restored || { a: snapshotEffect(candidates[0]), b: snapshotEffect(candidates[1]), amount: 50, name: "Blended effect" };
  let alive = true, busy = false, saved = null, storageError = false, error = "", ready = false, paused = false, beat = false;
  let time = 0, last = 0, raf = 0, needsFrame = false;
  const zones = litZones().length ? litZones() : sampleZones(), { points, slices } = ledPoints(zones);
  const sheet = openSheet(`<div class="body st-crossblend" id="st-crossblend">
    <div class="st-crossblend-head"><div><h2>Blend saved effects</h2><p>Two effects. One new look.</p></div><button class="icon-btn" data-close aria-label="Close blend"><svg><use href="#i-close"/></svg></button></div>
    <div class="st-crossblend-stage"><canvas id="st-crossblend-canvas" aria-label="Blended lighting preview"></canvas><span class="st-crossblend-badge">Screen preview only</span></div>
    <div class="st-crossblend-transport"><button class="btn quiet" id="st-crossblend-play">Pause</button><label class="check"><input type="checkbox" id="st-crossblend-beat"> Test beat</label><span class="hint" id="st-crossblend-meter">Preparing preview…</span></div>
    <div class="st-crossblend-endpoints"><label><span class="lab">Effect A</span><select class="select" id="st-crossblend-a"></select></label><button class="btn quiet" id="st-crossblend-swap" aria-label="Swap effects A and B">Swap A / B</button><label><span class="lab">Effect B</span><select class="select" id="st-crossblend-b"></select></label></div>
    <label class="lab" for="st-crossblend-range">Blend toward B</label><div class="st-crossblend-mix"><span>A</span><input type="range" min="0" max="100" step="1" id="st-crossblend-range" value="${state.amount}"><span>B</span><input class="text" type="number" min="0" max="100" step="1" id="st-crossblend-number" aria-label="Blend toward B, percent" value="${state.amount}"><span>%</span></div>
    <label class="field"><span class="lab">New effect name</span><input class="text" id="st-crossblend-name" maxlength="40" value="${esc(state.name)}" placeholder="Name this blend"></label>
    <p class="hint">A and B keep copies of their saved settings. Saving creates a separate effect with a Blend control; open it to apply it to your lights.</p>
    <p class="hint" id="st-crossblend-status" role="status" aria-live="polite"></p>
    <div class="actions"><button class="btn quiet" data-close>Close</button><button class="btn quiet" id="st-crossblend-reset">Reset</button><span class="spacer"></span><button class="btn" id="st-crossblend-retry">Retry preview</button><button class="btn primary" id="st-crossblend-save">Save blend</button><button class="btn primary" id="st-crossblend-open" hidden>Open saved effect</button></div>
  </div>`);
  const root = $("#st-crossblend", sheet), find = id => root.querySelector(`#st-crossblend-${id}`);
  const active = () => alive && sheet.open && root.isConnected;
  const motionReduced = () => document.hidden || document.documentElement.dataset.motion === "reduced"
    || window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  function status() {
    if (!active()) return;
    find("status").textContent = [error, storageError ? "Recovery storage is unavailable. Save before closing this window." : "",
      !error && !storageError ? saved ? `Saved “${saved.name}”. Open it when you are ready to apply it.`
        : restored ? "Last blend restored from copies on this PC. Your saved originals remain available." : "Your last blend is kept on this PC. Save to add it to your library." : ""].filter(Boolean).join(" ");
    find("status").classList.toggle("st-crossblend-error", !!error || storageError);
    find("save").hidden = !!saved; find("open").hidden = !saved;
    for (const input of root.querySelectorAll("input,select,#st-crossblend-swap,#st-crossblend-reset,#st-crossblend-save,#st-crossblend-open,#st-crossblend-retry")) input.disabled = busy;
    find("reset").disabled = busy || candidates.length < 2;
    find("save").disabled = busy || !state.name.trim() || state.a.source === state.b.source;
  }
  function persist() { saved = null; storageError = !storage.write(state); status(); }
  function selectors() {
    for (const slot of ["a", "b"]) {
      const current = state[slot], select = find(slot);
      select.innerHTML = `<option value="snapshot">${esc(current.name)} · selected copy</option>` + candidates.map((effect, index) =>
        `<option value="${index}">${esc(effect.name)}</option>`).join("");
      select.value = "snapshot";
    }
  }
  function setAmount(value) {
    state.amount = Math.round(Math.max(0, Math.min(100, value)));
    find("range").value = String(state.amount); find("number").value = String(state.amount); rangeFill(find("range"));
    preview.setAmount(state.amount); persist();
    needsFrame = true;
  }
  const preview = createBlendPreview({
    build: body => post("/api/studio/blend", body), createRunner: handlers => new PreviewRunner(handlers), points,
    onFrame(colors, duration) {
      if (!active()) return;
      paintRig(find("canvas"), zoneFrames(colors, slices), { fit: "stage", pad: 24, maxScale: 3, zones });
      find("meter").textContent = `${duration.toFixed(1)} ms / frame${paused || motionReduced() ? " · Still preview" : ""}`;
    },
    onReady() { if (!active()) return; ready = true; needsFrame = true; status(); },
    onError(message) { if (!active()) return; error = message; ready = false; find("meter").textContent = "Preview unavailable"; status(); },
  });
  function rebuild() { ready = false; error = ""; find("meter").textContent = "Preparing preview…"; status(); preview.load(state); }
  for (const slot of ["a", "b"]) find(slot).addEventListener("change", event => {
    if (busy || event.target.value === "snapshot") return;
    state[slot] = snapshotEffect(candidates[Number(event.target.value)]); persist(); selectors(); rebuild();
  });
  for (const id of ["range", "number"]) {
    find(id).addEventListener("input", event => {
      if (!busy && event.target.value.trim() && Number.isFinite(event.target.valueAsNumber)) setAmount(event.target.valueAsNumber);
    });
    find(id).addEventListener("change", () => { find(id).value = String(state.amount); });
    find(id).addEventListener("blur", () => { find(id).value = String(state.amount); });
  }
  find("swap").addEventListener("click", () => { if (busy) return; [state.a, state.b] = [state.b, state.a]; state.amount = 100 - state.amount; setAmount(state.amount); selectors(); rebuild(); });
  find("name").addEventListener("input", event => { state.name = event.target.value; persist(); });
  find("play").addEventListener("click", () => { paused = !paused; find("play").textContent = paused ? "Play" : "Pause"; });
  find("beat").addEventListener("change", event => { beat = event.target.checked; needsFrame = true; });
  find("retry").addEventListener("click", rebuild);
  find("reset").addEventListener("click", () => {
    if (busy || candidates.length < 2) return;
    state = { a: snapshotEffect(candidates[0]), b: snapshotEffect(candidates[1]), name: "Blended effect", amount: 50 };
    find("name").value = state.name; setAmount(50); selectors(); rebuild();
  });
  find("save").addEventListener("click", async () => {
    if (busy) return;
    busy = true; error = ""; status();
    try {
      const built = await post("/api/studio/blend", structuredClone(state));
      if (!active()) return;
      const result = await post("/api/studio/save", { source: built.source });
      emit("refresh");
      if (!active()) return;
      saved = result.effect;
    } catch (exception) { if (active()) error = `Could not save: ${exception.message}`; }
    finally { busy = false; status(); }
  });
  find("open").addEventListener("click", () => { if (!saved) return; const effect = saved; closeSheet(); dispose(); onSaved(effect); });
  function loop(now) {
    if (!active()) return dispose();
    if (now - last >= 1000 / 30) {
      const dt = Math.min(.1, Math.max(0, (now - last) / 1000)); last = now;
      const playing = !paused && !motionReduced();
      if (ready && (playing || needsFrame)) {
        if (playing) time += dt;
        if (preview.tick(time, playing ? dt : 0, beat ? testAudio(time) : null)) needsFrame = false;
      }
    }
    raf = requestAnimationFrame(loop);
  }
  function dispose() {
    if (!alive) return;
    alive = false; cancelAnimationFrame(raf); preview.destroy(); sheet.removeEventListener("close", dispose);
    if (disposeCurrent === dispose) disposeCurrent = null;
  }
  disposeCurrent = dispose; sheet.addEventListener("close", dispose, { once: true });
  selectors(); rangeFill(find("range")); status(); rebuild(); raf = requestAnimationFrame(loop);
}
