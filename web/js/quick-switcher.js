import { $, $$, S, cap, emit, esc, post, setHTML, toast } from "./core.js";
import { activateOverviewWorkspace } from "./overview.js";
import { closeSheet, openSheet } from "./sheet.js";

function commands(sections, ready) {
  const navigation = sections.map(id => ({ kind: "section", id, name: cap(id), action: `Open ${cap(id)}` }));
  if (!ready) return navigation;
  return [
    ...navigation,
    ...S.effects.map(({ id, name }) => ({ kind: "effect", id, name, action: "Apply effect" })),
    ...S.presets.map(({ id, name, fan_curve, fan_points, fan_profile }) => ({
      kind: "profile", id, name, action: "Apply profile",
      detail: fan_curve || fan_points?.length || fan_profile ? "Lighting and cooling" : "Lighting",
    })),
    ...(S.overview?.workspaces || []).map(({ id, name }) => ({ kind: "workspace", id, name, action: "Switch workspace" })),
  ];
}

function search(items, value) {
  const query = value.trim().toLocaleLowerCase();
  if (!query) return items;
  const words = query.split(/\s+/);
  return items.filter(item => {
    const searchable = `${item.name} ${item.action} ${item.kind} ${item.detail || ""}`.toLocaleLowerCase();
    return words.every(word => searchable.includes(word));
  }).sort((a, b) => rank(a) - rank(b));

  function rank(item) {
    const name = item.name.toLocaleLowerCase();
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
  return { message: confirmedMessage(status, item.name, "Profile") };
}

export function initQuickSwitcher(sections, catalogState) {
  let current = null, busy = "";
  const active = () => current?.root.isConnected && $("#sheet").open;
  const refresh = () => { if (active()) current.render(); };

  function open() {
    if (active()) { current.input.focus(); return; }
    if ($("#sheet").open) return;
    const previous = document.activeElement;
    const sheet = openSheet(`<div class="body quick-switcher">
      <div class="sheet-head"><h2>Quick switch</h2><button class="icon-btn" data-quick-close aria-label="Close Quick switch"><svg aria-hidden="true"><use href="#i-close"/></svg></button></div>
      <label class="sr" for="qs-search">Search actions</label>
      <input id="qs-search" class="text search" type="search" role="combobox" aria-autocomplete="list" aria-controls="qs-results" aria-expanded="false" placeholder="Search actions" autocomplete="off" autofocus>
      <div id="qs-results" class="qs-results" role="listbox" aria-label="Matching actions"></div>
      <p class="qs-status" role="status" hidden></p>
    </div>`);
    const root = $(".quick-switcher", sheet), input = $("#qs-search", root), list = $("#qs-results", root), status = $(".qs-status", root);
    let items = [], selected = 0, message = "";
    const session = { root, input, render };
    current = session;

    function dismiss() {
      closeSheet();
      if (previous?.isConnected) previous.focus();
    }

    function markSelection(scroll = false) {
      const options = $$("[role=option]", list);
      for (const [index, option] of options.entries()) option.setAttribute("aria-selected", String(index === selected));
      const row = options[selected];
      if (row) {
        input.setAttribute("aria-activedescendant", row.id);
        if (scroll) row.scrollIntoView({ block: "nearest" });
      } else input.removeAttribute("aria-activedescendant");
    }

    function render() {
      const state = catalogState();
      items = search(commands(sections, state === "ready"), input.value);
      selected = Math.min(selected, Math.max(0, items.length - 1));
      const body = items.map((item, index) => `<div id="qs-option-${index}" class="qs-option" role="option" data-index="${index}" aria-disabled="${!!busy}">
        <span>${esc(item.name)}</span><small>${esc(item.action)}${item.detail ? ` · ${esc(item.detail)}` : ""}</small></div>`).join("");
      setHTML(list, body);
      list.hidden = !items.length;
      input.setAttribute("aria-expanded", String(!!items.length));
      markSelection();
      const note = busy || message || (state === "loading" ? "Loading effects, profiles and workspaces…" :
        state === "unavailable" ? "Effects, profiles and workspaces are unavailable. Airglow is reconnecting." :
        !items.length ? "No matching actions." : "");
      status.hidden = !note;
      status.classList.toggle("err", !!message);
      status.textContent = note;
    }

    async function choose(index) {
      const item = items[index];
      if (!item || busy) return;
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
      if (outcome.route) location.hash = `#${outcome.route}`;
      if (outcome.message || outcome.error) toast(outcome.message || outcome.error, !!outcome.error);
    }

    input.addEventListener("input", () => { selected = 0; message = ""; render(); list.scrollTop = 0; });
    input.addEventListener("keydown", (event) => {
      if (event.isComposing) return;
      if (event.key === "Enter") { event.preventDefault(); choose(selected); return; }
      if ((event.key !== "ArrowDown" && event.key !== "ArrowUp") || !items.length) return;
      event.preventDefault();
      selected = (selected + (event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length;
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

  document.addEventListener("keydown", (event) => {
    if (!event.ctrlKey || event.shiftKey || event.altKey || event.metaKey || event.isComposing || event.key.toLowerCase() !== "k") return;
    if ($("#sheet").open && !active()) return;
    event.preventDefault();
    open();
  });
  return refresh;
}
