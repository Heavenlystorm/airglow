import { $, esc, get, rangeFill, setHTML, unitF } from "./core.js";
import { R, loadCatalog } from "./readings.js";
import { openCoolingWorkbench } from "./cooling-workbench.js";
import { displayScale, elapsedText, formatValue, graphBounds, nearestRow, sharedTimes, signals, stepTime, traceFor } from "./session-traces.js";
import { liveMarkerMarkup, mountNotebook, notebookMarkup } from "./session-notebook.js";
import { chartReport, reportPng } from "./session-report.js";
import { saveLocalFile } from "./save-local-file.js";

const picker = `<details class="session-picker"><summary>Additional sensors <span data-session-count></span></summary>
  <div class="session-picker-body"><label for="session-catalog">Sensor catalog</label><div class="session-add-row">
    <select id="session-catalog" aria-label="Sensor to add"></select><button class="btn" type="button" data-session-add>Add sensor</button></div>
    <ul class="session-selected" data-session-selected aria-label="Additional sensors"></ul>
    <p class="hint" data-session-picker-note></p></div></details>`;

const analysis = `<div class="session-analysis"><div class="session-chart-tools">
  <label>Reading<select data-session-signal></select></label>
  <label>Overlay a saved recording<select data-session-overlay><option value="">No comparison</option></select></label>
  </div><p class="hint" data-session-overlay-state role="status"></p>
  <figure class="session-figure"><figcaption><div><h3 data-session-signal-name>Recorded readings</h3><span data-session-source></span></div>
    <div class="session-legend" data-session-legend></div></figcaption>
    <div data-session-plot class="session-plot"></div><div data-session-event-now class="session-event-now" hidden></div></figure>
  <div class="session-inspect" data-session-inspect hidden><div class="session-scrub">
    <label for="session-time">Inspect elapsed time</label><output for="session-time" data-session-time></output>
    <input type="range" id="session-time" aria-label="Inspect elapsed time" aria-description="Arrow keys visit readings and event markers. Page Up and Page Down move ten seconds." min="0" step="0.001">
    <button type="button" class="btn quiet" data-session-latest>Latest time</button></div><div data-session-probes class="session-probes"></div></div>
  <div data-session-stats class="session-stats"></div>
  <div class="session-report-tools" data-report-tools hidden><button type="button" class="btn" data-report-save>Save chart PNG</button>
    <label><input type="checkbox" data-report-markers> Include event labels</label>
    <span role="status" data-report-status></span></div></div>`;

const runName = run => run?.name || run?.annotation || "Untitled workload";
const runLabel = run => `${runName(run)} · ${new Date(run.started_at * 1000).toLocaleString()}`;
const svgLine = (x, y) => `${x.toFixed(2)} ${y.toFixed(2)}`;

