import { S, curveLabel, effectName, esc, post, unitF } from "./core.js";
import { openSheet } from "./sheet.js";

const days = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const timeValue = minutes => `${String(Math.floor(minutes / 60)).padStart(2, "0")}:${String(minutes % 60).padStart(2, "0")}`;
const minutesAt = value => value.split(":").reduce((h, m) => Number(h) * 60 + Number(m), 0);
const channels = { lighting: "lighting", fans: "cooling" };
const sceneText = scene => scene.on ? `${effectName(scene.effect)} · ${scene.brightness}%` : "Lights off";
const icon = name => `<svg aria-hidden="true"><use href="#i-${name}"/></svg>`;
const boundaryDays = (w, which) => w.days.map(d => (d + (which === "end" && w.end < w.start ? 1 : 0)) % 7).sort((a, b) => a - b);

function results(data, sentence, temperature) {
  const rank = Object.fromEntries(data.rows.map(row => [row.rule.id, row.rank]));
  const outcome = (key, label, value, base, symbol) => {
    const id = data.winners[key];
    return `<section class="rehearse-outcome" data-channel="${key}"><div>${icon(symbol)}<h3>${label}</h3></div>
      <strong>${esc(value)}</strong><span>${id ? `Rule #${rank[id]} wins` : "Base setting"}</span>
      <small>After all matches clear · ${esc(base)}</small></section>`;
  };
  return `<div class="rehearse-outcomes">${outcome("lighting", "Lighting", sceneText(data.scene), sceneText(data.base.scene), "bulb")}
    ${outcome("fans", "Cooling", curveLabel(data.curve), curveLabel(data.base.curve), "fan")}</div>
    ${data.rows.length ? `<h3>Rules in priority order</h3><ol class="rehearse-ranks">${data.rows.map(row => {
      const { rule } = row, wins = row.targets.filter(key => data.winners[key] === rule.id);
      const notes = [];
      if (!rule.enabled) notes.push("Switched off");
      else if (row.active) {
        if (!wins.length) notes.push("Matches");
        for (const key of row.targets) notes.push(data.winners[key] === rule.id ? `Wins ${channels[key]}` : `#${rank[data.winners[key]]} wins ${channels[key]}`);
        if (!row.targets.length) notes.push("The profile is unavailable");
      } else notes.push("No match");
      if (rule.enabled && rule.when.type === "temp_above") {
        notes.push(row.temperature === null ? "Reading unavailable" : `${temperature(row.temperature)} measured · threshold ${temperature(row.threshold)}`);
      }
      return `<li data-outcome="${wins.length ? "winner" : row.active ? "matched" : "inactive"}"><span class="rehearse-rank" aria-label="Priority ${row.rank}">${row.rank}</span>
        <div><p>${sentence(rule)}</p><small>${esc(notes.join(" · "))}</small></div></li>`;
    }).join("")}</ol>` : '<p class="hint">There are no saved rules. Both outputs follow their base settings.</p>'}`;
}

