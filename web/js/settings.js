// Settings screen.

import { $, $$, S, emit, esc, fail, get, on, post, rangeFill, setHTML, toast } from "./core.js";
import { paintTools, refreshTools } from "./helpers.js";
import { openWelcome } from "./onboarding.js";
import { openShortcuts } from "./help.js";
import { openSheet, closeSheet } from "./sheet.js";
import { DESIGNS, applyDesign, currentDesign } from "./design.js";
import { DEFAULT_APPEARANCE, applyAppearance, normalizeAppearance, createAppearanceTransaction,
  serializeWrites, parseLookRecipe, encodeLookRecipe } from "./appearance.js";
import { exportRig, openRigImport } from "./rig-migration.js";

const SECTIONS = [["general", "General"], ["lighting", "Lighting"], ["fans", "Fans"], ["helpers", "Helper programs"],
  ["sensors", "Sensors"], ["devices", "More devices"], ["migration", "Rig migration"], ["about", "About"]];

const row = (label, help, ctl) => `<div class="set-row"><div><div class="label">${label}</div>${help ? `<p>${help}</p>` : ""}</div><div class="ctl">${ctl}</div></div>`;
const sw = (id, on, label) => `<label class="switch"><input type="checkbox" id="${id}"${on ? " checked" : ""}${label ? ` aria-label="${esc(label)}"` : ""}><span></span></label>`;

// ------------------------------------------------------------ updates
const ONLINE_HELP = "Looks for updates a minute after Airglow starts and every 6 hours, and downloads them in the background. When this is off, Airglow only goes online when you press a button.";

const RELEASES = `<a href="https://github.com/Heavenlystorm/airglow/releases" target="_blank" rel="noopener">releases page</a>`;

// why this copy can't install the update itself, and where to get it
function selfUpdate(kind) {
  if (kind === "source") return `You're running Airglow from source: pull the latest code, or get a release from the ${RELEASES}.`;
  if (kind === "build") return `This is a build that wasn't installed (like dist\\Airglow from build.ps1), so it can't update itself. Get the new version from the ${RELEASES}.`;
  return `This copy can't update itself: get the new version from the ${RELEASES}.`;
}

function updateText(u) {
  switch (u.state) {
    case "checking": return "Checking GitHub for a newer release.";
    case "downloading": return `Downloading Airglow ${esc(u.latest)}: ${u.progress}%.`;
    case "ready": return `Airglow ${esc(u.latest)} is downloaded. It's installed the next time Airglow starts, or now if you restart.`;
    case "latest": return `You have the latest version (${esc(u.current)}).`;
    case "available": return `Airglow ${esc(u.latest)} is available. ${selfUpdate(u.kind)}`;
    case "error": return `The last check didn't work: ${esc(u.error)}`;
    default: return u.kind === "velopack" ? "Checks GitHub for a newer release and installs it." : "Checks GitHub for a newer release.";
  }
}

function updateButton(u) {
  if (u.state === "ready") return `<button class="btn primary" id="s-restart">Restart to update</button>`;
  const busy = u.state === "checking" || u.state === "downloading";
  return `<button class="btn" id="s-update"${busy ? " disabled" : ""}>Check for updates</button>`;
}

function updateRows(v) {
  const u = S.state?.update || v.update;
  if (!v.updates_available || !u) return "";
  if (u.kind === "scoop") return row("Updates", `Installed with Scoop, which keeps it up to date: <span class="path">scoop update airglow</span>`, "");
  const s = v.settings;
  const channel = s.update_channel;
  return row("Check automatically", ONLINE_HELP, sw("s-online", s.online_checks, "Check automatically"))
    + (u.kind === "velopack" ? row("Update channel", "Beta gets new features first, and the stable releases too. Going back to Stable keeps this version until a stable release is newer.",
      `<div class="seg" id="s-channel" role="radiogroup" aria-label="Update channel"><button role="radio" data-v="stable" aria-checked="${channel === "stable"}">Stable</button><button role="radio" data-v="beta" aria-checked="${channel === "beta"}">Beta</button></div>`) : "")
    + `<div id="s-update-row">${row("Updates", updateText(u), updateButton(u))}</div>`;
}

// the status row follows the live state while Settings is open
function paintUpdate() {
  const box = $("#s-update-row");
  const u = S.state?.update;
  if (!box || !u) return;
  setHTML(box, row("Updates", updateText(u), updateButton(u)));
}

