import { $, S, curveLabel, esc, rangeFill, setHTML, unitF } from "./core.js";
import { closeSheet, openSheet } from "./sheet.js";
import { demand, displayTemp, largestGapPoint, MAX_POINTS, MAX_TEMP, MIN_TEMP, parseRows, pointRows, targetLine } from "./curve-sandbox-model.js";

// Scratch curves stay in this app window while the user moves between the sandbox and the live editor.
const scratch = new Map();
const equal = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const clone = points => points.map(p => [...p]);
const percent = v => Number.isFinite(v) ? `${Number(v.toFixed(2)).toLocaleString()}%` : "Unavailable";
const xy = ([t, d]) => `${((t - MIN_TEMP) / (MAX_TEMP - MIN_TEMP) * 600).toFixed(2)},${(140 - d * 1.3).toFixed(2)}`;

export function openCurveSandbox({ name, editorDraft, review }) {
  const curves = S.curves?.curves, settings = S.settings?.settings || {};
  if (!curves?.[name]) throw new Error("Saved curves are unavailable.");
  const { min_duty: floor, safety_temp: safety } = S.curves;
  const failsafe = settings.fan_failsafe_duty, maxStep = settings.fan_max_step;
  const fahrenheit = unitF(), degree = fahrenheit ? "°F" : "°C";
  const display = t => `${displayTemp(t, fahrenheit).toLocaleString()}${degree}`;
  const modes = { normal: "Temperature available", missing: "Temperature missing", hot: "Another part is too hot" };
  const names = Object.keys(curves);
  for (const key of scratch.keys()) if (!names.includes(key)) scratch.delete(key);
  function draftFor(id) {
    const base = curves[id], editor = id === name && editorDraft && !equal(editorDraft, base) ? editorDraft : null;
    let entry = scratch.get(id);
    if (entry) {
      const clean = parseRows(entry.rows, floor, entry.fahrenheit).points;
      // A clean scratchpad follows updated saved curves automatically. Unfinished input stays available.
      if ((!equal(base, entry.base) && clean && (equal(clean, entry.base) || equal(clean, base))) || (editor && !equal(editor, entry.editor))) entry = null;
    }
    if (!entry) {
      entry = { base: clone(base), rows: pointRows(editor || base, fahrenheit), fahrenheit, editor: editor ? clone(editor) : null };
      scratch.set(id, entry);
    } else if (entry.fahrenheit !== fahrenheit) {
      entry.rows = entry.rows.map(([t, d]) => {
        if (!t.trim() || !Number.isFinite(Number(t))) return ["", d];
        const c = entry.fahrenheit ? (Number(t) - 32) / 1.8 : Number(t);
        return [String(Math.round(displayTemp(c, fahrenheit) * 10) / 10), d];
      });
      entry.fahrenheit = fahrenheit;
    }
    return entry;
  }
  let selected = name, draft = draftFor(selected), compare = name, mode = "normal", temperature = 60, timer = null, paused = false;
  let previous = Number.isFinite(S.state?.fans?.duty) ? S.state.fans.duty : null;
  const options = names.map(n => `<option value="${esc(n)}">${esc(curveLabel(n))}</option>`).join("");
  const sheet = openSheet(`<div class="body curve-sandbox">
    <div class="sheet-head"><div><h2>Curve sandbox</h2><p>Edit a draft and sweep a temperature through it. Fan settings stay live in the main editor.</p></div><button type="button" class="icon-btn" data-close aria-label="Close curve sandbox"><svg aria-hidden="true"><use href="#i-close"/></svg></button></div>
    <div class="sb-compare"><label>Draft curve<select data-sb-draft>${options}</select></label>
      <label>Compare with<select data-sb-comparison>${options}</select></label></div>
    <figure class="sb-figure"><figcaption><div><h3>Target duty</h3><span data-sb-caption></span></div><div class="sb-key"><span data-sb-key-a></span><span data-sb-key-b></span></div></figcaption>
      <div class="sb-graph" data-sb-graph></div>
      <div class="sb-sweep"><div><label for="sb-temperature">Simulated temperature</label> <output for="sb-temperature" data-sb-temperature></output></div>
        <input type="range" id="sb-temperature" min="${MIN_TEMP}" max="${MAX_TEMP}" step="1" value="${temperature}"><button type="button" class="btn quiet" data-sb-play>Run sweep</button></div>
      <dl class="sb-readouts"><div><dt>Draft target</dt><dd data-sb-target></dd></div><div><dt>Saved target</dt><dd data-sb-saved></dd></div><div><dt>Next draft command</dt><dd data-sb-command></dd></div></dl>
    </figure>
    <section class="sb-scenario"><div><h3>Control conditions</h3><p data-sb-ramp></p></div><div class="sb-scenario-inputs">
      <label>Temperature state<select data-sb-mode>${Object.entries(modes).map(([k,v]) => `<option value="${k}">${v}</option>`).join("")}</select></label>
      <label>Previous command (%)<input data-sb-previous type="number" inputmode="numeric" min="0" max="100" step="1" value="${previous ?? ""}" placeholder="No prior command"></label></div>
      <p class="sb-note" data-sb-policy></p></section>
    <section class="sb-points"><div class="sb-point-head"><div><h3>Draft points</h3><p data-sb-origin></p></div><button type="button" class="btn quiet" data-sb-add>Add point</button></div>
      <div class="sb-point-labels" aria-hidden="true"><span>Temperature (${degree})</span><span>Duty (%)</span></div><div data-sb-rows></div>
      <p class="sb-note" data-sb-round>${fahrenheit ? "Curve temperatures are stored as whole °C. Fahrenheit entries round to the nearest 1.8°F step." : "Curve temperatures are stored as whole degrees Celsius."}</p>
      <p class="sb-error" data-sb-error role="status"></p>
      <div class="sb-point-actions"><button type="button" class="btn quiet" data-sb-reset>Restore saved points</button><button type="button" class="btn primary" data-sb-review>Review draft in editor</button></div></section>
    </div>`);
  const root = $(".curve-sandbox", sheet), slider = $("#sb-temperature", root);
  const pick = $("[data-sb-draft]", root), other = $("[data-sb-comparison]", root);
  const rowList = $("[data-sb-rows]", root), previousInput = $("[data-sb-previous]", root);
  pick.value = selected; other.value = compare;
  const active = () => sheet.open && root.isConnected;
  function renderRows() {
    setHTML(rowList, draft.rows.map(([t, d], i) => `<div class="sb-point-row" data-row="${i}">
      <input type="number" data-sb-cell="0" aria-label="Point ${i + 1} temperature in ${degree}" min="${displayTemp(MIN_TEMP, fahrenheit)}" max="${displayTemp(MAX_TEMP, fahrenheit)}" step="any" value="${esc(t)}">
      <input type="number" data-sb-cell="1" aria-label="Point ${i + 1} duty in percent" min="${floor}" max="100" step="1" value="${esc(d)}">
      <button type="button" data-sb-remove aria-label="Remove point ${i + 1}" title="Remove point"${draft.rows.length <= 2 ? " disabled" : ""}><svg aria-hidden="true"><use href="#i-close"/></svg></button></div>`).join(""));
  }
  function drawGraph(points, saved, sample) {
    const hot = mode === "hot", threshold = (safety - MIN_TEMP) * 600 / (MAX_TEMP - MIN_TEMP);
    const line = (pts, cls) => pts?.length ? `<polyline class="sb-line ${cls}" points="${targetLine(pts, safety, hot).map(xy).join(" ")}"/>` : "";
    const x = (temperature - MIN_TEMP) * 600 / (MAX_TEMP - MIN_TEMP);
    const marker = mode === "missing" ? "" : `<line class="sb-cursor" x1="${x}" x2="${x}" y1="6" y2="145"/>
      ${Number.isFinite(sample.target) ? `<circle class="sb-sample" cx="${x}" cy="${140 - sample.target * 1.3}" r="3.5"/>` : ""}`;
    setHTML($("[data-sb-graph]", root), `<div class="sb-y" aria-hidden="true"><span>100%</span><span>50%</span><span>0%</span></div>
      <svg viewBox="-4 0 608 150" preserveAspectRatio="none" role="img" aria-label="${esc(curveLabel(selected))} draft and saved ${esc(curveLabel(compare))} target duty by temperature. Full speed at ${display(safety)}.">
        <rect class="sb-hot-band" x="${threshold}" y="6" width="${600 - threshold}" height="137"/>
        <path class="sb-grid" d="M0 10H600M0 75H600M0 140H600"/>${line(points, "sb-draft-line")}${line(saved, "sb-saved-line")}${marker}</svg>
      <div class="sb-x" aria-hidden="true"><span>${display(MIN_TEMP)}</span><span>${display(40)}</span><span>${display(60)}</span><span>${display(80)}</span><span>${display(MAX_TEMP)}</span></div>`);
  }
  function paint() {
    const { points, error } = parseRows(draft.rows, floor, fahrenheit);
    const saved = curves[compare];
    const scenario = { failsafe, safety, previous, maxStep, hot: mode === "hot" };
    const t = mode === "missing" ? null : temperature;
    const a = demand(points, t, scenario), b = demand(saved, t, scenario);
    const previousInvalid = previousInput.validity.badInput || (previousInput.value !== "" && (!Number.isInteger(Number(previousInput.value)) || Number(previousInput.value) < 0 || Number(previousInput.value) > 100));
    $("[data-sb-target]", root).textContent = previousInvalid && t === null ? "Unavailable" : percent(a.target);
    $("[data-sb-saved]", root).textContent = previousInvalid && t === null ? "Unavailable" : percent(b.target);
    $("[data-sb-command]", root).textContent = previousInvalid ? "Unavailable" : percent(a.next);
    $("[data-sb-key-a]", root).textContent = `${curveLabel(selected)} · draft`;
    $("[data-sb-key-b]", root).textContent = `${curveLabel(compare)} · saved`;
    $("[data-sb-temperature]", root).textContent = mode === "missing" ? "Missing" : display(temperature);
    slider.value = temperature; slider.disabled = mode === "missing"; slider.setAttribute("aria-valuetext", display(temperature)); rangeFill(slider);
    $("[data-sb-play]", root).disabled = mode === "missing";
    $("[data-sb-play]", root).textContent = timer !== null ? "Pause sweep" : paused ? "Resume sweep" : "Run sweep";
    $("[data-sb-add]", root).disabled = !points || draft.rows.length >= MAX_POINTS;
    $("[data-sb-reset]", root).disabled = !!points && equal(points, curves[selected]);
    $("[data-sb-review]", root).disabled = !points;
    $("[data-sb-origin]", root).textContent = `${curveLabel(selected)} · ${draft.rows.length} of ${MAX_POINTS} points`;
    $("[data-sb-error]", root).textContent = error || (previousInvalid ? "Previous command must be a whole duty between 0 and 100% or empty." : "");
    $("[data-sb-caption]", root).textContent = mode === "hot" ? "Safety override across the sweep"
      : mode === "missing" ? "Curve reference · temperature unavailable" : `Full speed at ${display(safety)}`;
    $("[data-sb-ramp]", root).textContent = Number.isFinite(maxStep) ? `Configured ramp limit · ${maxStep} percentage points per control cycle` : "Ramp setting unavailable";
    $("[data-sb-policy]", root).textContent = mode === "missing"
      ? `With no temperature, the controller keeps the higher of the previous command and the ${Number.isFinite(failsafe) ? failsafe + "% " : ""}failsafe duty.`
      : mode === "hot" ? `A CPU, GPU or GPU hotspot reading at or above ${display(safety)} sends a full-speed command immediately.`
      : "The target follows the curve. Each next command starts from the entered previous command. Full speed skips the ramp.";
    drawGraph(points, saved, a);
  }
  function stop() { if (timer !== null) clearInterval(timer); timer = null; paused = false; }
  function choose() { stop(); selected = pick.value; draft = draftFor(selected); renderRows(); paint(); }
  pick.addEventListener("change", choose);
  other.addEventListener("change", () => { compare = other.value; paint(); });
  slider.addEventListener("input", () => { stop(); temperature = Number(slider.value); paint(); });
  $("[data-sb-mode]", root).addEventListener("change", e => { stop(); mode = e.target.value; paint(); });
  previousInput.addEventListener("input", () => { previous = previousInput.value === "" ? null : Number(previousInput.value); paint(); });
  rowList.addEventListener("input", e => {
    const input = e.target.closest("[data-sb-cell]");
    if (!input) return;
    draft.rows[Number(input.closest("[data-row]").dataset.row)][Number(input.dataset.sbCell)] = input.value;
    paint();
  });
  root.addEventListener("click", e => {
    const button = e.target.closest("button");
    if (!button || button.disabled) return;
    if (button.matches("[data-sb-play]")) {
      if (timer !== null) { stop(); paused = true; }
      else {
        if (!paused) temperature = MIN_TEMP;
        paused = false;
        timer = setInterval(() => {
          if (!active() || document.hidden) { stop(); paused = true; if (active()) paint(); return; }
          temperature = Math.min(MAX_TEMP, temperature + 1);
          if (temperature === MAX_TEMP) stop();
          paint();
        }, 180);
      }
      paint(); return;
    }
    if (button.matches("[data-sb-remove]")) {
      const i = Number(button.closest("[data-row]").dataset.row);
      draft.rows.splice(i, 1); renderRows(); paint();
      rowList.querySelector(`[data-row="${Math.min(i, draft.rows.length - 1)}"] input`)?.focus(); return;
    }
    if (button.matches("[data-sb-reset]")) {
      draft.base = clone(curves[selected]); draft.rows = pointRows(draft.base, fahrenheit);
      renderRows(); paint(); rowList.querySelector("input")?.focus(); return;
    }
    const { points } = parseRows(draft.rows, floor, fahrenheit);
    if (!points) return;
    if (button.matches("[data-sb-add]")) {
      const newPoint = largestGapPoint(points, temperature);
      const ordered = [...points, newPoint].sort((a,b) => a[0] - b[0]);
      draft.rows = pointRows(ordered, fahrenheit); renderRows(); paint();
      rowList.querySelector(`[data-row="${ordered.indexOf(newPoint)}"] input`)?.focus(); return;
    }
    if (button.matches("[data-sb-review]")) {
      // This handoff only changes the main editor's unsaved selection.
      stop(); closeSheet(); review(selected, points, draft.base);
      draft.editor = clone(points);
    }
  });
  sheet.addEventListener("close", stop, { once: true });
  renderRows(); paint();
}
