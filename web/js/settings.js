// Settings screen.

import { $, $$, S, emit, esc, fail, get, on, post, rangeFill, setHTML, toast } from "./core.js";
import { paintTools, refreshTools } from "./helpers.js";
import { openWelcome } from "./onboarding.js";
import { openShortcuts } from "./help.js";

const SECTIONS = [["general", "General"], ["lighting", "Lighting"], ["fans", "Fans"], ["helpers", "Helper programs"],
  ["sensors", "Sensors"], ["devices", "More devices"], ["about", "About"]];

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

export async function loadSettings() {
  try { S.settings = await get("/api/settings"); renderSettings(); } catch (e) { fail(e); }
  refreshTools();
}

export function renderSettings() {
  const v = S.settings;
  if (!v) return;
  const s = v.settings;
  const sensorsOk = S.state?.sensors?.online;
  setHTML($("#settings"), `
    <section class="set-group" id="set-general"><h2>General</h2>
      ${row("Start with Windows", "Opens quietly in the tray when you sign in, so lighting and fan curves apply right away.", sw("s-startup", v.start_with_windows, "Start with Windows"))}
      ${row("Temperature unit", "", `<div class="seg" id="s-unit" role="radiogroup" aria-label="Temperature unit"><button role="radio" data-v="C" aria-checked="${s.temp_unit === "C"}">°C</button><button role="radio" data-v="F" aria-checked="${s.temp_unit === "F"}">°F</button></div>`)}
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

async function save(patch, label = "Saved") {
  // Rebuilding the page is what the server's answer is; keep the scroll position and the focused control where they
  // were instead of dropping the user at the top after every change.
  const view = $('[data-view="settings"]');
  const scrollTop = view?.scrollTop || 0;
  const focused = document.activeElement?.id;
  try {
    S.settings = await post("/api/settings", patch);
    renderSettings();
    if (focused) document.getElementById(focused)?.focus({ preventScroll: true });
    if (view) {
      view.scrollTop = scrollTop;
      requestAnimationFrame(() => { if (view) view.scrollTop = scrollTop; paintNav(); });
    }
    paintNav();
    noteSaved(label);
    emit("refresh");
  } catch (e) { fail(e); renderSettings(); }
}

export function initSettings() {
  const box = $("#settings");
  box.addEventListener("change", (e) => {
    const t = e.target;
    if (t.id === "s-startup") save({ start_with_windows: t.checked }, t.checked ? "Airglow will start with Windows" : "Airglow won't start with Windows");
    else if (t.id === "s-sleep") save({ lights_off_on_sleep: t.checked });
    else if (t.id === "s-lock") save({ lights_off_on_lock: t.checked });
    else if (t.id === "s-source") save({ temp_source: t.value });
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
    if (e.target.closest("#s-update")) {
      post("/api/updates/check").then((u) => { S.state.update = u; paintUpdate(); emit("refresh"); }).catch(fail);
    }
    if (e.target.closest("#s-restart")) restartToUpdate();
    if (e.target.closest("#s-orgb-start")) {
      post("/api/openrgb/start").then(() => toast("Starting OpenRGB")).catch(fail);
    }
    if (e.target.closest("#s-reset")) {
      if (!confirm("Put the settings back to how they start on a new PC?\n\nZones, presets, fan curves and overview tiles are kept.")) return;
      post("/api/settings/reset", {}).then((v) => { S.settings = v; renderSettings(); noteSaved("Settings reset"); emit("refresh"); }).catch(fail);
    }
  });
  on("state", paintUpdate);
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