export function openRehearsal({ sentence, rules, waitForSaves }) {
  const now = new Date(), weekday = (now.getDay() + 6) % 7;
  const fahrenheit = unitF(), unit = fahrenheit ? "°F" : "°C";
  const convert = c => fahrenheit ? c * 9 / 5 + 32 : c;
  const tempText = c => `${Number(convert(c).toFixed(2))}${unit}`;
  const temperatures = [["cpu", "Processor"], ["gpu", "Graphics card"], ["liquid", "Coolant"]];
  const windows = rules.map((r, i) => ({ ...r, rank: i + 1 })).filter(r => r.when.type === "time");
  const apps = [...new Set(rules.filter(r => r.when.exe).map(r => r.when.exe))];
  const d = openSheet(`<div class="body rehearse"><header class="rehearse-head"><div><h2>Rehearse rules</h2>
      <p class="hint">Try a scenario against your saved rules. Results stay in this window.</p></div><button class="btn quiet" data-close>Close</button></header>
    <div class="rehearse-layout"><form class="rehearse-form"><fieldset><legend>Local time</legend>
      <div class="rehearse-pair"><label><span id="rehearse-day-label">Day</span><select class="select" name="weekday" aria-labelledby="rehearse-day-label">${days.map((d, i) => `<option value="${i}"${weekday === i ? " selected" : ""}>${d}</option>`).join("")}</select></label>
      <label>Time<input class="text" type="time" name="clock" value="${timeValue(now.getHours() * 60 + now.getMinutes())}" required></label></div>
      ${windows.length ? `<label><span id="rehearse-boundary-label">Schedule boundary</span><select class="select" name="boundary" aria-labelledby="rehearse-boundary-label"><option value="">Jump to a rule’s start or end</option>${windows.map((r, i) =>
        ["start", "end"].map(which => `<option value="${i}-${which}">#${r.rank} · ${which === "start" ? "Starts" : "Ends"} at ${r.when[which]} · ${boundaryDays(r.when, which).map(d => days[d].slice(0, 3)).join(", ")}</option>`).join("")).join("")}</select></label>` : ""}
      <div class="btn-row"><button class="btn quiet" type="button" data-minute="-1">−1 minute</button><button class="btn quiet" type="button" data-minute="1">+1 minute</button></div></fieldset>
      <fieldset><legend>PC activity</legend><label>Foreground app<input class="text" name="foreground" list="rehearse-apps" placeholder="e.g. game.exe" autocomplete="off"></label>
      <datalist id="rehearse-apps">${apps.map(a => `<option value="${esc(a)}"></option>`).join("")}</datalist>
      <label>Other running apps<input class="text" name="running" placeholder="game.exe, music.exe" autocomplete="off"></label>
      <div class="rehearse-pair"><label>Idle minutes<input class="text" name="idle" type="number" min="0" max="10080" step="any" value="0" required></label>
      <label class="rehearse-check"><input type="checkbox" name="locked"> PC locked</label></div></fieldset>
      <fieldset><legend>Temperatures (${unit})</legend><div class="rehearse-temps">${temperatures.map(([key, label]) => {
        const value = S.state?.telemetry_stale ? null : S.state?.sensors?.[key + "_temp"];
        return `<label>${label}<input class="text" name="${key}" type="number" step="any" placeholder="Unknown" value="${Number.isFinite(value) ? Number(convert(value).toFixed(3)) : ""}"></label>`;
      }).join("")}</div><label class="rehearse-check"><input type="checkbox" name="carry" disabled> Carry temperature matches from the last run</label></fieldset>
      <button class="btn primary" type="submit">Run rehearsal</button></form>
    <div class="rehearse-response"><p class="rehearse-status" role="status"></p><div class="rehearse-result"></div>
      <p class="hint rehearse-scope">These are rule decisions before power, lock and thermal protection. Pausing live automations has no effect on a rehearsal.</p>
      <button type="button" class="btn quiet rehearse-edit-inputs">Edit scenario</button></div></div></div>`);
  const root = d.querySelector(".rehearse"), form = root.querySelector("form"), f = form.elements;
  const result = root.querySelector(".rehearse-result"), status = root.querySelector("[role=status]");
  let revision = 0, previous = [];
  function edited() {
    revision++; result.hidden = true;
    status.textContent = "Inputs changed. Run the rehearsal to update the result.";
  }
  async function run(focusResult = false) {
    if (!form.reportValidity()) return;
    const id = ++revision;
    const readings = Object.fromEntries(temperatures.map(([k]) => {
      const v = f[k].valueAsNumber;
      return [k, !Number.isFinite(v) ? null : fahrenheit ? (v - 32) * 5 / 9 : v];
    }));
    const scenario = { weekday: Number(f.weekday.value), clock: f.clock.value, idle: f.idle.valueAsNumber,
      locked: f.locked.checked, foreground: f.foreground.value.trim(), running: f.running.value.split(",").map(n => n.trim()).filter(Boolean),
      previous: f.carry.checked ? previous : [], ...readings };
    status.textContent = "Checking the rules…"; result.hidden = true;
    const current = () => id === revision && root.isConnected && d.open;
    try {
      await waitForSaves();
      if (!current()) return;
      const data = await post("/api/automations/rehearse", scenario);
      if (!current()) return;
      previous = data.active; f.carry.disabled = false;
      result.innerHTML = results(data, sentence, tempText); result.hidden = false;
      status.textContent = `${days[scenario.weekday]} at ${scenario.clock} · ${previous.length} matching ${previous.length === 1 ? "rule" : "rules"}`;
      if (focusResult && window.matchMedia("(max-width: 700px)").matches) {
        status.tabIndex = -1; status.focus(); status.scrollIntoView({ block: "start" });
      }
    } catch (err) { if (current()) status.textContent = `Rehearsal failed. ${err.message}`; }
  }
  form.addEventListener("input", edited);
  form.addEventListener("submit", e => { e.preventDefault(); run(true); });
  root.querySelector(".rehearse-edit-inputs").addEventListener("click", () => f.weekday.focus());
  form.addEventListener("click", e => {
    const b = e.target.closest("[data-minute]");
    if (!b || !f.clock.value) return;
    const mins = minutesAt(f.clock.value) + Number(b.dataset.minute);
    f.weekday.value = String((Number(f.weekday.value) + Math.floor(mins / 1440) + 7) % 7);
    f.clock.value = timeValue((mins + 1440) % 1440); edited(); run();
  });
  f.boundary?.addEventListener("change", () => {
    if (!f.boundary.value) return;
    const [index, which] = f.boundary.value.split("-"), w = windows[Number(index)].when;
    const day = Number(f.weekday.value);
    const offset = Math.min(...boundaryDays(w, which).map(d => (d - day + 7) % 7));
    f.weekday.value = String((day + offset) % 7);
    f.clock.value = w[which]; f.boundary.value = ""; edited(); run();
  });
  run();
}
