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
  const segments = []; let segment = [];
  for (const sample of samples) {
    const value = sample[key];
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

export function openCoolingWorkbench() {
  const sheet = openSheet(`<div class="body workbench cooling-workbench">
    <div class="sheet-head"><h2>Cooling workbench</h2><button class="icon-btn" data-close aria-label="Close cooling workbench"><svg><use href="#i-close"/></svg></button></div>
    <div class="wb-record"><label for="cooling-annotation">Workload / conditions</label><input id="cooling-annotation" maxlength="1000" placeholder="e.g. game benchmark, room 22°C">
      <div class="wb-section-head"><p data-recording-status role="status">Loading recordings…</p><div class="btn-row"><button class="btn primary" data-cooling-start disabled>Start recording</button><button class="btn" data-cooling-stop disabled>Stop and save</button></div></div>
      <p class="hint">Recording observes current settings. Up to 1,800 samples at two-second intervals; 12 saved sessions. Closing this window keeps recording.</p>
    </div>
    <p class="wb-error" data-cooling-error role="alert" hidden></p>
    <div class="wb-recovery" data-cooling-recovery hidden><p data-cooling-save-error></p><button class="btn" data-cooling-retry>Retry saving</button></div>
    <section><div class="wb-session-tools"><label>Recording<select data-cooling-session aria-label="Recording"></select></label><button class="btn quiet" data-cooling-refresh>Refresh</button></div>
      <p class="hint" data-session-summary></p><div data-cooling-chart><p class="wb-empty">Loading history…</p></div>
      <div class="btn-row"><button class="btn" data-cooling-export="json" disabled>Export JSON</button><button class="btn" data-cooling-export="csv" disabled>Export CSV</button><button class="btn danger" data-cooling-delete disabled>Delete recording</button></div>
      <div class="wb-delete" data-cooling-delete-confirm hidden><span>Delete this saved recording?</span><button class="btn danger" data-cooling-confirm-delete>Delete</button><button class="btn quiet" data-cooling-cancel-delete>Keep recording</button></div>
    </section>
    <section class="wb-compare"><h3>Compare recordings</h3><div class="wb-session-tools"><label>Compare with<select data-cooling-compare-select aria-label="Compare with"></select></label><button class="btn" data-cooling-compare disabled>Compare</button></div><div data-cooling-comparison></div><p class="hint">Workload and ambient conditions affect comparisons. RPM is not a noise measurement.</p></section>
    <section class="wb-draft"><h3>Replay draft targets</h3><p class="hint">Compare targets against recorded temperatures. This does not predict temperature response or apply settings.</p>
      <div class="wb-draft-fields"><label>Curve points · °C: duty %<textarea data-cooling-points rows="4" spellcheck="false" aria-label="Draft curve points">30: 30\n60: 55\n80: 100</textarea></label><label>Temperature source<select data-cooling-source><option value="max">Hotter of CPU / GPU</option><option value="cpu">CPU</option><option value="gpu">GPU</option><option value="liquid">Coolant (CPU / GPU fallback)</option></select></label></div>
      <div class="btn-row"><button class="btn" data-cooling-replay disabled>Replay draft</button><button class="btn quiet" data-cooling-use-curve>Use current curve</button></div><p class="hint" data-cooling-replay-note></p>
    </section>
  </div>`);
  const root = $(".cooling-workbench", sheet);
  const current = () => sheet.open && $(".cooling-workbench", sheet) === root;
  const select = $("[data-cooling-session]", root), compareSelect = $("[data-cooling-compare-select]", root);
  const errorBox = $("[data-cooling-error]", root);
  let state = null, session = null, replay = null, busy = false, reading = null, timer, requestId = 0;
  const showError = (error) => { if (current()) { errorBox.hidden = !error; errorBox.textContent = error?.message || ""; } };
  const chooseOptions = (element, sessions, fallback) => {
    const before = element.value;
    const html = sessions.length ? sessions.map((s) => `<option value="${esc(s.id)}">${esc(s.annotation || "Untitled workload")} · ${esc(new Date(s.started_at * 1000).toLocaleString())}${s.ended_reason === "interrupted" ? " · Interrupted" : ""}</option>`).join("") : `<option value="">No recordings yet</option>`;
    setHTML(element, html);
    element.value = sessions.some((s) => s.id === before) ? before : fallback || sessions[0]?.id || "";
  };
  function paintControls() {
    if (!state || !current()) return;
    $("[data-cooling-start]", root).disabled = busy || !!state.saving || !!state.active || !!state.load_error || !!state.save_error || state.sessions.length >= 12;
    $("[data-cooling-stop]", root).disabled = busy || !state.active;
    $("[data-cooling-retry]", root).disabled = busy || !!state.saving;
    $("#cooling-annotation", root).disabled = busy || !!state.active;
    root.querySelectorAll("[data-cooling-export], [data-cooling-replay]").forEach((b) => { b.disabled = busy || !select.value; });
    $("[data-cooling-delete]", root).disabled = busy || !select.value || state.active?.id === select.value;
    $("[data-cooling-compare]", root).disabled = busy || !select.value || !compareSelect.value || compareSelect.value === select.value;
    $("[data-cooling-confirm-delete]", root).disabled = busy;
  }
  async function loadSession() {
    const id = select.value, serial = ++requestId;
    if (!id) { session = null; setHTML($("[data-cooling-chart]", root), `<p class="wb-empty">No recordings yet. Start a recording during a workload to capture temperatures, duty and RPM.</p>`); return; }
    const data = await get(`/api/cooling/session?id=${encodeURIComponent(id)}`);
    if (!current() || serial !== requestId || id !== select.value) return;
    session = data;
    const last = data.samples.at(-1);
    $("[data-session-summary]", root).textContent = `${data.samples.length} samples · ${last ? Math.round(last.elapsed) : 0}s · ${data.ended_reason || "Recording"}${data.recovery_note ? ". " + data.recovery_note : ""}`;
    setHTML($("[data-cooling-chart]", root), chartMarkup(data, replay));
  }
  async function readStatus() {
    try {
      const next = await get("/api/cooling/sessions");
      if (!current()) return;
      state = next;
      $("[data-recording-status]", root).textContent = state.active ? `Recording · ${state.active.sample_count} / 1,800 samples` : state.saving ? "Saving recording…" : "Recorder stopped";
      $("[data-cooling-recovery]", root).hidden = !state.save_error;
      $("[data-cooling-save-error]", root).textContent = state.save_error || "";
      if (state.load_error) showError(new Error(state.load_error));
      else if (state.sample_error) showError(new Error(`A sample could not be recorded: ${state.sample_error}. Fan control continues.`));
      const sessions = [...(state.active ? [state.active] : []), ...state.sessions];
      const before = select.value;
      chooseOptions(select, sessions, state.active?.id);
      chooseOptions(compareSelect, state.sessions, state.sessions.find((s) => s.id !== select.value)?.id);
      if (before !== select.value) { replay = null; $("[data-cooling-replay-note]", root).textContent = ""; }
      paintControls();
      if (!session || session.id !== select.value || state.active?.id === select.value) await loadSession();
    } catch (error) { showError(error); }
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
    replay = null; session = null; $("[data-cooling-comparison]", root).textContent = "";
    $("[data-cooling-replay-note]", root).textContent = ""; $("[data-cooling-delete-confirm]", root).hidden = true;
    paintControls(); loadSession().catch(showError);
  });
  compareSelect.addEventListener("change", paintControls);
  root.addEventListener("click", async (event) => {
    const target = event.target.closest("button");
    if (!target || target.disabled) return;
    if (target.matches("[data-cooling-refresh]")) { showError(null); await refresh(true); return; }
    if (target.matches("[data-cooling-delete]")) { $("[data-cooling-delete-confirm]", root).hidden = false; return; }
    if (target.matches("[data-cooling-cancel-delete]")) { $("[data-cooling-delete-confirm]", root).hidden = true; return; }
    if (target.matches("[data-cooling-use-curve]")) {
      await action(async () => {
        const curves = await get("/api/curves");
        const name = S.state?.fans?.curve || "balanced";
        const points = curves.curves?.[name] || session?.samples.at(-1)?.curve?.points;
        if (!points) throw new Error("Current curve is unavailable. Enter draft points directly.");
        if (current()) $("[data-cooling-points]", root).value = points.map((p) => p.join(": ")).join("\n");
      }); return;
    }
    await action(async () => {
      if (target.matches("[data-cooling-start]")) {
        await post("/api/cooling/start", { annotation: $("#cooling-annotation", root).value });
        select.value = ""; session = null; await refresh(true);
      } else if (target.matches("[data-cooling-stop]")) {
        await post("/api/cooling/stop", {}); session = null; await refresh(true);
      } else if (target.matches("[data-cooling-retry]")) {
        await post("/api/cooling/retry-save", {}); await refresh(true);
      } else if (target.matches("[data-cooling-confirm-delete]")) {
        const id = select.value;
        await post("/api/cooling/delete", { id });
        $("[data-cooling-delete-confirm]", root).hidden = true; session = replay = null; await refresh(true);
      } else if (target.matches("[data-cooling-export]")) {
        download(await get(`/api/cooling/export?id=${encodeURIComponent(select.value)}&format=${target.dataset.coolingExport}`));
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