function capturePicker(root, requested, current) {
  const selected = new Set(), select = $("#session-catalog", root), add = $("[data-session-add]", root);
  const note = $("[data-session-picker-note]", root), list = $("[data-session-selected]", root);
  let catalog = {}, active = null, locked = false, unavailable = false, initialized = false;
  let removed = [];
  function draw() {
    const entries = Object.values(catalog).filter(m => !selected.has(m.id));
    const before = select.value;
    setHTML(select, entries.length ? entries.map(m => `<option value="${esc(m.id)}">${esc(m.name)} · ${esc(m.group)}${m.unit ? " · " + esc(m.unit) : ""}</option>`).join("")
      : `<option value="">${unavailable ? "Sensor catalog unavailable" : "No additional sensors available"}</option>`);
    if (entries.some(m => m.id === before)) select.value = before;
    select.disabled = add.disabled = locked || !entries.length || selected.size >= 6;
    const fields = active ? active.sensors || [] : [...selected].map(id => ({ id, name: catalog[id]?.name || id }));
    setHTML(list, fields.map(f => `<li><span>${esc(f.name)}</span><button type="button" data-session-remove="${esc(f.id)}" aria-label="Remove ${esc(f.name)}"${locked ? " disabled" : ""}>×</button></li>`).join(""));
    $("[data-session-count]", root).textContent = `${fields.length} / 6`;
    note.textContent = unavailable ? "Sensor catalog unavailable. The five cooling readings can still be recorded."
      : removed.length ? "No longer available for a new recording: " + removed.join(", ") + "."
      : "CPU, GPU, coolant, commanded curve duty and mean measured fan speed are included in every recording.";
  }
  function ready() {
    if (!current() || !R.loaded || (initialized && catalog === R.meta)) return;
    const previous = catalog;
    catalog = R.meta; unavailable = false;
    const missing = [...selected].filter(id => !catalog[id]);
    if (missing.length) {
      removed = missing.map(id => previous[id]?.name || id);
      for (const id of missing) selected.delete(id);
    }
    if (!initialized) {
      for (const id of requested) if (catalog[id] && selected.size < 6) selected.add(id);
      initialized = true;
    }
    draw();
  }
  if (R.loaded) ready();
  else { draw(); loadCatalog().then(ready).catch(() => { if (current()) { unavailable = true; draw(); } }); }
  add.addEventListener("click", () => {
    if (locked || selected.size >= 6 || !catalog[select.value]) return;
    removed = []; selected.add(select.value); draw();
    if (select.disabled) list.querySelector("li:last-child button")?.focus();
    else select.focus();
  });
  list.addEventListener("click", e => {
    const button = e.target.closest("[data-session-remove]");
    if (!button || locked) return;
    removed = []; selected.delete(button.dataset.sessionRemove); draw(); select.focus();
  });
  return { selection() { ready(); return [...selected]; }, lock(value, run) {
    ready();
    if (locked !== value || active?.id !== run?.id) { locked = value; active = run; draw(); }
  } };
}