// ------------------------------------------------------------ interface designs
// A miniature of the app, not a colour chip: each card shows its design's own tokens and structural marks. It is
// decorative (aria-hidden), so it has no controls and no focus stops inside the card's preview.
function previewHTML() {
  return `<span class="dsg-bar"><span class="dsg-mark"></span><span class="dsg-tab on">Overview</span><span class="dsg-tab">Fans</span><span class="dsg-pill"></span></span>
    <span class="dsg-panel"><span class="dsg-label">Processor</span><span class="dsg-num">46<span class="dsg-unit">°C</span></span>
      <svg class="dsg-spark" viewBox="0 0 120 24" preserveAspectRatio="none" aria-hidden="true"><path d="M0 18L18 14L36 17L54 9L72 12L90 6L120 10"/></svg></span>
    <span class="dsg-ctl"><span class="dsg-btn">Save</span><span class="dsg-switch on"></span><span class="dsg-meter"><i></i></span></span>`;
}

function markDesign(id) {
  const d = $("#sheet");
  if (!d) return;
  d.querySelectorAll("[data-design-card]").forEach((card) => {
    const on = card.dataset.designCard === id;
    card.setAttribute("aria-checked", String(on));
    let mark = card.querySelector(".dsg-inuse");
    if (on && !mark) {
      mark = document.createElement("span");
      mark.className = "dsg-inuse";
      mark.textContent = "In use";
      card.querySelector(".dsg-name")?.append(mark);
    } else if (!on && mark) {
      mark.remove();
    }
  });
}

// One write at a time, so two quick clicks cannot land out of order and leave the server on the first design.
let designQueue = Promise.resolve();
let designRevision = 0, pendingDesign = null;
function pickDesign(id) {
  const revision = ++designRevision;
  const chosen = applyDesign(id, { persist: false });
  pendingDesign = chosen;
  markDesign(chosen);
  designQueue = save({ design: chosen }, "Design saved")
    .then(() => {
      if (revision !== designRevision) return;
      pendingDesign = null;
      const confirmed = S.settings?.settings?.design || chosen;
      applyDesign(confirmed);
      markDesign(confirmed);
    })
    .catch(() => {});
}

function designSheet() {
  const current = currentDesign();
  openSheet(`<div class="body design-sheet">
    <h2>Interface design</h2>
    <p class="lede">Changes how Airglow looks. Your lights stay their own colours in every design.</p>
    <div class="design-gallery" role="radiogroup" aria-label="Interface design">${DESIGNS.map((d) => `<button type="button" role="radio" class="dsg-card" data-design-card="${d.id}" aria-checked="${d.id === current}">
      <span class="dsg-preview" data-design="${d.id}" aria-hidden="true">${previewHTML()}</span>
      <span class="dsg-name">${esc(d.name)}${d.id === current ? `<span class="dsg-inuse">In use</span>` : ""}</span>
      <span class="dsg-blurb">${esc(d.blurb)}</span></button>`).join("")}</div>
    <div class="actions"><span class="spacer"></span><button type="button" class="btn primary" data-close>Done</button></div></div>`);
  const d = $("#sheet");
  d.querySelectorAll("[data-design-card]").forEach((card) => {
    card.addEventListener("click", () => pickDesign(card.dataset.designCard));
  });
  // openSheet has already opened the modal; deferred focus can override the user's next keypress.
  (d.querySelector('[aria-checked="true"]') || d.querySelector("[data-design-card]"))?.focus({ preventScroll: true });
}

// This sheet edits one draft, including its saved-look library. Only Keep writes it.
let appearanceSession = null;
const appearanceName = (value) => {
  const a = normalizeAppearance(value);
  return `${a.scale}% · ${a.density === "compact" ? "Compact" : "Comfortable"} · ${a.rig === "frame" ? "Framed rig" : "Minimal rig"}`;
};
const setPresentation = (value, persist = false) => {
  applyDesign(value.design, { persist });
  applyAppearance(value.appearance, { persist });
};

