// Automations screen: rules as plain sentences, a builder, and editable starter rules.

import { $, S, curveLabel, emit, esc, fail, get, on, post, setHTML, toast, unitF } from "./core.js";
import { closeSheet, openSheet } from "./sheet.js";

const DAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const DAY_GROUPS = { "Every day": [0, 1, 2, 3, 4, 5, 6], Weekdays: [0, 1, 2, 3, 4], Weekends: [5, 6] };

function scheduleError(w) {
  if (!w.start || !w.end) return "Choose a start and end time.";
  if (w.start === w.end) return "Choose different start and end times.";
  if (!w.days?.length) return "Choose at least one starting day.";
  return "";
}

const clockMinutes = value => Number(value.slice(0, 2)) * 60 + Number(value.slice(3));

function scheduleRibbon(w) {
  if (w.type !== "time" || !w.start || !w.end || w.start === w.end || w.days?.length === 0) return "";
  const start = clockMinutes(w.start), end = clockMinutes(w.end);
  const intervals = end < start ? [[0, end], [start, 1440]] : [[start, end]];
  const bars = intervals.filter(([a, b]) => b > a).map(([a, b]) =>
    `<i style="left:${100 * a / 1440}%;width:${100 * (b - a) / 1440}%"></i>`).join("");
  return `<div class="schedule-ribbon" aria-hidden="true"><div class="schedule-hours">${bars}</div><div class="schedule-ticks"><span>00h</span><span>06h</span><span>12h</span><span>18h</span><span>24h</span></div></div>`;
}

function scheduleDetail(w) {
  const error = scheduleError(w);
  if (error) return error;
  const difference = clockMinutes(w.end) - clockMinutes(w.start);
  const length = (difference + 1440) % 1440;
  const hours = Math.floor(length / 60), remainder = length % 60;
  const duration = [hours ? `${hours}h` : "", remainder ? `${remainder}m` : ""].filter(Boolean).join(" ");
  return difference < 0 ? `Runs for ${duration}, ending at ${w.end} the following day.` : `Runs for ${duration} on each selected day.`;
}

function daysText(days) {
  if (!days || days.length === 7) return "";
  const s = days.join(",");
  if (s === "0,1,2,3,4") return " on weekdays";
  if (s === "5,6") return " at weekends";
  return " on " + days.map((d) => DAYS[d]).join(", ");
}

const tempIn = (c) => (unitF() ? Math.round(c * 9 / 5 + 32) : c);      // rules keep whole °C; the box shows your unit
const tempOut = (v) => Math.round(unitF() ? (v - 32) * 5 / 9 : v);
const tempTxt = (c) => `${tempIn(c)}°${unitF() ? "F" : "C"}`;

// names: extra profile names by id (a shared profile's automations call it "self")
export function sentence(r, html = true, names = {}) {
  const b = (t) => (html ? `<b>${esc(t)}</b>` : t);
  const w = r.when, t = r.then;
  const presetName = names[t.preset] || S.presets.find((p) => p.id === t.preset)?.name || "a deleted profile";
  const when = {
    app_running: () => `When ${b(w.exe)} is running`,
    app_focused: () => `When ${b(w.exe)} is the app in front`,
    idle: () => `When the PC has been idle for ${b(`${w.minutes} minutes`)}`,
    time: () => `Between ${b(w.start)} and ${b(w.end)}${daysText(w.days)}`,
    locked: () => "While the PC is locked",
    temp_above: () => `When ${b({ max: "the hotter part", cpu: "the processor", gpu: "the graphics card", liquid: "the coolant" }[w.source])} is above ${b(tempTxt(w.temp))}`,
  }[w.type]();
  const then = {
    preset: () => `use ${b(presetName)}`,
    lights_off: () => `turn the lights ${b("off")}`,
    dim: () => `dim the lights to ${b(`${t.brightness}%`)}`,
    fan_curve: () => `switch the fans to ${b(curveLabel(t.curve))}`,
  }[t.type]();
  // a chip right before a comma or full stop keeps less room on its right, so the stop doesn't look spaced off
  return `${when}, ${then}.`.replace(/<b>([^<]*)<\/b>(?=[,.])/g, '<b class="end">$1</b>');
}

