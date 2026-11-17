// Lighting screen: effect deck, inspector (scene or a selected zone), profiles.

import { $, $$, QUICK, S, coalesce, curveLabel, effectName, emit, esc, fail, fmtTemp, get, on, post, rangeFill, sceneColor, sceneSwatch, setHTML, toast } from "./core.js";
import { applyLightingScene, undoLightingScene, waitForLightingScene } from "./scene-commands.js";
import { select } from "./stage.js";
import { openSheet, closeSheet } from "./sheet.js";
import { editProfilesSheet, openGallery, saveProfileSheet } from "./profiles.js";
import { favoriteButton } from "./quick-preferences.js";

let selectedZone = null;
const zoneFor = (key) => S.zones?.zones?.find(z => z.key === key);
const zoneEdits = new Map();
const zoneUndos = new Map();
const zoneBusy = new Set();
const sameOverride = (a, b) => JSON.stringify(a) === JSON.stringify(b);
let editingParams = 0;          // timestamp of the last local edit; pauses re-rendering from polled state

// ------------------------------------------------------------ parameter controls (built from the schema)
// Shared with the Devices tab, which draws device plugin settings from the same schema, and with the Studio.
// opts.reset adds a back-to-default button; Studio passes it, the other screens keep their compact look.
const resetBtn = (spec) => `<button type="button" class="field-reset" data-reset title="Back to the default" aria-label="Reset ${esc(spec.label)}"><svg aria-hidden="true"><use href="#i-rotate"/></svg></button>`;
const stepNum = (v, spec) => {
  const step = +spec.step || 1;
  const lo = Number.isFinite(+spec.min) ? +spec.min : -Infinity;
  const hi = Number.isFinite(+spec.max) ? +spec.max : Infinity;
  const n = Math.round(v / step) * step;
  return +Math.min(hi, Math.max(lo, n)).toFixed(4);
};
export function fieldHTML(spec, value, prefix, opts = {}) {
  const id = esc(`${prefix}-${spec.id}`);
  const param = esc(spec.id);
  if (spec.type === "color") {
    value = /^#[0-9a-f]{6}$/i.test(value) ? value : "#ffffff";
    return `<div class="field" data-param="${param}"><div class="lab"><span>${esc(spec.label)}</span>${opts.reset ? resetBtn(spec) : ""}</div>
      <div class="color-row"><label class="swatch" style="background:${esc(value)}"><input type="color" id="${id}" value="${esc(value)}" aria-label="${esc(spec.label)}"></label>
      <div class="quick">${QUICK.map((c) => `<button type="button" data-quick="${c}" style="background:${c}" aria-label="${c}" aria-pressed="${c === value}"></button>`).join("")}</div></div></div>`;
  }
  if (spec.type === "colors") {
    value = (Array.isArray(value) ? value : []).map(c => /^#[0-9a-f]{6}$/i.test(c) ? c : "#ffffff");
    const sw = value.map((c, i) => `<label class="swatch" style="background:${esc(c)}"><input type="color" data-stop="${i}" value="${esc(c)}" aria-label="Colour ${i + 1}"></label>`).join("");
    return `<div class="field" data-param="${param}"><div class="lab"><span>${esc(spec.label)}</span>${opts.reset ? resetBtn(spec) : ""}</div>
      <div class="stops">${sw}
      ${value.length < spec.max ? `<button type="button" class="icon-btn" data-add-stop title="Add a colour"><svg><use href="#i-plus"/></svg></button>` : ""}
      ${value.length > spec.min ? `<button type="button" class="icon-btn" data-remove-stop title="Remove the last colour"><svg><use href="#i-trash"/></svg></button>` : ""}</div></div>`;
  }
  if (spec.type === "range") {
    return `<div class="field" data-param="${param}"><div class="lab"><label for="${id}"><span>${esc(spec.label)}</span></label>
      <span class="val"><input type="number" class="num" id="${id}-num" min="${spec.min}" max="${spec.max}" step="${spec.step}" value="${value}" aria-label="${esc(spec.label)}" inputmode="decimal">${spec.unit ? `<span class="unit">${esc(spec.unit)}</span>` : ""}${opts.reset ? resetBtn(spec) : ""}</span></div>
      <input type="range" id="${id}" min="${spec.min}" max="${spec.max}" step="${spec.step}" value="${value}"></div>`;
  }
  if (spec.type === "choice") {
    return `<div class="field" data-param="${param}"><div class="lab"><span>${esc(spec.label)}</span>${opts.reset ? resetBtn(spec) : ""}</div>
      <select class="select" id="${id}" aria-label="${esc(spec.label)}">${spec.options.map((o) => `<option value="${esc(o.value)}"${o.value === value ? " selected" : ""}>${esc(o.label)}</option>`).join("")}</select></div>`;
  }
  if (spec.type === "toggle") {
    return `<div class="field" data-param="${param}"><label class="lab" style="align-items:center"><span>${esc(spec.label)}</span>
      <span class="switch"><input type="checkbox" id="${id}"${value ? " checked" : ""}><span></span></span></label></div>`;
  }
  return "";
}

// Wire a block of fields; onChange(newParams) receives the full params object.
export function wireFields(root, effect, params, onChange) {
  const cur = structuredClone(params);
  const send = (live = true) => { editingParams = Date.now(); onChange(structuredClone(cur), live); };
  for (const spec of effect.params) {
    const box = root.querySelector(`[data-param="${spec.id}"]`);
    if (!box) continue;
    if (spec.type === "color") {
      const input = box.querySelector("input[type=color]");
      const sync = (v) => { cur[spec.id] = v; input.value = v; input.parentElement.style.background = v;
        box.querySelectorAll("[data-quick]").forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.quick === v))); };
      input.addEventListener("input", () => { sync(input.value); send(); });
      box.querySelectorAll("[data-quick]").forEach((b) => b.addEventListener("click", () => { sync(b.dataset.quick); send(); }));
      box.querySelector("[data-reset]")?.addEventListener("click", () => { sync(spec.default); send(false); });
    } else if (spec.type === "colors") {
      box.querySelectorAll("[data-stop]").forEach((inp) => inp.addEventListener("input", () => {
        cur[spec.id][+inp.dataset.stop] = inp.value; inp.parentElement.style.background = inp.value; send();
      }));
      box.querySelector("[data-add-stop]")?.addEventListener("click", () => { cur[spec.id].push(cur[spec.id].at(-1)); send(false); });
      box.querySelector("[data-remove-stop]")?.addEventListener("click", () => { cur[spec.id].pop(); send(false); });
      box.querySelector("[data-reset]")?.addEventListener("click", () => { cur[spec.id] = structuredClone(spec.default); send(false); });
    } else if (spec.type === "range") {
      const input = box.querySelector("input[type=range]");
      const num = box.querySelector("input.num");
      rangeFill(input);
      input.addEventListener("input", () => {
        cur[spec.id] = +input.value; rangeFill(input);
        if (num && document.activeElement !== num) num.value = input.value;
        send();
      });
      num?.addEventListener("input", () => {
        const v = parseFloat(num.value);
        if (!Number.isFinite(v)) return;
        cur[spec.id] = stepNum(v, spec); input.value = cur[spec.id]; rangeFill(input); send();
      });
      num?.addEventListener("change", () => {
        const fallback = Number.isFinite(+spec.default) ? +spec.default : +input.min;
        const v = Number.isFinite(parseFloat(num.value)) ? stepNum(parseFloat(num.value), spec) : fallback;
        num.value = v; cur[spec.id] = v; input.value = v; rangeFill(input); send(false);
      });
      box.querySelector("[data-reset]")?.addEventListener("click", () => {
        const v = Number.isFinite(+spec.default) ? +spec.default : +input.min;
        cur[spec.id] = v; input.value = v; if (num) num.value = v; rangeFill(input); send(false);
      });
    } else if (spec.type === "choice") {
      box.querySelector("select").addEventListener("change", (e) => { cur[spec.id] = e.target.value; send(false); });
    } else if (spec.type === "toggle") {
      box.querySelector("input").addEventListener("change", (e) => { cur[spec.id] = e.target.checked; send(false); });
    }
  }
}

