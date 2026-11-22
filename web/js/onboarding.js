// First-run welcome: what was found, what needs attention, and a single way forward.

import { $, S, activeFanFailsafe, emit, esc, fail, on, post, setHTML, toast } from "./core.js";
import { refreshTools } from "./helpers.js";
import { closeSheet, openSheet } from "./sheet.js";

function item(ok, title, text, action = "") {
  return `<div class="onb-item"><i class="dot ${ok === null ? "" : ok ? "on" : "off"}"></i><div style="flex:1"><strong>${title}</strong><span>${text}</span></div>${action}</div>`;
}

function welcomeList(st) {
  const devices = st.devices || [];
  const online = devices.filter(d => d.online);
  const sensors = st.sensors || {};
  const attention = [], found = [];
  for (const c of st.conflicts || []) {
    attention.push(item(false, `${esc(c.name)} is running`, `It also controls ${esc(c.conflicts_with)} and may cause flicker.`,
      `<button class="btn" data-close-app="${esc(c.name)}">Close it</button>`));
  }
  if (!online.length) attention.push(item(false, "No lighting devices found", "Connect a supported USB controller or set up OpenRGB for other devices."));
  for (const d of devices) {
    if (!d.online) {
      attention.push(item(false, esc(d.name), esc(d.error || "Not connected")));
      continue;
    }
    const n = d.zones?.length || 0;
    found.push(item(true, esc(d.name), `${esc(d.type)} · ${n} zone${n === 1 ? "" : "s"}${d.kind === "openrgb" ? " · OpenRGB" : ""}`));
  }
  const unavailable = [];
  for (const [key, label] of [["cpu", "Processor"], ["gpu", "Graphics card"]]) {
    if (sensors.online && Number.isFinite(sensors[key + "_temp"])) {
      found.push(item(true, `${label} temperature`, esc(sensors[key + "_name"] || "Available through LibreHardwareMonitor")));
    } else unavailable.push(label);
  }
  if (unavailable.length) {
    const title = unavailable.length === 2 ? "Processor and graphics card temperatures unavailable" : unavailable[0] + " temperature unavailable";
    const reason = sensors.online ? "LibreHardwareMonitor is connected. The reading may appear as hardware is discovered."
      : "Airglow gets these readings from LibreHardwareMonitor. Check the helper setup below.";
    attention.push(item(false, title, reason));
  }
  if (activeFanFailsafe(st.fans)) attention.unshift(item(false, "Fan failsafe is active", "Airglow is waiting for its selected temperature reading."));
  const readings = 2 - unavailable.length;
  const summary = [online.length && `${online.length} lighting device${online.length === 1 ? "" : "s"}`,
    readings && `${readings} temperature reading${readings === 1 ? "" : "s"}`].filter(Boolean).join(" · ");
  const discovery = found.length ? `<details class="onb-found"><summary>${summary}</summary>${found.join("")}</details>` : "";
  return attention.join("") + discovery;
}

const welcomeHint = (pending) => pending
  ? "During setup, each light blinks so you can match it to its place in your PC."
  : "Choose Customize on the Overview to move tiles or add a reading.";

function renderWelcomeStatus() {
  const status = $("#onb-status");
  if (!status || !S.state) return;
  const pending = S.state.setup_pending;
  const focused = document.activeElement;
  const details = status.querySelector(".onb-found");
  const expanded = details?.open;
  const summaryFocused = details?.querySelector("summary") === focused;
  const controls = new Map([...status.querySelectorAll("[data-close-app]")].map((b) => [b.dataset.closeApp, b]));
  setHTML(status, welcomeList(S.state));
  const next = status.querySelector(".onb-found");
  if (next && details && next !== details) {
    next.open = expanded;
    if (summaryFocused) next.querySelector("summary").focus({ preventScroll: true });
  }
  for (const button of status.querySelectorAll("[data-close-app]")) {
    const retained = controls.get(button.dataset.closeApp);
    if (retained && retained !== button) button.replaceWith(retained);
    if (retained === focused) retained.focus();
  }
  $("#onb-step").textContent = pending ? "Step 1 of 2" : "";
  $("#onb-hint").textContent = welcomeHint(pending);
  $("#onb-skip").textContent = pending ? "Later" : "Check devices";
  $("#onb-start").textContent = pending ? "Set up lights" : "Open Overview";
}
on("state", renderWelcomeStatus);

