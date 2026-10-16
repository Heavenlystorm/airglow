// Lighting screen: effect deck, inspector (scene or a selected zone), presets.

import { $, $$, QUICK, S, coalesce, curveLabel, effectName, emit, esc, fail, get, on, post, rangeFill, sceneColor, sceneSwatch, setHTML, toast } from "./core.js";
import { select } from "./stage.js";
import { openSheet, closeSheet } from "./sheet.js";

let selectedZone = null;
let editingParams = 0;          // timestamp of the last local edit; pauses re-rendering from polled state

// ------------------------------------------------------------ parameter controls (built from the schema)
// Shared with the Devices tab, which draws device plugin settings from the same schema.
export function fieldHTML(spec, value, prefix) {
  const id = `${prefix}-${spec.id}`;
  if (spec.type === "color") {
    return `<div class="field" data-param="${spec.id}"><div class="lab"><span>${esc(spec.label)}</span></div>
      <div class="color-row"><label class="swatch" style="background:${value}"><input type="color" id="${id}" value="${value}" aria-label="${esc(spec.label)}"></label>
      <div class="quick">${QUICK.map((c) => `<button type="button" data-quick="${c}" style="background:${c}" aria-label="${c}" aria-pressed="${c === value}"></button>`).join("")}</div></div></div>`;
  }
  if (spec.type === "colors") {
    const sw = value.map((c, i) => `<label class="swatch" style="background:${c}"><input type="color" data-stop="${i}" value="${c}" aria-label="Colour ${i + 1}"></label>`).join("");
    return `<div class="field" data-param="${spec.id}"><div class="lab"><span>${esc(spec.label)}</span></div>
      <div class="stops">${sw}
      ${value.length < spec.max ? `<button type="button" class="icon-btn" data-add-stop title="Add a colour"><svg><use href="#i-plus"/></svg></button>` : ""}
      ${value.length > spec.min ? `<button type="button" class="icon-btn" data-remove-stop title="Remove the last colour"><svg><use href="#i-trash"/></svg></button>` : ""}</div></div>`;
  }
  if (spec.type === "range") {
    return `<div class="field" data-param="${spec.id}"><label class="lab" for="${id}"><span>${esc(spec.label)}</span><output>${value}${spec.unit || ""}</output></label>
      <input type="range" id="${id}" min="${spec.min}" max="${spec.max}" step="${spec.step}" value="${value}"></div>`;
  }
  if (spec.type === "choice") {
    return `<div class="field" data-param="${spec.id}"><div class="lab"><span>${esc(spec.label)}</span></div>
      <select class="select" id="${id}">${spec.options.map((o) => `<option value="${esc(o.value)}"${o.value === value ? " selected" : ""}>${esc(o.label)}</option>`).join("")}</select></div>`;
  }
  if (spec.type === "toggle") {
    return `<div class="field" data-param="${spec.id}"><label class="lab" style="align-items:center"><span>${esc(spec.label)}</span>
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
    } else if (spec.type === "colors") {
      box.querySelectorAll("[data-stop]").forEach((inp) => inp.addEventListener("input", () => {
        cur[spec.id][+inp.dataset.stop] = inp.value; inp.parentElement.style.background = inp.value; send();
      }));
      box.querySelector("[data-add-stop]")?.addEventListener("click", () => { cur[spec.id].push(cur[spec.id].at(-1)); send(false); });
      box.querySelector("[data-remove-stop]")?.addEventListener("click", () => { cur[spec.id].pop(); send(false); });
    } else if (spec.type === "range") {
      const input = box.querySelector("input");
      rangeFill(input);
      input.addEventListener("input", () => {
        cur[spec.id] = +input.value; rangeFill(input);
        box.querySelector("output").textContent = `${input.value}${spec.unit || ""}`; send();
      });
    } else if (spec.type === "choice") {
      box.querySelector("select").addEventListener("change", (e) => { cur[spec.id] = e.target.value; send(false); });
    } else if (spec.type === "toggle") {
      box.querySelector("input").addEventListener("change", (e) => { cur[spec.id] = e.target.checked; send(false); });
    }
  }
}

// ------------------------------------------------------------ deck
function renderDeck() {
  const scene = S.state.scene;
  const c = sceneColor(scene);
  setHTML($("#deck"), S.effects.map((e) => {
    const extra = e.id === "spectrum" ? Array.from({ length: 7 }, (_, i) => `<i style="--i:${i}"></i>`).join("") : "";
    const tint = ["solid", "breathing", "pulse"].includes(e.id) ? ` style="--c:${scene.effect === e.id ? c : { solid: "#009bde", breathing: "#ff5a1f", pulse: "#8a3dff" }[e.id]}"` : "";
    return `<button class="fx" role="radio" aria-checked="${scene.effect === e.id}" data-effect="${e.id}" title="${esc(e.description)}">
      <span class="thumb t-${e.id}"${tint}>${extra}</span><span class="name">${esc(e.name)}</span></button>`;
  }).join(""));
}