// ------------------------------------------------------------ deck
// A name with one word longer than its card ("Temperature" in a narrow column) gets a smaller size instead of losing
// its last letters.
function fitNames() {
  for (const n of $("#deck").querySelectorAll(".name")) {
    n.style.fontSize = "";
    for (let px = 11.5; px >= 9 && n.scrollWidth > n.clientWidth; px -= 0.5) n.style.fontSize = `${px}px`;
  }
}

function renderDeck() {
  const zone = zoneFor(selectedZone);
  const unavailable = selectedZone && !zone?.device.online;
  const scene = zone?.config.override || S.state.scene;
  const c = sceneColor(scene);
  const deck = $("#deck"), before = deck._html;
  const previous = deck.contains(document.activeElement) ? document.activeElement : null;
  const choice = previous?.closest(".fx-choice")?.querySelector("[data-effect]")?.dataset.effect;
  deck.setAttribute("aria-label", selectedZone ? `Effects for ${zone?.config.name || "unavailable zone"}` : "Effects for whole rig");
  setHTML(deck, S.effects.map((e) => {
    const extra = e.id === "spectrum" ? Array.from({ length: 7 }, (_, i) => `<i style="--i:${i}"></i>`).join("") : "";
    const tint = ["solid", "breathing", "pulse"].includes(e.id) ? ` style="--c:${scene.effect === e.id ? c : { solid: "#009bde", breathing: "#ff5a1f", pulse: "#8a3dff" }[e.id]}"` : "";
    const thumb = e.studio ? `t-studio${e.studio.permissions?.includes("audio") ? " t-music" : ""}` : `t-${e.id}`;
    return `<div class="fx-choice"><button class="fx" role="radio" aria-checked="${!unavailable && scene.effect === e.id}" data-effect="${esc(e.id)}"${unavailable ? " disabled" : ""} title="${esc(e.description || "Made in the Studio")}">
      <span class="thumb ${thumb}"${tint}>${extra}</span><span class="name">${esc(e.name)}</span></button>${favoriteButton("effect", e.id, e.name)}</div>`;
  }).join("") + `<a class="fx fx-new" href="#studio/new" title="Make your own effect in the Studio"><span class="thumb t-new" aria-hidden="true">+</span><span class="name">Create your own</span></a>`);
  if (deck._html !== before) {
    fitNames();
    if (choice) {
      const effect = deck.querySelector(`[data-effect="${CSS.escape(choice)}"]`);
      const target = previous.dataset.effect ? effect : effect?.nextElementSibling;
      target?.focus({ preventScroll: true });
    }
  }
}