export async function openAppearance() {
  // A prior theme selection must be confirmed before taking the Cancel baseline.
  await designQueue;
  await pendingSettings;
  if (appearanceSession && !appearanceSession.closed) return;
  const initial = S.settings?.settings;
  if (!initial) return;
  const tx = createAppearanceTransaction(initial, {
    apply: (value) => setPresentation(value),
    save: async (value) => (await save(value, "Personal look saved", true)).settings,
  });
  appearanceSession = tx;
  const a = tx.draft.appearance;
  const d = openSheet(`<form id="appearance-form" class="appearance-sheet">
    <div class="appearance-heading"><div><h2>Your personal look</h2><p class="lede">Try it across Airglow. Keep what feels right.</p></div><span class="appearance-state">Live preview</span></div>
    <div class="appearance-editor">
      <div class="appearance-controls">
        <label class="appearance-field" for="ap-design"><span>Starting design</span><select class="select" id="ap-design">${DESIGNS.map((item) => `<option value="${item.id}"${item.id === tx.draft.design ? " selected" : ""}>${esc(item.name)}</option>`).join("")}</select></label>
        <fieldset class="appearance-accent"><legend>Interface accent</legend><div class="appearance-color-row"><input type="color" id="ap-color" value="${a.accent || "#dfb878"}" aria-label="Choose interface accent"><input class="text" id="ap-hex" aria-label="Interface accent hex color" maxlength="7" value="${a.accent || ""}" placeholder="Design default" spellcheck="false"><button type="button" class="btn quiet" id="ap-default-accent">Use default</button></div><p class="hint" id="ap-accent-note">Adjusted for legibility. Lights keep their own colors.</p></fieldset>
        <div class="appearance-pair">
          <label class="appearance-field" for="ap-density"><span>Density</span><select class="select" id="ap-density"><option value="comfortable">Comfortable</option><option value="compact">Compact</option></select></label>
          <label class="appearance-field" for="ap-scale"><span>Interface size</span><select class="select" id="ap-scale">${[90, 100, 110, 120].map((scale) => `<option value="${scale}">${scale}%${scale === 100 ? " · Default" : ""}</option>`).join("")}</select></label>
        </div>
        <div class="appearance-pair">
          <label class="appearance-field" for="ap-rig"><span>Rig presentation</span><select class="select" id="ap-rig"><option value="frame">Case frame</option><option value="minimal">Minimal</option></select></label>
          <label class="appearance-field" for="ap-motion"><span>Motion</span><select class="select" id="ap-motion"><option value="system">Follow system</option><option value="reduced">Reduced</option></select></label>
        </div>
        <div class="appearance-portable"><button type="button" class="btn quiet" id="ap-import">Import recipe</button><button type="button" class="btn quiet" id="ap-export">Export recipe</button><input type="file" id="ap-file" accept=".json,application/json" hidden></div>
      </div>
      <aside class="appearance-library" aria-label="Saved looks"><div class="appearance-library-heading"><h3>Saved looks</h3><span class="hint" id="ap-look-count"></span></div><div id="ap-looks"></div>
        <label class="appearance-field" for="ap-name"><span>Name this look</span><input class="text" id="ap-name" maxlength="40" placeholder="Evening desk" autocomplete="off"></label>
        <button type="button" class="btn" id="ap-save-look">Add to saved looks</button>
        <p class="hint">Your collection is saved when you keep changes.</p>
      </aside>
    </div>
    <p class="appearance-error" id="ap-error" role="alert" hidden></p>
    <div class="actions appearance-actions"><button type="button" class="btn quiet" id="ap-reset">Reset appearance</button><span class="spacer"></span><button type="button" class="btn" id="ap-cancel">Cancel</button><button type="submit" class="btn primary" id="ap-keep">Keep changes</button></div>
  </form>`);
  const form = d.querySelector("#appearance-form");
  const find = (id) => form.querySelector(`#${id}`);
  let importedName = "", renameId = null;
  const error = (message = "") => { find("ap-error").textContent = message; find("ap-error").hidden = !message; };
  const sync = () => {
    const value = tx.draft;
    find("ap-design").value = value.design;
    for (const key of ["density", "scale", "rig", "motion"]) find(`ap-${key}`).value = String(value.appearance[key]);
    find("ap-color").value = value.appearance.accent || "#dfb878";
    find("ap-hex").value = value.appearance.accent || "";
    find("ap-hex").setCustomValidity("");
    find("ap-accent-note").textContent = value.design === "contrast"
      ? "High contrast keeps its yellow accent. Your custom color is retained for other designs."
      : "Adjusted for legibility. Lights keep their own colors.";
  };
  const paintLooks = () => {
    const looks = tx.draft.looks;
    find("ap-look-count").textContent = `${looks.length} / 12`;
    find("ap-save-look").disabled = !renameId && looks.length >= 12;
    find("ap-save-look").textContent = renameId ? "Rename look" : "Add to saved looks";
    setHTML(find("ap-looks"), looks.length ? looks.map((look) => `<div class="appearance-look" data-look="${look.id}">
      <button type="button" class="appearance-look-apply" data-look-apply="${look.id}" aria-label="Apply ${esc(look.name)}"><span class="appearance-look-swatch dsg-preview" data-design="${look.design}" aria-hidden="true"><i style="background:${look.appearance.accent || "var(--brass)"}"></i></span><span><strong>${esc(look.name)}</strong><small>${esc(DESIGNS.find((item) => item.id === look.design)?.name || "Graphite")} · ${look.appearance.scale}%</small></span></button>
      <div class="appearance-look-actions"><button type="button" class="btn quiet" data-look-rename="${look.id}" aria-label="Rename ${esc(look.name)}">Rename</button><button type="button" class="btn quiet" data-look-delete="${look.id}" aria-label="Delete ${esc(look.name)}">Delete</button></div></div>`).join("")
      : `<p class="appearance-empty">A quieter evening. A closer view.<br>Keep a few looks that feel like yours.</p>`);
  };
  const preview = (patch) => { tx.preview(patch); sync(); error(); };
  const restore = () => {
    tx.cancel();
    if (appearanceSession === tx) appearanceSession = null;
    observer.disconnect();
    d.removeEventListener("cancel", onCancel);
    d.removeEventListener("close", onClose);
  };
  const onCancel = (event) => { if (tx.saving) event.preventDefault(); };
  const onClose = () => restore();
  // Replacing a shared sheet is also a Cancel, even without a native close event.
  const observer = new MutationObserver(() => { if (!d.contains(form)) restore(); });
  observer.observe(d, { childList: true });
  d.addEventListener("cancel", onCancel);
  d.addEventListener("close", onClose);
  form.addEventListener("change", (event) => {
    const target = event.target;
    if (target.id === "ap-design") preview({ design: target.value });
    else if (["ap-density", "ap-scale", "ap-rig", "ap-motion"].includes(target.id)) {
      const key = target.id.slice(3);
      preview({ appearance: { ...tx.draft.appearance, [key]: key === "scale" ? Number(target.value) : target.value } });
    } else if (target.id === "ap-hex") {
      const value = target.value.trim();
      if (value && !/^#[0-9a-f]{6}$/i.test(value)) { target.setCustomValidity("Use a six-digit hex color, such as #dfb878."); target.reportValidity(); return; }
      preview({ appearance: { ...tx.draft.appearance, accent: value || null } });
    }
  });
  find("ap-hex").addEventListener("input", () => find("ap-hex").setCustomValidity(""));
  find("ap-color").addEventListener("input", (event) => preview({ appearance: { ...tx.draft.appearance, accent: event.target.value } }));
  find("ap-default-accent").addEventListener("click", () => preview({ appearance: { ...tx.draft.appearance, accent: null } }));
  find("ap-reset").addEventListener("click", () => preview({ appearance: DEFAULT_APPEARANCE }));
  find("ap-cancel").addEventListener("click", () => { if (!tx.saving) { restore(); closeSheet(); } });
  find("ap-looks").addEventListener("click", (event) => {
    const button = event.target.closest("button");
    if (!button || tx.saving) return;
    const id = button.dataset.lookApply || button.dataset.lookRename || button.dataset.lookDelete;
    const look = tx.draft.looks.find((item) => item.id === id);
    if (!look) return;
    if (button.dataset.lookApply) { preview({ design: look.design, appearance: look.appearance }); importedName = look.name; }
    else if (button.dataset.lookRename) { renameId = id; find("ap-name").value = look.name; paintLooks(); find("ap-name").focus(); find("ap-name").select(); }
    else { preview({ looks: tx.draft.looks.filter((item) => item.id !== id) }); if (renameId === id) renameId = null; paintLooks(); }
  });
  find("ap-save-look").addEventListener("click", () => {
    const name = find("ap-name").value.trim();
    if (!name) { error("Give this look a name first."); find("ap-name").focus(); return; }
    const draft = tx.draft;
    if (renameId) draft.looks = draft.looks.map((look) => look.id === renameId ? { ...look, name } : look);
    else {
      if (draft.looks.length >= 12) { error("Your collection is full. Delete a look before adding another."); return; }
      draft.looks.push({ id: `look-${crypto.randomUUID().replaceAll("-", "").slice(0, 24)}`, name, design: draft.design, appearance: draft.appearance });
    }
    preview({ looks: draft.looks }); renameId = null; importedName = name; find("ap-name").value = ""; paintLooks();
  });
  find("ap-name").addEventListener("keydown", (event) => {
    if (event.key === "Enter") { event.preventDefault(); find("ap-save-look").click(); }
  });
  find("ap-import").addEventListener("click", () => find("ap-file").click());
  find("ap-file").addEventListener("change", async (event) => {
    const file = event.target.files?.[0];
    if (!file) return;
    try {
      if (file.size > 65536) throw new Error("Choose an appearance recipe smaller than 64 KB.");
      const recipe = parseLookRecipe(await file.text());
      if (tx.closed || tx.saving || !d.contains(form)) return;
      importedName = recipe.name; renameId = null;
      preview({ design: recipe.design, appearance: recipe.appearance });
      find("ap-name").value = recipe.name; paintLooks();
    } catch (e) { error(e.message); }
    event.target.value = "";
  });
  find("ap-export").addEventListener("click", () => {
    const name = find("ap-name").value.trim() || importedName || "My look";
    const url = URL.createObjectURL(new Blob([encodeLookRecipe({ ...tx.draft, name })], { type: "application/json" }));
    const link = document.createElement("a"); link.href = url; link.download = `${name.toLowerCase().replace(/[^a-z0-9]+/g, "-").slice(0, 40) || "my-look"}.airglow-look.json`;
    link.click(); setTimeout(() => URL.revokeObjectURL(url), 1000);
  });
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (tx.saving || !form.reportValidity()) return;
    error();
    const controls = [...form.querySelectorAll("button, input, select")];
    controls.forEach((control) => { control.disabled = true; });
    find("ap-keep").textContent = "Saving…";
    try {
      const confirmed = await tx.keep();
      setPresentation(confirmed, true);
      const stillHere = d.contains(form);
      restore();
      if (stillHere) closeSheet();
      renderSettings();
    } catch (e) {
      error(`Could not save: ${e.message}. Your preview is still here.`);
      controls.forEach((control) => { control.disabled = false; });
      find("ap-keep").textContent = "Keep changes";
      paintLooks();
    }
  });
  sync(); paintLooks();
  find("ap-design").focus({ preventScroll: true });
}

