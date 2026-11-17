// Devices screen: every device and its zones (LED counts, shape, colour order, identify), and OpenRGB setup.

import { $, $$, S, emit, esc, fail, on, post, refreshZones, setHTML, toast } from "./core.js";
import { fieldHTML, wireFields } from "./lighting.js";

let current = null;              // selected device id, or "openrgb"
let frames = {};

const ORDERS = ["RGB", "RBG", "BGR", "BRG", "GBR", "GRB"];
const zoneSaves = new Map();
const expandedZones = new Set();
let deferredDetail = false;

function devices() {
  const byId = new Map();
  for (const z of S.zones?.zones || []) {
    if (!byId.has(z.device.id)) byId.set(z.device.id, { ...z.device, zones: [] });
    byId.get(z.device.id).zones.push(z);
  }
  return [...byId.values()];
}

function renderList() {
  const devs = devices();
  const native = devs.filter((d) => d.kind === "native"), orgb = devs.filter((d) => d.kind === "openrgb");
  const dyn = devs.filter((d) => d.kind === "dynamic");
  // the selected device went away (unplugged, OpenRGB closed): show another one instead of an empty page
  if ((!current || (current !== "openrgb" && !devs.some((d) => d.id === current))) && devs.length) current = devs[0].id;
  const item = (d) => {
    const lit = d.zones.filter((z) => z.config.enabled).length;
    const sub = d.online ? `${d.type}, ${lit} of ${d.zones.length} zone${d.zones.length === 1 ? "" : "s"} lit` : (d.error || "Not connected");
    return `<button class="dev-item" role="option" data-dev="${esc(d.id)}" aria-selected="${current === d.id}">
      <i class="dot ${d.online ? "on" : "off"}"></i><span>${esc(d.name)}${d.experimental ? ` <span class="tag">Experimental</span>` : ""}</span><span class="sub">${esc(sub)}</span></button>`;
  };
  const o = S.state?.openrgb || {};
  const focused = document.activeElement;
  const list = $("#dev-list");
  setHTML(list, `
    <button class="btn primary dev-setup" data-open-setup${S.state?.setup_pending ? `="start">Set up ${S.state.setup_pending} new light${S.state.setup_pending === 1 ? "" : "s"}` : ">Set up lights"}</button>
    ${native.length ? `<h2>Connected directly</h2>${native.map(item).join("")}` : ""}
    <h2>Through OpenRGB</h2>${orgb.map(item).join("")}
    <button class="dev-item" role="option" data-dev="openrgb" aria-selected="${current === "openrgb"}">
      <i class="dot ${o.status?.startsWith("Connected") ? "on" : ""}"></i><span>${orgb.length ? "OpenRGB connection" : "Add more devices"}</span><span class="sub">${esc(o.status || "")}</span></button>
    ${dyn.length ? `<h2>Through Windows Dynamic Lighting</h2>${dyn.map(item).join("")}` : ""}`);
  if (focused?.dataset?.dev && !focused.isConnected) {
    (list.querySelector(`[data-dev="${CSS.escape(focused.dataset.dev)}"]`) || list.querySelector('[aria-selected="true"]'))?.focus({ preventScroll: true });
  }
}

