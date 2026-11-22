// Local help and keyboard reference, including when device discovery is still starting.
import { $, S, emit, esc, on } from "./core.js";
import { closeSheet, openSheet } from "./sheet.js";

const tasks = [
  { group: "Get started", icon: "chip", title: "Connect your first lights", text: "Open Devices to see detected controllers, check each zone and find setup controls.", target: "#devices", destination: "Devices", keywords: "USB missing discover wiring OpenRGB" },
  { group: "Get started", icon: "temp", title: "Find missing temperature readings", text: "Check the sensor connection and LibreHardwareMonitor address in Settings.", target: "#settings/sensors", destination: "Sensor settings", keywords: "cpu gpu processor graphics unavailable lhm offline" },
  { group: "Get started", icon: "bulb", title: "Browse lighting looks", text: "Try a ready-made profile on the sample rig and adjust its look on screen.", tool: "lookbook", destination: "Lighting lookbook", keywords: "offline no hardware first unconfigured lights sample gallery lighting presets" },
  { group: "Create & arrange", icon: "effect", title: "Recover a Studio draft", text: "Resume work from Your workspace, or browse named checkpoints and open an independent copy.", target: "#studio", destination: "Studio", keywords: "restore local saved undo compare versions history edit" },
  { group: "Create & arrange", icon: "sliders", title: "Compare saved profiles", text: "Inspect lighting and cooling settings together, then compare two looks in your library.", tool: "profiles", destination: "Profile library", keywords: "profiles scene preview fan curves compare library saved import" },
  { group: "Create & arrange", icon: "bulb", title: "Build a palette from a photo", text: "In Lighting, open an effect’s colour or palette editor and choose From a photo.", target: "#lighting", destination: "Lighting", keywords: "image colors colours palette sample swatch rgb" },
  { group: "Create & arrange", icon: "workspace", title: "Arrange lighting zones", text: "Open Arrange on the Lighting stage to position your devices, align zones and review the layout.", target: "#lighting", destination: "Lighting", keywords: "move offset nudge rig position place multiple keyboard" },
  { group: "Understand behavior", icon: "bolt", title: "Rehearse an automation", text: "Try a time, app or temperature and see which rule wins for lighting and cooling.", tool: "rules", destination: "Rule rehearsal", keywords: "priority overnight running foreground locked idle simulator" },
  { group: "Understand behavior", icon: "fan", title: "Try a fan curve", text: "Open Curve sandbox on the Fans screen to explore how a curve responds to temperatures.", target: "#fans", destination: "Fans", keywords: "cooling pump duty thermal safety simulation sandbox" },
  { group: "Understand behavior", icon: "clock", title: "Compare recorded sessions", text: "Open Recordings to inspect sensor traces and compare previous runs.", tool: "sessions", destination: "Recordings", keywords: "record measurements telemetry history performance compare" },
  { group: "Understand behavior", icon: "sliders", title: "Check what controls your hardware", text: "Inspect device ownership, current readings and recent events in Control center.", tool: "status", destination: "Control center", keywords: "diagnostics troubleshooting status conflicts offline" },
];

const shortcuts = [
  ["Navigation", ["Ctrl", "1…7"], "Switch between Overview, Lighting, Studio, Fans, Automations, Devices and Settings.", "tabs sections 1 2 3 4 5 6 7"],
  ["Quick switch", ["Ctrl", "K"], "Search and switch effects, profiles or workspaces, or find a zone or setting.", "quick switch palette command tools"],
  ["Navigation", ["F1", "or", "?"], "Open keyboard shortcuts outside text fields.", "help guide question mark"],
  ["Navigation", ["Esc"], "Close the current dialog.", "escape dismiss cancel"],
  ["Quick switch", ["↑", "/", "↓"], "Move through matching actions.", "arrow up down search quick"],
  ["Quick switch", ["PgUp", "/", "PgDn"], "Move a page through Quick switch results.", "page up down"],
  ["Quick switch", ["Enter"], "Choose the selected Quick switch action.", "return"],
  ["Studio", ["Ctrl", "S"], "Save the effect you are editing.", "save current effect code"],
  ["Studio", ["Ctrl", "Z"], "Undo the last document edit outside text fields.", "undo history"],
  ["Studio", ["Ctrl", "Shift", "Z"], "Redo an edit. Ctrl + Y also works.", "redo history"],
  ["Studio", ["Ctrl", "D"], "Duplicate the selected layer.", "copy layers"],
  ["Studio", ["Delete"], "Remove the selected layer outside text fields.", "backspace delete layers"],
  ["Studio", ["← → ↑ ↓", "then", "Space"], "While painting on the preview, choose an LED and paint or erase it.", "arrow left right up down brush mask"],
  ["Lighting", ["Enter", "or", "Space"], "Select a focused zone. In Arrange, Space adds or removes it from the selection.", "select multiple rig"],
  ["Lighting", ["← → ↑ ↓"], "Nudge selected zones while arranging. Hold Shift for a fine step.", "arrow left right up down move placement"],
  ["Lighting", ["Ctrl", "Z"], "Undo a layout edit in Arrange. Ctrl + Shift + Z redoes it.", "redo layout history"],
  ["Fans", ["← → ↑ ↓"], "Move the focused fan-curve point. Hold Shift to move in steps of five.", "arrow left right up down temperature duty"],
  ["Fans", ["Delete"], "Remove a curve point when more than two remain.", "backspace remove"],
  ["Overview", ["← → ↑ ↓"], "Move the focused tile in Customize.", "arrow left right up down tiles rearrange"],
  ["Overview", ["Shift", "← → ↑ ↓"], "Resize the focused tile in Customize.", "arrow left right up down grow tiles"],
  ["Overview", ["Enter"], "Open settings for the focused tile in Customize.", "tile settings customize"],
  ["Overview", ["Delete"], "Remove the focused tile in Customize.", "backspace remove dashboard"],
];
const keyGroup = rows => [...new Set(rows)];
const normalize = s => s.toLowerCase().normalize("NFKD").replace(/\p{M}/gu, "").replace(/\bcontrol\b/g, "ctrl").replace(/[+]/g, " ").trim();
const matches = (text, query) => normalize(query).split(/\s+/).every(w => normalize(text).includes(w));
const keysHTML = keys => keys.map((k, i) => {
  if (["/", "or", "then"].includes(k)) return `<span>${k}</span>`;
  const plus = i && !["/", "or", "then"].includes(keys[i - 1]) ? "<span>+</span>" : "";
  return `${plus}<kbd>${k}</kbd>`;
}).join("");