export async function loadSettings() {
  try { S.settings = await get("/api/settings"); renderSettings(); } catch (e) { fail(e); }
  refreshTools();
}

export function renderSettings() {
  const v = S.settings;
  if (!v) return;
  const s = v.settings;
  if (appearanceSession && !appearanceSession.closed) setPresentation(appearanceSession.draft);
  else if (pendingDesign) setPresentation({ ...s, design: pendingDesign });
  else setPresentation(s, true);
  const designName = DESIGNS.find((d) => d.id === s.design)?.name || "Graphite";
  const sensorsOk = S.state?.sensors?.online;
  setHTML($("#settings"), `
    <section class="set-group" id="set-general"><h2>General</h2>
      ${row("Start with Windows", "Opens quietly in the tray when you sign in, so lighting and fan curves apply right away.", sw("s-startup", v.start_with_windows, "Start with Windows"))}
      ${row("Temperature unit", "", `<div class="seg" id="s-unit" role="radiogroup" aria-label="Temperature unit"><button role="radio" data-v="C" aria-checked="${s.temp_unit === "C"}">°C</button><button role="radio" data-v="F" aria-checked="${s.temp_unit === "F"}">°F</button></div>`)}
      ${row("Interface design", "Changes how Airglow looks, not what it does. Your lights keep their real colours in every design.",
        `<span class="design-current"><span class="dsg-preview" data-design="${esc(s.design)}" aria-hidden="true">${previewHTML()}</span><button type="button" class="btn" id="s-design" aria-label="Change interface design (currently ${esc(designName)})">Change</button></span>`)}
      ${row("Personal look", esc(appearanceName(s.appearance)), `<button type="button" class="btn" id="s-appearance">Customize</button>`)}
      ${row("Electricity price", "Feeds the Cost per hour tile. It starts at the German household average of 37.97 ct/kWh (Destatis, first half of 2026); Hamburg tariffs run from about 35 ct to 45 ct. Only the processor and graphics card power Airglow already reads is counted.",
        `<input class="text num" id="s-price" type="number" min="0.01" max="5" step="0.01" value="${s.energy_price}" aria-label="Electricity price per kilowatt hour" style="width:110px"><span class="hint">€/kWh</span>`)}
    </section>
    <section class="set-group" id="set-lighting"><h2>Lighting</h2>
      ${row("Lights off when the PC sleeps or shuts down", "Many boards keep their RGB on in sleep. This turns everything dark first.", sw("s-sleep", s.lights_off_on_sleep, "Lights off when the PC sleeps or shuts down"))}
      ${row("Lights off while the PC is locked", "", sw("s-lock", s.lights_off_on_lock, "Lights off while the PC is locked"))}
    </section>
    <section class="set-group" id="set-fans"><h2>Fans</h2>
      ${row("Follow the temperature of", s.temp_source === "liquid" && S.state?.sensors?.liquid_temp == null
        ? "The fan curve reacts to this reading. No coolant temperature is coming through, so the hotter part is used until it does."
        : "The fan curve reacts to this reading. Coolant suits radiator fans: it changes slowly, so they don't rev up and down.", `<select class="select" id="s-source" aria-label="Follow the temperature of" style="width:220px">
        <option value="max"${s.temp_source === "max" ? " selected" : ""}>Whichever is hotter</option>
        <option value="cpu"${s.temp_source === "cpu" ? " selected" : ""}>Processor</option>
        <option value="gpu"${s.temp_source === "gpu" ? " selected" : ""}>Graphics card</option>
        <option value="liquid"${s.temp_source === "liquid" ? " selected" : ""}>Coolant (liquid cooler)</option></select>`)}
      ${row("Failsafe speed", "Used when temperatures can't be read, and left on the fans when Airglow quits.", `<input type="range" id="s-failsafe" aria-label="Failsafe speed" min="30" max="100" step="5" value="${s.fan_failsafe_duty}"><output id="s-failsafe-v">${s.fan_failsafe_duty}%</output>`)}
      ${row("Ramp speed", "How much fan speed may change every 2 seconds. Lower is calmer.", `<input type="range" id="s-step" aria-label="Ramp speed" min="1" max="25" value="${s.fan_max_step}"><output id="s-step-v">${s.fan_max_step}%</output>`)}
      ${row("Motherboard and cooler fans", `Airglow can drive the board's fan headers and the pumps and fans of many coolers through LibreHardwareMonitor: pick them on the <a href="#fans">Fans</a> tab. Until you do, they keep your BIOS or cooler curve. ${v.fancontrol_installed
        ? (v.fan_profiles.length ? `FanControl has ${v.fan_profiles.length} saved profile${v.fan_profiles.length > 1 ? "s" : ""}, which profiles can switch between too.` : "FanControl is installed but has no saved profiles yet.")
        : ""}`, "")}
    </section>
    <section class="set-group" id="set-helpers"><h2>Helper programs</h2>
      <p class="hint">Temperatures and motherboard and cooler fan control come from LibreHardwareMonitor, and most RGB devices other than NZXT and ASUS from OpenRGB.</p>
      <div class="tools-box" data-tools="full"></div>
    </section>
    <section class="set-group" id="set-sensors"><h2>Sensors</h2>
      ${row("LibreHardwareMonitor address", sensorsOk ? "Connected. Temperatures and power are coming through." : "Not responding. Open LibreHardwareMonitor and turn on Options, Remote Web Server, Run.", `<input class="text" id="s-lhm" aria-label="LibreHardwareMonitor address" value="${esc(s.lhm_url)}" spellcheck="false">`)}
    </section>
    <section class="set-group" id="set-devices"><h2>More devices</h2>
      ${row("Use OpenRGB for other devices", esc(S.state?.openrgb?.status || ""),
        `${S.state?.openrgb?.enabled && !S.state?.openrgb?.connected && S.state?.openrgb?.installed ? `<button class="btn" id="s-orgb-start">Start OpenRGB</button>` : ""}${sw("s-orgb", s.openrgb_enabled, "Use OpenRGB for other devices")}`)}
      ${row("OpenRGB address", "Only change this if OpenRGB runs on another port or another PC.", `<input class="text" id="s-orgb-host" aria-label="OpenRGB host" value="${esc(s.openrgb_host)}" style="width:150px" spellcheck="false"><input class="text" id="s-orgb-port" aria-label="OpenRGB port" type="number" value="${s.openrgb_port}" style="width:90px">`)}
      ${row("Windows Dynamic Lighting devices <span class=\"tag\">Experimental</span>", `Keyboards, mice and accessories that Windows controls in Settings, Personalization, Dynamic Lighting. Windows only lets Airglow light them while its window is in front. ${s.dynamic_lighting_enabled ? esc(S.state?.dynamic_lighting?.status || "") : ""}`, sw("s-dyn", s.dynamic_lighting_enabled, "Windows Dynamic Lighting devices"))}
    </section>
    <section class="set-group" id="set-migration"><h2>Rig migration</h2>
      ${row("Export rig setup", "Download a local JSON bundle with zone names, areas and layouts, profiles, curves, automations and interface preferences. Fan ownership, helper paths and credentials are excluded.", `<button type="button" class="btn" id="s-rig-export">Export rig bundle</button>`)}
      ${row("Import on this rig", "Validate a bundle, choose the target for every lighting zone, then review and apply. Imported automations start disabled; active cooling stays as it is.", `<button type="button" class="btn" id="s-rig-import">Import rig bundle</button>`)}
    </section>
    <section class="set-group" id="set-about"><h2>About</h2>
      ${row(`Airglow ${esc(v.version)}`, `Free and open source under the MIT licence. Settings and logs are stored in <span class="path">${esc(v.data_dir)}</span>`, `<button class="btn" id="s-logs">Open log folder</button>`)}
      ${row("Diagnostics report", "For a bug report: what Airglow found, its settings, a sensor list and the logs, in one zip. Your user name, PC name and device serial numbers are taken out. Nothing is sent; you attach it yourself.", `<button class="btn" id="s-diag">Save a report</button>`)}
      ${updateRows(v)}
      ${row("Keyboard shortcuts", "Sections, the fan curve and the Studio, without the mouse.", `<button class="btn quiet" id="s-keys">View shortcuts</button>`)}
      ${row("Welcome guide", "Shows what Airglow found on this PC and what to check.", `<button class="btn quiet" id="s-welcome">Show it again</button>`)}
      ${row("Reset settings", "Puts the settings on this page back to how they start on a new PC. Your zones, presets, fan curves and overview tiles are kept.", `<button class="btn quiet" id="s-reset">Reset</button>`)}
    </section>`);
  $$("#settings input[type=range]").forEach(rangeFill);
  paintTools();
  paintNav();
}

