import { $, $$, S, cap, emit, esc, on, post, setHTML, toast } from "./core.js";
import { activateOverviewWorkspace } from "./overview.js";
import { closeSheet, openSheet } from "./sheet.js";
import { groupedActions, initQuickPreferences, isFavorite, setFavorite } from "./quick-preferences.js";
import { SETTINGS_SECTIONS } from "./settings-sections.js";

const categories = [
  { kind: "all", label: "All" },
  { kind: "section", label: "Sections", icon: "section" },
  { kind: "effect", label: "Effects", icon: "effect" },
  { kind: "profile", label: "Profiles", icon: "sliders" },
  { kind: "workspace", label: "Workspaces", icon: "workspace" },
  { kind: "zone", label: "Zones", icon: "bulb" },
  { kind: "setting", label: "Settings", icon: "gear" },
  { kind: "favorite", label: "Pinned" },
];
const icons = Object.fromEntries(categories.map(({ kind, icon }) => [kind, icon]));
const SECTION_ICONS = { overview: "workspace", lighting: "bulb", studio: "effect", fans: "fan", automations: "bolt", devices: "chip", settings: "gear" };

function commands(sections, ready) {
  const activeTab = (location.hash || "#overview").slice(1).split("/")[0];
  const navigation = sections.map(id => ({ kind: "section", id, name: cap(id), action: `Open ${cap(id)}`, current: id === activeTab }));
  const settings = SETTINGS_SECTIONS.map(([id, label, detail]) => ({
    kind: "setting", id, name: label, action: "Open settings", detail,
  }));
  if (!ready) return [...navigation, ...settings];
  return [
    ...navigation,
    ...S.effects.map(({ id, name }) => ({ kind: "effect", id, name, action: "Apply effect" })),
    ...S.presets.map(({ id, name, fan_curve, fan_points, fan_profile }) => ({
      kind: "profile", id, name, action: "Apply profile",
      detail: fan_curve || fan_points?.length || fan_profile ? "Lighting and cooling" : "Lighting",
    })),
    ...(S.overview?.workspaces || []).map(({ id, name }) => ({
      kind: "workspace", id, name, action: "Switch workspace", current: id === S.overview.active_workspace,
    })),
    ...(S.zones?.zones || []).filter(zone => zone.device.online).map(({ key, config, device }) => ({
      kind: "zone", id: key, name: config.name, action: "Open lighting zone", detail: device.name,
    })),
    ...settings,
  ];
}

