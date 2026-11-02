// Local rig files: validate first, explicitly map every source zone, then apply the reviewed result.
import { $, $$, S, emit, esc, get, post, refreshZones, toast } from "./core.js";
import { openSheet } from "./sheet.js";
import { applyDesign } from "./design.js";

const MAX_BYTES = 192 * 1024;

export async function readRigFile(file) {
  if (!file || file.size > MAX_BYTES) throw new Error("Choose a rig JSON file of at most 192 KiB");
  const text = await file.text();
  if (new TextEncoder().encode(text).length > MAX_BYTES) throw new Error("Rig files may be at most 192 KiB");
  let bundle;
  try { bundle = JSON.parse(text); } catch { throw new Error("This file is not valid JSON"); }
  if (!bundle || typeof bundle !== "object" || Array.isArray(bundle)) throw new Error("Choose an Airglow rig JSON object");
  return bundle;
}

export async function exportRig() {
  const bundle = await get("/api/rig/export");
  const blob = new Blob([JSON.stringify(bundle, null, 2)], { type: "application/json" });
  if (blob.size > MAX_BYTES) throw new Error("This rig exceeds the 192 KiB portable bundle limit");
  const url = URL.createObjectURL(blob), link = document.createElement("a");
  link.href = url;
  link.download = `airglow-rig-${new Date().toISOString().slice(0, 10)}.json`;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
  toast("Rig bundle downloaded");
}

function changeList(title, entries, format) {
  return `<details class="rig-change"><summary>${entries.length} ${title}</summary>${entries.length
    ? `<ul>${entries.map((entry) => `<li>${format(entry)}</li>`).join("")}</ul>` : `<p class="hint">None in this bundle.</p>`}</details>`;
}

function previewMarkup(preview, mapping) {
  const changes = preview.changes;
  return `<section aria-labelledby="rig-mapping-title"><h3 id="rig-mapping-title">Map lighting zones</h3>
    <p class="hint">Choose a target or Skip for every source zone. Zone names, area assignments and layouts are copied to your choices. Each target keeps its current enabled setting.</p>
    <div class="rig-zone-grid">${preview.zones.map((zone, index) => {
      const chosen = Object.hasOwn(mapping, zone.id) ? (mapping[zone.id] === null ? "skip" : `target:${mapping[zone.id]}`) : "";
      return `<label class="rig-zone" for="rig-zone-${index}"><span><b>${esc(zone.name)}</b><small>${esc(zone.device)} · ${esc(zone.area)} · ${zone.leds} LEDs</small></span>
        <select class="select" id="rig-zone-${index}" data-rig-source="${esc(zone.id)}" aria-describedby="rig-zone-status-${index}">
          <option value=""${!chosen ? " selected" : ""}>Choose a target…</option>
          <option value="skip"${chosen === "skip" ? " selected" : ""}>Skip this zone</option>
          ${preview.targets.map((target) => `<option value="target:${esc(target.key)}"${chosen === `target:${target.key}` ? " selected" : ""}>${esc(target.device)} / ${esc(target.name)} (${target.max_leds} LEDs max)${target.online ? "" : " — offline"}</option>`).join("")}
        </select><small id="rig-zone-status-${index}" class="rig-zone-status">${zone.status === "unresolved" ? "A mapping choice is required." : zone.status === "skipped" ? "Skipped; no local zone will change." : zone.changes.length ? `Changes: ${zone.changes.map(esc).join(", ")}` : "Mapped; zone settings already match."}</small></label>`;
    }).join("") || `<p class="hint">This bundle has no lighting zones to map.</p>`}</div></section>
    <section aria-labelledby="rig-changes-title"><h3 id="rig-changes-title">Review changes</h3>
      ${changeList("settings changes", changes.settings, (change) => `${esc(change.key.replaceAll("_", " "))}: ${esc(change.before)} → <b>${esc(change.after)}</b>`)}
      ${changeList("profiles to add", changes.profiles, (change) => `${esc(change.name)}${change.from !== change.to ? ` (new identity: ${esc(change.to)})` : ""}`)}
      ${changeList("curves in the bundle", changes.curves, (change) => `${esc(change.from)} → ${esc(change.to)} · ${change.created ? "saved as a new curve" : "identical local curve kept"}`)}
      ${changeList("automations to add, disabled", changes.automations, (change) => `${esc(change.to)} · review before enabling`)}
      ${preview.warnings.length ? `<ul class="rig-warnings">${preview.warnings.map((warning) => `<li>${esc(warning)}</li>`).join("")}</ul>` : ""}
      <p class="hint">${esc(preview.preserved || "Fan and pump ownership, active curves, safety limits, helper paths and startup settings are preserved.")}</p>
      <p class="hint">${esc(preview.persistence || "Ordinary write failures roll back. Power loss can interrupt a multi-file import.")}</p>
    </section>`;
}