let navWired = false;
function paintNav() {
  const nav = $("#settings-nav-links");
  if (!nav) return;
  if (!navWired) {
    nav.innerHTML = SECTIONS.map(([id, label]) => `<button type="button" data-set="${id}">${esc(label)}</button>`).join("");
    nav.addEventListener("click", (e) => {
      const b = e.target.closest("[data-set]");
      if (!b) return;
      const sec = document.getElementById(`set-${b.dataset.set}`);
      sec?.scrollIntoView({ block: "start", behavior: "smooth" });
      const h = sec?.querySelector("h2");
      if (h) {
        h.tabIndex = -1;
        h.focus({ preventScroll: true });
      } else {
        b.focus({ preventScroll: true });
      }
    });
    navWired = true;
  }
  const view = $('[data-view="settings"]');
  if (!view) return;
  let active = SECTIONS[0][0];
  const top = view.getBoundingClientRect().top + 70;
  for (const [id] of SECTIONS) {
    const sec = document.getElementById(`set-${id}`);
    if (sec && sec.getBoundingClientRect().top <= top) active = id;
  }
  if (view.scrollTop + view.clientHeight >= view.scrollHeight - 4) active = SECTIONS.at(-1)[0];
  nav.querySelectorAll("[data-set]").forEach((b) => {
    if (b.dataset.set === active) b.setAttribute("aria-current", "true");
    else b.removeAttribute("aria-current");
  });
}

