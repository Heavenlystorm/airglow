import { $, S, esc, get, post, setHTML } from "./core.js";
import { applyDesign } from "./design.js";
import { applyAppearance } from "./appearance.js";
import { fmt, fmtText, level } from "./readings.js";
import { DESK_FACE_KEY as FACE_KEY, DESK_TIMER_KEY as TIMER_KEY, durationText, facePreferences, minuteTrace, plateForDay, readTimer, remaining } from "./desk-state.js";
import { displayPins } from "./desk-pins.js";
import { lookbookCatalog } from "./lookbook-catalog.js";

const body = document.body;
const text = (el, value) => { if (el.textContent !== value) el.textContent = value; };
function read(key) {
  try { return JSON.parse(localStorage.getItem(key)); } catch { return null; }
}
function store(key, value, dialog) {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, JSON.stringify(value));
    text(dialog.querySelector(".desk-save-error"), "");
    return true;
  } catch {
    text(dialog.querySelector(".desk-save-error"), "This window can use your changes, but local storage is unavailable.");
    return false;
  }
}

let prefs = facePreferences(read(FACE_KEY)), timer = readTimer(read(TIMER_KEY));
let clockFormat, shortFormat, clockTask, pollTask, generation = 0, lastData = null;
let pinRevision = 0, pinBusy = false, native = null, presentation = "", offline = false, announced = false;
const options = $("#desk-options"), timerDialog = $("#desk-timer");
const calendar = new Intl.DateTimeFormat(undefined, { weekday: "long", month: "long", day: "numeric" });
let plates = [];
function paintScenery(now) {
  const tile = plateForDay(prefs.scenery, now), panel = $(".desk-plate");
  const plate = plates.find(item => item.tile === tile);
  const daily = $("#desk-daily-name");
  daily.hidden = !plate || prefs.scenery !== "daily";
  if (!daily.hidden) text(daily, "Today · " + plate.profile.name);
  panel.hidden = !plate;
  if (plate && panel.dataset.tile !== String(tile)) {
    panel.dataset.tile = String(tile);
    panel.style.setProperty("--plate-x", ((tile % 3) * 50) + "%");
    panel.style.setProperty("--plate-y", (Math.floor(tile / 3) * 100) + "%");
  }
}

function applyFace() {
  const lastLayout = body.dataset.layout;
  body.dataset.layout = prefs.layout;
  body.dataset.scenery = prefs.scenery;
  body.dataset.seconds = String(prefs.seconds);
  $("#desk-layout").value = prefs.layout; $("#desk-scenery").value = prefs.scenery;
  $("#desk-format").value = prefs.format; $("#desk-seconds").checked = prefs.seconds;
  const format = { hour: "numeric", minute: "2-digit" };
  if (prefs.format !== "system") format.hourCycle = prefs.format === "24" ? "h23" : "h12";
  shortFormat = new Intl.DateTimeFormat(undefined, format);
  clockFormat = new Intl.DateTimeFormat(undefined, { ...format, ...(prefs.seconds ? { second: "2-digit" } : {}) });
  paintClock();
  if (lastData && prefs.layout !== lastLayout) visibility();
}

function paintClock() {
  clearTimeout(clockTask);
  if (document.hidden) return;
  const now = new Date(), left = remaining(timer, now.getTime());
  paintScenery(now);
  const complete = !!timer && left === 0;
  body.dataset.timing = String(!!timer);
  text($("#desk-date"), calendar.format(now));
  const parts = clockFormat.formatToParts(now);
  const digits = $("#desk-time");
  if (timer) {
    digits.removeAttribute("datetime");
    text(digits, durationText(left));
    text($("#desk-period"), "");
  } else {
    digits.dateTime = now.toISOString();
    text(digits, parts.filter(p => p.type !== "dayPeriod").map(p => p.value).join("").trim());
    text($("#desk-period"), parts.find(p => p.type === "dayPeriod")?.value || "");
  }
  const progress = $("#desk-progress");
  progress.hidden = !timer;
  if (timer) {
    progress.value = 100 * (1 - left / (timer.minutes * 60000));
    progress.setAttribute("aria-valuetext", durationText(left) + " remaining");
  }
  const label = !timer ? "Focus timer" : complete ? "Timer finished" : timer.mode === "paused" ? "Timer paused" : "Focus timer running";
  text($("#desk-timer-open"), label);
  text($("#desk-clock-detail"), timer ? `${timer.minutes} min · ${shortFormat.format(now)}` : "");
  $("#desk-duration-form").hidden = !!timer;
  $("#desk-timer-active").hidden = !timer;
  if (timer) {
    text($("#desk-timer-status"), complete ? "Your focus timer has finished." : `${durationText(left)} ${timer.mode === "paused" ? "remaining · paused" : "remaining"}`);
    text($("#desk-pause"), complete ? "Start again" : timer.mode === "paused" ? "Resume" : "Pause");
    text($("#desk-end"), complete ? "Done" : "End timer");
  }
  if (complete && !announced && !offline) { text($("#desk-feedback"), "Focus timer finished."); announced = true; }
  if (!complete) announced = false;
  const quantum = prefs.seconds || timer?.mode === "running" && !complete ? 1000 : 60000;
  const delay = timer?.mode === "running" && !complete ? (left % 1000 || 1000) : quantum - now.getTime() % quantum;
  clockTask = setTimeout(paintClock, delay + 12);
}