$("#deck").addEventListener("click", async (e) => {
  const b = e.target.closest("[data-effect]");
  if (!b) return;
  if (selectedZone) { selectedZone = null; select(null); }
  try {
    const r = await post("/api/scene", { effect: b.dataset.effect });
    S.state.scene = r.scene;
    S.state.active_preset = null;
    renderDeck(); renderInspector(true);
  } catch (err) { fail(err); }
});

// ------------------------------------------------------------ inspector: scene
const sendScene = coalesce(async (patch) => {
  const r = await post("/api/scene", patch);
  S.state.scene = r.scene;
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

export function describeShort(rule) {
  const w = rule.when;
  return { app_running: `${w.exe} is running`, app_focused: `${w.exe} is in front`, idle: `idle ${w.minutes} min`,
           time: `${w.start} to ${w.end}`, locked: "PC locked", temp_above: `above ${w.temp}°` }[w.type] || "rule";
}

function presetsHTML() {
  const active = S.state.active_preset;
  return `<div class="ins-block"><div class="ins-title"><h2 style="font-size:15px">Presets</h2>
      <button class="text-link" id="edit-presets">Edit</button></div>
    <div class="presets">${S.presets.map((p) => `<button class="preset" data-preset="${esc(p.id)}" aria-pressed="${p.id === active}" style="--pc:${sceneColor(p)};--ps:${sceneSwatch(p)}">
      <span class="pdot"></span>${esc(p.name)}</button>`).join("")}
      <button class="preset" id="save-preset" title="Save the current lighting and fan curve as a preset"><svg width="14" height="14"><use href="#i-plus"/></svg>Save current</button></div></div>`;
}

function renderSceneInspector() {
  const scene = S.state.scene;
  const eff = S.effects.find((e) => e.id === scene.effect) || S.effects[0];
  const ins = $("#inspector");
  ins.innerHTML = `${overrideBanner()}${presetsHTML()}<div class="rule-line"></div>
    <div class="ins-block"><div class="ins-title"><h2>${esc(eff.name)}</h2><span class="meta">Whole rig</span></div>
      <p class="ins-desc">${esc(eff.description)}${eff.needs.includes("audio") ? " Plays from whatever your speakers or headphones are playing." : ""}${eff.needs.includes("screen") ? " Uses your main display. Nothing is recorded or saved." : ""}</p>
      <div class="ins-block" id="scene-fields">${eff.params.map((p) => fieldHTML(p, scene.params[p.id], "sp")).join("")}</div>
      <div class="field"><label class="lab" for="bri"><span>Brightness</span><output id="bri-out">${scene.brightness}%</output></label>
        <input type="range" id="bri" min="0" max="100" step="1" value="${scene.brightness}"></div>
    </div>`;
  wireFields($("#scene-fields"), eff, scene.params, (params, live) => {
    S.state.scene.params = params;
    if (live) sendScene({ params }); else post("/api/scene", { params }).then((r) => { S.state.scene = r.scene; renderSceneInspector(); renderDeck(); }).catch(fail);
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
  const p = e.target.closest("[data-preset]");
  if (p) {
    const preset = S.presets.find((x) => x.id === p.dataset.preset);
    try {
      const r = await post(`/api/presets/${encodeURIComponent(preset.id)}/apply`);
      const failed = r.steps.filter((s) => !s.ok);
      if (failed.length) toast(`${preset.name} is on, but ${failed[0].step} didn't apply: ${failed[0].error}`, true);
      else toast(`${preset.name} is on`);
      emit("refresh");
    } catch (err) { fail(err); }
    return;
  }
  if (e.target.closest("#save-preset")) return savePresetSheet();
  if (e.target.closest("#edit-presets")) return editPresetsSheet();
  const pause = e.target.closest("[data-pause]");
  if (pause) {
    try { await post("/api/automations/pause", { minutes: +pause.dataset.pause }); toast("Automations paused for an hour"); emit("refresh"); }
    catch (err) { fail(err); }
  }
});

function savePresetSheet() {
  openSheet(`<form id="sp-form"><h2>Save as preset</h2>
    <p class="hint">Saves the current effect, its settings, brightness and the ${esc(curveLabel(S.state.fans.base_curve))} fan curve.</p>
    <label class="field"><span class="lab">Name</span><input class="text" id="sp-name" required maxlength="40" autocomplete="off" placeholder="Movie night"></label>
    <p class="err" id="sp-err" hidden></p>
    <div class="actions"><span class="spacer"></span><button type="button" class="btn quiet" data-close>Cancel</button><button class="btn primary">Save preset</button></div></form>`);
  $("#sp-name").focus();
  $("#sp-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    try {
      const r = await post("/api/presets/from-current", { name: $("#sp-name").value.trim() });
      S.presets = r.presets;
      S.state.active_preset = r.id;
      closeSheet(); renderInspector(true); toast(`${$("#sp-name").value.trim()} saved`);
    } catch (err) { $("#sp-err").textContent = err.message; $("#sp-err").hidden = false; }
  });
}

function editPresetsSheet() {
  const draft = structuredClone(S.presets);
  const curves = Object.keys(S.curves.curves);
  const profiles = S.settings?.fan_profiles || [];
  const row = (p, i) => `<div class="field" data-row="${i}" style="grid-template-columns:auto 1fr auto auto;align-items:center;gap:10px;display:grid">
      <span class="pdot" style="width:14px;height:14px;border-radius:50%;background:${sceneSwatch(p)}"></span>
      <input class="text" data-name value="${esc(p.name)}" maxlength="40" aria-label="Preset name">
      <select class="select" data-curve style="width:auto" aria-label="Fan curve"><option value="">Fans unchanged</option>${curves.map((c) => `<option value="${c}"${c === p.fan_curve ? " selected" : ""}>${curveLabel(c)} fans</option>`).join("")}</select>
      <button type="button" class="icon-btn" data-del="${i}" title="Delete ${esc(p.name)}"><svg><use href="#i-trash"/></svg></button>
      ${profiles.length ? `<select class="select" data-fc style="grid-column:2/4" aria-label="FanControl profile"><option value="">Motherboard fans unchanged</option>${profiles.map((f) => `<option value="${esc(f)}"${f === p.fan_profile ? " selected" : ""}>FanControl: ${esc(f.replace(/\.json$/i, ""))}</option>`).join("")}</select>` : ""}
    </div>`;
  const render = () => {
    openSheet(`<form id="ep-form"><h2>Presets</h2><p class="hint">Rename presets, choose their fan curve, or delete them. To change a preset's lighting, apply it, adjust, and save it again under the same name.</p>
      <div class="ins-block" id="ep-rows">${draft.map(row).join("") || `<p class="hint">No presets.</p>`}</div>
      <p class="err" id="ep-err" hidden></p>
      <div class="actions"><span class="spacer"></span><button type="button" class="btn quiet" data-close>Cancel</button><button class="btn primary">Save presets</button></div></form>`);
    $("#ep-rows").addEventListener("click", (e) => {
      const d = e.target.closest("[data-del]");
      if (d) { collect(); draft.splice(+d.dataset.del, 1); render(); }
    });
    $("#ep-form").addEventListener("submit", async (e) => {
      e.preventDefault();
      collect();
      try { S.presets = await post("/api/presets", { presets: draft }); closeSheet(); renderInspector(true); toast("Presets saved"); }
      catch (err) { $("#ep-err").textContent = err.message; $("#ep-err").hidden = false; }
    });
  };
  const collect = () => $$("#ep-rows [data-row]").forEach((r) => {
    const p = draft[+r.dataset.row];
    p.name = r.querySelector("[data-name]").value.trim();
    p.fan_curve = r.querySelector("[data-curve]").value || null;
    const fc = r.querySelector("[data-fc]");
    if (fc) p.fan_profile = fc.value || null;
  });
  render();
}

// ------------------------------------------------------------ inspector: zone
function renderZoneInspector(key) {
  const z = S.zones.zones.find((x) => x.key === key);
  if (!z) { selectedZone = null; return renderSceneInspector(); }
  const ov = z.config.override;
  const eff = ov ? S.effects.find((e) => e.id === ov.effect) : null;
  const ins = $("#inspector");
  ins.innerHTML = `<div class="ins-block"><div class="ins-title"><h2>${esc(z.config.name)}</h2><button class="text-link" id="zone-done">Back to whole rig</button></div>
      <p class="ins-desc">${esc(z.device.name)}, ${z.positions.length} LED${z.positions.length === 1 ? "" : "s"}${z.config.enabled ? "" : ". This zone is switched off."}</p>
      <div class="btn-row"><button class="btn" id="zone-identify">Identify</button><a class="btn quiet" href="#devices/${encodeURIComponent(z.key)}">Zone settings</a></div></div>
    <div class="rule-line"></div>
    <div class="ins-block"><div class="field"><div class="lab"><span>Lighting for this zone</span></div>
      <div class="seg" role="radiogroup" id="zone-mode"><button role="radio" data-mode="follow" aria-checked="${!ov}">Main effect</button><button role="radio" data-mode="own" aria-checked="${!!ov}">Its own effect</button></div></div>
      ${ov ? `<div class="field"><label class="lab" for="zone-effect"><span>Effect</span></label><select class="select" id="zone-effect">${S.effects.map((e) => `<option value="${e.id}"${e.id === ov.effect ? " selected" : ""}>${esc(e.name)}</option>`).join("")}</select></div>
      <div class="ins-block" id="zone-fields">${eff.params.map((p) => fieldHTML(p, ov.params[p.id], "zp")).join("")}</div>` : `<p class="hint">Follows ${esc(effectName(S.state.scene.effect))} like the rest of the rig.</p>`}
    </div>`;
  const saveZone = coalesce(async (cfg) => {
    const r = await post(`/api/zones/${encodeURIComponent(z.key)}`, cfg);
    z.config = r.config;
  });
  $("#zone-done").addEventListener("click", () => { selectedZone = null; select(null); renderSceneInspector(); });
  $("#zone-identify").addEventListener("click", () => post(`/api/zones/${encodeURIComponent(z.key)}/identify`).then(() => toast("Watch that zone: the first LED blinks green, then a white dot walks through every LED in order.")).catch(fail));
  $("#zone-mode").addEventListener("click", async (e) => {
    const m = e.target.dataset.mode;
    if (!m) return;
    const override = m === "own" ? { effect: S.state.scene.effect, params: S.state.scene.params } : null;
    try { const r = await post(`/api/zones/${encodeURIComponent(z.key)}`, { ...z.config, override }); z.config = r.config; renderZoneInspector(key); }
    catch (err) { fail(err); }
  });
  $("#zone-effect")?.addEventListener("change", async (e) => {
    try { const r = await post(`/api/zones/${encodeURIComponent(z.key)}`, { ...z.config, override: { effect: e.target.value, params: {} } }); z.config = r.config; renderZoneInspector(key); }
    catch (err) { fail(err); }
  });
  if (ov) wireFields($("#zone-fields"), eff, ov.params, (params) => {
    z.config.override = { effect: ov.effect, params };
    saveZone({ ...z.config });
  });
}

// ------------------------------------------------------------ refresh
function renderInspector(force = false) {
  if (!force && Date.now() - editingParams < 2000) return;          // don't rebuild under the user's hands
  if (!force && document.activeElement && $("#inspector").contains(document.activeElement)) return;
  if (selectedZone) renderZoneInspector(selectedZone); else renderSceneInspector();
}

let lastKey = "";
export function renderLighting() {
  if (!S.state) return;
  renderDeck();
  // rebuild the inspector only when something it shows actually changed
  const key = JSON.stringify([S.state.scene, S.state.active_preset, S.state.override, S.presets.map((p) => p.id + p.name), selectedZone]);
  if (key !== lastKey) { lastKey = key; renderInspector(); }
}

export function initLighting() {
  on("select-zone", (key) => { selectedZone = key; lastKey = ""; renderInspector(true); });
  on("state", renderLighting);
}
