// Automations screen: rules as plain sentences, a builder, and one-click starters.

import { $, $$, S, curveLabel, emit, esc, fail, get, on, post, setHTML, toast, unitF } from "./core.js";
import { closeSheet, openSheet } from "./sheet.js";

const DAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

function daysText(days) {
  if (!days || days.length === 7) return "";
  const s = days.join(",");
  if (s === "0,1,2,3,4") return " on weekdays";
  if (s === "5,6") return " at weekends";
  return " on " + days.map((d) => DAYS[d]).join(", ");
}

const tempIn = (c) => (unitF() ? Math.round(c * 9 / 5 + 32) : c);      // rules keep whole °C; the box shows your unit
const tempOut = (v) => Math.min(100, Math.max(30, Math.round(unitF() ? (v - 32) * 5 / 9 : v)));
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

const STARTERS = [
  { title: "Lights off at night", text: "Between 23:00 and 07:00, the lights go dark.", rule: { when: { type: "time", start: "23:00", end: "07:00" }, then: { type: "lights_off" } } },
  { title: "Dim when you step away", text: "After 10 idle minutes, lights drop to 15%.", rule: { when: { type: "idle", minutes: 10 }, then: { type: "dim", brightness: 15 } } },
  { title: "Game mode", text: "While your game runs, use the Gaming profile.", rule: { when: { type: "app_running", exe: "" }, then: { type: "preset", preset: "gaming" } }, needsInput: true },
  { title: "Quiet when idle", text: "After 15 idle minutes, the fans go Silent.", rule: { when: { type: "idle", minutes: 15 }, then: { type: "fan_curve", curve: "silent" } } },
  { title: "Dark while locked", text: "Lights off whenever you lock the PC.", rule: { when: { type: "locked" }, then: { type: "lights_off" } } },
  { title: "Cool down under load", text: () => `Above ${tempTxt(80)}, the fans switch to Performance.`, rule: { when: { type: "temp_above", temp: 80, source: "max" }, then: { type: "fan_curve", curve: "performance" } } },
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
  setHTML($("#rules"), rules.map((r, i) => {
    const isActive = active.has(r.id) && r.enabled;
    const lights = r.id === o.lighting, fans = r.id === o.fans;
    const winning = lights || fans;
    const control = lights && fans ? "In control of lights and fans now" : lights ? "In control of the lights now" : fans ? "In control of the fans now" : "";
    const state = !r.enabled ? "Switched off" : paused ? "Paused" : isActive ? (winning ? control : "Applies now, but a rule above it wins") : "Waiting";
    return `<li class="rule${isActive && winning ? " active" : ""}${r.enabled ? "" : " disabled"}" data-id="${esc(r.id)}">
      <div><div class="sentence">${sentence(r)}</div><div class="state">${state}</div></div>
      <div class="tools">
        <label class="switch" title="${r.enabled ? "Switch off" : "Switch on"}"><input type="checkbox" data-toggle${r.enabled ? " checked" : ""} aria-label="Automation on"><span></span></label>
        <button class="icon-btn" data-up ${i === 0 ? "disabled" : ""} title="Move up (higher priority)"><svg><use href="#i-up"/></svg></button>
        <button class="icon-btn" data-down ${i === rules.length - 1 ? "disabled" : ""} title="Move down"><svg><use href="#i-down"/></svg></button>
        <button class="btn quiet" data-edit>Edit</button>
        <button class="icon-btn" data-del title="Delete"><svg><use href="#i-trash"/></svg></button>
      </div></li>`;
  }).join(""));
  const until = paused ? new Date(o.paused_until * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : "";
  $("#auto-pause").textContent = `Resume automations (paused until ${until})`;
  $("#auto-pause").hidden = !rules.length || !paused;
  $("#auto-pause-for").hidden = !rules.length || paused;
  const used = new Set(rules.map((r) => JSON.stringify([r.when.type, r.then.type])));
  const starters = STARTERS.filter((s) => !used.has(JSON.stringify([s.rule.when.type, s.rule.then.type])));
  setHTML($("#starters"), starters.length ? `<h2>${rules.length ? "More ideas" : "Start with one of these"}</h2>
    <div class="starter-grid">${starters.map((s) => `<button class="starter" data-starter="${STARTERS.indexOf(s)}"><strong>${esc(s.title)}</strong>${esc(typeof s.text === "function" ? s.text() : s.text)}</button>`).join("")}</div>` : "");
}

// ------------------------------------------------------------ builder
async function openBuilder(rule, isNew) {
  const r = structuredClone(rule);
  let apps = [];
  try { apps = await get("/api/apps"); } catch { /* typing the name still works */ }
  const curves = Object.keys(S.curves.curves);
  const render = (err = "") => {
    const w = r.when, t = r.then;
    const whenFields = {
      app_running: () => appField(w), app_focused: () => appField(w),
      idle: () => `<span>for</span><input class="text" type="number" min="1" max="240" data-w="minutes" aria-label="Idle minutes" value="${w.minutes ?? 10}" style="width:80px"><span>minutes</span>`,
      time: () => `<input class="text" type="time" data-w="start" aria-label="Start time" value="${w.start ?? "23:00"}" style="width:auto"><span>to</span><input class="text" type="time" data-w="end" aria-label="End time" value="${w.end ?? "07:00"}" style="width:auto">
        <div class="days" role="group" aria-label="Days">${DAYS.map((d, i) => `<button type="button" data-day="${i}" aria-pressed="${(w.days ?? [0, 1, 2, 3, 4, 5, 6]).includes(i)}">${d}</button>`).join("")}</div>`,
      locked: () => "",
      temp_above: () => `<select class="select" data-w="source" aria-label="Temperature source" style="width:auto">${[["max", "Hotter part"], ["cpu", "Processor"], ["gpu", "Graphics card"], ["liquid", "Coolant"]].map(([v, l]) => `<option value="${v}"${(w.source ?? "max") === v ? " selected" : ""}>${l}</option>`).join("")}</select>
        <span>above</span><input class="text" type="number" min="${tempIn(30)}" max="${tempIn(100)}" data-w="temp" aria-label="Temperature" value="${tempIn(w.temp ?? 80)}" style="width:80px"><span>°${unitF() ? "F" : "C"}</span>`,
    }[w.type]();
    const thenFields = {
      preset: () => `<select class="select" data-t="preset" aria-label="Profile" style="width:auto">${S.presets.map((p) => `<option value="${esc(p.id)}"${p.id === t.preset ? " selected" : ""}>${esc(p.name)}</option>`).join("")}</select>`,
      lights_off: () => "",
      dim: () => `<span>to</span><input class="text" type="number" min="0" max="100" data-t="brightness" aria-label="Brightness" value="${t.brightness ?? 20}" style="width:80px"><span>%</span>`,
      fan_curve: () => `<select class="select" data-t="curve" aria-label="Fan curve" style="width:auto">${curves.map((c) => `<option value="${c}"${c === t.curve ? " selected" : ""}>${curveLabel(c)}</option>`).join("")}</select>`,
    }[t.type]();
    let preview = "";
    const needsExe = ["app_running", "app_focused"].includes(w.type) && !w.exe;
    try { preview = needsExe ? "" : sentence(r); } catch { preview = ""; }
    if (needsExe) preview = "Type the program's file name, or pick one that's open right now, to finish this rule.";
    openSheet(`<form id="rb-form"><h2>${isNew ? "New automation" : "Edit automation"}</h2>
      <div class="builder">
        <div class="row"><span>When</span><select class="select" id="rb-when" aria-label="When">${[["app_running", "A program is running"], ["app_focused", "A program is in front"], ["idle", "The PC is idle"], ["time", "It's between two times"], ["locked", "The PC is locked"], ["temp_above", "It's running hot"]].map(([v, l]) => `<option value="${v}"${v === w.type ? " selected" : ""}>${l}</option>`).join("")}</select></div>
        ${whenFields ? `<div class="row"><span></span><div class="sub">${whenFields}</div></div>` : ""}
        <div class="row"><span>Then</span><select class="select" id="rb-then" aria-label="Then">${[["preset", "Use a profile"], ["lights_off", "Turn the lights off"], ["dim", "Dim the lights"], ["fan_curve", "Switch the fan curve"]].map(([v, l]) => `<option value="${v}"${v === t.type ? " selected" : ""}>${l}</option>`).join("")}</select></div>
        ${thenFields ? `<div class="row"><span></span><div class="sub">${thenFields}</div></div>` : ""}
        <div class="preview-sentence">${preview || "Fill in the details to see the rule."}</div>
        <p class="hint">When the condition stops being true, your own lighting and fan settings come back by themselves.</p>
      </div>
      ${err ? `<p class="err">${esc(err)}</p>` : ""}
      <div class="actions"><span class="spacer"></span><button type="button" class="btn quiet" data-close>Cancel</button><button class="btn primary">${isNew ? "Add automation" : "Save automation"}</button></div></form>`);
    const form = $("#rb-form");
    form.querySelector("#rb-when").addEventListener("change", (e) => { r.when = { type: e.target.value }; render(); });
    form.querySelector("#rb-then").addEventListener("change", (e) => { r.then = { type: e.target.value, ...(e.target.value === "preset" ? { preset: S.presets[0]?.id } : e.target.value === "fan_curve" ? { curve: "silent" } : {}) }; render(); });
    form.querySelectorAll("[data-w]").forEach((el) => el.addEventListener("change", () => {
      const v = el.type === "number" ? +el.value : el.value;
      r.when[el.dataset.w] = el.dataset.w === "temp" ? tempOut(v) : v; render();
    }));
    form.querySelectorAll("[data-t]").forEach((el) => el.addEventListener("change", () => { r.then[el.dataset.t] = el.type === "number" ? +el.value : el.value; render(); }));
    form.querySelector("[data-app-pick]")?.addEventListener("change", (e) => { if (e.target.value) { r.when.exe = e.target.value; render(); } });
    form.querySelectorAll("[data-day]").forEach((b) => b.addEventListener("click", () => {
      const d = +b.dataset.day, days = new Set(r.when.days ?? [0, 1, 2, 3, 4, 5, 6]);
      days.has(d) ? days.delete(d) : days.add(d);
      r.when.days = [...days].sort(); render();
    }));
    form.addEventListener("submit", async (e) => {
      e.preventDefault();
      if (r.when.type === "time") { r.when.start ??= "23:00"; r.when.end ??= "07:00"; }
      if (r.when.type === "idle") r.when.minutes ??= 10;
      if (r.when.type === "temp_above") { r.when.temp ??= 80; r.when.source ??= "max"; }
      if (r.then.type === "dim") r.then.brightness ??= 20;
      const next = isNew ? [...currentRules(), r] : currentRules().map((x) => (x.id === r.id ? r : x));
      try { await writeRules(next); closeSheet(); emit("refresh"); toast(isNew ? "Automation added" : "Automation saved"); }
      catch (err2) { render(err2.message); }
    });
  };
  const appField = (w) => `<input class="text" data-w="exe" aria-label="Program file name" value="${esc(w.exe ?? "")}" placeholder="Cyberpunk2077.exe" spellcheck="false" style="width:220px">
      ${apps.length ? `<select class="select" data-app-pick aria-label="Pick a running program" style="width:auto"><option value="">Pick a running program</option>${apps.map((a) => `<option value="${esc(a)}">${esc(a)}</option>`).join("")}</select>` : ""}`;
  render();
}

// ------------------------------------------------------------ events
export function initAutomations() {
  $("#rules").addEventListener("click", async (e) => {
    const li = e.target.closest("[data-id]");
    if (!li) return;
    const i = currentRules().findIndex((r) => r.id === li.dataset.id);
    const rules = structuredClone(currentRules());
    if (e.target.closest("[data-up]") && i > 0) { [rules[i - 1], rules[i]] = [rules[i], rules[i - 1]]; saveRules(rules); }
    else if (e.target.closest("[data-down]") && i < rules.length - 1) { [rules[i + 1], rules[i]] = [rules[i], rules[i + 1]]; saveRules(rules); }
    else if (e.target.closest("[data-edit]")) openBuilder(rules[i], false);
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
    if (rule.then.type === "preset" && !S.presets.some((p) => p.id === rule.then.preset)) rule.then.preset = S.presets[0]?.id;
    if (s.needsInput) openBuilder(rule, true);
    else saveRules([...currentRules(), rule], `Added: ${s.title}`);
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
