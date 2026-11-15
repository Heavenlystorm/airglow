// Screen-only auditions share production renderers, never the hardware preview route.
import { $, S, esc, get, on } from "./core.js";
import { PreviewRunner } from "./fxpreview.js";
import { ledPoints, sampleZones, testAudio, zoneFrames } from "./fxrun.js";
import { litZones, paintRig } from "./stage.js";
import { openSheet } from "./sheet.js";
import { reduced } from "./motion.js";
import { applyLightingScene, undoLightingScene } from "./scene-commands.js";

const cache = new Map();
let library = null, deck = null, deckKey = null, generation = 0, cancelStill = null, audition = null;
let deckCanvases = [];
const visible = node => node?.isConnected && !document.hidden && !node.closest("[hidden]");
const rig = () => { const zones = litZones(); return { zones: zones.length ? zones : sampleZones(), sample: !zones.length }; };
const paramsFor = e => S.state?.scene?.effect === e.id ? structuredClone(S.state.scene.params || {})
  : Object.fromEntries((e.params || []).map(p => [p.id, structuredClone(p.default)]));

async function runnable(e, params) {
  if (!e.studio) return { kind: "builtin", id: e.id, params };
  if (!library) {
    const request = get("/api/studio/effects").catch(error => { if (library === request) library = null; throw error; });
    library = request;
  }
  const source = (await library).effects.find(x => x.fx_id === e.id);
  if (!source) throw new Error("This effect is no longer installed. Refresh the library and try again.");
  if (source.kind === "layers") return { kind: "layers", layers: { v: 2, layers: source.layers, master: source.master, expose: source.expose }, params };
  return { kind: "code", code: source.code, params };
}

function stopStills() { generation++; cancelStill?.(); cancelStill = null; deckKey = null; }
function remember(key, frames) {
  cache.delete(key); cache.set(key, frames);
  if (cache.size > 48) cache.delete(cache.keys().next().value);
}
function showStill(canvas, frames, zones) {
  paintRig(canvas, frames, { zones, pad: 8 });
  delete canvas.dataset.previewError;
  canvas.removeAttribute("title");
  canvas.dataset.previewReady = "true";
}
function stillError(canvas, message) {
  delete canvas.dataset.previewReady;
  canvas.dataset.previewError = "true";
  canvas.title = `Preview unavailable: ${message}`;
}
function still(canvas, effect, zones, stamp, gen) {
  const stored = cache.get(stamp);
  if (stored) { showStill(canvas, stored, zones); return Promise.resolve(); }
  return new Promise(resolve => {
    const { points, slices } = ledPoints(zones);
    let finished = false, timeout;
    const finish = () => { if (finished) return; finished = true; clearTimeout(timeout); runner.destroy(); if (cancelStill === finish) cancelStill = null; resolve(); };
    const error = info => { if (gen === generation) stillError(canvas, info.message || "Try Preview again"); finish(); };
    const runner = new PreviewRunner({
      onReady: () => runner.tick(2, 1 / 30, testAudio(2)),
      onFrame: colors => {
        if (gen === generation && visible(canvas)) {
          const frames = zoneFrames(colors, slices);
          remember(stamp, frames);
          showStill(canvas, frames, zones);
        }
        finish();
      }, onError: error, onStopped: finish,
    });
    cancelStill = finish;
    timeout = setTimeout(() => error({ message: "The preview did not start" }), 16000);
    runner.load(effect, points);
  });
}

export function mountScenePreviews(root) {
  deck = root;
  if (!visible(root) || audition) return;
  const { zones } = rig();
  const entries = [...root.querySelectorAll("canvas[data-preview-effect]")].map(canvas => {
    const effect = S.effects.find(e => e.id === canvas.dataset.previewEffect);
    return { canvas, effect, params: effect && paramsFor(effect) };
  }).filter(x => x.effect);
  const key = JSON.stringify([entries.map(x => [x.effect.id, x.params]), zones]);
  const sameCanvases = entries.length === deckCanvases.length && entries.every((x, i) => x.canvas === deckCanvases[i]);
  if (key === deckKey && sameCanvases && entries.every(x => x.canvas.dataset.previewReady || x.canvas.dataset.previewError)) return;
  if (key === deckKey && sameCanvases && cancelStill) return;
  stopStills(); deckKey = key; deckCanvases = entries.map(x => x.canvas);
  const gen = generation;
  (async () => {
    for (const { canvas, effect, params } of entries) {
      if (gen !== generation || !visible(root) || audition) break;
      try {
        const run = await runnable(effect, params);
        if (gen !== generation) break;
        await still(canvas, run, zones, JSON.stringify([run, zones]), gen);
      } catch (error) { if (gen === generation) stillError(canvas, error.message); }
    }
  })();
}