function renderScope() {
  const picker = $("#zone-pick"), zones = (S.zones?.zones || []).filter(z => z.device.online || z.key === selectedZone);
  const missing = selectedZone && !zones.some(z => z.key === selectedZone);
  setHTML(picker, '<option value="">Whole rig</option>' + zones.map(z =>
    `<option value="${esc(z.key)}">${esc(z.config.name)} · ${esc(z.device.name)}${z.device.online ? "" : " (offline)"}</option>`).join("") +
    (missing ? `<option value="${esc(selectedZone)}">Unavailable zone</option>` : ""));
  picker.value = selectedZone || "";
}

// Local sliders are debounced. A later effect selection must invalidate any older unsent edit.
function nextZoneEdit(key) {
  const revision = (zoneEdits.get(key) || 0) + 1;
  zoneEdits.set(key, revision);
  zoneBusy.add(key); paintZoneUndo(key);
  return revision;
}

function currentUndo(key) {
  const saved = zoneUndos.get(key), zone = zoneFor(key);
  return saved && zone?.device.online && sameOverride(saved.after, zone.config.override) ? saved : null;
}

function paintZoneUndo(key) {
  const notice = $("#zone-saved");
  if (selectedZone !== key || !notice) return;
  notice.hidden = !currentUndo(key);
  notice.querySelector("button").disabled = zoneBusy.has(key);
}

