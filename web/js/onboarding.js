// First-run welcome: what was found, what needs attention, and a single way forward.

import { $, S, emit, esc, fail, post, toast } from "./core.js";
import { refreshTools } from "./helpers.js";
import { closeSheet, openSheet } from "./sheet.js";
import { openSetup } from "./setup.js";

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
    ${onlineItem()}
    <p class="hint">${st.setup_pending ? "Next, tell Airglow what's plugged in where. Each light blinks while you set it up, and it takes about a minute." : "The Overview is yours to arrange: Customize moves and resizes tiles, and any tile can show a different reading."}</p>
    <div class="actions"><button class="btn quiet" id="onb-skip">${st.setup_pending ? "Later" : "Check devices"}</button><span class="spacer"></span>
      <button class="btn primary" id="onb-start">${st.setup_pending ? "Set up lights" : "Start"}</button></div></div>`);
  refreshTools();
  const done = () => {
    if ($("#onb-online")) post("/api/settings", { online_checks: $("#onb-online").checked }).catch(() => {});
    return post("/api/onboarded").then(() => emit("refresh")).catch(() => {});
  };
  $("#onb-start").addEventListener("click", () => { done(); if (st.setup_pending) openSetup({ start: true }); else closeSheet(); });
  $("#onb-skip").addEventListener("click", () => { done(); closeSheet(); if (!st.setup_pending) location.hash = "#devices"; });
}

// Going online is opt-in: asked once, on the welcome sheet, or on its own for people who already went through it.
const ONLINE_TEXT = "Looks for updates a minute after Airglow starts and every 6 hours. Off means Airglow only goes online when you press a button. You can change this in Settings.";

function onlineItem() {
  const u = S.state.update;
  if (!u || u.kind === "scoop" || S.state.online_asked) return "";
  return `<label class="onb-item"><div style="flex:1"><strong>Keep Airglow up to date automatically</strong><span>${ONLINE_TEXT}</span></div>
    <span class="switch"><input type="checkbox" id="onb-online"><span></span></span></label>`;
}

export function openOnlineQuestion() {
  openSheet(`<div class="body"><h2>Keep Airglow up to date?</h2>
    <p class="lede" style="margin:0">Airglow can now check GitHub for new versions on its own and install them when it next starts.</p>
    <p class="hint">${ONLINE_TEXT}</p>
    <div class="actions"><button class="btn quiet" id="online-no">No, I'll check myself</button><span class="spacer"></span>
      <button class="btn primary" id="online-yes">Yes, check automatically</button></div></div>`);
  const answer = (yes) => post("/api/settings", { online_checks: yes })
    .then((s) => { S.settings = s; toast(yes ? "Airglow will look for updates on its own" : "Airglow only goes online when you ask"); emit("refresh"); })
    .catch(fail).finally(closeSheet);
  $("#online-yes").addEventListener("click", () => answer(true));
  $("#online-no").addEventListener("click", () => answer(false));
}

export async function closeConflict(button) {
  button.disabled = true;
  try {
    const r = await post("/api/conflicts/close", { name: button.dataset.closeApp });
    if (r.services?.length) toast(`${button.dataset.closeApp} runs as a Windows service, so Airglow can't close it and it would start again on its own. Stop it in Services (services.msc) or uninstall it.`, true);
    else if (r.failed.length) toast(`Couldn't close ${r.failed.join(", ")}. It may need to be closed from its own tray icon.`, true);
    else { toast(`${button.dataset.closeApp} closed`); button.closest(".onb-item")?.remove(); }
    emit("refresh");
  } catch (err) { fail(err); button.disabled = false; }
}

// one delegated listener for every "Close it" button (welcome sheet and the notice bar)
document.addEventListener("click", (e) => {
  const b = e.target.closest("[data-close-app]");
  if (b) closeConflict(b);
});