function prospectiveScene(id) {
  const effect = S.effects.find(e => e.id === id);
  if (!effect) throw new Error("This effect is no longer installed.");
  const { zones, sample } = rig();
  const scene = S.state?.scene || {}, status = S.state?.scene_status;
  const params = paramsFor(effect), brightness = scene.brightness ?? 100, on = id !== "off";
  const { points, slices } = ledPoints(zones);
  const needs = new Set();
  let hasOverrides = false;
  const selections = slices.map(({ key, count }, i) => {
    const z = zones[i], override = on && (z.config.override || scene.areas?.[z.spot]);
    if (override) hasOverrides = true;
    const selected = override?.effect || id;
    for (const need of S.effects.find(e => e.id === selected)?.needs || []) needs.add(need);
    return { key, count, id: selected, params: override?.params || params, layout: sample ? undefined : z.config.layout };
  });
  const inputs = [sample ? "Sample rig" : "Your rig", `${brightness}% brightness`,
    ...(needs.has("audio") ? ["Sample audio · 120 BPM"] : []),
    ...(needs.has("sensors") ? ["Sample temperature · 60°C"] : []),
    ...(needs.has("screen") ? ["Sample screen colours"] : [])];
  const run = { kind: "scene", id, brightness, zones: selections };
  // The renderer gets the same positions and per-LED indices as the live engine.
  const coordinates = points.map(([x, y, spot, local, fan]) => [Math.round(x * 100) / 100, Math.round(y * 100) / 100, spot, local, fan]);
  const patch = { effect: id, params, on, expected_effect: scene.effect,
    expected_revision: status?.desired_revision, expected_session: status?.session };
  return { run, points: coordinates, zones, slices, inputs, hasOverrides, patch,
    stamp: JSON.stringify([run, coordinates]), studio: id.startsWith("fx:") || selections.some(z => z.id.startsWith("fx:")) };
}