let savedTimer;
function noteSaved(label = "Saved") {
  const el = $("#settings-saved");
  if (!el) return;
  el.textContent = label;
  el.classList.add("show");
  clearTimeout(savedTimer);
  savedTimer = setTimeout(() => { el.textContent = ""; el.classList.remove("show"); }, 2400);
}

const writeSettings = serializeWrites(({ path, patch }) => post(path, patch));
let pendingSettings = Promise.resolve();
function save(patch, label = "Saved", propagate = false, path = "/api/settings") {
  const result = saveSettings(patch, label, propagate, path);
  pendingSettings = result.catch(() => {});
  return result;
}
async function saveSettings(patch, label, propagate, path) {
  // Rebuilding the page is what the server's answer is; keep the scroll position and the focused control where they
  // were instead of dropping the user at the top after every change.
  const view = $('[data-view="settings"]');
  const scrollTop = view?.scrollTop || 0;
  const focused = document.activeElement?.id;
  try {
    S.settings = await writeSettings({ path, patch });
    renderSettings();
    if (focused) document.getElementById(focused)?.focus({ preventScroll: true });
    if (view) {
      view.scrollTop = scrollTop;
      requestAnimationFrame(() => { if (view) view.scrollTop = scrollTop; paintNav(); });
    }
    paintNav();
    noteSaved(label);
    emit("refresh");
    return S.settings;
  } catch (e) { if (!propagate) fail(e); renderSettings(); if (propagate) throw e; }
}

