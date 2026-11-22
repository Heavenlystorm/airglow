import { get, post } from "./core.js";
import { openSheet } from "./sheet.js";

const endpoint = "/api/windows-shortcut";
const target = action => action === "desk" ? "Desk Display" : "Airglow";
const chord = key => `Ctrl + Shift + ${key}`;
export function shortcutSummary(saved, live) {
  const choice = live?.saved || saved;
  if (!["F7", "F8", "F9"].includes(choice?.key)) return "Open Airglow or Desk Display while another app is in front. Off by default.";
  return live?.registered ? `${chord(choice.key)} opens ${target(choice.action)}. Registered with Windows.`
    : `${chord(choice.key)} is saved for ${target(choice.action)}. ${live?.error || "No active Windows registration."}`;
}

export function openWindowsShortcut(onSave) {
  const d = openSheet(`<div class="body" data-windows-shortcut><h2>Windows shortcut</h2>
    <p class="hint">Open a window while Airglow is running, including from the system tray.</p>
    <form class="field"><label for="ws-key">Key combination</label><select class="select" id="ws-key" disabled><option value="off">Off</option></select>
      <label for="ws-target">Window to open</label><select class="select" id="ws-target" disabled><option value="main">Airglow</option><option value="desk">Desk Display</option></select>
      <p class="hint" role="status" data-ws-status>Checking Windows registration…</p><p role="alert" data-ws-error hidden></p>
      <div class="btn-row"><button class="btn primary" type="submit" disabled>Save shortcut</button><button class="btn quiet" type="button" data-ws-refresh>Refresh status</button><button class="btn quiet" type="button" data-close>Close</button></div></form></div>`);
  const root = d.querySelector("[data-windows-shortcut]"), form = root.querySelector("form"), key = root.querySelector("#ws-key");
  const action = root.querySelector("#ws-target"), save = root.querySelector("[type=submit]"), status = root.querySelector("[role=status]");
  const error = root.querySelector("[role=alert]"), retry = root.querySelector("[data-ws-refresh]");
  const current = () => d.open && root.isConnected;
  let state, busy = false;
  function report(message) { error.textContent = message; error.hidden = !message; }
  function controls() {
    key.disabled = busy || !state?.available;
    action.disabled = key.disabled || key.value === "off";
    const changed = state && (key.value !== state.saved.key || (key.value !== "off" && action.value !== state.saved.action));
    const unregistered = state && !changed && key.value !== "off" && !state.registered;
    save.textContent = unregistered ? "Try registration again" : "Save shortcut";
    save.disabled = key.disabled || (!changed && !unregistered);
    retry.disabled = busy;
  }
  function accept(next) {
    state = next;
    const options = [...(next.keys || [])];
    if (next.saved.key !== "off" && !options.includes(next.saved.key)) options.push(next.saved.key);
    key.replaceChildren(new Option("Off", "off"), ...options.map(k => new Option(chord(k), k)));
    key.value = next.saved.key; action.value = next.saved.action;
    status.textContent = next.registered ? `Registered with Windows. ${chord(next.saved.key)} opens ${target(next.saved.action)}.`
      : next.error || (next.available ? "Off. No Windows shortcut is registered." : "Windows shortcuts are unavailable right now.");
  }
  async function refresh() {
    if (busy) return;
    busy = true; report(""); controls();
    try { const next = await get(endpoint); if (current()) accept(next); }
    catch (err) { if (current()) { state = null; status.textContent = "Registration status unavailable."; report(err.message); } }
    finally { busy = false; if (current()) controls(); }
  }
  form.addEventListener("change", controls);
  retry.addEventListener("click", refresh);
  form.addEventListener("submit", async e => {
    e.preventDefault();
    if (busy || save.disabled) return;
    const body = { choice: { key: key.value, action: action.value }, session: state.session, version: state.version };
    busy = true; report(""); controls();
    try {
      const next = await post(endpoint, body);
      if (current()) accept(next);
      await onSave?.();
    } catch (err) {
      if (current()) {
        try { const next = await get(endpoint); if (current()) accept(next); }
        catch { state = null; status.textContent = "Registration status unavailable."; }
        if (current()) report(err.message);
      }
    } finally { busy = false; if (current()) { controls(); (key.disabled ? retry : key).focus(); } }
  });
  refresh();
}
