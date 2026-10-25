// Lighting screen: effect deck, inspector (scene or a selected zone), profiles.

import { $, $$, QUICK, S, coalesce, curveLabel, effectName, emit, esc, fail, fmtTemp, get, on, post, rangeFill, sceneColor, sceneSwatch, setHTML, toast } from "./core.js";
import { select } from "./stage.js";
import { openSheet, closeSheet } from "./sheet.js";
import { editProfilesSheet, openGallery, saveProfileSheet } from "./profiles.js";

let selectedZone = null;
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
  const id = `${prefix}-${spec.id}`;
  if (spec.type === "color") {
    return `<div class="field" data-param="${spec.id}"><div class="lab"><span>${esc(spec.label)}</span>${opts.reset ? resetBtn(spec) : ""}</div>
      <div class="color-row"><label class="swatch" style="background:${value}"><input type="color" id="${id}" value="${value}" aria-label="${esc(spec.label)}"></label>
      <div class="quick">${QUICK.map((c) => `<button type="button" data-quick="${c}" style="background:${c}" aria-label="${c}" aria-pressed="${c === value}"></button>`).join("")}</div></div></div>`;
  }
  if (spec.type === "colors") {
    const sw = value.map((c, i) => `<label class="swatch" style="background:${c}"><input type="color" data-stop="${i}" value="${c}" aria-label="Colour ${i + 1}"></label>`).join("");
    return `<div class="field" data-param="${spec.id}"><div class="lab"><span>${esc(spec.label)}</span>${opts.reset ? resetBtn(spec) : ""}</div>
      <div class="stops">${sw}
      ${value.length < spec.max ? `<button type="button" class="icon-btn" data-add-stop title="Add a colour"><svg><use href="#i-plus"/></svg></button>` : ""}
      ${value.length > spec.min ? `<button type="button" class="icon-btn" data-remove-stop title="Remove the last colour"><svg><use href="#i-trash"/></svg></button>` : ""}</div></div>`;
  }
  if (spec.type === "range") {
    return `<div class="field" data-param="${spec.id}"><div class="lab"><label for="${id}"><span>${esc(spec.label)}</span></label>
      <span class="val"><input type="number" class="num" id="${id}-num" min="${spec.min}" max="${spec.max}" step="${spec.step}" value="${value}" aria-label="${esc(spec.label)}" inputmode="decimal">${spec.unit ? `<span class="unit">${esc(spec.unit)}</span>` : ""}${opts.reset ? resetBtn(spec) : ""}</span></div>
      <input type="range" id="${id}" min="${spec.min}" max="${spec.max}" step="${spec.step}" value="${value}"></div>`;
  }
  if (spec.type === "choice") {
    return `<div class="field" data-param="${spec.id}"><div class="lab"><span>${esc(spec.label)}</span>${opts.reset ? resetBtn(spec) : ""}</div>
      <select class="select" id="${id}" aria-label="${esc(spec.label)}">${spec.options.map((o) => `<option value="${esc(o.value)}"${o.value === value ? " selected" : ""}>${esc(o.label)}</option>`).join("")}</select></div>`;
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
  const scene = S.state.scene;
  const c = sceneColor(scene);
  const deck = $("#deck"), before = deck._html;
  setHTML(deck, S.effects.map((e) => {
    const extra = e.id === "spectrum" ? Array.from({ length: 7 }, (_, i) => `<i style="--i:${i}"></i>`).join("") : "";
    const tint = ["solid", "breathing", "pulse"].includes(e.id) ? ` style="--c:${scene.effect === e.id ? c : { solid: "#009bde", breathing: "#ff5a1f", pulse: "#8a3dff" }[e.id]}"` : "";
    const thumb = e.studio ? `t-studio${e.studio.permissions.includes("audio") ? " t-music" : ""}` : `t-${e.id}`;
    return `<button class="fx" role="radio" aria-checked="${scene.effect === e.id}" data-effect="${esc(e.id)}" title="${esc(e.description || "Made in the Studio")}">
      <span class="thumb ${thumb}"${tint}>${extra}</span><span class="name">${esc(e.name)}</span></button>`;
  }).join("") + `<a class="fx fx-new" href="#studio/new" title="Make your own effect in the Studio"><span class="thumb t-new" aria-hidden="true">+</span><span class="name">Create your own</span></a>`);
  if (deck._html !== before) fitNames();
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
           time: `${w.start} to ${w.end}`, locked: "PC locked", temp_above: `above ${fmtTemp(w.temp)}` }[w.type] || "rule";
}