let clearWelcomeListeners = () => {};
export function openWelcome() {
  clearWelcomeListeners();
  const st = S.state;
  openSheet(`<div class="body welcome-sheet"><div class="sheet-head"><div class="welcome-heading"><h2>Welcome to Airglow</h2>
      <p>Lighting and cooling for this PC. No account needed.</p></div>
      <div class="sheet-head-side"><span class="sheet-step" id="onb-step">${st.setup_pending ? "Step 1 of 2" : ""}</span>
      <button class="icon-btn" id="onb-close" aria-label="Close"><svg><use href="#i-close"/></svg></button></div></div>
    <div class="onb-list" id="onb-status">${welcomeList(st)}</div>
    <div class="tools-box" data-tools></div>
    ${onlineItem()}
    <p class="hint" id="onb-hint">${welcomeHint(st.setup_pending)}</p>
    <div class="actions"><button class="btn quiet" id="onb-skip">${st.setup_pending ? "Later" : "Check devices"}</button><span class="spacer"></span>
      <button class="btn primary" id="onb-start">${st.setup_pending ? "Set up lights" : "Open Overview"}</button></div></div>`);
  refreshTools();
  const sheet = $("#sheet");
  const root = $("#onb-status");
  let finished = false, saving = false;
  const done = async () => {
    if (finished) return true;
    if (saving) return false;
    saving = true;
    const controls = [$("#onb-close"), $("#onb-start"), $("#onb-skip"), $("#onb-online")].filter(Boolean);
    controls.forEach((b) => { b.disabled = true; });
    try {
      const online = $("#onb-online");
      if (online) S.settings = await post("/api/settings", { online_checks: online.checked });
      await post("/api/onboarded");
      finished = true;
      clear();
      emit("refresh");
      return $("#onb-status") === root;
    } catch (err) { fail(err); return false; }
    finally { saving = false; controls.forEach((b) => { b.disabled = false; }); }
  };
  const dismiss = async () => { if (await done()) closeSheet(); };
  const cancel = (e) => {
    if ($("#onb-status") !== root) { clear(); return; }
    e.preventDefault();
    dismiss();
  };
  const clear = () => {
    sheet.removeEventListener("cancel", cancel);
    sheet.removeEventListener("close", clear);
  };
  clearWelcomeListeners = clear;
  sheet.addEventListener("cancel", cancel);
  sheet.addEventListener("close", clear, { once: true });
  $("#onb-close").addEventListener("click", dismiss);
  $("#onb-start").addEventListener("click", async () => {
    if (!await done()) return;
    if (S.state.setup_pending) import("./setup.js").then((m) => m.openSetup({ start: true })).catch(fail);
    else { closeSheet(); location.hash = "#overview"; }
  });
  $("#onb-skip").addEventListener("click", async () => {
    if (!await done()) return;
    closeSheet();
    if (!S.state.setup_pending) location.hash = "#devices";
  });
}

// Going online is opt-in: asked once, on the welcome sheet, or on its own for people who already went through it.
const ONLINE_TEXT = "Looks for updates a minute after Airglow starts and every 6 hours. Off means Airglow only goes online when you press a button. You can change this in Settings.";

function onlineItem() {
  const u = S.state.update;
  if (!u || u.kind === "scoop" || S.state.online_asked) return "";
  return `<label class="onb-item"><div style="flex:1"><strong>Keep Airglow up to date automatically</strong><span>${ONLINE_TEXT}</span></div>
    <span class="switch"><input type="checkbox" id="onb-online"${S.settings?.settings?.online_checks ? " checked" : ""}><span></span></span></label>`;
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