function zoneCard(z) {
  const c = zoneSaves.get(z.key)?.draft || z.config, L = c.layout, max = z.zone.max_leds;
  const total = L.shape === "fans" ? L.fans * L.leds_per_fan : L.leds;
  const grid = z.zone.matrix, expanded = expandedZones.has(z.key);
  const shape = L.shape === "fans" ? `${L.fans} fan${L.fans === 1 ? "" : "s"}` : L.shape === "matrix" ? `${L.rows} × ${L.cols} grid` : "Strip";
  const equipment = !c.enabled && z.zone.default_leds === 0 ? "Nothing detected" : `${shape} · ${total} LED${total === 1 ? "" : "s"}`;
  const id = `wiring-${esc(z.key)}`;
  return `<div class="zone-card${c.enabled ? "" : " off"}" id="zc-${esc(z.key)}" data-key="${esc(z.key)}">
    <div class="zone-summary"><label class="switch"><input type="checkbox" data-f="enabled"${c.enabled ? " checked" : ""} aria-label="${esc(c.name)} on"><span></span></label>
      <div><strong>${esc(c.name)}</strong><span>${esc(equipment)} · ${c.enabled ? "On" : "Off"}</span></div></div>
    <button type="button" class="btn zone-configure" data-configure aria-expanded="${expanded}" aria-controls="${id}" aria-label="Configure ${esc(c.name)}">Configure <svg aria-hidden="true"><use href="#i-down"/></svg></button>
    <div class="zstrip" aria-hidden="true" data-strip="${esc(z.key)}">${Array.from({ length: Math.min(total, 60) }, () => "<i></i>").join("")}</div>
    <div class="zone-wiring" id="${id}"${expanded ? "" : " hidden"}>
    <div class="zfields">
      <label>Name<input class="text" data-f="name" value="${esc(c.name)}" maxlength="40" aria-label="Zone name" spellcheck="false"></label>
      <label>Shape<select class="select" data-f="shape" aria-label="Shape"><option value="fans"${L.shape === "fans" ? " selected" : ""}>Fans</option><option value="strip"${L.shape === "strip" ? " selected" : ""}>Strip</option>${grid ? `<option value="matrix"${L.shape === "matrix" ? " selected" : ""}>Grid</option>` : ""}</select></label>
      ${L.shape === "fans"
        ? `<label>Fans<input class="text num" type="number" min="1" max="12" data-f="fans" aria-label="Fans" value="${L.fans}"></label>
           <label>LEDs per fan<input class="text num" type="number" min="1" max="128" data-f="leds_per_fan" aria-label="LEDs per fan" value="${L.leds_per_fan}"></label>`
        : L.shape === "matrix"
        ? `<span class="hint">${L.rows} × ${L.cols} grid, as OpenRGB lays it out</span>`
        : `<label>LEDs<input class="text num" type="number" min="1" max="${max}" data-f="leds" aria-label="LEDs" value="${L.leds}"></label>`}
      <label>Colour order<select class="select" data-f="order" aria-label="Colour order">${ORDERS.map((o) => `<option${o === c.order ? " selected" : ""}>${o}</option>`).join("")}</select></label>
      <label>Direction<select class="select" data-f="reverse" aria-label="Direction"><option value="0"${L.reverse ? "" : " selected"}>Normal</option><option value="1"${L.reverse ? " selected" : ""}>Reversed</option></select></label>
      <span class="hint" style="margin-left:auto">${!c.enabled && z.zone.default_leds === 0 ? "Nothing detected on this channel" : `${total} of up to ${max} LEDs`}</span>
    </div>
    <div class="btn-row">${z.zone.resizable ? `<button class="btn" data-open-setup data-zone="${esc(z.key)}">What's plugged in?</button>` : ""}<button class="btn" data-identify${z.device.online ? "" : " disabled"}>Identify</button></div>
    </div>
  </div>`;
}

function renderDetail() {
  const box = $("#dev-detail");
  if (current === "openrgb") {
    const o = S.state?.openrgb || {};
    setHTML(box, `<div class="dev-head"><div><h1>OpenRGB</h1><p class="meta">${esc(o.status || "")}</p></div>
        <label class="switch" title="Use OpenRGB"><input type="checkbox" id="orgb-on"${o.enabled ? " checked" : ""} aria-label="Use OpenRGB"><span></span></label></div>
      <p class="lede">OpenRGB is a free program that supports hundreds of RGB devices: RAM, graphics cards, keyboards, mice, strips and more. When it runs with its SDK server switched on, those devices appear here and follow your effects.</p>
      <div class="openrgb-box"><strong>Set it up once</strong><ol>
        <li>Download OpenRGB from <code>openrgb.org</code> and open it.</li>
        <li>Go to the <b>SDK Server</b> tab and click <b>Start Server</b>. In Settings, turn on <b>Start server</b> and <b>Start at login</b> so it's always ready.</li>
        <li>Airglow connects within about 10 seconds. New devices start with their lighting on, except keyboards, mice and headsets, which you can switch on here.</li>
      </ol><p class="hint">Devices Airglow already drives directly (the NZXT hub and the ASUS board) are left out of OpenRGB, so two programs never fight over them.</p></div>`);
    $("#orgb-on").onchange = (e) => post("/api/settings", { openrgb_enabled: e.target.checked }).then(() => { toast(e.target.checked ? "OpenRGB bridge on" : "OpenRGB bridge off"); emit("refresh"); }).catch(fail);
    return;
  }
  const d = devices().find((x) => x.id === current);
  if (!d) {
    setHTML(box, `<div><h1>No devices yet</h1><p class="lede">Airglow looks for NZXT Control Hubs and ASUS Aura boards over USB, and for everything else through OpenRGB. Pick <b>Add more devices</b> to set that up.</p></div>`);
    return;
  }
  const how = { native: "Direct USB connection", openrgb: "Through OpenRGB", dynamic: "Through Windows Dynamic Lighting, lit while Airglow's window is in front" }[d.kind] || "";
  const active = document.activeElement;
  const focusedZone = active?.closest?.("[data-key]")?.dataset.key;
  const activeControl = focusedZone && Object.keys(active.dataset).find(key => ["f", "configure", "identify", "openSetup"].includes(key));
  // Replacing a dirty native input fires change on Chromium and silently commits a partial name/count.
  // Keep this field alive until the user's own blur. Discovery can still update the device list.
  if (!pendingScroll && activeControl === "f" && ["text", "number"].includes(active.type)
      && active.value !== active.defaultValue && d.zones.some(zone => zone.key === focusedZone)) {
    deferredDetail = true;
    return;
  }
  deferredDetail = false;
  setHTML(box, `<div class="dev-head"><div><h1>${esc(d.name)}</h1><p class="meta">${esc(d.vendor)}, ${esc(d.type)}. ${how}.${d.online ? "" : ` <span style="color:var(--warm)">${esc(d.error || "Not connected")}</span>`}</p></div></div>
    ${d.experimental ? `<p class="hint warn"><b>Experimental support.</b> This controller follows the same protocol as ones Airglow is tested with, but nobody has confirmed it with Airglow yet.${d.note ? ` ${esc(d.note)}` : ""} If something looks wrong, save a diagnostics report (Settings, About) and open an issue.</p>` : ""}
    <p class="hint">Choose Configure to rename a zone, adjust its wiring or identify its lights.</p>
    ${paramsBlock(d)}
    ${d.zones.map(zoneCard).join("")}`);
  wireParams(d);
  if (pendingScroll) {
    const card = document.getElementById(`zc-${pendingScroll}`);
    card?.scrollIntoView({ block: "center" });
    card?.querySelector("[data-configure]").focus({ preventScroll: true });
    pendingScroll = null;
  } else if (activeControl && !active.isConnected) {
    const card = document.getElementById(`zc-${focusedZone}`);
    const selector = activeControl === "f" ? `[data-f="${CSS.escape(active.dataset.f)}"]` : "[data-" + (activeControl === "openSetup" ? "open-setup" : activeControl) + "]";
    const field = card?.querySelector(selector);
    (field || card?.querySelector("[data-configure]"))?.focus({ preventScroll: true });
  }
}

let pendingScroll = null;

// Settings a device plugin declares (airglow/plugins/*.py, PARAMETERS), drawn from the same schema as effects.
function paramsBlock(d) {
  if (!d.parameters?.length) return "";
  return `<section class="dev-params" aria-label="Device settings">
    ${d.parameters.map((p) => fieldHTML(p, d.params?.[p.id] ?? p.default, `dp-${d.id}`)).join("")}</section>`;
}

let paramTimer = null;
function wireParams(d) {
  const root = $("#dev-detail .dev-params");
  if (!root || root.dataset.wired) return;
  root.dataset.wired = "true";
  wireFields(root, { params: d.parameters }, d.params || {}, (params, live) => {
    clearTimeout(paramTimer);
    paramTimer = setTimeout(() => post(`/api/devices/${encodeURIComponent(d.id)}/params`, params)
      .then((r) => {
        for (const z of S.zones.zones) if (z.device.id === d.id) z.device.params = r.params;
        if (!live) toast("Saved");
      }).catch(fail), live ? 250 : 0);
  });
}

function saveZone(key, mutate) {
  const z = S.zones.zones.find((x) => x.key === key);
  if (!z) { fail(new Error("That zone isn't connected right now.")); return Promise.resolve(); }
  const queue = zoneSaves.get(key) || { draft: structuredClone(z.config), revision: 0, tail: Promise.resolve() };
  zoneSaves.set(key, queue);
  mutate(queue.draft);
  const snapshot = structuredClone(queue.draft), revision = ++queue.revision;
  queue.tail = queue.tail.then(async () => {
    try {
      const r = await post(`/api/zones/${encodeURIComponent(key)}`, snapshot);
      const current = S.zones.zones.find((x) => x.key === key);
      if (current) current.config = r.config;
      await refreshZones();
      emit("zones");
    } catch (err) { fail(err); }
    finally {
      if (revision === queue.revision) zoneSaves.delete(key);
      renderList(); renderDetail();
    }
  });
  return queue.tail;
}

function paintStrips() {
  for (const el of $$("#dev-detail [data-strip]")) {
    const hex = frames[el.dataset.strip];
    if (el._painted === hex) continue;              // an unchanged strip must not touch every cell again
    el._painted = hex;
    const cells = el.children;
    for (let i = 0; i < cells.length; i++) {
      cells[i].style.background = hex && hex.length >= (i + 1) * 6 ? `#${hex.substr(i * 6, 6)}` : "";
    }
  }
}