async function writeZoneOverride(key, override, revision, chosenEffect = null, undo = null) {
  try {
    if (!zoneFor(key)?.device.online || revision !== zoneEdits.get(key)) return;
    const patch = undo ? { override, expected_override: undo.after } : { override };
    const result = await post(`/api/zones/${encodeURIComponent(key)}`, patch);
    const current = zoneFor(key);
    if (current) current.config = result.config;
    if (undo) zoneUndos.delete(key);
    else if (!sameOverride(result.previous_override, result.config.override)) {
      zoneUndos.set(key, { before: result.previous_override, after: result.config.override });
    }
    if (chosenEffect && result.config.override?.effect === chosenEffect) emit("effect:applied", chosenEffect);
  } finally {
    if (revision === zoneEdits.get(key)) { zoneBusy.delete(key); paintZoneUndo(key); }
  }
}

async function chooseZoneEffect(key, override, chosenEffect) {
  try { await writeZoneOverride(key, override, nextZoneEdit(key), chosenEffect); }
  catch (err) { fail(err); }
  if (selectedZone !== key) return;
  const focused = document.activeElement;
  const restore = focused?.id === "zone-effect" ? "#zone-effect"
    : focused?.dataset.mode ? `#zone-mode [data-mode="${CSS.escape(focused.dataset.mode)}"]` : null;
  renderDeck(); renderInspector();
  if (restore) $(restore)?.focus({ preventScroll: true });
}

$("#deck").addEventListener("click", async (e) => {
  const b = e.target.closest("[data-effect]");
  if (!b || b.disabled) return;
  const id = b.dataset.effect;
  if (selectedZone) {
    const own = zoneFor(selectedZone)?.config.override;
    if (own?.effect === id) return;
    const params = S.state.scene.effect === id ? S.state.scene.params : {};
    return chooseZoneEffect(selectedZone, { effect: id, params }, id);
  }
  try {
    await applyLightingScene({ effect: id });
    S.state.active_preset = null;
    renderDeck(); renderInspector();
  } catch (err) { fail(err); }
});

// ------------------------------------------------------------ inspector: scene
const sendScene = coalesce(async (patch) => {
  const r = await applyLightingScene(patch);
});

function overrideBanner() {
  const o = S.state.override;
  if (!o) return "";
  if (o.locked) return `<div class="banner">Lights are off while the PC is locked. You can change this in Settings.</div>`;
  if (!o.lighting) return "";
  const rule = S.rules.find((r) => r.id === o.lighting);
  return `<div class="banner"><span>The automation <b>${esc(rule ? describeShort(rule) : "rule")}</b> is controlling the lights right now. Your changes here take over again when it stops.</span>
    <div><button class="btn" data-pause="60">Pause automations for an hour</button></div></div>`;
}

function readinessBanner() {
  const status = S.state.scene_status;
  if (!status) return "";
  if (status.status === "pending") return '<div class="banner" role="status">Starting the effect. Your current lights stay on until its first frame is ready.</div>';
  if (status.status === "failed") return `<div class="banner" role="alert">${esc(status.error)} <button class="btn" data-scene-retry>Retry current scene</button></div>`;
  return status.undo_available ? '<div class="lighting-saved"><span role="status">Lighting applied.</span><button class="text-link" data-scene-undo aria-label="Undo lighting">Undo</button></div>' : "";
}