function presetsHTML() {
  const active = S.state.active_preset;
  return `<div class="ins-block"><div class="ins-title"><h2 style="font-size:15px">Profiles</h2>
      <span class="btn-row" style="gap:14px"><button class="text-link" id="browse-profiles">Browse</button><button class="text-link" id="edit-presets">Edit</button></span></div>
    <div class="presets">${S.presets.map((p) => `<button class="preset" data-preset="${esc(p.id)}" aria-pressed="${p.id === active}" style="--pc:${sceneColor(p)};--ps:${sceneSwatch(p)}">
      <span class="pdot"></span>${esc(p.name)}</button>`).join("")}
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
  ins.innerHTML = `${overrideBanner()}${presetsHTML()}<div class="rule-line"></div>
    <div class="ins-block"><div class="ins-title"><h2>${esc(eff.name)}</h2></div>
      <div class="zone-line"><label for="zone-pick">Lighting for</label><select class="select zone-pick" id="zone-pick" aria-label="Choose a lighting zone">
      <option value="">Whole rig</option>${(S.zones?.zones || []).filter((z) => z.device.online).map((z) => `<option value="${esc(z.key)}">${esc(z.config.name)} · ${esc(z.device.name)}</option>`).join("")}
    </select></div>
      <p class="ins-desc">${esc(eff.description)}${eff.needs.includes("audio") ? " Plays from whatever your speakers or headphones are playing." : ""}${eff.needs.includes("screen") ? " Uses your main display. Nothing is recorded or saved." : ""}</p>
      ${studioButton(eff)}
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
  $("#zone-pick")?.addEventListener("change", (e) => {
    const key = e.target.value;
    if (!key) { selectedZone = null; select(null); renderInspector(true); return; }
    selectedZone = key; select(key); renderInspector(true);
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
  if (e.target.closest("#save-preset")) return saveProfileSheet(() => renderInspector(true));
  if (e.target.closest("#edit-presets")) return editProfilesSheet(() => renderInspector(true));
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
  const z = S.zones.zones.find((x) => x.key === key);
  if (!z) { selectedZone = null; return renderSceneInspector(); }
  const ov = z.config.override;
  const eff = ov ? (S.effects.find((e) => e.id === ov.effect) || null) : null;   // the effect may be deleted
  const ins = $("#inspector");
  ins.innerHTML = `<div class="ins-block"><div class="ins-title"><h2>${esc(z.config.name)}</h2><button class="text-link" id="zone-done">Back to whole rig</button></div>
      <p class="ins-desc">${esc(z.device.name)}, ${z.positions.length} LED${z.positions.length === 1 ? "" : "s"}${z.config.enabled ? "" : ". This zone is switched off."}</p>
      <div class="btn-row"><button class="btn" id="zone-identify">Identify</button><a class="btn quiet" href="#devices/${encodeURIComponent(z.key)}">Zone settings</a></div></div>
    <div class="rule-line"></div>
    <div class="ins-block"><div class="field"><div class="lab"><span>Lighting for this zone</span></div>
      <div class="seg" role="radiogroup" id="zone-mode"><button role="radio" data-mode="follow" aria-checked="${!ov}">Main effect</button><button role="radio" data-mode="own" aria-checked="${!!ov}">Its own effect</button></div></div>
      ${ov ? `<div class="field"><label class="lab" for="zone-effect"><span>Effect</span></label><select class="select" id="zone-effect">${S.effects.map((e) => `<option value="${e.id}"${e.id === ov.effect ? " selected" : ""}>${esc(e.name)}</option>`).join("")}</select></div>
      <div class="ins-block" id="zone-fields">${eff ? eff.params.map((p) => fieldHTML(p, ov.params[p.id], "zp")).join("")
        : '<p class="hint">That effect isn\'t on this PC any more. Pick another one to give this zone its own light.</p>'}</div>` : `<p class="hint">Follows ${esc(effectName(S.state.scene.effect))} like the rest of the rig.</p>`}
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
  if (ov && eff) wireFields($("#zone-fields"), eff, ov.params, (params) => {
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
  new ResizeObserver(fitNames).observe($("#deck"));
  document.fonts?.ready.then(fitNames);
  on("select-zone", (key) => { selectedZone = key; lastKey = ""; renderInspector(true); });
  on("state", renderLighting);
}
