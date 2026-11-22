// Local opt-in recordings and target comparisons. No operation here applies a fan curve.
import { $, S, esc, get, post, setHTML } from "./core.js";
import { openSheet } from "./sheet.js";

export function parseDraftPoints(text) {
  const lines = text.trim().split(/\n|;/).filter((line) => line.trim());
  if (lines.length < 2 || lines.length > 8) throw new Error("Enter at least two and at most eight curve points");
  return lines.map((line) => {
    const match = line.trim().match(/^(\d+(?:\.\d+)?)\s*:\s*(\d+(?:\.\d+)?)$/);
    if (!match) throw new Error("Use temperature: duty on each line, for example 50: 45");
    return [Number(match[1]), Number(match[2])];
  });
}

export function seriesSegments(samples, key, duration, maximum) {
  const segments = []; let segment = [], previous = null;
  for (const sample of samples) {
    const value = sample[key];
    if (previous !== null && sample.elapsed - previous > 6 && segment.length) { segments.push(segment.join(" ")); segment = []; }
    previous = sample.elapsed;
    if (value == null || !Number.isFinite(value)) {
      if (segment.length) segments.push(segment.join(" "));
      segment = []; continue;
    }
    segment.push(`${Math.max(0, Math.min(600, sample.elapsed / Math.max(1, duration) * 600)).toFixed(2).replace(/\.00$/, "")},${(88 - Math.max(0, Math.min(1, value / maximum)) * 80).toFixed(2)}`);
  }
  if (segment.length) segments.push(segment.join(" "));
  return segments;
}

const valueText = (value, unit = "") => value == null ? "Missing" : `${Math.round(value * 10) / 10}${unit}`;

export function chartMarkup(session, replay = null) {
  const samples = session?.samples || [];
  if (!samples.length) return `<p class="wb-empty">No samples in this recording yet. Available readings are captured every two seconds.</p>`;
  const duration = samples.at(-1).elapsed;
  const rpms = samples.map((s) => s.rpm).filter(Number.isFinite);
  const rpmMax = Math.max(1000, Math.ceil(Math.max(0, ...rpms) / 500) * 500);
  const charts = [
    { label: "Temperature · °C", max: 110, series: [["cpu", "CPU", "cpu"], ["gpu", "GPU", "gpu"], ["liquid", "Coolant", "liquid"]] },
    { label: "Curve command · %", max: 100, series: [["duty", "Recorded", "duty"], ...(replay ? [["draft_target", "Draft target", "draft"]] : [])] },
    { label: "Measured RPM", max: rpmMax, series: [["rpm", "Mean of available channels", "rpm"]] },
  ];
  return charts.map((chart) => `<figure class="wb-chart"><figcaption><strong>${chart.label}</strong><span>0–${chart.max}</span></figcaption>
    <svg viewBox="-8 0 616 100" role="img" aria-label="${chart.label} over ${Math.round(duration)} seconds; gaps are missing readings">
    <path class="wb-chart-grid" d="M0 8H600M0 48H600M0 88H600"/>
    ${chart.series.map(([key, , color]) => {
      const points = key === "draft_target" ? replay.samples : samples;
      return seriesSegments(points, key, duration, chart.max).map((segment) => segment.includes(" ")
        ? `<polyline class="wb-line wb-line-${color}" points="${segment}"/>`
        : `<circle class="wb-dot wb-line-${color}" cx="${segment.split(",")[0]}" cy="${segment.split(",")[1]}" r="2.5"/>`).join("");
    }).join("")}</svg><div class="wb-chart-key">${chart.series.map(([, label, color]) => `<span class="wb-key-${color}">${label}</span>`).join("")}</div>
    <div class="wb-chart-time"><span>0:00</span><span>${Math.floor(duration / 60)}:${String(Math.floor(duration % 60)).padStart(2, "0")}</span></div></figure>`).join("");
}

