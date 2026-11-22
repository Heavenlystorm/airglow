import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { api, rigPage, root, until } from "./isolated-browser-rig.mjs";

const out = process.env.AIRGLOW_SESSION_OUTPUT || join(root, "output/session-lab");
mkdirSync(out, { recursive: true });
const overview = (t) => rigPage(t, "overview");
async function open(page) {
  await page.locator("#ov-tools").click();
  await page.locator("[data-telemetry-sessions]").click();
  await page.locator(".session-lab").waitFor();
}
async function posted(page, suffix, action) {
  const response = page.waitForResponse(r => r.url().endsWith(suffix) && r.request().method() === "POST");
  await action(); const r = await response;
  assert.equal(r.status(), 200, await r.text()); return r.json();
}
async function start(page, name, count = 4) {
  await page.locator("#cooling-annotation").fill(name);
  const result = await posted(page, "/api/cooling/start", () => page.locator("[data-cooling-start]").click());
  const id = result.active.id;
  await until(() => api(`/api/cooling/trace?id=${id}`), r => r.samples.length >= count, 20000);
  return id;
}

async function exportReport(page, filename) {
  await page.evaluate(() => {
    Object.defineProperty(window, "showSaveFilePicker", { value: undefined, configurable: true });
    if (!window.reportLabels) {
      const original = CanvasRenderingContext2D.prototype.fillText;
      CanvasRenderingContext2D.prototype.fillText = function(value, ...args) {
        if (this.canvas.width === 1440) window.reportLabels.push(value);
        return original.call(this, value, ...args);
      };
    }
    window.reportLabels = [];
  });
  const writes = [], observe = r => { if (r.method() === "POST") writes.push(new URL(r.url()).pathname); };
  page.on("request", observe);
  try {
    const pending = page.waitForEvent("download");
    await page.locator("[data-report-save]").click();
    const download = await pending;
    assert.equal(download.suggestedFilename(), filename);
    await download.saveAs(join(out, filename));
    const bytes = readFileSync(await download.path());
    assert.equal(bytes.subarray(1,4).toString(), "PNG");
    assert.equal(bytes.readUInt32BE(16), 1440);
    assert.ok(bytes.readUInt32BE(20) >= 650);
    assert.deepEqual(writes, [], "chart export uses the loaded recordings and never sends a hardware command");
    return page.evaluate(() => window.reportLabels);
  } finally { page.off("request", observe); }
}