// Treat apostrophes, accents, hyphens and spaces as people commonly type them.
function searchText(value) {
  return value.normalize("NFKD").replace(/\p{M}/gu, "").toLowerCase()
    .replace(/['’]/g, "").replace(/[^\p{L}\p{N}]+/gu, " ").trim();
}

function search(items, value) {
  if (!value.trim()) return items;
  const query = searchText(value);
  if (!query) return [];
  const words = query.split(/\s+/);
  return items.filter(item => {
    const searchable = searchText(`${item.name} ${item.action} ${item.kind} ${item.detail || ""}`);
    return words.every(word => searchable.includes(word));
  }).sort((a, b) => rank(a) - rank(b));

  function rank(item) {
    const name = searchText(item.name);
    return name === query ? 0 : name.startsWith(query) ? 1 : name.includes(query) ? 2 : 3;
  }
}

function confirmedMessage(status, name, kind, on = true) {
  if (status?.status === "failed") throw new Error(status.error || "Lighting could not apply this change.");
  if (status?.overridden) return `${name} saved. Lighting is currently overridden.`;
  if (status?.status === "ready") return on ? `${name} applied.` : `${name} saved. Lights are off.`;
  return `${kind} command accepted for ${name}. Lighting readiness is pending.`;
}

async function activate(item) {
  if (item.kind === "section") return { route: item.id };
  if (item.kind === "setting") return { route: `settings/${item.id}` };
  if (item.kind === "zone") return { route: `lighting/${encodeURIComponent(item.id)}` };
  if (item.kind === "workspace") {
    activateOverviewWorkspace(item.id);
    return { route: "overview" };
  }
  if (item.kind === "effect") {
    const { applyLightingScene } = await import("./scene-commands.js");
    const result = await applyLightingScene({ effect: item.id }, { wait: true });
    emit("refresh");
    return { message: confirmedMessage(result.scene_status, item.name, "Effect", result.scene?.on) };
  }
  const { profileUndoPoint } = await import("./profile-undo.js");
  const undoPoint = profileUndoPoint(S.presets.find(preset => preset.id === item.id) || { id: item.id });
  const result = await post(`/api/presets/${encodeURIComponent(item.id)}/apply`);
  emit("refresh");
  const steps = result.steps || [];
  let status = result.scene_status;
  const failed = steps.filter(step => !step.ok).map(step => `${step.step}: ${step.error || "Could not apply"}`);
  if (status?.status === "pending") {
    try {
      const { waitForLightingScene } = await import("./scene-commands.js");
      const ready = await waitForLightingScene(status.desired_revision, status.session);
      status = ready.scene_status;
    } catch (error) {
      failed.push(error.message || String(error));
    }
  }
  if (status?.status === "failed" && !steps.some(step => !step.ok && step.step === "lighting")) {
    failed.push(`Lighting: ${status.error || "Could not apply"}`);
  }
  if (failed.length || result.ok === false) {
    const detail = status?.overridden ? "Lighting is currently overridden." : status?.status === "ready" ? "Lighting applied." : "";
    return { error: [item.name, detail, ...(failed.length ? failed : ["Profile could not apply."])].filter(Boolean).join(" · ") };
  }
  return { message: confirmedMessage(status, item.name, "Profile"), undo: status?.overridden ? null : undoPoint() };
}

export function initQuickSwitcher(sections, catalogState) {
  initQuickPreferences();
  let current = null, busy = "";
  const active = () => current?.root.isConnected && $("#sheet").open;
  const refresh = () => { if (active()) current.render(); };
  on("state", refresh);
  on("zones", refresh);
  on("quick:changed", refresh);

  function open() {
    if (active()) { current.input.focus(); return; }
    if ($("dialog[open]")) return;
    const previous = document.activeElement;
    const sheet = openSheet(`<div class="body quick-switcher">
      <div class="sheet-head"><h2>Quick switch</h2><button class="icon-btn" data-quick-close aria-label="Close Quick switch"><svg aria-hidden="true"><use href="#i-close"/></svg></button></div>
      <label class="sr" for="qs-search">Search actions</label>
      <div class="qs-field">
        <svg aria-hidden="true"><use href="#i-search"/></svg>
        <input id="qs-search" class="text search" type="search" role="combobox" aria-autocomplete="list" aria-controls="qs-results" aria-expanded="false" placeholder="Search by name or action" autocomplete="off" autofocus>
      </div>
      <div class="qs-filters" role="group" aria-label="Filter actions">
        ${categories.map(({ kind, label }) => `<button type="button" data-qs-kind="${kind}" aria-pressed="${kind === "all"}">${label}</button>`).join("")}
      </div>
      <label class="qs-mobile-filter">Search in
        <select class="select">${categories.map(({ kind, label }) => `<option value="${kind}">${kind === "all" ? "All actions" : label}</option>`).join("")}</select>
      </label>
      <div id="qs-results" class="qs-results" role="listbox" aria-label="Matching actions"></div>
      <div class="qs-footer"><button class="text-link qs-manage" type="button" aria-pressed="false">Edit favorites</button>
        <span class="hint qs-count" aria-live="polite" aria-atomic="true"></span></div>
      <p class="qs-status" role="status" hidden></p>
      <button class="text-link qs-reset" type="button" hidden>Show all actions</button>
    </div>`);
    const root = $(".quick-switcher", sheet), input = $("#qs-search", root), list = $("#qs-results", root), status = $(".qs-status", root);
    const filters = $(".qs-filters", root), mobileFilter = $(".qs-mobile-filter select", root), reset = $(".qs-reset", root);
    const manage = $(".qs-manage", root);
    const count = $(".qs-count", root);
    let items = [], selected = 0, message = "", kind = "all", editingFavorites = false;
    const session = { root, input, render };
    current = session;

    function dismiss() {
      closeSheet();
      if (previous?.isConnected) previous.focus();
    }

    function markSelection(scroll = false) {
      const options = $$("[role=option]", list);
      for (const [index, option] of options.entries()) {
        const value = String(index === selected);
        if (option.getAttribute("aria-selected") !== value) option.setAttribute("aria-selected", value);
      }
      const row = options[selected];
      if (row) {
        input.setAttribute("aria-activedescendant", row.id);
        if (scroll) row.scrollIntoView({ block: "nearest" });
      } else input.removeAttribute("aria-activedescendant");
    }

    function render(keepSelection = true) {
      const state = catalogState();
      const previousItem = keepSelection ? items[selected] : null;
      const catalog = commands(sections, state === "ready");
      const matches = search(catalog.filter(item => (kind === "all" || item.kind === kind || (kind === "favorite" && isFavorite(item.kind, item.id))) &&
        (!editingFavorites || item.kind === "profile" || item.kind === "effect")), input.value);
      const groups = input.value.trim() ? [["", matches]] : groupedActions(matches).filter(([, entries]) => entries.length);
      items = groups.flatMap(([, entries]) => entries);
      if (previousItem) selected = items.findIndex(item => item.kind === previousItem.kind && item.id === previousItem.id);
      else if (!keepSelection) selected = 0;
      let index = 0;
      function option(item) {
        const i = index++;
        const action = editingFavorites ? isFavorite(item.kind, item.id) ? "Unpin favorite" : "Pin favorite" :
          item.kind === "section" ? "Open section" : item.action;
        return `<div id="qs-option-${i}" class="qs-option" role="option" data-kind="${item.kind}" data-index="${i}" aria-disabled="${!!busy}">
        <svg class="qs-type" aria-hidden="true"><use href="#i-${item.kind === "section" && SECTION_ICONS[item.id] || icons[item.kind]}"/></svg>
        <span>${esc(item.name)}</span><small>${esc(action)}${item.detail ? ` · ${esc(item.detail)}` : ""}${item.current ? " · Current" : ""}</small></div>`;
      }
      const body = groups.map(([label, entries]) => {
        const rows = entries.map(option).join("");
        if (!label || (label === "All actions" && groups.length === 1)) return rows;
        return `<div role="group" aria-label="${label}"><div class="qs-group-title" aria-hidden="true">${label}</div>${rows}</div>`;
      }).join("");
      setHTML(list, body);
      list.hidden = !items.length;
      input.setAttribute("aria-expanded", String(!!items.length));
      markSelection();
      const note = busy || message || (state === "loading" ? "Loading effects, profiles, zones and workspaces…" :
        state === "unavailable" ? "Effects, profiles, zones and workspaces are unavailable. Airglow is reconnecting." :
        !items.length ? kind === "favorite" && !input.value.trim() ? "No pinned favorites. Choose Edit favorites to pin an effect or profile." : "No matching actions." : "");
      status.hidden = !note;
      status.classList.toggle("err", !!message);
      if (status.textContent !== note) status.textContent = note;
      const total = !note && (input.value.trim() || kind !== "all") ? `${items.length} result${items.length === 1 ? "" : "s"}` : "";
      if (count.textContent !== total) count.textContent = total;
      reset.hidden = !!items.length || !!busy || (!input.value && kind === "all");
    }

    async function choose(index) {
      const item = items[index];
      if (!item || busy) return;
      if (editingFavorites) {
        selected = index;
        busy = "Saving favorites…";
        message = "";
        render();
        try { await setFavorite(item.kind, item.id, !isFavorite(item.kind, item.id)); }
        catch (error) {
          message = `Could not save favorite: ${error.message}`;
          if (current !== session || !active()) toast(message, true);
        }
        finally {
          busy = "";
          if (current === session && active()) { render(); input.focus(); }
          else refresh();
        }
        return;
      }
      if (!commands(sections, catalogState() === "ready").some(command => command.kind === item.kind && command.id === item.id)) {
        message = "This action is no longer available. Choose another result.";
        render();
        return;
      }
      selected = index;
      message = "";
      busy = item.kind === "effect" || item.kind === "profile" ? `Applying ${item.name}…` : `${item.action}…`;
      render();
      let outcome;
      try { outcome = await activate(item); }
      catch (error) { outcome = { error: error.message || String(error) }; }
      busy = "";
      if (current === session && active()) {
        if (outcome.error) {
          message = outcome.error;
          render();
          input.focus();
          return;
        }
        closeSheet();
      } else refresh();
      if (outcome.route) {
        if (location.hash === `#${outcome.route}` && (item.kind === "setting" || item.kind === "zone")) emit("route:revisit");
        else location.hash = `#${outcome.route}`;
      }
      if (outcome.undo && outcome.message) {
        const { undoAction } = await import("./profile-undo.js");
        toast(outcome.message, { action: undoAction(outcome.undo) });
      } else if (outcome.message || outcome.error) toast(outcome.message || outcome.error, !!outcome.error);
    }

    function refilter(nextKind) {
      kind = nextKind;
      mobileFilter.value = kind;
      message = "";
      for (const button of $$("[data-qs-kind]", filters)) button.setAttribute("aria-pressed", String(button.dataset.qsKind === kind));
      render(false);
      list.scrollTop = 0;
    }
    input.addEventListener("input", () => refilter(kind));
    manage.addEventListener("click", () => {
      if (busy) return;
      editingFavorites = !editingFavorites;
      manage.setAttribute("aria-pressed", String(editingFavorites));
      manage.textContent = editingFavorites ? "Done editing favorites" : "Edit favorites";
      list.setAttribute("aria-label", editingFavorites ? "Choose an item to pin or unpin" : "Matching actions");
      for (const filter of [...filters.querySelectorAll("button"), ...mobileFilter.options]) {
        const value = filter.dataset.qsKind || filter.value;
        filter.hidden = editingFavorites && !["all", "effect", "profile"].includes(value);
      }
      refilter(editingFavorites && !["effect", "profile"].includes(kind) ? "all" : kind);
      input.focus();
    });
    filters.addEventListener("click", event => {
      const button = event.target.closest("[data-qs-kind]");
      if (!button) return;
      refilter(button.dataset.qsKind);
      input.focus();
    });
    mobileFilter.addEventListener("change", () => refilter(mobileFilter.value));
    reset.addEventListener("click", () => {
      input.value = "";
      refilter("all");
      input.focus();
    });
    input.addEventListener("keydown", (event) => {
      if (event.isComposing) return;
      if (event.key === "Enter") { event.preventDefault(); choose(selected); return; }
      if (items.length && (event.key === "PageDown" || event.key === "PageUp")) {
        event.preventDefault();
        const row = list.querySelector(`[data-index="${Math.max(0, selected)}"]`);
        const step = Math.max(1, Math.floor(list.clientHeight / (row?.offsetHeight || 44)));
        selected = Math.max(0, Math.min(items.length - 1, selected + (event.key === "PageDown" ? step : -step)));
        markSelection(true);
        return;
      }
      if ((event.key !== "ArrowDown" && event.key !== "ArrowUp") || !items.length) return;
      event.preventDefault();
      if (selected < 0) selected = event.key === "ArrowDown" ? 0 : items.length - 1;
      else selected = (selected + (event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length;
      markSelection(true);
    });
    list.addEventListener("mousedown", event => { if (event.target.closest("[data-index]")) event.preventDefault(); });
    list.addEventListener("click", (event) => {
      const option = event.target.closest("[data-index]");
      if (option) choose(Number(option.dataset.index));
    });
    $("[data-quick-close]", root).addEventListener("click", dismiss);
    root.addEventListener("keydown", event => {
      if (event.key !== "Escape" || event.isComposing) return;
      event.preventDefault();
      event.stopPropagation();
      dismiss();
    });
    render();
    input.focus();
  }

  $("#quick-open").addEventListener("click", open);
  document.addEventListener("keydown", (event) => {
    if (!event.ctrlKey || event.shiftKey || event.altKey || event.metaKey || event.isComposing || event.key.toLowerCase() !== "k") return;
    if ($("dialog[open]") && !active()) return;
    event.preventDefault();
    open();
  });
  return refresh;
}