export function openRigImport() {
  const sheet = openSheet(`<div class="body wide rig-import" id="rig-import"><h2>Import rig setup</h2>
    <p class="lede">Review a local bundle and map its lighting zones to this rig.</p>
    <label class="field"><span class="lab">Airglow rig bundle · JSON, up to 192 KiB</span><input class="text" type="file" id="rig-file" accept=".json,application/json"></label>
    <p class="hint" id="rig-status" role="status" aria-live="polite">Choose a file to validate it. Preview does not change any settings.</p>
    <p class="err" id="rig-error" role="alert" hidden></p><div id="rig-preview"></div>
    <div class="actions"><button type="button" class="btn quiet" data-close>Cancel</button><span class="spacer"></span>
      <button type="button" class="btn" id="rig-recheck" disabled>Preview again</button>
      <button type="button" class="btn primary" id="rig-apply" disabled>Apply reviewed import</button></div></div>`);
  const root = $("#rig-import"), previewBox = $("#rig-preview"), fileInput = $("#rig-file");
  let bundle = null, mapping = {}, reviewed = null, sequence = 0, applying = false;
  const current = () => sheet.open && $("#rig-import") === root;
  const error = (message) => {
    $("#rig-error").textContent = message;
    $("#rig-error").hidden = !message;
  };
  const controls = () => {
    $("#rig-apply").disabled = applying || !reviewed?.ready || !reviewed?.preview_token;
    $("#rig-recheck").disabled = applying || !bundle;
    fileInput.disabled = applying;
    previewBox.querySelectorAll("select").forEach((select) => { select.disabled = applying; });
  };
  const preview = async () => {
    if (!bundle || applying) return;
    const generation = ++sequence, selected = structuredClone(mapping), source = bundle;
    const focused = document.activeElement?.dataset?.rigSource;
    reviewed = null;
    controls(); error("");
    $("#rig-status").textContent = "Validating the bundle and mapping…";
    try {
      const result = await post("/api/rig/preview", { bundle: source, mapping: selected });
      if (generation !== sequence || !current()) return;
      reviewed = result;
      previewBox.innerHTML = previewMarkup(result, selected);
      $("#rig-status").textContent = result.ready ? "Ready to apply. Review the changes below."
        : `${result.unresolved.length} zone${result.unresolved.length === 1 ? " needs" : "s need"} a mapping choice.`;
      if (focused) $$("[data-rig-source]", previewBox).find((element) => element.dataset.rigSource === focused)?.focus({ preventScroll: true });
    } catch (failure) {
      if (generation !== sequence || !current()) return;
      error(`${failure.message}. Your file and mapping choices are kept; correct them and preview again.`);
      $("#rig-status").textContent = "Preview failed; nothing was imported.";
    } finally {
      if (generation === sequence && current()) controls();
    }
  };
  controls();
  fileInput.addEventListener("change", async () => {
    const generation = ++sequence;
    bundle = null; mapping = {}; reviewed = null;
    previewBox.innerHTML = "";
    controls(); error("");
    try {
      const selected = await readRigFile(fileInput.files?.[0]);
      if (generation !== sequence || !current()) return;
      bundle = selected;
      await preview();
    } catch (failure) {
      if (generation !== sequence || !current()) return;
      error(failure.message);
      $("#rig-status").textContent = "File not loaded.";
    }
  });
  previewBox.addEventListener("change", async (event) => {
    const source = event.target.dataset.rigSource;
    if (!source || applying) return;
    if (event.target.value === "skip") mapping[source] = null;
    else if (event.target.value.startsWith("target:")) mapping[source] = event.target.value.slice(7);
    else delete mapping[source];
    await preview();
  });
  $("#rig-recheck").addEventListener("click", preview);
  $("#rig-apply").addEventListener("click", async () => {
    if (applying || !reviewed?.ready || !reviewed?.preview_token) return;
    const body = { bundle, mapping: structuredClone(mapping), preview_token: reviewed.preview_token };
    applying = true; controls(); error("");
    $("#rig-status").textContent = "Applying the reviewed import…";
    let result;
    try {
      result = await post("/api/rig/apply", body);
      reviewed = null;
    } catch (failure) {
      reviewed = null;
      if (current()) {
        error(`${failure.message}. Your file and mapping are kept. Preview again before retrying; after a timeout, check your profiles first.`);
        $("#rig-status").textContent = "Import could not be confirmed.";
      }
    } finally {
      if (!result) applying = false;
      if (current()) controls();
    }
    if (!result) return;
    // The import has succeeded; a failed refresh must not suggest repeating a non-idempotent import.
    const refresh = await Promise.allSettled([get("/api/presets"), get("/api/curves"), get("/api/automations"),
      get("/api/settings"), refreshZones()]);
    ["presets", "curves", "rules", "settings"].forEach((key, index) => {
      if (refresh[index].status === "fulfilled") S[key] = refresh[index].value;
    });
    if (S.settings?.settings?.design) applyDesign(S.settings.settings.design);
    emit("refresh"); emit("zones"); emit("rig:imported");
    if (current()) openSheet(`<div class="body rig-import"><h2>Rig setup imported</h2>
      <p class="lede">${result.changes.profiles.length} profiles added, ${result.zones.filter((zone) => zone.status === "mapped").length} zones mapped, ${result.skipped.length} zones skipped.</p>
      <p>Imported automations are disabled. Review them in Automations before enabling them.</p>
      ${refresh.some((entry) => entry.status === "rejected") ? `<p class="hint">The import was saved, but part of the screen could not refresh. Reload Airglow to see it; do not repeat the import.</p>` : ""}
      <div class="actions"><span class="spacer"></span><button type="button" class="btn primary" data-close>Done</button></div></div>`);
    toast("Rig setup imported");
  });
  fileInput.focus();
}
