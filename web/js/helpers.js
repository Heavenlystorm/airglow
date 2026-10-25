// Helper programs (LibreHardwareMonitor, OpenRGB): status, one-button setup and removal. Every element with
// data-tools on the page shows the same state; while a setup runs, /api/tools is polled and they all repaint.

import { $$, emit, esc, fail, get, post, setHTML, toast } from "./core.js";

let st = null, timer = 0, lastState = null;

const mb = (bytes) => Math.round(bytes / 1048576);
const busy = () => ["working", "waiting"].includes(st?.job.state);

function toolRow(t) {
  const name = `${esc(t.name)} ${esc(t.version)}`;
  if (t.working) return row(true, esc(t.name), `Working, for ${esc(t.purpose)}.`);
  if (t.other_copy) {
    const how = t.id === "lhm" ? "turn on Options, Remote Web Server, Run in it" : "turn on its SDK server (Settings, SDK Server, Start Server)";
    return row(false, `${esc(t.name)} is running, but Airglow can't reach it`, `Either ${how}, or close it so Airglow can set up its own copy.`);
  }
  return row(null, `${name} isn't set up`, `Needed for ${esc(t.purpose)}.`);
}

function row(ok, title, text) {
  return `<div class="onb-item"><i class="dot ${ok === null ? "" : ok ? "on" : "off"}"></i><div><strong>${title}</strong><span>${text}</span></div></div>`;
}

function jobLine(j) {
  if (j.state === "working" || j.state === "waiting") {
    const pct = j.state === "working" && j.progress > 0 && j.progress < 1 ? `<i style="width:${(j.progress * 100).toFixed(0)}%"></i>` : "";
    return `<div class="tools-job" role="status"><span>${esc(j.step)}…</span>${pct ? `<b class="tools-bar">${pct}</b>` : ""}</div>`;
  }
  if (j.state === "failed") return `<div class="tools-job error" role="alert">${j.kind === "remove" ? "Removing" : "Setting up"} didn't finish: ${esc(j.error)}</div>`;
  if (j.state === "cancelled") return `<div class="tools-job" role="status">Cancelled at the Windows prompt. Nothing was changed.</div>`;
  if (j.state === "done" && j.kind === "setup") return `<div class="tools-job ok" role="status">Done. The programs start with Windows from now on, and readings appear within a few seconds.</div>`;
  if (j.state === "done" && j.kind === "remove") return `<div class="tools-job ok" role="status">Removed.</div>`;
  return "";
}

function markup(full) {
  if (!st) return "";
  const missing = st.tools.filter((t) => !t.working && !t.other_copy);
  const offer = missing.length && !busy()
    ? `<p class="hint">Airglow can download ${missing.map((t) => `${esc(t.name)} ${esc(t.version)}`).join(" and ")} (about ${mb(missing.reduce((n, t) => n + t.size, 0))} MB) from ${missing.length > 1 ? "their official sources" : "its official source"}, check ${missing.length > 1 ? "each file" : "it"} against a known checksum, and install ${missing.length > 1 ? "them" : "it"} so ${missing.length > 1 ? "they start" : "it starts"} with Windows. Windows asks for permission once.</p>`
    : "";
  const buttons = [
    missing.length && !busy() ? `<button class="btn${full ? " primary" : ""}" data-tools-setup>${full ? "Set up automatically" : "Install OpenRGB &amp; sensors"}</button>` : "",
    full && st.installed_any && !busy() ? `<button class="btn quiet" data-tools-remove>Remove them</button>` : "",
  ].join("");
  if (!full && !missing.length && !["working", "waiting", "failed", "done"].includes(st.job.state)) return "";
  return `${full ? `<div class="onb-list">${st.tools.map(toolRow).join("")}</div>` : ""}
    ${offer}${jobLine(st.job)}${buttons ? `<div class="tools-actions">${buttons}</div>` : ""}`;
}

export function paintTools() {
  for (const el of $$("[data-tools]")) setHTML(el, markup(el.dataset.tools === "full"));
}

export async function refreshTools() {
  clearTimeout(timer);
  try { st = await get("/api/tools"); } catch { timer = setTimeout(refreshTools, 2000); return; }   // keep watching
  paintTools();
  const s = st.job.state;
  if (lastState && lastState !== s && s === "done") {
    toast(st.job.kind === "setup" ? "Helper programs set up" : "Helper programs removed");
    emit("refresh");
  }
  lastState = s;
  if (busy()) timer = setTimeout(refreshTools, 800);
}

document.addEventListener("click", (e) => {
  if (e.target.closest("[data-tools-setup]")) {
    post("/api/tools/setup", {}).then((r) => { st = r; paintTools(); refreshTools(); }).catch(fail);
  } else if (e.target.closest("[data-tools-remove]")) {
    if (!confirm("Remove LibreHardwareMonitor and OpenRGB that Airglow set up? Temperatures and OpenRGB devices stop working until you set them up again.")) return;
    post("/api/tools/remove", {}).then((r) => { st = r; paintTools(); refreshTools(); }).catch(fail);
  }
});