test("records two workloads, restores a recording after closing the panel, and compares and exports measured data", async t => {
  const page = await overview(t);
  await open(page);
  assert.match(await page.locator(".session-empty").textContent(), /Start a recording/);
  await page.locator(".session-picker summary").click();
  while (await page.locator("[data-session-remove]").count()) await page.locator("[data-session-remove]").first().click();
  await page.locator("#session-catalog").selectOption("auto/cpu_power");
  await page.locator("[data-session-add]").click();
  await page.locator(".session-picker summary").click();
  const first = await start(page, "Cool start", 2);
  await page.locator("#live-marker-label").fill("Launched game");
  const markerA = (await posted(page, "/api/cooling/mark", () => page.locator("#live-marker-label").press("Enter"))).markers[0];
  await page.getByRole("button", { name: "Close telemetry sessions" }).click();
  await open(page);
  await until(() => page.locator("#cooling-annotation").inputValue(), v => v === "Cool start");
  await page.locator(".session-picker summary").click();
  assert.deepEqual(await page.locator("[data-session-remove]").evaluateAll(buttons => buttons.map(b => b.dataset.sessionRemove)), ["auto/cpu_power"]);
  await page.locator(".session-picker summary").click();
  await until(() => api(`/api/cooling/trace?id=${first}`), r => r.sample_count >= 4);
  await posted(page, "/api/cooling/stop", () => page.locator("[data-cooling-stop]").click());
  await page.locator(".session-picker summary").click();
  while (await page.locator("[data-session-remove]").count()) await page.locator("[data-session-remove]").first().click();
  await page.locator("#session-catalog").selectOption("auto/cpu_power"); await page.locator("[data-session-add]").click();
  await page.locator(".session-picker summary").click();
  const second = await start(page, "Sustained workload", 2);
  await page.locator("#live-marker-label").fill("Closed case");
  const markerB = (await posted(page, "/api/cooling/mark", () => page.locator("[data-mark-now]").click())).markers[0];
  await until(() => api(`/api/cooling/trace?id=${second}`), r => r.sample_count >= 4);
  await posted(page, "/api/cooling/stop", () => page.locator("[data-cooling-stop]").click());
  await page.locator("[data-cooling-session]").selectOption(first);
  await page.locator("[data-session-overlay]").selectOption(second);
  await page.locator("[data-session-legend] .run-1").waitFor();
  await page.locator("[data-session-signal]").selectOption(JSON.stringify(["auto/cpu_power","W"]));
  await page.locator(".session-line.run-1").first().waitFor();
  const data = await api(`/api/cooling/trace?id=${first}`);
  const other = await api(`/api/cooling/trace?id=${second}`);
  const index = data.sensors.findIndex(s => s.id === "auto/cpu_power");
  assert.ok(index >= 0 && data.samples.some(s => Number.isFinite(s.readings[index])));
  const coverage = await page.locator(".session-stats tbody tr").last().innerText();
  assert.ok(coverage.includes(`${data.samples.filter(s => s.readings[index] !== null).length} / ${data.sample_count}`));
  const markerSelect = page.locator("#session-events");
  await markerSelect.selectOption(JSON.stringify([first, markerA.id]));
  const maximum = Math.max(data.duration_seconds, other.duration_seconds);
  const cursor = page.locator(".session-crosshair");
  assert.ok(Math.abs(Number(await cursor.getAttribute("x1")) * maximum / 600 - markerA.elapsed) < .001);
  assert.ok(await page.locator(".session-event-mark.run-0").count());
  await markerSelect.selectOption(JSON.stringify([second, markerB.id]));
  assert.ok(Math.abs(Number(await cursor.getAttribute("x1")) * maximum / 600 - markerB.elapsed) < .001);
  assert.ok(await page.locator("[data-event-remove]").isDisabled(), "Run A actions never remove an overlaid run's events");
  await page.locator(".session-notes-details summary").click();
  await page.locator("#session-name").fill("Open case");
  await page.locator("#session-notes").fill("Side panel open. Same game benchmark for both runs.");
  await posted(page, "/api/cooling/annotate", () => page.locator("[data-note-save]").click());
  await until(() => page.locator("[data-session-legend] .run-0").textContent(), v => v.includes("Open case"));
  await page.locator(".session-notes-details summary").click();
  await page.locator("#session-time").focus(); await page.keyboard.press("Home");
  assert.equal(Number(await page.locator("#session-time").inputValue()), 0);
  await page.keyboard.press("ArrowRight");
  assert.ok(Number(await page.locator("#session-time").inputValue()) > 0);
  await page.locator("#session-place-label").fill("Checked fan speed");
  const placed = (await posted(page, "/api/cooling/mark", () => page.locator("[data-event-place]").click())).markers.find(m => m.kind === "placed");
  await markerSelect.selectOption(JSON.stringify([first, placed.id]));
  await page.locator("[data-event-remove]").click(); await page.locator("[data-event-keep]").click();
  await markerSelect.selectOption(JSON.stringify([first, markerA.id]));
  await page.screenshot({ path: join(out, "recording-comparison-desktop.png") });
  assert.equal(await page.locator("[data-report-markers]").isChecked(), false);
  const plain = await exportReport(page, "airglow-recording-comparison.png");
  assert.ok(plain.includes("Open case") && plain.includes("Sustained workload") && plain.includes(data.sensors[index].name));
  assert.ok(plain.some(s => /Mean.*W/.test(s)));
  assert.doesNotMatch(plain.join(" "), /Closed case|Launched game|Checked fan speed|Side panel open|auto\/cpu_power/);
  await page.locator("[data-report-markers]").check();
  const marked = await exportReport(page, "airglow-recording-comparison.png");
  assert.ok(marked.includes("Launched game") && marked.includes("Closed case") && marked.includes("Checked fan speed"));
  assert.doesNotMatch(marked.join(" "), /Side panel open|auto\/cpu_power/);
  for (const kind of ["json", "csv", "timeline-csv"]) {
    const download = page.waitForEvent("download");
    await page.locator(`[data-cooling-export="${kind}"]`).click();
    const file = await download; const content = readFileSync(await file.path(), "utf8");
    if (kind === "json") {
      const json = JSON.parse(content);
      assert.deepEqual(json.samples.map(s => s.readings), data.samples.map(s => s.readings));
      assert.equal(json.name, "Open case"); assert.equal(json.markers.length, 2); assert.match(json.notes, /Same game/);
    } else if (kind === "csv") {
      assert.match(content, /sensor auto\/cpu_power \(W\)/); assert.match(content, /Cool start/); assert.doesNotMatch(content, /Checked fan speed/);
    } else { assert.match(content, /Launched game/); assert.match(content, /Checked fan speed/); assert.match(content, /Same game/); }
  }
  assert.ok(other.sample_count >= 4);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.locator("[data-session-signal-name]").scrollIntoViewIfNeeded();
  await page.screenshot({ path: join(out, "recording-comparison-mobile.png") });
  const fits = await page.locator("#sheet").evaluate(d => d.scrollWidth <= d.clientWidth + 1 && d.getBoundingClientRect().right <= window.innerWidth);
  assert.ok(fits, "mobile panel fits without horizontal scrolling");
  await markerSelect.selectOption(JSON.stringify([first, placed.id]));
  await page.locator("[data-event-remove]").click();
  await posted(page, "/api/cooling/unmark", () => page.locator("[data-event-delete]").click());
  assert.deepEqual((await api(`/api/cooling/trace?id=${second}`)).markers, [markerB]);
  await page.locator("[data-session-overlay]").selectOption("");
  await until(() => page.locator("[data-session-legend] .run-1").count(), n => n === 0);
  const single = await exportReport(page, "airglow-recording.png");
  assert.ok(single.includes("Open case") && !single.includes("Sustained workload"));
});