function sessionInspector(root, current, requested, refresh) {
  const recording = capturePicker(root, requested, current);
  const signal = $("[data-session-signal]", root), overlay = $("[data-session-overlay]", root);
  const slider = $("#session-time", root), figure = $("[data-session-plot]", root);
  const inspect = $("[data-session-inspect]", root), overlayState = $("[data-session-overlay-state]", root);
  let first = null, second = null, firstId = "", request = 0, held = null, model = null, preferredId = requested[0];
  let loadingOverlay = false, fahrenheit = unitF();
  const notebook = mountNotebook(root, {current, refresh, seek(time) { held = time; scrub(); }});
  const reportSave = $("[data-report-save]", root), reportMarkers = $("[data-report-markers]", root);
  const reportStatus = $("[data-report-status]", root);
  let exporting = false, reportSelection = "";
  function reportControls() {
    $("[data-report-tools]", root).hidden = !first;
    reportSave.disabled = exporting || loadingOverlay || !model?.bounds;
    reportMarkers.disabled = !model?.events.length;
    reportSave.title = model?.bounds ? "Save this reading as a local chart image" : "Choose a reading with measured samples.";
  }
  reportSave.addEventListener("click", async () => {
    if (reportSave.disabled) return;
    const selection = reportSelection;
    const snapshot = chartReport(model, [first, second], reportMarkers.checked);
    exporting = true; reportStatus.textContent = ""; reportControls();
    try {
      const result = await saveLocalFile({ filename: second ? "airglow-recording-comparison.png" : "airglow-recording.png",
        mime: "image/png", description: "Airglow telemetry chart", data: () => reportPng(snapshot) });
      if (current() && selection === reportSelection) reportStatus.textContent = result === "saved" ? "Chart saved." : result === "downloaded" ? "Chart download started." : "";
    } catch (error) {
      if (current()) reportStatus.textContent = error.message || "The chart could not be saved.";
    } finally { exporting = false; if (current()) reportControls(); }
  });
  const display = signal => formatValue(signal, model.transform);
  const x = t => t / (model.duration || 1) * 600;
  const y = value => 136 - (value * model.transform.scale + model.transform.offset - model.bounds.lo) / (model.bounds.hi - model.bounds.lo) * 128;
  function drawPlot() {
    if (!model.bounds && !model.events.length && !(first && model.duration > 0)) {
      setHTML(figure, `<p class="session-empty">${!first ? (firstId ? "Loading recording…" : "Start a recording during a workload to see its readings here.") : "No measurements for this reading in the selected recordings."}</p>`);
      return;
    }
    const { lo, hi } = model.bounds || {};
    const axisLabel = n => formatValue((n - model.transform.offset) / model.transform.scale, model.transform);
    const lines = model.traces.map((t, index) => t.segments.map(segment => {
      const coordinates = segment.map(row => svgLine(x(row.time), y(row.value)));
      const path = coordinates.length > 1 ? coordinates.join(" L") : `${coordinates[0]} l0.001 0`;
      return `<path class="session-line run-${index}" d="M${path}"/>`;
    }).join("")).join("");
    const markers = model.events.map(({run, marker, index}) => {
      const at = x(marker.elapsed), top = index ? 11 : 1;
      return `<path class="session-event-mark run-${index}" data-event-key="${esc(JSON.stringify([run.id, marker.id]))}" data-event-elapsed="${marker.elapsed}" d="M${at.toFixed(3)} ${top}v9"><title>${esc(marker.label)} · ${elapsedText(marker.elapsed, true)}</title></path>`;
    }).join("");
    setHTML(figure, `${model.bounds ? "" : '<p class="hint">No measurements for this reading.</p>'}<div class="session-chart-layout"><div class="session-y-axis" aria-hidden="true">${model.bounds ? `<span>${esc(axisLabel(hi))}</span><span>${esc(axisLabel((hi + lo) / 2))}</span><span>${esc(axisLabel(lo))}</span>` : ""}</div>
      <svg viewBox="-3 0 606 144" preserveAspectRatio="none" role="img" aria-label="${esc(model.metric.name)} by elapsed recording time. Gaps indicate unavailable readings. ${model.events.length} event markers.">
        <path class="session-grid" d="M0 8H600M0 72H600M0 136H600"/>${lines}<line class="session-crosshair" y1="4" y2="140" visibility="hidden"/>${markers}</svg>
      <div class="session-x-axis" aria-hidden="true"><span>0:00</span><span>Elapsed time</span><span>${elapsedText(model.duration, model.duration < 1)}</span></div></div>`);
  }
  function scrub() {
    const times = model?.times || [];
    inspect.hidden = !first || (!times.length && !model.duration);
    const eventInfo = $("[data-session-event-now]", root);
    if (inspect.hidden) { eventInfo.hidden = true; notebook.setTime(null); return; }
    const time = held ?? times.at(-1) ?? 0, label = elapsedText(time, true);
    const exactEvents = model.events.filter(e => Math.abs(e.marker.elapsed - time) < .0001);
    eventInfo.hidden = !exactEvents.length;
    if (exactEvents.length) {
      const labels = exactEvents.slice(0, 2).map(e => `<span class="run-${e.index}"><b>${e.index ? "B" : "A"}</b> ${esc(e.marker.label)}</span>`);
      if (exactEvents.length > 2) labels.push(`<span>+${exactEvents.length - 2} more</span>`);
      setHTML(eventInfo, labels.join(""));
    }
    notebook.setTime(time);
    slider.max = model.duration; slider.value = time; slider.disabled = !model.duration;
    if (model.duration) rangeFill(slider); else slider.style.setProperty("--fill", "0%");
    slider.setAttribute("aria-valuetext", label);
    $("[data-session-time]", root).textContent = label;
    $("[data-session-latest]", root).disabled = held === null;
    const marker = figure.querySelector(".session-crosshair");
    if (marker) { marker.setAttribute("x1", x(time)); marker.setAttribute("x2", x(time)); marker.setAttribute("visibility", "visible"); }
    setHTML($("[data-session-probes]", root), model.traces.map((t, i) => {
      const row = time <= t.duration && t.metric ? nearestRow(t.rows, time) : null;
      const reading = !t.metric ? "Not recorded" : !row ? "No sample nearby" : row.value === null ? "Missing reading" : display(row.value);
      return `<span class="run-${i}"><b>${i ? "B" : "A"}</b> ${esc(reading)}${row ? " · " + elapsedText(row.time, true) : ""}</span>`;
    }).join(""));
  }
  function drawStats() {
    const { traces, transform } = model;
    const metrics = [["Low", t => t.low?.value], ["Mean", t => t.mean], ["Peak", t => t.high?.value]];
    const cells = values => values.map(v => `<td>${esc(v)}</td>`).join("");
    const rows = metrics.map(([label, f]) => {
      const values = traces.map(f), a = values[0], b = values[1];
      const fields = values.map(display);
      if (second) fields.push(Number.isFinite(a) && Number.isFinite(b) ? formatValue(b - a, transform, true) : "—");
      return `<tr><th scope="row">${label}</th>${cells(fields)}</tr>`;
    });
    const coverage = traces.map(t => t.metric ? `${t.count} / ${t.rows.length}` : "Not recorded");
    if (second) coverage.push("—");
    rows.push(`<tr><th scope="row">Samples with a reading</th>${cells(coverage)}</tr>`);
    setHTML($("[data-session-stats]", root), first ? `<table><caption class="sr-only">Summary for ${esc(model.metric.name)}</caption><thead><tr><th scope="col">Reading summary</th><th scope="col">Run A</th>${second ? '<th scope="col">Run B</th><th scope="col">B minus A</th>' : ""}</tr></thead><tbody>${rows.join("")}</tbody></table>` : "");
  }
  function draw() {
    const combined = new Map([...signals(first), ...signals(first && second)].map(metric => [metric.key, metric]));
    const prior = signal.value;
    setHTML(signal, combined.size ? [...combined.values()].map(m => `<option value="${esc(m.key)}">${esc(m.name)}${m.group !== "Cooling" && m.group ? " · " + esc(m.group) : ""}</option>`).join("") : '<option value="">No recording selected</option>');
    if (combined.has(prior)) signal.value = prior;
    else if (preferredId) {
      const match = [...combined.values()].find(m => m.id === preferredId);
      if (match) signal.value = match.key;
    }
    signal.disabled = !combined.size;
    const metric = combined.get(signal.value) || { name: "Recorded readings", group: "", unit: "" };
    const traces = [traceFor(first, metric.key)];
    if (first && second) traces.push(traceFor(second, metric.key));
    const transform = displayScale(metric.unit, traces, unitF());
    const events = [first, first && second].flatMap((run, index) => (run?.markers || []).map(marker => ({run, index, marker})));
    const times = [...new Set([...sharedTimes(traces), ...events.map(e => e.marker.elapsed)])].sort((a,b) => a - b);
    model = { metric, traces, transform, times, events, bounds: graphBounds(traces, transform), duration: Math.max(...traces.map(t => t.duration)) };
    model.stops = [...new Set([0, ...times, model.duration])].sort((a,b) => a - b);
    const selection = JSON.stringify([first?.id, second?.id, metric.key, transform.unit]);
    if (reportSelection !== selection) {
      reportMarkers.checked = false; reportStatus.textContent = ""; reportSelection = selection;
    }
    reportControls();
    if (held !== null && held > model.duration) held = null;
    notebook.updateRuns(first, first && second);
    $("[data-session-signal-name]", root).textContent = metric.name;
    $("[data-session-source]", root).textContent = metric.group;
    setHTML($("[data-session-legend]", root), [first, first && second].filter(Boolean).map((run, i) => `<span class="run-${i}" title="${esc(runLabel(run))}"><b>${i ? "B" : "A"}</b> ${esc(runName(run))}</span>`).join(""));
    drawPlot(); drawStats(); scrub();
  }
  async function loadOverlay() {
    const serial = ++request, id = overlay.value;
    loadingOverlay = !!id;
    second = null; held = null;
    overlayState.textContent = id ? "Loading comparison…" : "";
    draw();
    if (!id) return;
    try {
      const data = await get(`/api/cooling/trace?id=${encodeURIComponent(id)}`);
      if (!current() || serial !== request) return;
      second = data; overlayState.textContent = ""; draw();
    } catch (error) {
      if (!current() || serial !== request) return;
      overlay.value = ""; overlayState.textContent = `Comparison unavailable. ${error.message}`;
    } finally {
      if (serial === request) { loadingOverlay = false; if (current()) reportControls(); }
    }
  }
  overlay.addEventListener("change", loadOverlay);
  signal.addEventListener("change", () => { preferredId = null; held = null; draw(); });
  slider.addEventListener("input", () => { held = Number(slider.value); scrub(); });
  slider.addEventListener("keydown", e => {
    if (!model || e.ctrlKey || e.altKey || e.metaKey) return;
    const t = Number(slider.value);
    if (e.key === "Home") held = 0;
    else if (e.key === "End") held = model.duration;
    else if (e.key === "PageUp" || e.key === "PageDown") held = Math.max(0, Math.min(model.duration, t + (e.key === "PageUp" ? 10 : -10)));
    else if (["ArrowLeft", "ArrowDown", "ArrowRight", "ArrowUp"].includes(e.key)) held = stepTime(model.stops, t, ["ArrowLeft", "ArrowDown"].includes(e.key) ? -1 : 1);
    else return;
    e.preventDefault(); scrub();
  });
  figure.addEventListener("click", e => {
    const key = e.target.closest("[data-event-key]")?.dataset.eventKey;
    if (key) notebook.selectMarker(key);
  });
  $("[data-session-latest]", root).addEventListener("click", () => { held = null; scrub(); slider.focus(); });
  return {
    ...recording,
    recorder: notebook.setRecorder,
    options(saved, selectedId) {
      notebook.retain(saved.map(s => s.id));
      const previous = overlay.value, options = saved.filter(s => s.id !== selectedId);
      const keep = options.some(s => s.id === previous);
      setHTML(overlay, '<option value="">No comparison</option>' + options.map(s => `<option value="${esc(s.id)}">${esc(runLabel(s))}</option>`).join(""));
      overlay.value = keep ? previous : "";
      overlay.disabled = !selectedId || !options.length;
      if (previous && !keep) loadOverlay();
      else if (keep && !loadingOverlay && second && (second.metadata_revision || 0) !== (saved.find(s => s.id === previous)?.metadata_revision || 0)) loadOverlay();
      if (selectedId !== firstId) { first = null; held = null; firstId = selectedId; draw(); }
      else if (fahrenheit !== unitF()) { fahrenheit = unitF(); draw(); }
    },
    async load(id, previous) {
      const old = previous?.id === id ? previous.samples : [];
      const data = await get(`/api/cooling/trace?id=${encodeURIComponent(id)}&after=${old.length}`);
      if (data.offset !== old.length || data.offset + data.samples.length !== data.sample_count) throw new Error("Recording changed while loading. Refresh to try again.");
      return { ...data, samples: [...old, ...data.samples] };
    },
    show(data) { first = data; draw(); },
  };
}

export function openSessionLab({ sensorIds = [] } = {}) {
  return openCoolingWorkbench({ picker, analysis, notebook: notebookMarkup, liveMarker: liveMarkerMarkup,
    attach: (root, current, refresh) => sessionInspector(root, current, sensorIds, refresh) });
}
