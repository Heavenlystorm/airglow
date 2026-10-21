// Boot, routing, the 2-second state poll, and the top bar.

import { $, $$, S, curveLabel, emit, esc, fail, get, on, post, setHTML, toast } from "./core.js";
import { initStage } from "./stage.js";
import { initAutomations, renderAutomations, pauseAutomations, sentence } from "./automations.js";
import { openOnlineQuestion, openWelcome } from "./onboarding.js";
import { renderTrial } from "./profiles.js";
import { initOverview, onFrames, renderOverview } from "./overview.js";
import { R, fmt, level, loadCatalog, pushReadings } from "./readings.js";
import { countTo, enter, moveMarker } from "./motion.js";
import { initShortcuts } from "./help.js";
import { startLive, sync as syncLive } from "./live.js";

const TABS = ["overview", "lighting", "studio", "fans", "automations", "devices", "settings"];
const views = {};                 // loaded on first use: Overview should not wait for Studio, Fans, Settings or Devices
let currentView = null;
const HISTORY_SECONDS = 600;
let connected = true;
let deviceKey = "";
let setupDismissed = 0;          // "Not now" on the new-lights notice: quiet until even more new lights appear
let updateDismissed = null;      // "Later" on the update notice, for that version
let bootFailed = false;          // "Couldn't reach Airglow" is said once, not on every retry
let lastStateAt = 0;             // when the last state arrived: a stream can die without ever reporting an error
let probing = false;             // one watchdog fetch at a time
let failsafeSeen = false;        // the fans' failsafe speed is mentioned once while readings are missing

// ------------------------------------------------------------ routing
let routeSeq = 0;
const loading = {};

async function loadView(view) {
  if (views[view]) return views[view];
  if (loading[view]) return loading[view];
  loading[view] = (async () => {
    let m;
    if (view === "lighting") { m = await import("./lighting.js"); m.initLighting(); }
    else if (view === "fans") { m = await import("./fans.js"); m.initFans(); }
    else if (view === "devices") { m = await import("./devices.js"); m.initDevices(); }
    else if (view === "settings") { m = await import("./settings.js"); m.initSettings(); }
    else if (view === "studio") { m = await import("./studio.js"); m.initStudio(); }
    else throw new Error(`unknown view: ${view}`);
    views[view] = m;
    return m;
  })().finally(() => { delete loading[view]; });
  return loading[view];
}

async function route() {
  const seq = ++routeSeq;
  const [tab, arg] = (location.hash || "#overview").slice(1).split("/");
  const view = TABS.includes(tab) ? tab : "overview";
  $$(".tabs a").forEach((a) => (a.dataset.tab === view ? a.setAttribute("aria-current", "page") : a.removeAttribute("aria-current")));
  $$(".view").forEach((v) => (v.hidden = v.dataset.view !== view));
  moveMarker($(".tab-ink"), $(`.tabs a[data-tab="${view}"]`), currentView === null);
  if (currentView === "studio" && view !== "studio") views.studio?.leaveStudio();
  if (currentView !== null && currentView !== view) enter($(`.view[data-view="${view}"]`));
  currentView = view;
  if (view === "overview") { renderOverview(); return; }
  if (view === "automations") { renderAutomations(); return; }
  let m;
  try { m = await loadView(view); } catch (e) { fail(e); return; }
  if (seq !== routeSeq) return;
  if (view === "fans") m.renderFans();
  else if (view === "devices") { if (arg) m.showZone(decodeURIComponent(arg)); m.renderDevices(); }
  else if (view === "settings") m.loadSettings();
  else if (view === "lighting") m.renderLighting();
  else if (view === "studio") await m.renderStudio(arg);
}
window.addEventListener("hashchange", () => { route().catch(fail); });

window.addEventListener("resize", () => moveMarker($(".tab-ink"), $(".tabs a[aria-current]"), true));