function commitTimer(next, close = true) {
  timer = next;
  const persisted = store(TIMER_KEY, next, timerDialog);
  if (!offline) text($("#desk-feedback"), "");
  paintClock();
  if (close && persisted) timerDialog.close();
}

function openDialog(el) {
  if (el === timerDialog) {
    paintClock();
    for (const button of el.querySelectorAll("[data-minutes]")) button.setAttribute("aria-pressed", String(button.dataset.minutes === $("#desk-minutes").value));
  }
  if (!el.open) el.showModal();
}

$("#desk-options-open").onclick = () => openDialog(options);
$("#desk-timer-open").onclick = () => openDialog(timerDialog);
for (const dialog of [timerDialog,options]) dialog.querySelector(".desk-close").onclick = () => dialog.close();
options.addEventListener("change", () => {
  prefs = facePreferences({ layout: $("#desk-layout").value, scenery: $("#desk-scenery").value,
    seconds: $("#desk-seconds").checked, format: $("#desk-format").value });
  store(FACE_KEY, prefs, options); applyFace();
});
function updateDuration() {
  for (const button of timerDialog.querySelectorAll("[data-minutes]")) button.setAttribute("aria-pressed", String(button.dataset.minutes === $("#desk-minutes").value));
}
timerDialog.addEventListener("click", e => {
  const preset = e.target.closest("[data-minutes]");
  if (preset) { $("#desk-minutes").value = preset.dataset.minutes; updateDuration(); }
});
$("#desk-minutes").addEventListener("input", updateDuration);
$("#desk-duration-form").onsubmit = e => {
  e.preventDefault();
  const minutes = $("#desk-minutes").valueAsNumber;
  if (Number.isInteger(minutes) && minutes >= 1 && minutes <= 240)
    commitTimer({ mode: "running", minutes, deadline: Date.now() + minutes * 60000 });
};
$("#desk-pause").onclick = () => {
  if (!timer) return;
  const now = Date.now(), left = remaining(timer, now);
  commitTimer(left === 0 || timer.mode === "paused" ?
    { mode: "running", minutes: timer.minutes, deadline: now + (left || timer.minutes * 60000) } :
    { mode: "paused", minutes: timer.minutes, left });
};
$("#desk-end").onclick = () => commitTimer(null);
window.addEventListener("storage", e => {
  if (e.key === FACE_KEY || e.key === null) { prefs = facePreferences(read(FACE_KEY)); applyFace(); }
  if (e.key === TIMER_KEY || e.key === null) { timer = readTimer(read(TIMER_KEY)); paintClock(); }
});

function paintReadings(data, connected) {
  const items = (data?.readings || []).map(m => pins.label(m));
  body.dataset.manyPins = String(items.length > 2);
  $("#desk-instruments").style.setProperty("--pin-count", Math.max(1, items.length));
  $("#desk-empty").hidden = items.length !== 0 || !connected;
  text($("#desk-empty"), pins.emptyMessage);
  $("#desk-range-note").hidden = !items.length;
  setHTML($("#desk-instruments"), items.map(m => {
    const v = connected ? m.value : null, f = fmt(m,v);
    const series = minuteTrace(data.times, connected ? m.samples : [], data.time);
    const title = m.short || m.name, l = level(m.id, m, v);
    return `<article class="desk-meter" data-level="${esc(l)}"><h3 title="${esc(m.name)}">${esc(title)}</h3>
      <div class="desk-reading"><strong>${esc(f.num)}</strong><span>${esc(f.unit)}</span></div>
      <svg viewBox="0 0 180 32" preserveAspectRatio="none" aria-hidden="true"><path d="${series.path}"/></svg>
      <p class="desk-extrema">${series.min === null ? "No recent samples" : `Low ${esc(fmtText(m,series.min))} · High ${esc(fmtText(m,series.max))}`}</p></article>`;
  }).join(""));
}