const gameProfile = () => S.presets.find(p => p.id === "gaming") || S.presets[0];

const STARTERS = [
  { title: "Lights off at night", icon: "moon", text: "Between 23:00 and 07:00, the lights go dark.", rule: { when: { type: "time", start: "23:00", end: "07:00" }, then: { type: "lights_off" } } },
  { title: "Dim when you step away", icon: "clock", text: "After 10 idle minutes, lights drop to 15%.", rule: { when: { type: "idle", minutes: 10 }, then: { type: "dim", brightness: 15 } } },
  { title: "Game mode", icon: "game", text: () => gameProfile() ? `While your game runs, use ${gameProfile().name}.` : "Choose what happens while your game runs.", rule: { when: { type: "app_running", exe: "" }, then: { type: "preset", preset: "gaming" } } },
  { title: "Quiet when idle", icon: "fan", text: "After 15 idle minutes, the fans go Silent.", rule: { when: { type: "idle", minutes: 15 }, then: { type: "fan_curve", curve: "silent" } } },
  { title: "Dark while locked", icon: "lock", text: "Lights off whenever you lock the PC.", rule: { when: { type: "locked" }, then: { type: "lights_off" } } },
  { title: "Cool down under load", icon: "temp", text: () => `Above ${tempTxt(80)}, the fans switch to Performance.`, rule: { when: { type: "temp_above", temp: 80, source: "max" }, then: { type: "fan_curve", curve: "performance" } } },
];

const newId = () => "rule-" + Date.now().toString(36) + Math.random().toString(36).slice(2, 5);

let rulesTail = Promise.resolve(), rulesRevision = 0, rulesPending = 0, rulesDraft = null, rulesSaved = null;
const currentRules = () => rulesPending ? rulesDraft : S.rules;

function writeRules(rules) {
  if (!rulesPending) rulesSaved = structuredClone(S.rules);
  const revision = ++rulesRevision, body = structuredClone(rules);
  rulesPending++;
  S.rules = rulesDraft = structuredClone(body);       // the next gesture must start from these pending edits
  renderAutomations();
  const run = rulesTail.then(async () => {
    try {
      rulesSaved = await post("/api/automations", { rules: body });
      if (revision === rulesRevision) rulesDraft = rulesSaved;
    } catch (err) {
      if (revision === rulesRevision) rulesDraft = structuredClone(rulesSaved);
      throw err;
    } finally {
      S.rules = rulesDraft;                         // an old reply or background refresh cannot replace newer edits
      rulesPending--;
      renderAutomations();
    }
  });
  rulesTail = run.catch(() => {});                  // a failed request must not prevent the next complete list
  return run;
}

async function saveRules(rules, msg) {
  try {
    await writeRules(rules);
    emit("refresh");
    if (msg) toast(msg);
    return true;
  } catch (err) { fail(err); return false; }
}

// Minutes until the clock next shows hour:00 (matches automation.minutes_until on the Python side).
const minutesUntil = (hour) => {
  const d = new Date();
  return ((hour * 60 - (d.getHours() * 60 + d.getMinutes())) % 1440 + 1440) % 1440 || 1440;
};

