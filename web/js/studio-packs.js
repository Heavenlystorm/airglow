// Local scene pack files: review, choose conflicts, then explicitly install. No /use or scene mutation here.
import { $, $$, S, emit, esc, get, post, toast } from "./core.js";
import { openSheet } from "./sheet.js";

const AREAS = { all: "Whole case", front: "Front", top: "Top", rear: "Rear", bottom: "Bottom", side: "Side panel",
  cooler: "CPU cooler", ram: "Memory", board: "Motherboard", gpu: "Graphics card", desk: "Desk" };
const RECEIPTS = "airglow.studio.packs.v1";
const MAX_BYTES = 240 * 1024;

export function serializePack(pack) {
  const text = JSON.stringify(pack, null, 2);
  if (new TextEncoder().encode(text).length > MAX_BYTES)
    throw new Error("This pack exceeds 240 KiB when formatted. Select fewer effects and download again.");
  return text;
}

function receipts() {
  try {
    const raw = localStorage.getItem(RECEIPTS) || "[]";
    if (raw.length > 65536) return [];
    const values = JSON.parse(raw);
    return Array.isArray(values) ? values.filter(p => p && typeof p.name === "string" && Array.isArray(p.areas)).slice(0, 12) : [];
  } catch { return []; }
}

export function packHistoryHTML() {
  const packs = receipts();
  if (!packs.length) return "";
  return `<section class="st-sec st-pack-history"><h2 class="st-h">Imported scene packs</h2>
    <p class="hint">Use the named areas as a setup guide on any rig. Importing leaves your current lights unchanged.</p>
    ${packs.map(p => `<details class="st-details"><summary>${esc(p.name)}</summary>
      ${p.areas.length ? `<ul>${p.areas.map(a => `<li><span>${esc(AREAS[a.area] || a.area)}</span>
        <button class="btn quiet" data-pack-open="${esc(a.fx_id)}">Preview effect</button>
        ${Object.keys(a.values || {}).length ? `<span class="hint">Controls: ${esc(JSON.stringify(a.values))}</span>` : ""}</li>`).join("")}</ul>`
        : `<p class="hint">Effects were added to your library without area assignments.</p>`}</details>`).join("")}</section>`;
}

export async function scenePackExport({ effects, selected = [], download }) {
  if (!effects.length) return toast("Save an effect before making a scene pack.", true);
  const ids = new Set(selected.length ? selected : effects.map(e => e.fx_id).slice(0, 12));
  openSheet(`<form class="body st-pack-form" id="st-pack-export"><h2>Export a scene pack</h2>
    <p>Include saved effects and optional assignments to named areas. The file works on another rig without device IDs.</p>
    <label class="field"><span class="lab">Pack name</span><input class="text" name="pack-name" maxlength="80" required value="My scene pack"></label>
    <fieldset><legend>Effects (up to 12)</legend><div class="st-pack-effects">${effects.map(e => `<label class="check"><input type="checkbox" name="pack-effect" value="${esc(e.fx_id)}"${ids.has(e.fx_id) ? " checked" : ""}> ${esc(e.name)}</label>`).join("")}</div></fieldset>
    <details class="st-details"><summary>Named areas and current control values</summary><p class="hint">An empty area is left unassigned. Values are included only when this effect is currently selected for that area.</p>
    ${Object.entries(AREAS).map(([area, name]) => {
      const scene = area === "all" ? S.state?.scene : S.state?.scene?.areas?.[area];
      return `<label class="st-pack-area"><span>${name}</span><select class="select" data-pack-area="${area}"><option value="">Unassigned</option>${effects.map(e => `<option value="${esc(e.fx_id)}"${ids.has(e.fx_id) && scene?.effect === e.fx_id ? " selected" : ""}>${esc(e.name)}</option>`).join("")}</select></label>`;
    }).join("")}</details>
    <p class="hint" id="st-pack-error" role="status"></p><div class="actions"><button type="button" class="btn quiet" data-close>Cancel</button><span class="spacer"></span><button class="btn primary">Download pack</button></div></form>`);
  $("#st-pack-export").addEventListener("submit", async ev => {
    ev.preventDefault();
    const form = ev.currentTarget, button = form.querySelector('button[type="submit"], button.primary');
    button.disabled = true;
    try {
      const chosen = [...form.querySelectorAll('[name="pack-effect"]:checked')].map(el => el.value);
      const areas = [...form.querySelectorAll("[data-pack-area]")].filter(el => el.value).map(el => {
        const area = el.dataset.packArea, scene = area === "all" ? S.state?.scene : S.state?.scene?.areas?.[area];
        return { area, fx_id: el.value, values: scene?.effect === el.value ? scene.params || {} : {} };
      });
      const { pack } = await post("/api/studio/packs/export", { name: form.elements["pack-name"].value, fx_ids: chosen, areas });
      download("airglow-scene-pack.json", serializePack(pack), "application/json");
      form.querySelector("#st-pack-error").textContent = "Downloaded. Import this file in Studio on the other rig.";
    } catch (err) { form.querySelector("#st-pack-error").textContent = err.message; }
    finally { button.disabled = false; }
  });
}

