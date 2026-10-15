// First-run welcome: what was found, what needs attention, and a single way forward.

import { $, S, emit, esc, fail, post, toast } from "./core.js";
import { refreshTools } from "./helpers.js";
import { closeSheet, openSheet } from "./sheet.js";

function item(ok, title, text, action = "") {
  return `<div class="onb-item"><i class="dot ${ok === null ? "" : ok ? "on" : "off"}"></i><div style="flex:1"><strong>${title}</strong><span>${text}</span></div>${action}</div>`;
}

export function openWelcome() {
  const st = S.state;
  const devs = st.devices || [];
  const lit = devs.filter((d) => d.online);
  const orgb = devs.filter((d) => d.kind === "openrgb");
  let list = "";
  for (const d of lit) {
    const n = d.zones.length;
    list += item(true, esc(d.name), `${esc(d.type)}, ${n} zone${n === 1 ? "" : "s"}${d.kind === "openrgb" ? ", through OpenRGB" : ""}`);
  }
  if (!lit.length) list += item(false, "No lighting devices found", "Airglow works with NZXT hubs and ASUS boards directly, and with hundreds more through OpenRGB.");
  if (!orgb.length) list += item(null, "More devices through OpenRGB", "RAM, graphics cards, keyboards and strips work too, through OpenRGB. Airglow can set it up for you, below.");
  list += st.sensors.online
    ? item(true, "Temperatures", `Reading ${esc(st.sensors.cpu_name || "the processor")} and ${esc(st.sensors.gpu_name || "graphics card")} through LibreHardwareMonitor.`)
    : item(false, "Temperatures aren't coming through", "They come from LibreHardwareMonitor, which Airglow can set up for you, below. Fans hold a safe speed until then.");
  for (const c of st.conflicts || []) {
    list += item(false, `${esc(c.name)} is running`, `It controls ${esc(c.conflicts_with)} too, which makes lights flicker.`,
      `<button class="btn" data-close-app="${esc(c.name)}">Close it</button>`);
  }
  openSheet(`<div class="body"><h2>Welcome to Airglow</h2>
    <p class="lede" style="margin:0">Here's what it found on this PC. Everything stays on this computer: no account, no cloud.</p>
    <div class="onb-list">${list}</div>
    <div class="tools-box" data-tools></div>
    <p class="hint">The Overview is yours to arrange: Customize moves and resizes tiles, and any tile can show a different reading, from memory use to drive temperature. Fans with RGB rings work best when their LED counts are right; NZXT accessories are detected automatically, check the rest in Devices.</p>
    <div class="actions"><a class="btn quiet" href="#devices" id="onb-devices">Check devices</a><span class="spacer"></span><button class="btn primary" id="onb-start">Start</button></div></div>`);
  refreshTools();
  const done = () => post("/api/onboarded").catch(() => {});
  $("#onb-start").addEventListener("click", () => { done(); closeSheet(); });
  $("#onb-devices").addEventListener("click", () => { done(); closeSheet(); });
}

export async function closeConflict(button) {
  button.disabled = true;
  try {
    const r = await post("/api/conflicts/close", { name: button.dataset.closeApp });
    if (r.failed.length) toast(`Couldn't close ${r.failed.join(", ")}. It may need to be closed from its own tray icon.`, true);
    else { toast(`${button.dataset.closeApp} closed`); button.closest(".onb-item")?.remove(); }
    emit("refresh");
  } catch (err) { fail(err); button.disabled = false; }
}

// one delegated listener for every "Close it" button (welcome sheet and the notice bar)
document.addEventListener("click", (e) => {
  const b = e.target.closest("[data-close-app]");
  if (b) closeConflict(b);
});