test("encoded chart pixels keep a missing interval empty and keep the Fahrenheit scale", async t => {
  const page = await overview(t);
  const result = await page.evaluate(async () => {
    const { chartReport, reportPng } = await import("/js/session-report.js");
    const { traceFor, graphBounds, displayScale } = await import("/js/session-traces.js");
    const run = { name: "Gap check", duration_seconds: 8, ended_reason: "manual",
      samples: [50,50,null,50,50].map((cpu,i) => ({elapsed:i * 2, cpu})) };
    const trace = traceFor(run, "cpu"), transform = displayScale("°C", [trace], true);
    const model = { traces: [trace], transform, bounds: graphBounds([trace], transform), metric: trace.metric, duration: 8, events: [] };
    const report = chartReport(model, [run], false);
    const labels = [], original = CanvasRenderingContext2D.prototype.fillText;
    CanvasRenderingContext2D.prototype.fillText = function(value,...args) { labels.push(value); return original.call(this,value,...args); };
    let bitmap;
    try { bitmap = await createImageBitmap(await reportPng(report)); }
    finally { CanvasRenderingContext2D.prototype.fillText = original; }
    const canvas = document.createElement("canvas"); canvas.width = bitmap.width; canvas.height = bitmap.height;
    const ctx = canvas.getContext("2d"); ctx.drawImage(bitmap, 0, 0); bitmap.close();
    const pixels = ctx.getImageData(0,0,canvas.width,canvas.height).data;
    const amber = (x,y) => { const i = 4 * (y * canvas.width + x); return pixels[i] === 255 && pixels[i+1] === 201 && pixels[i+2] === 132; };
    let matched = false;
    for (let y=280; y<canvas.height-150; y++) {
      if (amber(320,y) && amber(1190,y) && !amber(720,y)) { matched = true; break; }
    }
    return { matched, labels, count:report.runs[0].measured, bytes: pixels.length };
  });
  assert.ok(result.labels.some(s => /Mean 122°F/.test(s))); assert.equal(result.count, 4);
  assert.ok(result.matched && result.bytes > 1440 * 650 * 4);
});

test("two windows can resolve conflicting notes without losing a draft or silently replacing another run", async t => {
  const first = (await api("/api/cooling/start", {annotation: "Notes editor A"})).active;
  await api("/api/cooling/stop", {id:first.id});
  const second = (await api("/api/cooling/start", {annotation: "Notes editor B"})).active;
  await api("/api/cooling/stop", {id:second.id});
  const page = await overview(t); await open(page);
  await page.locator("[data-cooling-session]").selectOption(first.id);
  await page.locator(".session-notes-details summary").click();
  await page.locator("#session-notes").fill("My local observation");
  await page.locator("[data-cooling-session]").selectOption(second.id);
  await until(() => page.locator("#session-notes").inputValue(), v => v !== "My local observation");
  await page.locator("[data-cooling-session]").selectOption(first.id);
  await until(() => page.locator("#session-notes").inputValue(), v => v === "My local observation");
  const snapshot = await api(`/api/cooling/trace?id=${first.id}`);
  await api("/api/cooling/annotate", { id: first.id, name: "Other editor", notes: "Observation from the other window", expected: { name:snapshot.name || "", notes:snapshot.notes || "" } });
  await page.locator("[data-cooling-refresh]").click();
  await page.locator("[data-note-conflict]").waitFor();
  assert.equal(await page.locator("#session-notes").inputValue(), "My local observation");
  assert.ok(await page.locator("[data-note-save]").isDisabled());
  await page.locator("[data-note-rebase]").click();
  await posted(page, "/api/cooling/annotate", () => page.locator("[data-note-save]").click());
  const saved = await api(`/api/cooling/trace?id=${first.id}`);
  assert.equal(saved.notes, "My local observation");
  assert.equal((await api(`/api/cooling/trace?id=${second.id}`)).notes || "", "");
});

test("focused sensor offers an opt-in recording and reconnects with no implicit start", async t => {
  const page = await overview(t);
  await page.locator(".tile-sensor").first().click();
  await page.locator("[data-session-focus]").click();
  await page.locator(".session-lab").waitFor();
  assert.equal((await api("/api/cooling/sessions")).active, null);
  await page.locator(".session-picker summary").click();
  assert.ok((await page.locator("[data-session-remove]").count()) > 0);
  await page.route("**/api/cooling/sessions", route => route.abort());
  await page.locator("[data-cooling-refresh]").click();
  await until(() => page.locator("[data-recording-status]").textContent(), s => s.includes("Reconnecting"));
  assert.ok(await page.locator("[data-cooling-start]").isDisabled());
  await page.unroute("**/api/cooling/sessions");
  await page.locator("[data-cooling-refresh]").click();
  await until(() => page.locator("[data-cooling-start]").isEnabled(), Boolean);
  assert.equal((await api("/api/cooling/sessions")).active, null);
});
