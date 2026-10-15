// Settings screen.

import { $, $$, S, emit, esc, fail, get, on, post, rangeFill, setHTML, toast } from "./core.js";
import { paintTools, refreshTools } from "./helpers.js";
import { openWelcome } from "./onboarding.js";

const row = (label, help, ctl) => `<div class="set-row"><div><div class="label">${label}</div>${help ? `<p>${help}</p>` : ""}</div><div class="ctl">${ctl}</div></div>`;
const sw = (id, on) => `<label class="switch"><input type="checkbox" id="${id}"${on ? " checked" : ""}><span></span></label>`;

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
    <section class="set-group"><h2>General</h2>
      ${row("Start with Windows", "Opens quietly in the tray when you sign in, so lighting and fan curves apply right away.", sw("s-startup", v.start_with_windows))}
      ${row("Temperature unit", "", `<div class="seg" id="s-unit" role="radiogroup"><button role="radio" data-v="C" aria-checked="${s.temp_unit === "C"}">°C</button><button role="radio" data-v="F" aria-checked="${s.temp_unit === "F"}">°F</button></div>`)}
    </section>
    <section class="set-group"><h2>Lighting</h2>
      ${row("Lights off when the PC sleeps or shuts down", "Many boards keep their RGB on in sleep. This turns everything dark first.", sw("s-sleep", s.lights_off_on_sleep))}
      ${row("Lights off while the PC is locked", "", sw("s-lock", s.lights_off_on_lock))}
    </section>
    <section class="set-group"><h2>Fans</h2>
      ${row("Follow the temperature of", "The fan curve reacts to this reading.", `<select class="select" id="s-source" style="width:220px">
        <option value="max"${s.temp_source === "max" ? " selected" : ""}>Whichever is hotter</option>
        <option value="cpu"${s.temp_source === "cpu" ? " selected" : ""}>Processor</option>
        <option value="gpu"${s.temp_source === "gpu" ? " selected" : ""}>Graphics card</option></select>`)}
      ${row("Failsafe speed", "Used when temperatures can't be read, and left on the fans when Airglow quits.", `<input type="range" id="s-failsafe" min="40" max="100" step="5" value="${s.fan_failsafe_duty}"><output id="s-failsafe-v">${s.fan_failsafe_duty}%</output>`)}
      ${row("Ramp speed", "How much fan speed may change every 2 seconds. Lower is calmer.", `<input type="range" id="s-step" min="1" max="25" value="${s.fan_max_step}"><output id="s-step-v">${s.fan_max_step}%</output>`)}
      ${row("Motherboard fans", v.fancontrol_installed
        ? (v.fan_profiles.length ? `FanControl has ${v.fan_profiles.length} saved profile${v.fan_profiles.length > 1 ? "s" : ""}. Presets can switch between them.` : "FanControl is installed but has no saved profiles yet. Save some in FanControl to use them in presets.")
        : "These follow your BIOS curve. Install FanControl if you want presets to switch them too.", "")}
    </section>
    <section class="set-group"><h2>Helper programs</h2>
      <p class="hint">Temperatures come from LibreHardwareMonitor, and most RGB devices other than NZXT and ASUS from OpenRGB.</p>
      <div class="tools-box" data-tools="full"></div>
    </section>
    <section class="set-group"><h2>Sensors</h2>
      ${row("LibreHardwareMonitor address", sensorsOk ? "Connected. Temperatures and power are coming through." : "Not responding. Open LibreHardwareMonitor and turn on Options, Remote Web Server, Run.", `<input class="text" id="s-lhm" value="${esc(s.lhm_url)}" spellcheck="false">`)}
    </section>
    <section class="set-group"><h2>More devices</h2>
      ${row("Use OpenRGB for other devices", esc(S.state?.openrgb?.status || ""), sw("s-orgb", s.openrgb_enabled))}
      ${row("OpenRGB address", "Only change this if OpenRGB runs on another port or another PC.", `<input class="text" id="s-orgb-host" value="${esc(s.openrgb_host)}" style="width:150px" spellcheck="false"><input class="text" id="s-orgb-port" type="number" value="${s.openrgb_port}" style="width:90px">`)}
      ${row("Windows Dynamic Lighting devices <span class=\"tag\">Experimental</span>", `Keyboards, mice and accessories that Windows controls in Settings, Personalization, Dynamic Lighting. Windows only lets Airglow light them while its window is in front. ${s.dynamic_lighting_enabled ? esc(S.state?.dynamic_lighting?.status || "") : ""}`, sw("s-dyn", s.dynamic_lighting_enabled))}
    </section>
    <section class="set-group"><h2>About</h2>
      ${row(`Airglow ${esc(v.version)}`, `Free and open source under the MIT licence. Settings and logs are stored in <span class="path">${esc(v.data_dir)}</span>`, `<button class="btn" id="s-logs">Open log folder</button>`)}
      ${row("Diagnostics report", "For a bug report: what Airglow found, its settings, a sensor list and the logs, in one zip. Your user name, PC name and device serial numbers are taken out. Nothing is sent; you attach it yourself.", `<button class="btn" id="s-diag">Save a report</button>`)}
      ${v.updates_available ? row("Updates", "Checks GitHub for a newer release. Airglow never goes online unless you press this.", `<button class="btn" id="s-update">Check for updates</button>`) : ""}
      ${row("Welcome guide", "Shows what Airglow found on this PC and what to check.", `<button class="btn quiet" id="s-welcome">Show it again</button>`)}
    </section>`);
  $$("#settings input[type=range]").forEach(rangeFill);
  paintTools();
}

async function save(patch, label = "Saved") {
  try { S.settings = await post("/api/settings", patch); renderSettings(); toast(label); emit("refresh"); }
  catch (e) { fail(e); renderSettings(); }
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
  });
  box.addEventListener("input", (e) => {
    const t = e.target;
    if (t.type === "range") { rangeFill(t); $(`#${t.id}-v`).textContent = `${t.value}%`; }
  });
  box.addEventListener("keydown", (e) => { if (e.key === "Enter" && e.target.classList.contains("text")) e.target.blur(); });
  box.addEventListener("click", (e) => {
    const u = e.target.closest("#s-unit [data-v]");
    if (u) save({ temp_unit: u.dataset.v });
    if (e.target.closest("#s-logs")) post("/api/open/logs").catch(fail);
    if (e.target.closest("#s-diag")) post("/api/diagnostics", {}).then((r) => toast(`Saved ${r.name}. Explorer shows where.`)).catch(fail);
    if (e.target.closest("#s-welcome")) openWelcome();
    if (e.target.closest("#s-update")) {
      post("/api/updates/check").then((r) => toast(r.newer ? `Airglow ${r.latest} is available. Download it from the GitHub releases page.` : `You have the latest version (${r.current}).`)).catch(fail);
    }
  });
}