export function describeShort(rule) {
  const w = rule.when;
  return { app_running: `${w.exe} is running`, app_focused: `${w.exe} is in front`, idle: `idle ${w.minutes} min`,
           time: `${w.start} to ${w.end}`, locked: "PC locked", temp_above: `above ${fmtTemp(w.temp)}` }[w.type] || "rule";
}

function presetsHTML() {
  const active = S.state.active_preset;
  return `<div class="ins-block"><div class="ins-title"><h2 style="font-size:15px">Profiles</h2>
      <span class="btn-row" style="gap:14px"><button class="text-link" id="browse-profiles">Browse</button><button class="text-link" id="edit-presets">Edit</button></span></div>
    <div class="presets">${S.presets.map((p) => `<span class="preset-choice"><button class="preset" data-preset="${esc(p.id)}" aria-pressed="${p.id === active}" style="--pc:${sceneColor(p)};--ps:${sceneSwatch(p)}">
      <span class="pdot"></span>${esc(p.name)}</button>${favoriteButton("profile", p.id, p.name)}</span>`).join("")}
      <button class="preset" id="save-preset" title="Save the current lighting and fan curve as a profile"><svg width="14" height="14"><use href="#i-plus"/></svg>Save current</button></div></div>`;
}

const REMIXABLE = ["solid", "breathing", "cycle", "rainbow", "gradient", "spectrum", "pulse"];
function studioButton(eff) {
  if (eff.studio) {
    const by = eff.studio.origin === "community" ? `By ${esc(eff.studio.author)}, from the community.` : "One of yours, made in the Studio.";
    return `<p class="hint" style="margin:0">${by}</p><button class="btn" data-studio-open="${esc(eff.id)}">${eff.studio.origin === "mine" ? "Edit in the Studio" : "Remix in the Studio"}</button>`;
  }
  return REMIXABLE.includes(eff.id) ? `<button class="btn" data-studio-remix="${esc(eff.id)}">Remix in the Studio</button>` : "";
}

function renderSceneInspector() {
  const scene = S.state.scene;
  const eff = S.effects.find((e) => e.id === scene.effect) || S.effects[0];
  const ins = $("#inspector");
  ins.innerHTML = `${readinessBanner()}${overrideBanner()}${presetsHTML()}<div class="rule-line"></div>
    <div class="ins-block"><div class="ins-title"><h2>${esc(eff.name)}</h2></div>
      <p class="ins-desc">${esc(eff.description)}${eff.needs.includes("audio") ? " Plays from whatever your speakers or headphones are playing." : ""}${eff.needs.includes("screen") ? " Uses your main display. Nothing is recorded or saved." : ""}</p>
      ${studioButton(eff)}
      <div class="ins-block" id="scene-fields">${eff.params.map((p) => fieldHTML(p, scene.params[p.id], "sp")).join("")}</div>
      <div class="field"><label class="lab" for="bri"><span>Brightness</span><output id="bri-out">${scene.brightness}%</output></label>
        <input type="range" id="bri" min="0" max="100" step="1" value="${scene.brightness}"></div>
    </div>`;
  wireFields($("#scene-fields"), eff, scene.params, (params, live) => {
    S.state.scene.params = params;
    const patch = { params, expected_effect: eff.id };
    if (live) sendScene(patch); else applyLightingScene(patch).then(() => { renderInspector(); renderDeck(); }).catch(fail);
    S.state.active_preset = null;
    $$(".preset[data-preset]").forEach((b) => b.setAttribute("aria-pressed", "false"));
  });
  const bri = $("#bri");
  rangeFill(bri);
  bri.addEventListener("input", () => {
    editingParams = Date.now(); rangeFill(bri); $("#bri-out").textContent = `${bri.value}%`;
    S.state.scene.brightness = +bri.value; sendScene({ brightness: +bri.value });
  });
}