export async function scenePackImport(file, onImported) {
  if (!file) return;
  if (file.size > MAX_BYTES) return toast("A scene pack can be at most 240 KiB.", true);
  const sheet = openSheet(`<div class="body"><h2>Review scene pack</h2><p id="st-pack-loading" role="status">Validating the file and its effect sources...</p><div class="actions"><button class="btn quiet" data-close>Close</button></div></div>`);
  const loading = $("#st-pack-loading", sheet);
  const reviewing = () => sheet.open && $("#st-pack-loading", sheet) === loading;
  try {
    const pack = JSON.parse(await file.text());
    if (!reviewing()) return;
    const preview = await post("/api/studio/packs/preview", { pack });
    if (!reviewing()) return;
    openSheet(`<form class="body st-pack-form" id="st-pack-import"><h2>${esc(preview.name)}</h2>
      <p>Review ${preview.effects.length} effect${preview.effects.length === 1 ? "" : "s"} before installing. Your lights will stay as they are.</p>
      <ul class="st-pack-review">${preview.effects.map(e => `<li><div><b>${esc(e.name)}</b><p class="hint">${e.kind === "code" ? "Code effect" : "Layers"} · ${e.bytes} bytes${e.author ? ` · by ${esc(e.author)}` : ""}${e.license ? ` · ${esc(e.license)}` : " · Local effect, no publishing licence"}</p>
        ${e.remix_of ? `<p class="hint">Remix of ${esc(e.remix_of)}</p>` : ""}${e.flags?.length ? `<p class="hint">Review: ${esc(e.flags.join("; "))}. Effects run in the existing sandbox.</p>` : ""}</div>
        ${e.conflict !== "none" ? `<label><span class="lab">${e.conflict === "same" ? "Already installed" : "Different version exists"}</span><select class="select" data-pack-choice="${esc(e.key)}">${e.conflict === "different" ? '<option value="">Choose an action</option>' : ""}<option value="keep">Keep existing</option><option value="copy">Import a copy</option></select></label>` : '<span class="hint">Add to library</span>'}</li>`).join("")}</ul>
      <h3>Named area assignments</h3>${preview.areas.length ? `<ul>${preview.areas.map(a => `<li>${esc(AREAS[a.area] || a.area)} — ${esc(preview.effects.find(e => e.key === a.effect)?.name)}${Object.keys(a.values).length ? ` · ${esc(JSON.stringify(a.values))}` : ""}</li>`).join("")}</ul>` : '<p class="hint">No area assignments.</p>'}
      <p class="hint">Assignments are a portable setup guide. Effects are installed locally; applying one is a separate action.</p>
      <p class="hint" id="st-pack-error" role="status"></p><div class="actions"><button type="button" class="btn quiet" data-close>Cancel</button><span class="spacer"></span><button class="btn primary" id="st-pack-confirm">Import effects</button></div></form>`);
    const form = $("#st-pack-import", sheet), button = $("#st-pack-confirm", form), status = $("#st-pack-error", form);
    const current = () => sheet.open && $("#st-pack-import", sheet) === form;
    form.addEventListener("submit", async ev => {
      ev.preventDefault();
      if (!current() || button.disabled) return;
      button.disabled = true;
      try {
        const choices = Object.fromEntries($$("[data-pack-choice]", ev.currentTarget).map(el => [el.dataset.packChoice, el.value]));
        const result = await post("/api/studio/packs/import", { pack, token: preview.token, choices, confirm: true });
        let remembered = true;
        try { localStorage.setItem(RECEIPTS, JSON.stringify([{ name: result.name, areas: result.areas }, ...receipts()].slice(0, 12))); }
        catch { remembered = false; }
        S.effects = await get("/api/effects"); emit("refresh");
        if (current()) {
          status.textContent = `${result.installed.length} effects added. Your lights are unchanged.${remembered ? " Area assignments are saved in Studio." : " Area assignments could not be saved locally; keep this pack file."}`;
          button.textContent = "Imported";
        }
        await onImported?.();
      } catch (err) { if (current()) { status.textContent = err.message; button.disabled = false; } }
    });
  } catch (err) {
    if (reviewing()) loading.textContent = `Could not import: ${err.message}`;
  }
}