function pinState(pinned) {
  $("#desk-pin").setAttribute("aria-pressed", String(pinned));
  $("#desk-pin").title = pinned ? "Stop keeping on top" : "Keep on top";
}
$("#desk-pin").onclick = async () => {
  if (pinBusy) return;
  pinBusy = true; pinRevision++;
  const button = $("#desk-pin"); button.setAttribute("aria-busy","true");
  try { pinState((await post("/api/desk-display/pin", { pinned: button.getAttribute("aria-pressed") !== "true" })).pinned); }
  catch (e) { text($("#desk-feedback"), e.message); }
  finally { pinBusy = false; pinRevision++; button.removeAttribute("aria-busy"); }
};
$("#desk-main").onclick = async () => {
  if (native === null) return;
  if (native === false) { location.assign("/#overview"); return; }
  try { await post("/api/window/show", {}); options.close(); }
  catch (e) { text(options.querySelector(".desk-save-error"), e.message); }
};
$("#desk-full").hidden = !document.fullscreenEnabled;
$("#desk-full").onclick = async () => {
  try {
    if (document.fullscreenElement) await document.exitFullscreen();
    else await document.documentElement.requestFullscreen();
  } catch { text($("#desk-feedback"), "Full screen is unavailable in this window."); }
};
document.addEventListener("fullscreenchange", () => {
  const active = !!document.fullscreenElement, b = $("#desk-full");
  b.setAttribute("aria-pressed", String(active)); b.setAttribute("aria-label", b.title = active ? "Exit full screen" : "Full screen");
});

async function poll(ticket) {
  const pinAtStart = pinRevision;
  try {
    const query = prefs.layout === "quiet" ? "?source=desk" : pins.query();
    const data = await get("/api/desk-display" + query, 5000);
    if (ticket !== generation) return;
    S.state = { temp_unit: data.temp_unit };
    const appearance = JSON.stringify([data.design, data.appearance]);
    if (appearance !== presentation) {
      applyDesign(data.design, { persist: false }); applyAppearance(data.appearance, { persist: false }); presentation = appearance;
    }
    native = data.window.native;
    $("#desk-main").disabled = false;
    $("#desk-pin").hidden = !(native && data.window.open);
    if (!pinBusy && pinAtStart === pinRevision) pinState(data.window.pinned);
    if (offline || !lastData) text($("#desk-feedback"), timer && remaining(timer) === 0 ? "Focus timer finished." : "");
    offline = false; lastData = data;
    paintReadings(data, true);
  } catch {
    if (ticket !== generation) return;
    offline = true; paintReadings(lastData, false);
    text($("#desk-feedback"), "Readings offline · reconnecting");
  } finally {
    if (ticket === generation && !document.hidden) pollTask = setTimeout(() => poll(ticket), prefs.layout === "quiet" && !offline ? 30000 : 2000);
  }
}

function visibility() {
  ++generation; clearTimeout(pollTask); clearTimeout(clockTask);
  if (!document.hidden) { paintClock(); poll(generation); }
}
document.addEventListener("visibilitychange", visibility);
const pins = displayPins(() => { lastData=null; paintReadings(null,false); visibility(); }, () => lastData);
window.addEventListener("pagehide", () => { ++generation; clearTimeout(pollTask); clearTimeout(clockTask); });
window.addEventListener("pageshow", e => { if (e.persisted) visibility(); });
text($("#desk-feedback"), "Connecting to Airglow…");
applyFace();
lookbookCatalog().then(items => {
  plates = items;
  for (const {tile,profile} of items) $("#desk-plates").append(new Option(profile.name, "plate-"+tile));
  $("#desk-scenery").value = prefs.scenery; paintScenery(new Date());
}).catch(() => { $("#desk-plates").disabled = true; });
visibility();