$("#inspector").addEventListener("click", async (e) => {
  if (e.target.closest("[data-zone-undo]")) {
    const key = selectedZone, undo = currentUndo(key);
    if (!undo || zoneBusy.has(key)) return;
    const focusUndo = document.activeElement === e.target.closest("button");
    try { await writeZoneOverride(key, undo.before, nextZoneEdit(key), null, undo); }
    catch (err) { fail(err); emit("refresh"); }
    if (selectedZone === key) {
      renderDeck(); renderInspector();
      if (focusUndo && document.activeElement === document.body) $("#zone-pick").focus({ preventScroll: true });
    }
    return;
  }
  if (e.target.closest("[data-scene-undo]")) {
    try { await undoLightingScene(); renderInspector(); renderDeck(); } catch (err) { fail(err); }
    return;
  }
  if (e.target.closest("[data-scene-retry]")) {
    try { await applyLightingScene({ ...S.state.scene, retry: true }, { wait: true }); renderInspector(); } catch (err) { fail(err); }
    return;
  }
  const p = e.target.closest("[data-preset]");
  if (p) {
    const preset = S.presets.find((x) => x.id === p.dataset.preset);
    try {
      const r = await post(`/api/presets/${encodeURIComponent(preset.id)}/apply`);
      if (r.scene_status?.status === "pending") r.scene_status = (await waitForLightingScene(r.scene_status.desired_revision, r.scene_status.session)).scene_status;
      const failed = r.steps.filter((s) => !s.ok);
      if (failed.length) toast(`${preset.name} is on, but ${failed[0].step} didn't apply: ${failed[0].error}`, true);
      else toast(r.scene_status?.overridden ? `${preset.name} saved. An automation controls the lights.` : `${preset.name} is on`);
      emit("refresh");
    } catch (err) { fail(err); }
    return;
  }
  if (e.target.closest("#save-preset")) return saveProfileSheet(() => renderInspector());
  if (e.target.closest("#edit-presets")) return editProfilesSheet(() => renderInspector());
  if (e.target.closest("#browse-profiles")) return openGallery();
  const so = e.target.closest("[data-studio-open]");
  if (so) return emit("studio:open", { fx_id: so.dataset.studioOpen });
  const sr = e.target.closest("[data-studio-remix]");
  if (sr) return emit("studio:open", { builtin: sr.dataset.studioRemix, params: S.state.scene.params });
  const pause = e.target.closest("[data-pause]");
  if (pause) {
    try { await post("/api/automations/pause", { minutes: +pause.dataset.pause }); toast("Automations paused for an hour"); emit("refresh"); }
    catch (err) { fail(err); }
  }
});