// Ctrl+1..7 switch tabs
document.addEventListener("keydown", (e) => {
  if (e.ctrlKey && !e.shiftKey && !e.altKey && e.key >= "1" && e.key <= "7") {
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

// The one line that says why the hardware is not doing what the user last set.
function renderWhy() {
  const s = S.state;
  const el = $("#why");
  if (!s) { el.hidden = true; return; }
  const o = s.override || {};
  const held = heldOff();
  let text = "", action = "";
  if (held) {
    text = held.text;
    action = held.rule
      ? `<button class="btn" data-why-pause>Pause all for an hour</button>`
      : `<a class="btn quiet" href="#settings">Settings</a>`;
  } else if (s.fans?.failsafe) {
    text = "Fans are at the failsafe speed because no temperature readings are coming through.";
    action = `<a class="btn quiet" href="#settings">Check sensors</a>`;
  } else if (o.paused_until) {
    const until = new Date(o.paused_until * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    text = `Automations are paused until ${until}.`;
    action = `<button class="btn" data-why-resume>Resume</button>`;
  } else if (o.fans) {
    const rule = S.rules.find((r) => r.id === o.fans);
    text = `Fans are following ${curveLabel(s.fans?.curve)} for now${rule ? `: “${sentence(rule, false)}”` : "."} Your own curve returns when it stops.`;
    action = `<button class="btn" data-why-pause>Pause all for an hour</button>`;
  } else if (o.lighting) {
    const rule = S.rules.find((r) => r.id === o.lighting);
    text = `An automation is controlling the lights${rule ? `: “${sentence(rule, false)}”` : "."} Your own setting returns when it stops.`;
    action = `<button class="btn" data-why-pause>Pause all for an hour</button>`;
  }
  if (!text) {
    el.hidden = true;
    setHTML(el, "");
    return;
  }
  const hadFocus = el.contains(document.activeElement);
  el.hidden = false;
  setHTML(el, `<span class="why-chip"><i class="why-dot" aria-hidden="true"></i>Why</span><span class="why-text">${esc(text)}</span>${action}`);
  if (hadFocus) el.querySelector("button, a")?.focus({ preventScroll: true });
}

// a Studio effect on your lights that Airglow had to turn off (it hung, crashed, or used too much)
function fxProblem() {
  const s = S.state;
  const used = new Set([s.effective?.effect, s.scene?.effect].filter(Boolean));
  return (s.studio?.problems || []).find((p) => used.has(p.effect));
}

function renderBar() {
  const u = S.state.update;
  renderPins();
  renderTrial();
  renderWhy();
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
  } else if (S.state.openrgb?.enabled && !S.state.openrgb.connected
             && (S.state.openrgb.installed || S.state.openrgb.used)) {
    const o = S.state.openrgb;
    n.hidden = false;
    setHTML(n, `<span><b>OpenRGB</b> isn't running, so the graphics card and other lights it drives stay dark.</span>
      ${o.installed ? `<button class="btn" data-openrgb-start>Start OpenRGB</button>` : `<a class="btn" href="#settings">Set it up</a>`}
      <a class="btn quiet" href="#settings">Settings</a><span class="spacer"></span>`);
  } else if (S.state.onboarded && S.state.setup_pending > setupDismissed) {
    const k = S.state.setup_pending;
    n.hidden = false;
    setHTML(n, `<span><b>${k} new light${k === 1 ? "" : "s"}</b> to set up. Tell Airglow what ${k === 1 ? "it is" : "they are"} so effects line up with your case.</span>
      <button class="btn" data-open-setup="start">Set up</button><button class="btn quiet" id="setup-later">Not now</button><span class="spacer"></span>`);
  } else if (fxProblem()) {
    const p = fxProblem();
    n.hidden = false;
    setHTML(n, `<span>${esc(p.text)}${p.message ? ` (${esc(p.message)})` : ""}</span>
      <button class="btn" data-fx-retry="${esc(p.effect)}">Try again</button><a class="btn quiet" href="#lighting">Pick another effect</a><span class="spacer"></span>`);
  } else if (u?.unsupported) {
    n.hidden = false;
    setHTML(n, `<span>This version of Airglow is no longer supported, so community features are off. Lighting, fans and devices keep working.</span>
      ${u.kind === "velopack" ? `<a class="btn" href="#settings">Update</a>` : ""}<span class="spacer"></span>`);
  } else if (u?.notice) {
    n.hidden = false;
    setHTML(n, `<span>${esc(u.notice)}</span><span class="spacer"></span>`);
  } else if (u?.state === "ready" && updateDismissed !== u.latest) {
    n.hidden = false;
    setHTML(n, `<span><b>Airglow ${esc(u.latest)}</b> is downloaded. It's installed the next time Airglow starts.</span>
      <button class="btn" id="update-restart">Restart now</button><button class="btn quiet" id="update-later">Later</button><span class="spacer"></span>`);
  } else {
    n.hidden = true;
  }
}

document.addEventListener("click", (e) => {
  if (e.target.closest("[data-why-pause]")) { pauseAutomations(60); return; }
  if (e.target.closest("[data-why-resume]")) { pauseAutomations(0); return; }
  const b = e.target.closest("[data-open-setup]");
  if (b) import("./setup.js").then((m) => m.openSetup({ start: b.dataset.openSetup === "start", only: b.dataset.zone || null })).catch(fail);
  if (e.target.closest("#setup-later")) { setupDismissed = S.state.setup_pending; renderBar(); }
  if (e.target.closest("#update-later")) { updateDismissed = S.state.update?.latest; renderBar(); }
  if (e.target.closest("#update-restart")) import("./settings.js").then((m) => m.restartToUpdate()).catch(fail);
  if (e.target.closest("[data-openrgb-start]")) {
    post("/api/openrgb/start").then(() => toast("Starting OpenRGB")).catch(fail);
  }
  const fx = e.target.closest("[data-fx-retry]");
  if (fx) post("/api/studio/retry", { fx_id: fx.dataset.fxRetry }).then(() => emit("refresh")).catch(fail);
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
  lastStateAt = Date.now();
  if (!connected) { connected = true; toast("Reconnected"); }
  const fs = !!S.state.fans?.failsafe;
  if (fs && !failsafeSeen) toast("No temperature readings, so the fans are at the failsafe speed. Check LibreHardwareMonitor.", true);
  failsafeSeen = fs;
  pushSample();
  pushReadings(S.state.readings);
  // device list or connection changed: reload zones so the stage and Devices stay accurate
  const key = JSON.stringify([S.state.zones_rev, S.state.devices.map((d) => [d.id, d.online, d.zones.length])]);
  if (key !== deviceKey) {
    S.zones = await get("/api/zones");
    deviceKey = key;                 // only once they arrived: a failed fetch is tried again with the next state
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
    if (!bootFailed) toast(`Couldn't reach Airglow: ${e.message}. Retrying.`, true);
    bootFailed = true;
    setTimeout(boot, 3000);
    return;
  }
  await initOverview(S.overview);
  initStage();
  initAutomations();
  renderBar();
  await route().catch(fail);
  if (!S.state.onboarded) openWelcome();
  else if (!S.state.online_asked && S.state.update && S.state.update.kind !== "scoop") openOnlineQuestion();
  startLive({
    state: (s) => applyState(s).then(syncLive).catch(lost),     // a lighting tile may have come or gone
    lost,
    // views showing the live LED preview
    frames: () => !$('[data-view="lighting"]').hidden || !$('[data-view="devices"]').hidden
      || (!$('[data-view="overview"]').hidden && !!document.querySelector(".tile-lighting")),
  });
  setInterval(() => loadCatalog().catch(() => {}), 60000);     // new hardware, LHM started later
  // The stream normally reports errors, but a silent one (a server that stops sending without closing) would leave
  // old readings on screen with no hint. No update for 12 s while the window is showing means it died.
  setInterval(() => {
    if (document.visibilityState === "hidden" || !connected || probing) return;
    if (Date.now() - lastStateAt < 12000) return;
    lost();
    probing = true;
    refresh().finally(() => { probing = false; });
  }, 4000);
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") {
      lastStateAt = Date.now();   // the stream reopens: give it a moment
      refresh();                  // don't show the readings from before the tray for a whole state tick
    }
  });
}

boot();
initShortcuts();
