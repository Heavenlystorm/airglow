// Boot, routing, the 2-second state poll, and the top bar.

import { $, $$, S, emit, esc, fail, get, on, post, setHTML, toast } from "./core.js";
import { initStage } from "./stage.js";
import { initLighting, renderLighting } from "./lighting.js";
import { initFans, renderFans } from "./fans.js";
import { initAutomations, renderAutomations } from "./automations.js";
import { initDevices, renderDevices, showZone } from "./devices.js";
import { initSettings, loadSettings } from "./settings.js";
import { openWelcome } from "./onboarding.js";
import { openSetup } from "./setup.js";
import { initOverview, onFrames, renderOverview } from "./overview.js";
import { R, fmt, level, loadCatalog, pushReadings } from "./readings.js";
import { countTo, enter, moveMarker } from "./motion.js";
import { pauseAutomations, sentence } from "./automations.js";
import { startLive, sync as syncLive } from "./live.js";

const TABS = ["overview", "lighting", "fans", "automations", "devices", "settings"];
let currentView = null;
const HISTORY_SECONDS = 600;
let connected = true;
let deviceKey = "";
let setupDismissed = 0;          // "Not now" on the new-lights notice: quiet until even more new lights appear

// ------------------------------------------------------------ routing
function route() {
  const [tab, arg] = (location.hash || "#overview").slice(1).split("/");
  const view = TABS.includes(tab) ? tab : "overview";
  $$(".tabs a").forEach((a) => (a.dataset.tab === view ? a.setAttribute("aria-current", "page") : a.removeAttribute("aria-current")));
  $$(".view").forEach((v) => (v.hidden = v.dataset.view !== view));
  moveMarker($(".tab-ink"), $(`.tabs a[data-tab="${view}"]`), currentView === null);
  if (currentView !== null && currentView !== view) enter($(`.view[data-view="${view}"]`));
  currentView = view;
  if (view === "overview") renderOverview();
  if (view === "fans") renderFans();
  if (view === "automations") renderAutomations();
  if (view === "devices") { if (arg) showZone(decodeURIComponent(arg)); renderDevices(); }
  if (view === "settings") loadSettings();
  if (view === "lighting") renderLighting();
}
window.addEventListener("hashchange", route);

window.addEventListener("resize", () => moveMarker($(".tab-ink"), $(".tabs a[aria-current]"), true));

// Ctrl+1..6 switch tabs
document.addEventListener("keydown", (e) => {
  if (e.ctrlKey && !e.shiftKey && !e.altKey && e.key >= "1" && e.key <= "6") {
    e.preventDefault();
    location.hash = "#" + TABS[+e.key - 1];
  }
});

// ------------------------------------------------------------ top bar
function renderPins() {
  const pins = S.overview?.pins || [];
  const box = $("#ambient");
  const key = pins.join("|") + (R.loaded ? "1" : "0");
  if (box._key !== key) {
    box._key = key;
    box.innerHTML = pins.map((id) => `<span class="pin" data-pin="${esc(id)}">${esc(R.meta[id]?.short || "")} <b></b><i></i></span>`).join("");
  }
  for (const el of box.querySelectorAll("[data-pin]")) {
    const id = el.dataset.pin, meta = R.meta[id], v = S.state.readings?.[id] ?? null;
    const f = fmt(meta, v);
    const b = el.querySelector("b");
    b.className = level(id, meta, v);
    countTo(b, f.num);
    el.querySelector("i").textContent = f.unit === "°C" || f.unit === "°F" ? "°" : f.unit === "%" ? "%" : ` ${f.unit}`;
  }
}

// Why the lights are dark although you left them on (an automation, or the lock setting), or null.
function heldOff() {
  const s = S.state;
  if (!s.scene.on || s.effective.on) return null;
  const rule = S.rules.find((r) => r.id === s.override?.lighting);
  if (rule) return { rule, text: `An automation is keeping them off: “${sentence(rule, false)}”` };
  if (s.override?.locked) return { text: "They're off while the PC is locked (Settings)." };
  return { text: "Something is keeping them off right now." };
}

