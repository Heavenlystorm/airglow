// Devices screen: every device and its zones (LED counts, shape, colour order, identify), and OpenRGB setup.

import { $, $$, S, emit, esc, fail, get, on, post, setHTML, toast } from "./core.js";

let current = null;              // selected device id, or "openrgb"
let frames = {};

const ORDERS = ["RGB", "RBG", "BGR", "BRG", "GBR", "GRB"];

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
  if (!current && devs.length) current = devs[0].id;
  const item = (d) => {
    const lit = d.zones.filter((z) => z.config.enabled).length;
    const sub = d.online ? `${d.type}, ${lit} of ${d.zones.length} zone${d.zones.length === 1 ? "" : "s"} lit` : (d.error || "Not connected");
    return `<button class="dev-item" role="option" data-dev="${esc(d.id)}" aria-selected="${current === d.id}">
      <i class="dot ${d.online ? "on" : "off"}"></i><span>${esc(d.name)}${d.experimental ? ` <span class="tag">Experimental</span>` : ""}</span><span class="sub">${esc(sub)}</span></button>`;
  };
  const o = S.state?.openrgb || {};
  setHTML($("#dev-list"), `
    ${native.length ? `<h2>Connected directly</h2>${native.map(item).join("")}` : ""}
    <h2>Through OpenRGB</h2>${orgb.map(item).join("")}
    <button class="dev-item" role="option" data-dev="openrgb" aria-selected="${current === "openrgb"}">
      <i class="dot ${o.status?.startsWith("Connected") ? "on" : ""}"></i><span>${orgb.length ? "OpenRGB connection" : "Add more devices"}</span><span class="sub">${esc(o.status || "")}</span></button>`);
}