export async function openSceneAudition(id) {
  const effect = S.effects.find(e => e.id === id);
  if (!effect) return;
  audition?.stop(); stopStills();
  const sheet = openSheet(`<div class="scene-audition"><div class="sheet-head"><h2>Audition ${esc(effect.name)}</h2><button class="btn quiet" data-close>Close</button></div>
    <p class="hint">${esc(effect.description || "A scene made in the Studio.")}</p>
    <canvas class="audition-canvas" aria-label="${esc(effect.name)} lighting preview"></canvas>
    <p class="audition-inputs"></p>
    <p class="hint" data-audition-overrides hidden>Apply keeps the current area and zone effects shown here.</p>
    <p class="audition-status" role="status" aria-live="polite">Starting preview…</p>
    <div class="btn-row"><button class="btn" data-audition-retry hidden>Retry preview</button><button class="btn" data-audition-apply disabled>Apply lighting</button><button class="btn quiet" data-audition-undo hidden>Undo lighting</button></div>
    <p class="hint">Apply changes lighting only. Fan settings stay as they are.</p></div>`);
  const content = sheet.querySelector(".scene-audition"), canvas = content.querySelector("canvas"), status = content.querySelector("[role=status]");
  const apply = content.querySelector("[data-audition-apply]"), undo = content.querySelector("[data-audition-undo]"), retry = content.querySelector("[data-audition-retry]");
  const inputs = content.querySelector(".audition-inputs"), overrides = content.querySelector("[data-audition-overrides]");
  let runner = null, draft = null, raf = 0, last = 0, sentAt = 2, t = 2, serial = 0, ended = false, ready = false, applying = false;
  let appliedGuard = null, catalogChanged = false;
  const stopRunner = () => { serial++; cancelAnimationFrame(raf); runner?.destroy(); runner = null; };
  const session = {
    stop: () => {
      if (ended) return; ended = true; stopRunner(); observer.disconnect();
      sheet.removeEventListener("close", session.stop); document.removeEventListener("visibilitychange", visibility);
      document.removeEventListener("appearancechange", motionChanged); media.removeEventListener("change", motionChanged);
      if (audition === session) audition = null;
      if (deck && visible(deck)) mountScenePreviews(deck);
    },
    refresh(force = false) {
      if (ended || applying || !visible(content)) return;
      try {
        const next = prospectiveScene(id);
        if (force || draft?.stamp !== next.stamp) start(next);
        else draft.patch = next.patch;
      } catch (error) {
        stopRunner(); draft = null; ready = false; apply.disabled = true;
        status.textContent = `Preview unavailable: ${error.message}`; retry.hidden = false;
      }
    },
    catalog() {
      if (!draft?.studio && !effect.studio) return;
      if (applying) { stopRunner(); ready = false; catalogChanged = true; }
      else this.refresh(true);
    },
  };
  audition = session;
  const loop = now => {
    if (ended || !visible(content) || reduced()) return;
    t += last ? Math.min((now - last) / 1000, .1) : 1 / 30; last = now;
    if (runner?.tick(t, Math.min(.25, t - sentAt), testAudio(t))) sentAt = t;
    raf = requestAnimationFrame(loop);
  };
  const start = (candidate = null) => {
    stopRunner(); ready = false; apply.disabled = true; retry.hidden = true; status.textContent = "Starting preview…";
    const current = serial;
    try {
      if (ended || document.hidden) return;
      draft = candidate || prospectiveScene(id);
      const { run, points, slices, zones } = draft;
      inputs.textContent = `Preview only · ${draft.inputs.join(" · ")}`;
      overrides.hidden = !draft.hasOverrides;
      const fail = error => { if (ended || current !== serial) return; stopRunner(); ready = false; apply.disabled = true; status.textContent = `Preview unavailable: ${error.message || "The renderer stopped"}`; retry.hidden = false; };
      runner = new PreviewRunner({ onReady: () => { if (runner.tick(t, 1 / 30, testAudio(t))) sentAt = t; },
        onFrame: colors => { if (ended || current !== serial) return; paintRig(canvas, zoneFrames(colors, slices), { zones, pad: 28 });
          if (!ready) { ready = true; apply.disabled = applying; status.textContent = "Preview ready"; if (!reduced()) { last = 0; raf = requestAnimationFrame(loop); } }
        }, onError: fail });
      runner.load(run, points);
    } catch (error) { if (!ended && current === serial) { status.textContent = `Preview unavailable: ${error.message}`; retry.hidden = false; } }
  };
  const visibility = () => { if (document.hidden) stopRunner(); else if (!ended) start(); };
  const motionChanged = () => { cancelAnimationFrame(raf); if (!reduced() && ready && visible(content)) { last = 0; raf = requestAnimationFrame(loop); } };
  const media = matchMedia("(prefers-reduced-motion: reduce)");
  const observer = new MutationObserver(() => { if (!content.isConnected) session.stop(); });
  observer.observe(sheet, { childList: true }); sheet.addEventListener("close", session.stop);
  document.addEventListener("visibilitychange", visibility); document.addEventListener("appearancechange", motionChanged); media.addEventListener("change", motionChanged);
  retry.addEventListener("click", () => start());
  const finishWrite = () => {
    applying = false;
    if (!ended) {
      apply.disabled = !ready; undo.disabled = false;
      if (catalogChanged) { catalogChanged = false; session.refresh(true); }
    }
  };
  apply.addEventListener("click", async () => {
    if (!ready || applying) return;
    applying = true; apply.disabled = true; undo.disabled = true; status.textContent = "Applying lighting…";
    try { const result = await applyLightingScene(draft.patch, { wait: true });
      if (!ended) { appliedGuard = { expected_revision: result.scene_status?.desired_revision, expected_session: result.scene_status?.session };
        status.textContent = result.scene_status?.overridden ? "Saved. An automation is currently controlling the lights." : "Lighting applied"; undo.hidden = !result.scene_status?.undo_available; }
    } catch (error) { if (!ended) status.textContent = `Could not apply: ${error.message}`; }
    finally { finishWrite(); }
  });
  undo.addEventListener("click", async () => {
    if (applying) return;
    applying = true; undo.disabled = true; apply.disabled = true;
    try { await undoLightingScene(appliedGuard || {}); if (!ended) { status.textContent = "Previous lighting restored"; undo.hidden = true; } }
    catch (error) { if (!ended) status.textContent = `Could not undo: ${error.message}`; }
    finally { finishWrite(); }
  });
  start();
}

document.addEventListener("visibilitychange", () => { if (document.hidden) stopStills(); else if (deck) mountScenePreviews(deck); });
window.addEventListener("hashchange", () => { stopStills(); if (audition) $("#sheet").close(); if (deck) requestAnimationFrame(() => mountScenePreviews(deck)); });
on("catalog", () => { library = null; cache.clear(); stopStills(); audition?.catalog(); if (deck) mountScenePreviews(deck); });
on("state", () => audition?.refresh());
on("zones", () => audition?.refresh());