export async function pauseAutomations(minutes) {
  try {
    const r = await post("/api/automations/pause", { minutes });
    const until = r.paused_until && new Date(r.paused_until * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    toast(minutes ? `Automations paused until ${until}` : "Automations are back on");
    emit("refresh");
  } catch (err) { fail(err); }
}

export function renderAutomations() {
  if (!S.state) return;
  const rules = currentRules();
  const o = S.state.override || {};
  const active = new Set(o.active || []);
  const paused = !!o.paused_until;
  const empty = $("#rules-empty");
  if (empty) empty.hidden = rules.length > 0;
  const query = $("#rule-search").value.trim().toLowerCase();
  const status = $("#rule-status").value;
  const filtered = !!query || status !== "all";
  $("#rule-filters").hidden = !rules.length && !filtered;
  $("#rule-clear").hidden = !filtered;
  let matches = 0;
  const list = $("#rules");
  const focused = document.activeElement;
  const focusId = list.contains(focused) ? focused.id : "";
  setHTML(list, rules.map((r, i) => {
    const isActive = active.has(r.id) && r.enabled;
    if ((status === "enabled" && !r.enabled) || (status === "disabled" && r.enabled) ||
        (status === "active" && (!isActive || paused)) || (query && !sentence(r, false).toLowerCase().includes(query))) return "";
    matches++;
    const lights = r.id === o.lighting, fans = r.id === o.fans;
    const winning = lights || fans;
    const control = lights && fans ? "In control of lights and fans now" : lights ? "In control of the lights now" : fans ? "In control of the fans now" : "";
    const state = !r.enabled ? "Switched off" : paused ? "Paused" : isActive ? (winning ? control : "Applies now, but a rule above it wins") : "Waiting";
    const id = esc(r.id);
    return `<li class="rule${isActive && winning ? " active" : ""}${r.enabled ? "" : " disabled"}" data-id="${id}" value="${i + 1}">
      <div><div class="sentence">${sentence(r)}</div><div class="state">${state}</div>${scheduleRibbon(r.when)}</div>
      <div class="tools">
        <label class="switch" title="${r.enabled ? "Switch off" : "Switch on"}"><input type="checkbox" id="enable-${id}" data-toggle${r.enabled ? " checked" : ""} aria-label="Automation on"><span></span></label>
        <button class="icon-btn" id="up-${id}" data-up ${filtered || i === 0 ? "disabled" : ""} aria-label="Move up (higher priority)" title="${filtered ? "Clear filters to reorder" : "Move up (higher priority)"}"><svg><use href="#i-up"/></svg></button>
        <button class="icon-btn" id="down-${id}" data-down ${filtered || i === rules.length - 1 ? "disabled" : ""} aria-label="Move down (lower priority)" title="${filtered ? "Clear filters to reorder" : "Move down (lower priority)"}"><svg><use href="#i-down"/></svg></button>
        <button class="btn quiet" id="edit-${id}" data-edit>Edit</button>
        <button class="icon-btn" id="copy-${id}" data-copy title="Duplicate automation" aria-label="Duplicate automation"><svg aria-hidden="true"><use href="#i-copy"/></svg></button>
        <button class="icon-btn" id="delete-${id}" data-del title="Delete"><svg><use href="#i-trash"/></svg></button>
      </div></li>`;
  }).join(""));
  if (focusId && document.activeElement === document.body) {
    const row = list.querySelector(`[data-id="${CSS.escape(focused.closest("[data-id]").dataset.id)}"]`);
    const target = row?.querySelector(`#${CSS.escape(focusId)}:enabled`) || row?.querySelector("[data-edit]");
    (target || (filtered ? $("#rule-search") : $("#auto-add"))).focus({ preventScroll: true });
  }
  const count = filtered ? (matches ? `${matches} of ${rules.length} ${rules.length === 1 ? "rule" : "rules"}` : "No matching rules") : "";
  if ($("#rule-count").textContent !== count) $("#rule-count").textContent = count;
  const until = paused ? new Date(o.paused_until * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : "";
  $("#auto-pause").textContent = `Resume automations (paused until ${until})`;
  $("#auto-pause").hidden = !rules.length || !paused;
  $("#auto-pause-for").hidden = !rules.length || paused;
  const used = new Set(rules.map((r) => JSON.stringify([r.when.type, r.then.type])));
  const starterIcon = { lights_off: "power", dim: "effect", fan_curve: "sliders", preset: "workspace" };
  const starters = STARTERS.filter((s) => !used.has(JSON.stringify([s.rule.when.type, s.rule.then.type])));
  setHTML($("#starters"), starters.length ? `<h2>${rules.length ? "More ideas" : "Start with one of these"}</h2>
    <div class="starter-grid">${starters.map((s) => `<button class="starter" id="starter-${STARTERS.indexOf(s)}" data-starter="${STARTERS.indexOf(s)}" aria-haspopup="dialog">
      <span class="rule-glyphs" aria-hidden="true"><svg class="starter-icon"><use href="#i-${s.icon}"/></svg><i></i><svg class="action-icon"><use href="#i-${starterIcon[s.rule.then.type]}"/></svg></span>
      <strong>${esc(s.title)}</strong><span class="starter-detail">${esc(typeof s.text === "function" ? s.text() : s.text)}</span><svg class="starter-arrow" aria-hidden="true"><use href="#i-back"/></svg></button>`).join("")}</div>` : "");
}

// ------------------------------------------------------------ builder
function triggerDraft(type) {
  const defaults = {
    time: { start: "23:00", end: "07:00", days: [...DAY_GROUPS["Every day"]] },
    idle: { minutes: 10 }, temp_above: { temp: 80, source: "max" },
  };
  return { type, ...defaults[type] };
}

function openBuilder(rule, isNew, title = "", copyFrom = null) {
  const r = structuredClone(rule);
  r.when = { ...triggerDraft(r.when.type), ...r.when };
  let apps = [], activeForm, saving = false;
  const curves = Object.keys(S.curves.curves);
  const previewText = () => {
    if (["app_running", "app_focused"].includes(r.when.type) && !r.when.exe)
      return "Type the program's file name, or pick one that's open right now, to finish this rule.";
    if (r.when.type === "time" && scheduleError(r.when)) return "Choose the schedule for this automation.";
    if (r.then.type === "preset" && !S.presets.some(p => p.id === r.then.preset))
      return "Choose a saved profile or a different action.";
    try { return sentence(r); } catch { return "Fill in the details to see the rule."; }
  };
  const render = (keepNumber = null) => {
    const active = document.activeElement;
    const focusId = active?.closest?.("#rb-form") && active.id ? active.id : null;
    const w = r.when, t = r.then;
    const whenFields = {
      app_running: () => appField(w), app_focused: () => appField(w),
      idle: () => `<span>for</span><input class="text" type="number" required min="1" max="240" data-w="minutes" aria-label="Idle minutes" value="${w.minutes}" style="width:80px"><span>minutes</span>`,
      time: () => `<input class="text" type="time" data-w="start" aria-label="Start time" value="${w.start}" required><span>to</span><input class="text" type="time" data-w="end" aria-label="End time" value="${w.end}" required>
        <div class="rb-schedule"><div class="rb-day-groups" role="group" aria-label="Day shortcuts">${Object.keys(DAY_GROUPS).map(label => `<button type="button" class="text-link" data-days="${label}">${label}</button>`).join("")}</div>
        <div class="days" role="group" aria-label="Starting days">${DAYS.map((d, i) => `<button type="button" data-day="${i}" aria-pressed="${w.days.includes(i)}">${d}</button>`).join("")}</div>
        <div id="rb-hours">${scheduleRibbon(w)}</div><p class="hint" id="rb-schedule-detail" role="status">${scheduleDetail(w)}</p></div>`,
      locked: () => "",
      temp_above: () => `<select class="select" data-w="source" aria-label="Temperature source" style="width:auto">${[["max", "Hotter part"], ["cpu", "Processor"], ["gpu", "Graphics card"], ["liquid", "Coolant"]].map(([v, l]) => `<option value="${v}"${(w.source ?? "max") === v ? " selected" : ""}>${l}</option>`).join("")}</select>
        <span>above</span><input class="text" type="number" required min="${tempIn(30)}" max="${tempIn(100)}" data-w="temp" aria-label="Temperature" value="${tempIn(w.temp ?? 80)}" style="width:80px"><span>°${unitF() ? "F" : "C"}</span>`,
    }[w.type]();
    const thenFields = {
      preset: () => `<select class="select" data-t="preset" aria-label="Profile" required style="width:auto">${S.presets.some(p => p.id === t.preset) ? "" : `<option value="">${S.presets.length ? "Choose a profile" : "No saved profiles"}</option>`}${S.presets.map((p) => `<option value="${esc(p.id)}"${p.id === t.preset ? " selected" : ""}>${esc(p.name)}</option>`).join("")}</select>`,
      lights_off: () => "",
      dim: () => `<span>to</span><input class="text" type="number" required min="0" max="100" data-t="brightness" aria-label="Brightness" value="${t.brightness ?? 20}" style="width:80px"><span>%</span>`,
      fan_curve: () => `<select class="select" data-t="curve" aria-label="Fan curve" style="width:auto">${curves.map((c) => `<option value="${c}"${c === t.curve ? " selected" : ""}>${curveLabel(c)}</option>`).join("")}</select>`,
    }[t.type]();
    openSheet(`<form id="rb-form"><h2>${esc(title || (isNew ? "New automation" : "Edit automation"))}</h2>
      <div class="builder">
        <div class="row"><span>When</span><select class="select" id="rb-when" aria-label="When">${[["app_running", "A program is running"], ["app_focused", "A program is in front"], ["idle", "The PC is idle"], ["time", "It's between two times"], ["locked", "The PC is locked"], ["temp_above", "It's running hot"]].map(([v, l]) => `<option value="${v}"${v === w.type ? " selected" : ""}>${l}</option>`).join("")}</select></div>
        ${whenFields ? `<div class="row"><span></span><div class="sub">${whenFields}</div></div>` : ""}
        <div class="row"><span>Then</span><select class="select" id="rb-then" aria-label="Then">${[["preset", "Use a profile"], ["lights_off", "Turn the lights off"], ["dim", "Dim the lights"], ["fan_curve", "Switch the fan curve"]].map(([v, l]) => `<option value="${v}"${v === t.type ? " selected" : ""}>${l}</option>`).join("")}</select></div>
        ${thenFields ? `<div class="row"><span></span><div class="sub">${thenFields}</div></div>` : ""}
        <div class="preview-sentence" id="rb-preview">${previewText()}</div>
        <p class="hint">When the condition stops being true, your own lighting and fan settings come back by themselves.</p>
      </div>
      <p class="err" role="alert" hidden></p>
      <div class="actions"><label class="rb-enabled"><input type="checkbox" id="rb-enabled"${r.enabled ? " checked" : ""}>Enable rule</label><span class="spacer"></span><button type="button" class="btn quiet" data-close>Cancel</button><button class="btn primary" id="rb-save">${isNew ? "Add automation" : "Save automation"}</button></div></form>`);
    const form = activeForm = $("#rb-form");
    // A number input can display a partial number even when .value is empty. Keep the actual input
    // when the other half of the rule changes, including its native validity and uncommitted text.
    if (keepNumber) {
      const field = keepNumber.hasAttribute("data-w") ? "data-w" : "data-t";
      form.querySelector(`input[type="number"][${field}]`).replaceWith(keepNumber);
    }
    const updatePreview = () => {
      const detail = form.querySelector("#rb-schedule-detail");
      if (detail) {
        detail.textContent = scheduleDetail(r.when);
        setHTML(form.querySelector("#rb-hours"), scheduleRibbon(r.when));
        form.querySelector('[data-w="end"]').setCustomValidity(scheduleError(r.when));
      }
      const number = form.querySelector('input[type="number"]:invalid');
      setHTML(form.querySelector("#rb-preview"), number ? `Choose ${esc(number.getAttribute("aria-label").toLowerCase())} between ${number.min} and ${number.max}.` : previewText());
    };
    form.querySelector("#rb-enabled").addEventListener("change", e => { r.enabled = e.target.checked; });
    form.querySelector("#rb-when").addEventListener("change", (e) => {
      r.when = triggerDraft(e.target.value);
      render(form.querySelector('input[type="number"][data-t]'));
    });
    form.querySelector("#rb-then").addEventListener("change", (e) => {
      const type = e.target.value;
      const defaults = { preset: { preset: S.presets[0]?.id }, fan_curve: { curve: curves.includes("silent") ? "silent" : curves[0] }, dim: { brightness: 20 } };
      r.then = { type, ...defaults[type] };
      render(form.querySelector('input[type="number"][data-w]'));
    });
    const updateField = (el) => {
      if (el.type !== "number" || el.validity.valid) {
        const value = el.type === "number" ? el.valueAsNumber : el.value;
        if (el.hasAttribute("data-w")) r.when[el.dataset.w] = el.dataset.w === "temp" ? tempOut(value) : value;
        else r.then[el.dataset.t] = value;
      }
      updatePreview();
    };
    form.addEventListener("input", e => {
      if (e.target.matches("input[data-w], input[data-t]")) updateField(e.target);
    });
    form.addEventListener("change", (e) => {
      if (e.target.matches("select[data-w], select[data-t]")) updateField(e.target);
      if (!e.target.matches("[data-app-pick]")) return;
      if (e.target.value) { r.when.exe = e.target.value; form.querySelector('[data-w="exe"]').value = e.target.value; updatePreview(); }
    });
    form.addEventListener("click", e => {
      const day = e.target.closest("[data-day]"), group = e.target.closest("[data-days]");
      if (!day && !group) return;
      const days = new Set(group ? DAY_GROUPS[group.dataset.days] : r.when.days);
      if (day) {
        const index = Number(day.dataset.day);
        if (days.has(index)) days.delete(index); else days.add(index);
      }
      r.when.days = [...days].sort();
      for (const button of form.querySelectorAll("[data-day]"))
        button.setAttribute("aria-pressed", String(days.has(Number(button.dataset.day))));
      updatePreview();
    });
    updatePreview();
    form.addEventListener("submit", async (e) => {
      e.preventDefault();
      if (saving) return;
      saving = true;
      form.setAttribute("aria-busy", "true");
      const controls = [...form.elements].filter(el => !el.disabled);
      controls.forEach(el => { el.disabled = true; });
      const sheet = $("#sheet");
      const preventClose = event => event.preventDefault();
      sheet.addEventListener("cancel", preventClose);
      let saved = false;
      try {
        const next = [...currentRules()];
        if (isNew) {
          const sourceIndex = copyFrom ? next.findIndex(x => x.id === copyFrom) : -1;
          next.splice(sourceIndex < 0 ? next.length : sourceIndex + 1, 0, r);
        } else {
          const index = next.findIndex(x => x.id === r.id);
          if (index < 0) throw new Error("This automation has been removed. Close the editor to see your current rules.");
          next[index] = r;
        }
        await writeRules(next);
        saved = true;
        if (activeForm === form && form.isConnected) closeSheet();
        emit("refresh"); toast(isNew ? "Automation added" : "Automation saved");
      } catch (error) {
        const message = form.querySelector(".err");
        message.textContent = error.message; message.hidden = false;
      } finally {
        sheet.removeEventListener("cancel", preventClose);
        saving = false; form.removeAttribute("aria-busy");
        controls.forEach(el => { el.disabled = false; });
        if (!saved && form.isConnected) form.querySelector("#rb-save").focus();
        if (saved && document.activeElement === document.body)
          $("#rules")?.querySelector(`[data-id="${CSS.escape(r.id)}"] [data-edit]`)?.focus({ preventScroll: true });
      }
    });
    if (focusId) form.querySelector(`#${CSS.escape(focusId)}`)?.focus({ preventScroll: true });
  };
  const appOptions = () => '<option value="">Pick a running program</option>' + apps.map(a => `<option value="${esc(a)}">${esc(a)}</option>`).join("");
  const appField = (w) => `<input class="text" data-w="exe" aria-label="Program file name" value="${esc(w.exe ?? "")}" placeholder="Cyberpunk2077.exe" spellcheck="false" required style="width:220px">
      <select class="select" data-app-pick aria-label="Pick a running program" style="width:auto"${apps.length ? "" : " hidden"}>${appOptions()}</select>`;
  render();
  get("/api/apps").then(list => {
    apps = list;
    const picker = activeForm?.querySelector("[data-app-pick]");
    if (picker && apps.length && activeForm.isConnected) { setHTML(picker, appOptions()); picker.hidden = false; }
  }).catch(() => {});
}

// ------------------------------------------------------------ events
export function initAutomations() {
  $("#rule-search").addEventListener("input", renderAutomations);
  $("#rule-status").addEventListener("change", renderAutomations);
  $("#rule-clear").addEventListener("click", () => {
    $("#rule-search").value = "";
    $("#rule-status").value = "all";
    renderAutomations();
    ($("#rule-filters").hidden ? $("#auto-add") : $("#rule-search")).focus();
  });
  $("#rules").addEventListener("click", async (e) => {
    const li = e.target.closest("[data-id]");
    if (!li) return;
    const i = currentRules().findIndex((r) => r.id === li.dataset.id);
    if (i < 0) return;
    const rules = structuredClone(currentRules());
    if (e.target.closest("[data-up]") && i > 0) { [rules[i - 1], rules[i]] = [rules[i], rules[i - 1]]; saveRules(rules); }
    else if (e.target.closest("[data-down]") && i < rules.length - 1) { [rules[i + 1], rules[i]] = [rules[i], rules[i + 1]]; saveRules(rules); }
    else if (e.target.closest("[data-edit]")) openBuilder(rules[i], false);
    else if (e.target.closest("[data-copy]")) {
      const copy = { ...rules[i], id: newId(), enabled: false };
      openBuilder(copy, true, "Duplicate automation", rules[i].id);
    }
    else if (e.target.closest("[data-del]")) {
      const index = i;
      const removed = rules.splice(i, 1)[0];
      if (await saveRules(rules)) {
        S._lastDeleted = removed;
        toast("Automation deleted", {
          action: {
            label: "Undo",
            onClick: () => {
              const next = [...currentRules()];
              next.splice(Math.min(index, next.length), 0, removed);
              saveRules(next, "Automation restored");
            },
          },
        });
      }
    }
  });
  $("#rules").addEventListener("change", (e) => {
    if (!e.target.matches("[data-toggle]")) return;
    const id = e.target.closest("[data-id]").dataset.id;
    saveRules(currentRules().map((r) => (r.id === id ? { ...r, enabled: e.target.checked } : r)));
  });
  $("#starters").addEventListener("click", (e) => {
    const b = e.target.closest("[data-starter]");
    if (!b) return;
    const s = STARTERS[+b.dataset.starter];
    const rule = { id: newId(), enabled: true, ...structuredClone(s.rule) };
    if (rule.then.type === "preset") rule.then.preset = gameProfile()?.id;
    openBuilder(rule, true, s.title);
  });
  $("#auto-add").addEventListener("click", () => openBuilder({ id: newId(), enabled: true, when: { type: "app_running", exe: "" }, then: { type: "preset", preset: S.presets[0]?.id } }, true));
  $("#auto-pause").addEventListener("click", () => pauseAutomations(0));
  $("#auto-pause-for").addEventListener("change", (e) => {
    const v = e.target.value;
    e.target.value = "";
    if (v) pauseAutomations(v === "morning" ? minutesUntil(6) : +v);
  });
  on("state", () => { if (!$('[data-view="automations"]').hidden) renderAutomations(); });
}