function zoneCard(z) {
  const c = z.config, L = c.layout, max = z.zone.max_leds;
  const total = L.shape === "fans" ? L.fans * L.leds_per_fan : L.leds;
  const grid = z.zone.matrix;
  return `<div class="zone-card${c.enabled ? "" : " off"}" id="zc-${esc(z.key)}" data-key="${esc(z.key)}">
    <div class="zname"><span class="switch" title="${c.enabled ? "Switch this zone off" : "Switch this zone on"}"><input type="checkbox" data-f="enabled"${c.enabled ? " checked" : ""} aria-label="Zone on"><span></span></span>
      <input data-f="name" value="${esc(c.name)}" maxlength="40" aria-label="Zone name" spellcheck="false"></div>
    <div class="btn-row"><button class="btn" data-identify>Identify</button></div>
    <div class="zfields">
      <label>Shape<select class="select" data-f="shape"><option value="fans"${L.shape === "fans" ? " selected" : ""}>Fans</option><option value="strip"${L.shape === "strip" ? " selected" : ""}>Strip</option>${grid ? `<option value="matrix"${L.shape === "matrix" ? " selected" : ""}>Grid</option>` : ""}</select></label>
      ${L.shape === "fans"
        ? `<label>Fans<input class="text num" type="number" min="1" max="12" data-f="fans" value="${L.fans}"></label>
           <label>LEDs per fan<input class="text num" type="number" min="1" max="64" data-f="leds_per_fan" value="${L.leds_per_fan}"></label>`
        : L.shape === "matrix"
        ? `<span class="hint">${L.rows} × ${L.cols} grid, as OpenRGB lays it out</span>`
        : `<label>LEDs<input class="text num" type="number" min="1" max="${max}" data-f="leds" value="${L.leds}"></label>`}
      <label>Colour order<select class="select" data-f="order">${ORDERS.map((o) => `<option${o === c.order ? " selected" : ""}>${o}</option>`).join("")}</select></label>
      <label>Direction<select class="select" data-f="reverse"><option value="0"${L.reverse ? "" : " selected"}>Normal</option><option value="1"${L.reverse ? " selected" : ""}>Reversed</option></select></label>
      <span class="hint" style="margin-left:auto">${!c.enabled && z.zone.default_leds === 0 ? "Nothing detected on this channel" : `${total} of up to ${max} LEDs`}</span>
    </div>
    <div class="zstrip" data-strip="${esc(z.key)}">${Array.from({ length: Math.min(total, 60) }, () => "<i></i>").join("")}</div>
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
    $("#orgb-on").addEventListener("change", (e) => post("/api/settings", { openrgb_enabled: e.target.checked }).then(() => { toast(e.target.checked ? "OpenRGB bridge on" : "OpenRGB bridge off"); emit("refresh"); }).catch(fail));
    return;
  }
  const d = devices().find((x) => x.id === current);
  if (!d) {
    setHTML(box, `<div><h1>No devices yet</h1><p class="lede">Airglow looks for NZXT Control Hubs and ASUS Aura boards over USB, and for everything else through OpenRGB. Pick <b>Add more devices</b> to set that up.</p></div>`);
    return;
  }
  const how = d.kind === "native" ? "Direct USB connection" : "Through OpenRGB";
  setHTML(box, `<div class="dev-head"><div><h1>${esc(d.name)}</h1><p class="meta">${esc(d.vendor)}, ${esc(d.type)}. ${how}.${d.online ? "" : ` <span style="color:var(--warm)">${esc(d.error || "Not connected")}</span>`}</p></div></div>
    ${d.experimental ? `<p class="hint warn"><b>Experimental support.</b> This controller follows the same protocol as ones Airglow is tested with, but nobody has confirmed it with Airglow yet.${d.note ? ` ${esc(d.note)}` : ""} If something looks wrong, save a diagnostics report (Settings, About) and open an issue.</p>` : ""}
    <p class="hint">Set how many LEDs each zone has and their shape, so effects line up. If you're not sure, press Identify: the first LED blinks green, then a white dot walks through every LED in order.</p>
    ${d.zones.map(zoneCard).join("")}`);
  if (pendingScroll) { document.getElementById(`zc-${pendingScroll}`)?.scrollIntoView({ block: "center" }); pendingScroll = null; }
}

let pendingScroll = null;

async function saveZone(key, mutate) {
  const z = S.zones.zones.find((x) => x.key === key);
  const cfg = structuredClone(z.config);
  mutate(cfg);
  try {
    const r = await post(`/api/zones/${encodeURIComponent(key)}`, cfg);
    z.config = r.config;
    S.zones = await get("/api/zones");
    emit("zones");
    renderList(); renderDetail();
  } catch (err) { fail(err); renderDetail(); }
}

function paintStrips() {
  for (const el of $$("#dev-detail [data-strip]")) {
    const hex = frames[el.dataset.strip];
    const cells = el.children;
    for (let i = 0; i < cells.length; i++) {
      cells[i].style.background = hex && hex.length >= (i + 1) * 6 ? `#${hex.substr(i * 6, 6)}` : "#22262c";
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
  if (z) { current = z.device.id; pendingScroll = key; }
}

export function initDevices() {
  $("#dev-list").addEventListener("click", (e) => {
    const b = e.target.closest("[data-dev]");
    if (b) { current = b.dataset.dev; renderList(); renderDetail(); paintStrips(); }
  });
  const box = $("#dev-detail");
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
          ? { ...c.layout, shape: "fans", fans: 1, leds_per_fan: Math.max(1, Math.min(total, 64)), size: 0.16 }
          : v === "matrix" && grid
          ? { ...c.layout, shape: "matrix", leds: Math.max(1, total), rows: grid.rows, cols: grid.cols, map: grid.map, length: 0.6 }
          : { ...c.layout, shape: "strip", leds: Math.max(1, total), length: 0.9 };
      } else c.layout[f] = Math.max(1, parseInt(v, 10) || 1);
    });
  });
  box.addEventListener("keydown", (e) => { if (e.key === "Enter" && e.target.matches("input[data-f=name]")) e.target.blur(); });
  box.addEventListener("click", (e) => {
    const card = e.target.closest("[data-key]");
    if (card && e.target.closest("[data-identify]")) {
      post(`/api/zones/${encodeURIComponent(card.dataset.key)}/identify`).then(() => toast("Watch the zone: the first LED blinks green, then a white dot walks through every LED.")).catch(fail);
    }
  });
  on("state", () => { if (!$('[data-view="devices"]').hidden) renderList(); });
  on("zones", () => { if (!$('[data-view="devices"]').hidden) renderDevices(); });
  on("frames:live", onLiveFrames);
}