function renderBar() {
  renderPins();
  const held = heldOff();
  $("#power").setAttribute("aria-pressed", String(S.state.scene.on));
  $("#power").toggleAttribute("data-held", !!held);
  $("#power").title = held ? `Lights are on, but dark. ${held.text}` : S.state.scene.on ? "Turn the lights off" : "Turn the lights on";

  const c = S.state.conflicts || [];
  const n = $("#notice");
  if (!connected) {
    n.hidden = false;
    setHTML(n, `<span>Airglow isn't responding. If you just updated it, it may be restarting.</span>`);
  } else if (S.state.healthy === false) {
    n.hidden = false;
    setHTML(n, `<span>Lighting has stopped updating. Quit Airglow from its tray icon and open it again. Details are in the log folder.</span>`);
  } else if (c.length) {
    n.hidden = false;
    setHTML(n, c.map((x) => `<span><b>${esc(x.name)}</b> is running and also controls ${esc(x.conflicts_with)}, so lights may flicker.</span>
      <button class="btn" data-close-app="${esc(x.name)}">Close ${esc(x.name)}</button>`).join("") + `<span class="spacer"></span>`);
  } else if (S.state.onboarded && S.state.setup_pending > setupDismissed) {
    const k = S.state.setup_pending;
    n.hidden = false;
    setHTML(n, `<span><b>${k} new light${k === 1 ? "" : "s"}</b> to set up. Tell Airglow what ${k === 1 ? "it is" : "they are"} so effects line up with your case.</span>
      <button class="btn" data-open-setup="start">Set up</button><button class="btn quiet" id="setup-later">Not now</button><span class="spacer"></span>`);
  } else {
    n.hidden = true;
  }
}

document.addEventListener("click", (e) => {
  const b = e.target.closest("[data-open-setup]");
  if (b) openSetup({ start: b.dataset.openSetup === "start", only: b.dataset.zone || null });
  if (e.target.closest("#setup-later")) { setupDismissed = S.state.setup_pending; renderBar(); }
});

$("#power").addEventListener("click", async () => {
  const held = heldOff();
  if (held?.rule) {                                   // pressing "on" would change nothing you can see
    if (confirm(`${held.text}\n\nPause automations for an hour so the lights come back?`)) pauseAutomations(60);
    return;
  }
  const on = !S.state.scene.on;
  try { const r = await post("/api/scene", { on }); S.state.scene = r.scene; renderBar(); emit("state"); }
  catch (e) { fail(e); }
});

// ------------------------------------------------------------ polling
function pushSample() {
  const s = S.state.sensors;
  S.history.push({ t: Date.now() / 1000, cpu: s.cpu_temp, gpu: s.gpu_temp, cpu_w: s.cpu_power, gpu_w: s.gpu_power, duty: S.state.fans.duty });
  const cutoff = Date.now() / 1000 - HISTORY_SECONDS;
  while (S.history.length && S.history[0].t < cutoff) S.history.shift();
}

// A state update, from the live stream or an explicit refresh after a change.
async function applyState(state) {
  S.state = state;
  if (!connected) { connected = true; toast("Reconnected"); }
  pushSample();
  pushReadings(S.state.readings);
  // device list or connection changed: reload zones so the stage and Devices stay accurate
  const key = JSON.stringify([S.state.zones_rev, S.state.devices.map((d) => [d.id, d.online, d.zones.length])]);
  if (key !== deviceKey) {
    deviceKey = key;
    S.zones = await get("/api/zones");
    emit("zones");
  }
  renderBar();
  emit("state");
}

function lost() {
  if (connected) { connected = false; renderBar(); }
}

async function refresh() {
  if (document.visibilityState === "hidden") return;   // closed to the tray: nothing to show
  try { await applyState(await get("/api/state")); } catch { lost(); }
}
on("refresh", refresh);
on("pins", () => { $("#ambient")._key = null; refresh(); });
on("frames", onFrames);

async function boot() {
  try {
    const [state, effects, presets, curves, settings, rules, zones, history, overview] = await Promise.all([
      get("/api/state"), get("/api/effects"), get("/api/presets"), get("/api/curves"), get("/api/settings"),
      get("/api/automations"), get("/api/zones"), get("/api/history"), get("/api/overview"), loadCatalog(),
    ]);
    Object.assign(S, { state, effects, presets, curves, settings, rules, zones, overview });
    S.history = history.filter((p) => p.t > Date.now() / 1000 - HISTORY_SECONDS);
    deviceKey = JSON.stringify([state.zones_rev, state.devices.map((d) => [d.id, d.online, d.zones.length])]);
  } catch (e) {
    toast(`Couldn't reach Airglow: ${e.message}. Retrying.`, true);
    setTimeout(boot, 3000);
    return;
  }
  await initOverview(S.overview);
  initStage();
  initLighting();
  initFans();
  initAutomations();
  initDevices();
  initSettings();
  renderBar();
  route();
  if (!S.state.onboarded) openWelcome();
  startLive({
    state: (s) => applyState(s).then(syncLive).catch(lost),     // a lighting tile may have come or gone
    lost,
    // views showing the live LED preview
    frames: () => !$('[data-view="lighting"]').hidden || !$('[data-view="devices"]').hidden
      || (!$('[data-view="overview"]').hidden && !!document.querySelector(".tile-lighting")),
  });
  setInterval(() => loadCatalog().catch(() => {}), 60000);     // new hardware, LHM started later
}

boot();