// LED frames arrive on the live stream (live.js) while this view is showing.
function onLiveFrames(f) {
  if ($('[data-view="devices"]').hidden) return;
  frames = f.zones;
  paintStrips();
}

export function renderDevices() {
  renderList();
  renderDetail();
  paintStrips();
}

export function showZone(key) {
  const z = S.zones?.zones.find((x) => x.key === key);
  if (z) { current = z.device.id; expandedZones.add(key); pendingScroll = key; }
}

export function initDevices() {
  $("#dev-list").addEventListener("click", (e) => {
    const b = e.target.closest("[data-dev]");
    if (b) { current = b.dataset.dev; renderList(); renderDetail(); paintStrips(); }
  });
  const box = $("#dev-detail");
  box.addEventListener("focusout", () => {
    if (deferredDetail) queueMicrotask(() => { if (deferredDetail) { renderDetail(); paintStrips(); } });
  });
  box.addEventListener("change", (e) => {
    const card = e.target.closest("[data-key]"), f = e.target.dataset.f;
    if (!card || !f) return;
    const key = card.dataset.key, v = e.target.type === "checkbox" ? e.target.checked : e.target.value;
    saveZone(key, (c) => {
      if (f === "enabled") c.enabled = v;
      else if (f === "name") c.name = v.trim();
      else if (f === "order") c.order = v;
      else if (f === "reverse") c.layout.reverse = v === "1";
      else if (f === "shape") {
        const total = c.layout.shape === "fans" ? c.layout.fans * c.layout.leds_per_fan : c.layout.leds;
        const grid = S.zones?.zones.find((x) => x.key === key)?.zone.matrix;
        c.layout = v === "fans"
          ? { ...c.layout, shape: "fans", fans: 1, leds_per_fan: Math.max(1, Math.min(total, 128)), size: 0.16 }
          : v === "matrix" && grid
          ? { ...c.layout, shape: "matrix", leds: Math.max(1, total), rows: grid.rows, cols: grid.cols, map: grid.map, length: 0.6 }
          : { ...c.layout, shape: "strip", leds: Math.max(1, total), length: 0.9 };
      } else c.layout[f] = Math.max(1, parseInt(v, 10) || 1);
    });
  });
  box.addEventListener("keydown", (e) => { if (e.key === "Enter" && e.target.matches("input[data-f=name]")) e.target.blur(); });
  box.addEventListener("click", (e) => {
    const card = e.target.closest("[data-key]");
    const configure = e.target.closest("[data-configure]");
    if (card && configure) {
      const key = card.dataset.key;
      if (expandedZones.has(key)) expandedZones.delete(key);
      else expandedZones.add(key);
      card.querySelector(".zone-wiring").hidden = !expandedZones.has(key);
      configure.setAttribute("aria-expanded", String(expandedZones.has(key)));
    }
    if (card && e.target.closest("[data-identify]")) {
      post(`/api/zones/${encodeURIComponent(card.dataset.key)}/identify`).then(() => toast("Watch the zone: the first LED blinks green, then a white dot walks through every LED.")).catch(fail);
    }
  });
  on("state", () => { if (!$('[data-view="devices"]').hidden) renderList(); });
  on("zones", () => { if (!$('[data-view="devices"]').hidden) renderDevices(); });
  on("frames:live", onLiveFrames);
}
