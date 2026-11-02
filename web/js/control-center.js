// Completed-work evidence, with explicit recheck actions. Opening the sheet is read-only.
import { $, esc, get, post, setHTML } from "./core.js";
import { openSheet, closeSheet } from "./sheet.js";

export function formatAge(seconds) {
  if (seconds == null) return "No completed work yet";
  if (seconds < 1) return "Just now";
  if (seconds < 60) return `${Math.floor(seconds)}s ago`;
  return `${Math.floor(seconds / 60)}m ${Math.floor(seconds % 60)}s ago`;
}

const states = { healthy: "Healthy", pending: "Pending", failed: "Failed", stale: "Stale", disconnected: "Disconnected", unmanaged: "Not controlled" };

export function statusRow(row) {
  if (!row) return "";
  const unmanaged = row.state === "unmanaged";
  const completion = unmanaged && row.age_seconds == null ? "No Airglow command requested" : formatAge(row.age_seconds);
  return `<li class="wb-status-row"><div class="wb-resource"><strong>${esc(row.label)}</strong><span>${esc(row.owner)}</span></div>
    <span class="wb-state" data-state="${esc(row.state)}">${esc(unmanaged && row.kind === "lhm-fan" ? "BIOS / device" : states[row.state] || row.state)}</span>
    <div class="wb-completion"><span>${esc(completion)}</span>${row.duration_ms == null ? "" : `<small>Operation ${esc(row.duration_ms)} ms</small>`}
    ${row.detail ? `<small>${esc(row.detail)}</small>` : ""}${row.action?.startsWith("#fans/") ? `<a href="${esc(row.action)}" data-control-link>Configure</a>` : ""}
    ${row.error ? `<small class="wb-error">${esc(row.error)}</small>` : ""}</div></li>`;
}

export function openControlCenter() {
  const sheet = openSheet(`<div class="body workbench control-workbench">
    <div class="sheet-head"><h2>Control center</h2><button class="icon-btn" data-close aria-label="Close control center"><svg><use href="#i-close"/></svg></button></div>
    <p class="hint">A command can be pending while hardware is still busy. Ages below measure completed work.</p>
    <div class="btn-row"><button class="btn" data-recheck="sensors">Recheck sensors</button><button class="btn" data-recheck="devices">Reconnect devices</button><button class="btn" data-recheck="lighting">Resend lighting</button></div>
    <p class="wb-feedback" role="status" data-control-message>Loading current status…</p>
    <section aria-labelledby="control-loops-title"><h3 id="control-loops-title">Runtime</h3><ul class="wb-status-list" data-control-loops></ul></section>
    <section aria-labelledby="control-hardware-title"><div class="wb-section-head"><h3 id="control-hardware-title">Outputs and ownership</h3><a href="#devices" data-control-link>Device controls</a></div><ul class="wb-status-list" data-control-devices></ul></section>
    <section aria-labelledby="control-events-title"><h3 id="control-events-title">Recent events</h3><ol class="wb-events" data-control-events></ol></section>
    <div class="btn-row"><a class="btn" href="#fans" data-control-link>Fan controls</a><a class="btn" href="#settings" data-control-link>Helper settings</a><button class="btn quiet" data-control-refresh>Refresh status</button></div>
  </div>`);
  const root = $(".control-workbench", sheet);
  const current = () => sheet.open && $(".control-workbench", sheet) === root;
  let reading = false, acting = false, keepMessage = false, timer;
  const message = $("[data-control-message]", root);
  async function refresh(manual = false) {
    if (!current() || reading) return;
    reading = true;
    try {
      const data = await get("/api/control/status");
      if (!current()) return;
      setHTML($("[data-control-loops]", root), [data.controller, data.sensors, data.lighting].map(statusRow).join(""));
      setHTML($("[data-control-devices]", root), data.devices?.length ? data.devices.map(statusRow).join("") : `<li class="wb-empty">No outputs discovered yet. Device controls show available hardware.</li>`);
      setHTML($("[data-control-events]", root), data.events?.length ? [...data.events].reverse().map((event) => `<li><time>${esc(new Date(event.at * 1000).toLocaleTimeString())}</time><span>${esc(event.message)}</span></li>`).join("") : `<li class="wb-empty">No events recorded in this run.</li>`);
      if ((!acting && !keepMessage) || manual) { message.textContent = "Status refreshed. Pending requests are not completed writes."; message.classList.remove("wb-error"); keepMessage = false; }
    } catch (error) {
      if (current()) { message.textContent = `${error.message}. Use Refresh status to try again.`; message.classList.add("wb-error"); }
    } finally {
      reading = false;
      clearTimeout(timer);
      if (current()) timer = setTimeout(() => refresh(), 2000);
    }
  }
  root.addEventListener("click", async (event) => {
    if (event.target.closest("[data-control-link]")) { closeSheet(); return; }
    if (event.target.closest("[data-control-refresh]")) { await refresh(true); return; }
    const button = event.target.closest("[data-recheck]");
    if (!button || acting) return;
    acting = true;
    keepMessage = true;
    button.disabled = true;
    message.textContent = "Requesting a recheck…";
    message.classList.remove("wb-error");
    try {
      await post("/api/control/recheck", { target: button.dataset.recheck });
      if (current()) message.textContent = "Recheck requested. Watch completion ages and events for the result.";
    } catch (error) {
      if (current()) { message.textContent = `${error.message}. Refresh status before requesting another recheck.`; message.classList.add("wb-error"); }
    } finally { acting = false; button.disabled = false; }
  });
  sheet.addEventListener("close", () => clearTimeout(timer), { once: true });
  return refresh();
}
