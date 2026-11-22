import { $, S, emit, get, on, post } from "./core.js";
import { openSheet } from "./sheet.js";

const endpoint = "/api/lighting/wind-down";
const clock = seconds => `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
let latest = null, observed = 0, capObserved = 0, panel = null, busy = false;
const retired = new Set();

function adopt(next) {
  if (!next?.session || retired.has(next.session)) return;
  if (latest?.session === next.session && (next.version < latest.version || (next.version === latest.version &&
      (latest.phase === "held" && next.phase === "fading" || next.remaining > latest.remaining)))) return;
  if (latest && latest.session !== next.session) retired.add(latest.session);
  const changed = latest?.session !== next.session || latest.version !== next.version;
  if (changed || next.remaining !== latest.remaining) observed = performance.now();
  if (changed || next.cap !== latest.cap) capObserved = performance.now();
  latest = next;
  const b = $("#wind-down-open");
  if (b) {
    b.dataset.phase = next.phase;
    b.querySelector("span").textContent = next.phase === "held" ? "Wind down · holding" :
      next.phase === "fading" ? `Wind down · ${Math.ceil(next.remaining / 60)}m` : "Wind down";
  }
  panel?.paint();
}
on("state", () => adopt(S.state?.wind_down));
on("zones", () => panel?.paint());

export function initWindDown() {
  $("#wind-down-open")?.addEventListener("click", openWindDown);
  adopt(S.state?.wind_down);
}

export function openWindDown() {
  panel?.release();
  const d = openSheet(`<div class="body wind-down"><header class="wd-head"><div><h2>Wind down</h2>
    <p class="hint">Fade the brightness Airglow sends to your lights. At zero, the timer holds until you release it or restart Airglow.</p></div><button class="btn quiet" data-close>Close</button></header>
    <div class="wd-dial"><div><span data-wd-label>Checking lighting…</span><strong role="timer" data-wd-time>—</strong></div>
      <svg viewBox="0 0 240 78" preserveAspectRatio="none" aria-hidden="true"><path class="wd-wash" d="M3 7 L237 69 L237 76 L3 76 Z"/>
        <path class="wd-slope" d="M3 7 L237 69"/><circle cx="3" cy="7" r="4" data-wd-point/></svg></div>
    <div class="wd-reading"><span>Scene brightness <b data-wd-under>—</b></span><span>Timer ceiling <b data-wd-cap>—</b></span></div>
    <form class="wd-form"><label for="wd-minutes">Fade duration in minutes</label><div>
      <input id="wd-minutes" class="text" type="number" min="5" max="120" step="1" value="30" required>
      <span class="hint">5–120 minutes</span></div>
      <button class="btn primary" type="submit" data-wd-start disabled>Start fade</button></form>
    <p class="hint wd-detail" data-wd-detail></p>
    <button class="btn" data-wd-cancel hidden>Cancel fade</button>
    <p class="wd-error" role="alert" hidden></p><button class="text-link" data-wd-retry hidden>Refresh timer status</button>
    <span class="sr" role="status" data-wd-live></span></div>`);
  const root = d.querySelector(".wind-down"), find = s => root.querySelector(s), form = find("form"), minutes = find("#wd-minutes");
  const cancel = find("[data-wd-cancel]"), start = find("[data-wd-start]"), error = find(".wd-error"), retry = find("[data-wd-retry]");
  let phase = null, closed = false, refreshing = false, readVersion = 0, checkedEnd = false;
  const current = () => !closed && d.open && root.isConnected;
  function paint() {
    if (!current()) return;
    const s = latest, active = s && s.phase !== "idle", holding = s?.phase === "held";
    const remaining = s?.phase === "fading" ? Math.max(0, s.remaining - Math.floor((performance.now() - observed) / 1000)) : 0;
    const duration = active ? s.duration : Math.max(0, minutes.valueAsNumber || 0) * 60;
    root.dataset.phase = s?.phase || "unknown";
    find("[data-wd-label]").textContent = holding ? "Output held at zero" : active ? (remaining ? "Time remaining" : "Checking hold…") : s ? "Fade duration" : "Checking lighting…";
    find("[data-wd-time]").textContent = holding ? "0%" : s && duration ? clock(active ? remaining : duration) : "—";
    find("[data-wd-under]").textContent = s ? `${s.underlying}%` : "—";
    const ceiling = active ? Math.max(0, s.cap - s.initial * (performance.now() - capObserved) / (1000 * s.duration)) : 0;
    find("[data-wd-cap]").textContent = active ? `${Number(ceiling.toFixed(1))}%` : "Inactive";
    const fraction = active ? 1 - remaining / duration : 0, point = find("[data-wd-point]");
    point.setAttribute("cx", 3 + 234 * fraction); point.setAttribute("cy", 7 + 62 * fraction);
    form.hidden = !!active;
    start.disabled = busy || !s || s.underlying <= 0;
    cancel.hidden = !active; cancel.disabled = busy;
    cancel.textContent = holding ? "Release hold" : "Cancel fade";
    const zones = S.zones?.zones;
    const noOutputs = Array.isArray(zones) && !zones.some(z => z.device?.online && z.config?.enabled && z.positions?.length);
    const detail = active ? (holding ? "Release the hold to return to the current scene and automations." : "Cancel to return to the current scene and automations.")
      : s?.underlying === 0 ? "Turn on a scene with brightness above zero to start a fade."
      : "Lower scene and automation brightness settings still apply.";
    find("[data-wd-detail]").textContent = `${noOutputs ? "No lighting zones are active. The ceiling applies when a zone is online and enabled. " : ""}${detail} Fan curves and temperature safeguards run independently.`;
    if (s && phase !== s.phase) {
      if (phase) find("[data-wd-live]").textContent = { idle: "Timer released.", fading: "Wind down started.", held: "Wind down finished. Lighting output is capped at zero." }[s.phase];
      phase = s.phase; checkedEnd = false;
    }
    if (s?.phase === "fading" && !remaining && !checkedEnd) { checkedEnd = true; refresh(); }
  }
  function report(message) { error.textContent = message; error.hidden = !message; retry.hidden = !message; }
  async function refresh(afterCommand = false) {
    if (!current() || (refreshing && !afterCommand)) return;
    const read = ++readVersion;
    refreshing = true;
    const before = latest;
    try {
      const s = await get(endpoint);
      if (!current() || read !== readVersion) return;
      if (latest === before || (latest?.session === s.session && (s.version > latest.version || s.remaining < latest.remaining))) adopt(s);
      report("");
    } catch (err) { if (current() && read === readVersion) report(`Could not check the timer. ${err.message}`); }
    finally { if (read === readVersion) refreshing = false; }
  }
  async function command(action) {
    if (busy || !latest || !current()) return;
    const before = latest;
    const body = { action, session: before.session, version: before.version };
    if (action === "start") {
      if (!form.reportValidity() || before.phase !== "idle") return;
      body.minutes = minutes.valueAsNumber;
    } else {
      if (before.phase === "idle") return;
      body.id = before.id;
    }
    let focusAfter;
    readVersion++; refreshing = false;
    busy = true; report(""); paint();
    try {
      const s = await post(endpoint, body);
      if (!latest || latest.session === before.session || latest.session === s.session) adopt(s);
      emit("refresh");
      focusAfter = s.phase === "idle" ? minutes : cancel;
    } catch (err) {
      // The request belongs to the running timer even if its original sheet closed. Re-read after the
      // uncertain write so an earlier GET cannot reinstate a timer that the server already released.
      if (panel) await panel.refresh(true);
      else emit("refresh");
      if (current()) report(`Timer command could not be confirmed. ${err.message}`);
    } finally { busy = false; panel?.paint(); if (current()) focusAfter?.focus(); }
  }
  function release() {
    if (closed) return;
    closed = true; clearInterval(interval); observer.disconnect();
    d.removeEventListener("close", onClose);
    if (panel?.release === release) panel = null;
  }
  function onClose() { if (!d.open) release(); }
  const observer = new MutationObserver(() => { if (!current()) release(); });
  observer.observe(d, { childList: true });
  const interval = setInterval(() => { if (!current()) release(); else if (!document.hidden) paint(); }, 1000);
  d.addEventListener("close", onClose);
  panel = { paint, release, refresh };
  form.addEventListener("submit", e => { e.preventDefault(); command("start"); });
  minutes.addEventListener("input", paint);
  cancel.addEventListener("click", () => command("cancel")); retry.addEventListener("click", () => refresh());
  adopt(S.state?.wind_down); paint(); refresh();
}