// ------------------------------------------------------------ inspector: zone
function renderZoneInspector(key) {
  const z = zoneFor(key);
  if (!z?.device.online) {
    $("#inspector").innerHTML = `<div class="ins-block"><h2>${esc(z?.config.name || "Unavailable zone")}</h2><p class="ins-desc">This zone is offline. Reconnect the device or choose another zone.</p><button class="text-link" id="zone-done">Back to whole rig</button></div>`;
    return;
  }
  const ov = z.config.override;
  const eff = ov ? (S.effects.find((e) => e.id === ov.effect) || null) : null;   // the effect may be deleted
  const ins = $("#inspector");
  ins.innerHTML = `<div class="lighting-saved" id="zone-saved" hidden><span role="status">Zone lighting saved.</span><button class="text-link" data-zone-undo aria-label="Undo zone lighting">Undo</button></div>
    <div class="ins-block"><div class="ins-title"><h2>${esc(z.config.name)}</h2><button class="text-link" id="zone-done">Back to whole rig</button></div>
      <p class="ins-desc">${esc(z.device.name)}, ${z.positions.length} LED${z.positions.length === 1 ? "" : "s"}${z.config.enabled ? "" : ". This zone is switched off."}</p>
      <div class="btn-row"><button class="btn" id="zone-identify">Identify</button><a class="btn quiet" href="#devices/${encodeURIComponent(z.key)}">Zone settings</a></div></div>
    <div class="rule-line"></div>
    <div class="ins-block"><div class="field"><div class="lab"><span>Lighting for this zone</span></div>
      <div class="seg" role="radiogroup" id="zone-mode"><button role="radio" data-mode="follow" aria-checked="${!ov}">Main effect</button><button role="radio" data-mode="own" aria-checked="${!!ov}">Its own effect</button></div></div>
      ${ov ? `<div class="field"><label class="lab" for="zone-effect"><span>Effect</span></label><select class="select" id="zone-effect">${S.effects.map((e) => `<option value="${e.id}"${e.id === ov.effect ? " selected" : ""}>${esc(e.name)}</option>`).join("")}</select></div>
      <div class="ins-block" id="zone-fields">${eff ? eff.params.map((p) => fieldHTML(p, ov.params[p.id], "zp")).join("")
        : '<p class="hint">That effect isn\'t on this PC any more. Pick another one to give this zone its own light.</p>'}</div>` : `<p class="hint">Follows ${esc(effectName(S.state.scene.effect))} like the rest of the rig.</p>`}
    </div>`;
  const saveZone = coalesce(({ override, revision }) => writeZoneOverride(key, override, revision));
  paintZoneUndo(key);
  $("#zone-identify").addEventListener("click", () => post(`/api/zones/${encodeURIComponent(z.key)}/identify`).then(() => toast("Watch that zone: the first LED blinks green, then a white dot walks through every LED in order.")).catch(fail));
  $("#zone-mode").addEventListener("click", async (e) => {
    const m = e.target.dataset.mode;
    if (!m) return;
    const override = m === "own" ? { effect: S.state.scene.effect, params: S.state.scene.params } : null;
    await chooseZoneEffect(key, override);
  });
  $("#zone-effect")?.addEventListener("change", async (e) => {
    const id = e.target.value;
    await chooseZoneEffect(key, { effect: id, params: {} }, id);
  });
  if (ov && eff) wireFields($("#zone-fields"), eff, ov.params, (params) => {
    saveZone({ override: { effect: ov.effect, params }, revision: nextZoneEdit(key) });
  });
}

// ------------------------------------------------------------ refresh
function renderInspector() {
  if (selectedZone) renderZoneInspector(selectedZone); else renderSceneInspector();
}

let lastKey = "";
export function showZone(key) {
  select(key || null); setScope(key);
}
function setScope(key) {
  selectedZone = key || null;
  lastKey = ""; renderScope(); renderDeck(); renderInspector();
}
export function renderLighting() {
  if (!S.state || !S.effects.length) return;
  renderScope(); renderDeck();
  // rebuild the inspector only when something it shows actually changed
  const zone = zoneFor(selectedZone);
  for (const key of zoneUndos.keys()) if (!currentUndo(key)) zoneUndos.delete(key);
  paintZoneUndo(selectedZone);
  const key = JSON.stringify([S.state.scene, S.state.scene_status, S.state.active_preset, S.state.override, S.presets.map((p) => p.id + p.name), selectedZone, zone?.device.online, zone?.config]);
  const editing = Date.now() - editingParams < 2000 || $("#inspector").contains(document.activeElement);
  if (key !== lastKey && (!editing || selectedZone && !zone?.device.online)) {
    lastKey = key; renderInspector();
  }
}

export function initLighting() {
  new ResizeObserver(fitNames).observe($("#deck"));
  document.fonts?.ready.then(fitNames);
  $("#zone-pick").addEventListener("change", (e) => showZone(e.target.value));
  $("#inspector").addEventListener("click", (e) => {
    if (e.target.closest("#zone-done")) { showZone(null); $("#zone-pick").focus(); }
  });
  on("select-zone", setScope);
  on("state", renderLighting);
  on("zones", renderLighting);
  on("scene:status", renderLighting);
}