function featureHint() {
  const zones = S.zones?.zones;
  const known = Array.isArray(zones);
  const active = known && zones.some(z => z.device?.online && z.config?.enabled && z.positions?.length);
  return active ? "See a template on your configured rig in Studio’s on-screen preview."
    : known ? "Studio has a sample case ready. Try a lighting design before connecting your own lights."
      : "Studio can preview a template on a sample case before you connect lights.";
}

function feature() {
  return `<section class="help-feature"><div><h3>Preview your first effect</h3><p>${featureHint()}</p>
    <a class="btn primary" href="#studio/new" data-help-route>Explore templates</a></div></section>`;
}

const launchers = {
  rules: async () => (await import("./automations.js")).openAutomationRehearsal,
  sessions: async () => (await import("./session-lab.js")).openSessionLab,
  status: async () => (await import("./control-center.js")).openControlCenter,
  profiles: async () => (await import("./profile-library.js")).openProfileLibrary,
  lookbook: async () => (await import("./lookbook.js")).openLookbook,
};

function openHelp(initial) {
  let mode = initial, loading = false;
  const filters = { guide: { search: "", group: "" }, keys: { search: "", group: "" } };
  const d = openSheet(`<div class="body help-workspace">
    <div class="help-head"><h2>${initial === "keys" ? "Keyboard shortcuts" : "Airglow help"}</h2><button class="btn quiet" data-close>Close</button></div>
    <div class="help-modes" role="tablist" aria-label="Help pages"><button id="help-tab-guide" data-help-mode="guide" role="tab" aria-controls="help-panel">Task guide</button>
      <button id="help-tab-keys" data-help-mode="keys" role="tab" aria-controls="help-panel">Keyboard shortcuts</button></div>
    <div id="help-panel" role="tabpanel"><div class="help-filter">
      <label class="help-search"><svg aria-hidden="true"><use href="#i-search"/></svg><input class="text" type="search" autocomplete="off" id="help-search"></label>
      <select class="select" aria-label="Filter by topic" id="help-group"></select></div>
      <p class="sr" role="status" data-help-count></p><div class="help-content" tabindex="0" role="region" aria-label="Help results"></div>
      <p class="hint" role="alert" data-help-error hidden></p></div></div>`);
  const root = d.querySelector(".help-workspace"), input = root.querySelector("#help-search"), group = root.querySelector("#help-group");
  const content = root.querySelector(".help-content"), count = root.querySelector("[data-help-count]"), error = root.querySelector("[data-help-error]");
  function taskHTML(t) {
    const name = esc(t.title);
    const body = `<span class="help-symbol" aria-hidden="true"><svg><use href="#i-${t.icon}"/></svg></span><span><strong>${name}</strong><small>${esc(t.text)}</small>
      <span class="help-destination">${esc(t.destination)} <i aria-hidden="true">↗</i></span></span>`;
    return t.target ? `<a class="help-task" href="${t.target}" data-help-route>${body}</a>`
      : `<button class="help-task" type="button" data-help-tool="${t.tool}">${body}</button>`;
  }
  function render() {
    const f = filters[mode], guiding = mode === "guide", rows = guiding
      ? tasks.filter(t => (!f.group || f.group === t.group) && matches([t.title, t.group, t.text, t.destination, t.keywords].join(" "), f.search))
      : shortcuts.filter(([g, keys, what, extra]) => (!f.group || f.group === g) && matches([g, keys.join(" "), what, extra].join(" "), f.search));
    const topic = r => guiding ? r.group : r[0];
    const groups = keyGroup(rows.map(topic));
    const featured = guiding && (!f.group || f.group === "Get started") && (!f.search || matches("get started first effect studio preview templates sample case no configured lights offline hardware", f.search));
    const total = rows.length + Number(featured);
    count.textContent = `${total} matching ${guiding ? "tasks" : "shortcuts"}`;
    content.innerHTML = (featured ? feature() : "") + groups.map(g => `<section class="help-group"><h3>${esc(g)}</h3>
      ${guiding ? `<div class="help-task-grid">${rows.filter(r => topic(r) === g).map(taskHTML).join("")}</div>`
        : `<dl class="help-keys">${rows.filter(r => topic(r) === g).map(([, k, what]) => `<div><dt>${keysHTML(k)}</dt><dd>${esc(what)}</dd></div>`).join("")}</dl>`}</section>`).join("") +
      (total ? "" : `<div class="help-empty"><p>No ${guiding ? "tasks" : "shortcuts"} match these filters.</p><button class="btn" data-help-reset>Clear filters</button></div>`);
    content.scrollTop = 0;
  }
  function switchMode(next) {
    mode = next;
    root.querySelector("h2").textContent = mode === "keys" ? "Keyboard shortcuts" : "Airglow help";
    for (const tab of root.querySelectorAll("[role=tab]")) {
      const selected = tab.dataset.helpMode === mode;
      tab.setAttribute("aria-selected", String(selected)); tab.tabIndex = selected ? 0 : -1;
    }
    root.querySelector("#help-panel").setAttribute("aria-labelledby", `help-tab-${mode}`);
    const label = mode === "keys" ? "Search shortcuts" : "Search the task guide";
    input.setAttribute("aria-label", label); input.placeholder = mode === "keys" ? "Search shortcuts" : "Search tasks";
    input.value = filters[mode].search;
    const groups = mode === "keys" ? keyGroup(shortcuts.map(r => r[0])) : keyGroup(tasks.map(t => t.group));
    group.innerHTML = '<option value="">All topics</option>' + groups.map(g => `<option>${esc(g)}</option>`).join("");
    group.value = filters[mode].group; error.hidden = true; render();
  }
  root.addEventListener("click", async e => {
    const tab = e.target.closest("[data-help-mode]"), link = e.target.closest("[data-help-route]"), tool = e.target.closest("[data-help-tool]");
    if (tab) return switchMode(tab.dataset.helpMode);
    if (e.target.closest("[data-help-reset]")) { filters[mode] = { search: "", group: "" }; switchMode(mode); input.focus(); return; }
    if (link && !e.ctrlKey && !e.metaKey && !e.shiftKey && !e.altKey) {
      e.preventDefault(); closeSheet();
      const hash = link.hash;
      if (hash === location.hash) emit("route:revisit"); else location.hash = hash;
      requestAnimationFrame(() => {
        if (location.hash !== hash || d.open) return;
        const nav = $(`[data-tab="${hash.slice(1).split("/")[0]}"]`);
        (nav?.getClientRects().length ? nav : $("#nav-select"))?.focus({ preventScroll: true });
      });
      return;
    }
    if (!tool || loading) return;
    loading = true; tool.setAttribute("aria-busy", "true");
    try {
      const launch = await launchers[tool.dataset.helpTool]();
      if (d.open && root.isConnected && tool.isConnected) await launch();
    } catch (err) {
      if (d.open && root.isConnected) { error.textContent = `Could not open this tool. ${err.message}`; error.hidden = false; }
    } finally { loading = false; tool.removeAttribute("aria-busy"); }
  });
  root.querySelector("[role=tablist]").addEventListener("keydown", e => {
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key)) return;
    e.preventDefault(); switchMode(e.key === "Home" ? "guide" : e.key === "End" ? "keys" : mode === "guide" ? "keys" : "guide");
    root.querySelector(`[data-help-mode="${mode}"]`).focus();
  });
  input.addEventListener("input", () => { filters[mode].search = input.value.trim(); render(); });
  group.addEventListener("change", () => { filters[mode].group = group.value; render(); });
  switchMode(mode); input.focus({ preventScroll: true });
}

export const openGuide = () => openHelp("guide");
export const openShortcuts = () => openHelp("keys");

export function initShortcuts() {
  document.querySelector("[data-help-open]")?.addEventListener("click", () => {
    openGuide();
    $("#sheet").addEventListener("close", () => {
      const toggle = $("#ov-tools");
      if (!$("#sheet").open && toggle?.getClientRects().length && document.activeElement === document.body) toggle.focus({ preventScroll: true });
    }, { once: true });
  });
  document.addEventListener("keydown", (e) => {
    if (e.key !== "?" && e.key !== "F1") return;
    if (e.ctrlKey || e.metaKey || e.altKey || e.isComposing || $("dialog[open]")) return;
    const t = e.target;
    if (t?.matches?.("input, textarea, select, [contenteditable=true]") || t?.closest?.(".CodeMirror")) return;
    e.preventDefault(); openShortcuts();
  });
  on("zones", () => {
    const current = $("#sheet[open] .help-feature p");
    if (current) current.textContent = featureHint();
  });
}