export function initSettings() {
  const box = $("#settings");
  box.addEventListener("change", (e) => {
    const t = e.target;
    if (t.id === "s-startup") save({ start_with_windows: t.checked }, t.checked ? "Airglow will start with Windows" : "Airglow won't start with Windows");
    else if (t.id === "s-sleep") save({ lights_off_on_sleep: t.checked });
    else if (t.id === "s-lock") save({ lights_off_on_lock: t.checked });
    else if (t.id === "s-source") save({ temp_source: t.value });
    else if (t.id === "s-price") {
      const price = Number(t.value);
      if (!Number.isFinite(price) || price < 0.01 || price > 5) {
        t.classList.add("invalid");                     // the saved value is what the page said before the edit
        t.value = S.settings?.settings?.energy_price ?? "";
        toast("Electricity price must be between 0.01 and 5 €/kWh", true);
      } else {
        t.classList.remove("invalid");
        save({ energy_price: price });
      }
    }
    else if (t.id === "s-failsafe") save({ fan_failsafe_duty: +t.value });
    else if (t.id === "s-step") save({ fan_max_step: +t.value });
    else if (t.id === "s-lhm") save({ lhm_url: t.value.trim() });
    else if (t.id === "s-dyn") save({ dynamic_lighting_enabled: t.checked }, t.checked ? "Looking for Dynamic Lighting devices" : "Dynamic Lighting devices off");
    else if (t.id === "s-orgb") save({ openrgb_enabled: t.checked }, t.checked ? "OpenRGB bridge on" : "OpenRGB bridge off");
    else if (t.id === "s-orgb-host") save({ openrgb_host: t.value.trim() });
    else if (t.id === "s-orgb-port") save({ openrgb_port: +t.value });
    else if (t.id === "s-online") save({ online_checks: t.checked }, t.checked ? "Airglow will look for updates on its own" : "Airglow only goes online when you ask");
  });
  box.addEventListener("input", (e) => {
    const t = e.target;
    if (t.id === "s-price") t.classList.remove("invalid");
    if (t.type === "range") { rangeFill(t); $(`#${t.id}-v`).textContent = `${t.value}%`; }
  });
  box.addEventListener("keydown", (e) => { if (e.key === "Enter" && e.target.classList.contains("text")) e.target.blur(); });
  box.addEventListener("click", (e) => {
    const u = e.target.closest("#s-unit [data-v]");
    if (u) save({ temp_unit: u.dataset.v });
    const ch = e.target.closest("#s-channel [data-v]");
    if (ch) save({ update_channel: ch.dataset.v }, ch.dataset.v === "beta" ? "You'll get beta releases" : "You'll get stable releases");
    if (e.target.closest("#s-logs")) post("/api/open/logs").catch(fail);
    if (e.target.closest("#s-diag")) post("/api/diagnostics", {}).then((r) => toast(`Saved ${r.name}. Explorer shows where.`)).catch(fail);
    if (e.target.closest("#s-welcome")) openWelcome();
    if (e.target.closest("#s-keys")) openShortcuts();
    if (e.target.closest("#s-design")) designSheet();
    if (e.target.closest("#s-appearance")) openAppearance().catch(fail);
    if (e.target.closest("#s-rig-import")) openRigImport();
    if (e.target.closest("#s-rig-export")) {
      const button = e.target.closest("#s-rig-export");
      button.disabled = true;
      exportRig().catch(fail).finally(() => { button.disabled = false; });
    }
    if (e.target.closest("#s-update")) {
      post("/api/updates/check").then((u) => { S.state.update = u; paintUpdate(); emit("refresh"); }).catch(fail);
    }
    if (e.target.closest("#s-restart")) restartToUpdate();
    if (e.target.closest("#s-orgb-start")) {
      post("/api/openrgb/start").then(() => toast("Starting OpenRGB")).catch(fail);
    }
    if (e.target.closest("#s-reset")) {
      if (!confirm("Put the settings back to how they start on a new PC?\n\nZones, presets, fan curves and overview tiles are kept.")) return;
      save({}, "Settings reset", false, "/api/settings/reset");
    }
  });
  on("state", paintUpdate);
  on("rig:imported", loadSettings);
  const view = $('[data-view="settings"]');
  let navRaf = 0;
  view.addEventListener("scroll", () => {
    if (navRaf) return;
    navRaf = requestAnimationFrame(() => { navRaf = 0; paintNav(); });
  }, { passive: true });
}

export function restartToUpdate() {
  post("/api/updates/restart").then(() => toast("Restarting to update")).catch(fail);
}