function download(data) {
  const url = URL.createObjectURL(new Blob([data.content], { type: data.mime }));
  const link = document.createElement("a");
  link.href = url; link.download = data.filename; link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

export function openCoolingWorkbench(lab = null) {
  const title = lab ? "Telemetry sessions" : "Cooling workbench";
  const sheet = openSheet(`<div class="body workbench cooling-workbench${lab ? " session-lab" : ""}">
    <div class="sheet-head"><h2>${title}</h2><button class="icon-btn" data-close aria-label="Close ${title.toLowerCase()}"><svg><use href="#i-close"/></svg></button></div>
    <div class="wb-record"><label for="cooling-annotation">Workload / conditions</label><input id="cooling-annotation" maxlength="1000" placeholder="e.g. game benchmark, room 22°C">
      ${lab?.picker || ""}
      <div class="wb-section-head"><p data-recording-status role="status">Loading recordings…</p><div class="btn-row"><button class="btn primary" data-cooling-start disabled>Start recording</button><button class="btn" data-cooling-stop disabled>Stop and save</button></div></div>
      ${lab?.liveMarker || ""}
      <p class="hint">Records locally about every two seconds. Up to 1,800 samples per run and 12 saved runs. Recording continues when you close this panel.</p>
    </div>
    <p class="wb-error" data-cooling-error role="alert" hidden></p>
    <div class="wb-recovery" data-cooling-recovery hidden><p data-cooling-save-error></p><button class="btn" data-cooling-retry>Retry saving</button></div>
    ${lab?.notebook || ""}
    <section><div class="wb-session-tools"><label>Recording<select data-cooling-session aria-label="Recording"></select></label><button class="btn quiet" data-cooling-refresh>Refresh</button></div>
      <p class="hint" data-session-summary></p>${lab?.analysis || '<div data-cooling-chart><p class="wb-empty">Loading history…</p></div>'}
      <div class="btn-row"><button class="btn" data-cooling-export="json" disabled>Export JSON</button><button class="btn" data-cooling-export="csv" disabled>Export CSV</button>${lab ? '<button class="btn" data-cooling-export="timeline-csv" disabled>Timeline CSV</button>' : ""}<button class="btn danger" data-cooling-delete disabled>Delete recording</button></div>
      <div class="wb-delete" data-cooling-delete-confirm hidden><span>Delete this saved recording?</span><button class="btn danger" data-cooling-confirm-delete>Delete</button><button class="btn quiet" data-cooling-cancel-delete>Keep recording</button></div>
    </section>
    ${lab ? "" : `<section class="wb-compare"><h3>Compare recordings</h3><div class="wb-session-tools"><label>Compare with<select data-cooling-compare-select aria-label="Compare with"></select></label><button class="btn" data-cooling-compare disabled>Compare</button></div><div data-cooling-comparison></div><p class="hint">Workload and ambient conditions affect comparisons. RPM is not a noise measurement.</p><button class="btn quiet" type="button" data-open-session-lab>Inspect recording timelines</button></section>
    <section class="wb-draft"><h3>Replay draft targets</h3><p class="hint">Compare targets against recorded temperatures. This does not predict temperature response or apply settings.</p>
      <div class="wb-draft-fields"><label>Curve points · °C: duty %<textarea data-cooling-points rows="4" spellcheck="false" aria-label="Draft curve points">30: 30\n60: 55\n80: 100</textarea></label><label>Temperature source<select data-cooling-source><option value="max">Hotter of CPU / GPU</option><option value="cpu">CPU</option><option value="gpu">GPU</option><option value="liquid">Coolant (CPU / GPU fallback)</option></select></label></div>
      <div class="btn-row"><button class="btn" data-cooling-replay disabled>Replay draft</button><button class="btn quiet" data-cooling-use-curve>Use current curve</button></div><p class="hint" data-cooling-replay-note></p>
    </section>`}
  </div>`);
  const root = $(".cooling-workbench", sheet);
  const current = () => sheet.open && $(".cooling-workbench", sheet) === root;
  const inspector = lab?.attach(root, current, refresh);
  const select = $("[data-cooling-session]", root), compareSelect = $("[data-cooling-compare-select]", root);
  const errorBox = $("[data-cooling-error]", root);
  let state = null, session = null, replay = null, busy = false, disconnected = false, reading = null, timer, requestId = 0;
  let pendingDelete = null, annotationDraft = null;
  const showError = (error) => { if (current()) { errorBox.hidden = !error; errorBox.textContent = error?.message || ""; } };
  const chooseOptions = (element, sessions, fallback) => {
    const before = element.value;
    const html = sessions.length ? sessions.map((s) => `<option value="${esc(s.id)}">${esc(s.name || s.annotation || "Untitled workload")} · ${esc(new Date(s.started_at * 1000).toLocaleString())}${s.ended_reason === "interrupted" ? " · Interrupted" : ""}</option>`).join("") : `<option value="">No recordings yet</option>`;
    setHTML(element, html);
    element.value = sessions.some((s) => s.id === before) ? before : fallback || sessions[0]?.id || "";
  };
  function paintControls() {
    if (!state || !current()) return;
    const paused = busy || disconnected;
    $("[data-cooling-start]", root).disabled = paused || !!state.saving || !!state.active || !!state.load_error || !!state.save_error || state.sessions.length >= 12;
    $("[data-cooling-stop]", root).disabled = paused || !state.active;
    $("[data-cooling-retry]", root).disabled = paused || !!state.saving;
    $("#cooling-annotation", root).disabled = paused || !!state.active;
    inspector?.lock(paused || !!state.active, state.active);
    inspector?.recorder(state.active, paused, !disconnected);
    select.disabled = paused || !select.value;
    root.querySelectorAll("[data-cooling-export], [data-cooling-replay]").forEach((b) => { b.disabled = paused || !select.value; });
    $("[data-cooling-delete]", root).disabled = paused || !select.value || state.active?.id === select.value;
    if (!inspector) $("[data-cooling-compare]", root).disabled = paused || !select.value || !compareSelect.value || compareSelect.value === select.value;
    $("[data-cooling-confirm-delete]", root).disabled = paused;
  }
  async function loadSession() {
    const id = select.value, serial = ++requestId;
    if (!id) {
      session = null; $("[data-session-summary]", root).textContent = "";
      if (inspector) inspector.show(null);
      else setHTML($("[data-cooling-chart]", root), `<p class="wb-empty">No recordings yet. Start a recording during a workload to capture temperatures, duty and RPM.</p>`);
      return;
    }
    const data = inspector ? await inspector.load(id, session) : await get(`/api/cooling/session?id=${encodeURIComponent(id)}`);
    if (!current() || serial !== requestId || id !== select.value) return;
    session = data;
    const last = data.samples.at(-1);
    const reason = { stopped: "Finished", sample_limit: "Sample limit reached", interrupted: "Interrupted" }[data.ended_reason] || "Recording";
    $("[data-session-summary]", root).textContent = `${data.samples.length} samples · ${Math.round(data.duration_seconds ?? last?.elapsed ?? 0)}s · ${reason}${data.recovery_note ? ". " + data.recovery_note : ""}`;
    if (inspector) inspector.show(data);
    else setHTML($("[data-cooling-chart]", root), chartMarkup(data, replay));
  }
  async function readStatus() {
    try {
      const next = await get("/api/cooling/sessions");
      if (!current()) return;
      state = next;
      if (disconnected) showError(null);
      disconnected = false;
      root.dataset.recording = state.active ? "active" : "idle";
      const annotation = $("#cooling-annotation", root);
      if (state.active) {
        if (annotationDraft === null) annotationDraft = annotation.value;
        annotation.value = state.active.annotation || "";
      } else if (annotationDraft !== null) {
        annotation.value = annotationDraft;
        annotationDraft = null;
      }
      $("[data-recording-status]", root).textContent = state.active ? `Recording · ${state.active.sample_count} / 1,800 samples` : state.saving ? "Saving recording…" : state.sessions.length >= 12 ? "Storage full · Export and delete a run to record again" : `Ready to record · ${state.sessions.length} / 12 saved`;
      $("[data-cooling-recovery]", root).hidden = !state.save_error;
      $("[data-cooling-save-error]", root).textContent = state.save_error || "";
      if (state.load_error) showError(new Error(state.load_error));
      else if (state.sample_error) showError(new Error(`A sample could not be recorded: ${state.sample_error}. Fan control continues.`));
      const sessions = [...(state.active ? [state.active] : []), ...state.sessions];
      const before = select.value;
      chooseOptions(select, sessions, state.active?.id);
      if (pendingDelete && pendingDelete !== select.value) { pendingDelete = null; $("[data-cooling-delete-confirm]", root).hidden = true; }
      if (inspector) inspector.options(state.sessions, select.value);
      else {
        chooseOptions(compareSelect, state.sessions, state.sessions.find((s) => s.id !== select.value)?.id);
        if (before !== select.value) { replay = null; $("[data-cooling-replay-note]", root).textContent = ""; }
      }
      paintControls();
      const summary = sessions.find(s => s.id === select.value);
      if (!session || session.id !== select.value || state.active?.id === select.value || session.ended_reason !== summary?.ended_reason
          || (session.metadata_revision || 0) !== (summary?.metadata_revision || 0)) {
        await loadSession().catch(showError);
      }
    } catch (error) {
      if (!current()) return;
      disconnected = true; root.dataset.recording = "unknown"; paintControls();
      $("[data-recording-status]", root).textContent = "Recorder status unavailable · Reconnecting…";
      showError(error);
    }
    finally { clearTimeout(timer); if (current()) timer = setTimeout(refresh, 2000); }
  }
  function refresh(force = false) {
    if (!current()) return Promise.resolve();
    if (reading) return force ? reading.then(() => refresh(true)) : reading;
    if (busy && !force) return Promise.resolve();
    reading = readStatus().finally(() => { reading = null; });
    return reading;
  }
  async function action(fn) {
    if (busy) return;
    busy = true; showError(null); paintControls();
    try {
      if (reading) await reading;
      if (current()) await fn();
    }
    catch (error) { await refresh(true); showError(error); }
    finally {
      busy = false; paintControls(); clearTimeout(timer);
      if (current()) timer = setTimeout(refresh, 2000);
    }
  }
  select.addEventListener("change", () => {
    replay = null; session = null; $("[data-session-summary]", root).textContent = "";
    if (inspector) inspector.options(state.sessions, select.value);
    else { $("[data-cooling-comparison]", root).textContent = ""; $("[data-cooling-replay-note]", root).textContent = ""; }
    pendingDelete = null; $("[data-cooling-delete-confirm]", root).hidden = true;
    paintControls(); loadSession().catch(showError);
  });
  if (!inspector) compareSelect.addEventListener("change", () => { $("[data-cooling-comparison]", root).textContent = ""; paintControls(); });
  root.addEventListener("click", async (event) => {
    const target = event.target.closest("button");
    if (!target || target.disabled) return;
    if (inspector && target.closest(".session-analysis,.session-notebook,.session-live-marker,.session-picker")) return;
    if (target.matches("[data-open-session-lab]")) {
      try { const { openSessionLab } = await import("./session-lab.js"); if (current()) await openSessionLab(); }
      catch (error) { showError(error); }
      return;
    }
    if (target.matches("[data-cooling-refresh]")) { showError(null); await refresh(true); return; }
    if (target.matches("[data-cooling-delete]")) {
      pendingDelete = select.value; $("[data-cooling-delete-confirm]", root).hidden = false;
      $("[data-cooling-cancel-delete]", root).focus(); return;
    }
    if (target.matches("[data-cooling-cancel-delete]")) {
      pendingDelete = null; $("[data-cooling-delete-confirm]", root).hidden = true;
      $("[data-cooling-delete]", root).focus(); return;
    }
    if (target.matches("[data-cooling-use-curve]")) {
      await action(async () => {
        const curves = await get("/api/curves");
        const name = S.state?.fans?.curve || "balanced";
        const points = curves.curves?.[name] || session?.samples.at(-1)?.curve?.points;
        if (!points) throw new Error("Current curve is unavailable. Enter draft points directly.");
        if (current()) $("[data-cooling-points]", root).value = points.map((p) => p.join(": ")).join("\n");
      }); return;
    }
    const selectedId = select.value, activeId = state?.active?.id, confirmedId = pendingDelete;
    await action(async () => {
      if (target.matches("[data-cooling-start]")) {
        await post("/api/cooling/start", { annotation: $("#cooling-annotation", root).value, ...(inspector ? { sensor_ids: inspector.selection() } : {}) });
        select.value = ""; session = null; await refresh(true);
      } else if (target.matches("[data-cooling-stop]")) {
        await post("/api/cooling/stop", { id: activeId }); session = null; await refresh(true);
      } else if (target.matches("[data-cooling-retry]")) {
        await post("/api/cooling/retry-save", {}); await refresh(true);
      } else if (target.matches("[data-cooling-confirm-delete]")) {
        if (!confirmedId || confirmedId !== selectedId) return;
        await post("/api/cooling/delete", { id: confirmedId }); pendingDelete = null;
        $("[data-cooling-delete-confirm]", root).hidden = true; session = replay = null; await refresh(true);
      } else if (target.matches("[data-cooling-export]")) {
        download(await get(`/api/cooling/export?id=${encodeURIComponent(selectedId)}&format=${target.dataset.coolingExport}`));
      } else if (target.matches("[data-cooling-compare]")) {
        const first = select.value, second = compareSelect.value;
        const result = await post("/api/cooling/compare", { first, second });
        if (!current() || select.value !== first || compareSelect.value !== second) return;
        const rows = [["CPU mean", "cpu_mean", "°C"], ["GPU mean", "gpu_mean", "°C"], ["Coolant mean", "liquid_mean", "°C"], ["Curve duty mean", "duty_mean", "%"], ["Measured RPM mean", "rpm_mean", " RPM"]];
        setHTML($("[data-cooling-comparison]", root), `<div class="wb-table-scroll"><table class="wb-compare-table"><thead><tr><th>Reading</th><th>Selected</th><th>Comparison</th><th>Difference</th></tr></thead><tbody>${rows.map(([label, key, unit]) => `<tr><th scope="row">${label}</th><td>${valueText(result.first[key], unit)}</td><td>${valueText(result.second[key], unit)}</td><td>${result.delta[key] > 0 ? "+" : ""}${valueText(result.delta[key], unit)}</td></tr>`).join("")}</tbody></table></div>`);
      } else if (target.matches("[data-cooling-replay]")) {
        const id = select.value;
        const draft = $("[data-cooling-points]", root).value, source = $("[data-cooling-source]", root).value;
        const result = await post("/api/cooling/replay", { id, points: parseDraftPoints(draft), source });
        if (!current() || id !== select.value || draft !== $("[data-cooling-points]", root).value || source !== $("[data-cooling-source]", root).value) return;
        replay = result;
        setHTML($("[data-cooling-chart]", root), chartMarkup(session, replay));
        $("[data-cooling-replay-note]", root).textContent = result.note;
      }
    });
  });
  sheet.addEventListener("close", () => clearTimeout(timer), { once: true });
  return refresh();
}
