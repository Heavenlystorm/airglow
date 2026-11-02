// End-to-end roadmap workflows. Only the isolated --demo fixture on 8790 is allowed.
import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { mkdirSync, readFileSync } from "node:fs";
import { isAbsolute, join } from "node:path";
import { pathToFileURL } from "node:url";

const BASE = "http://127.0.0.1:8790";
const OUTPUT = process.env.AIRGLOW_ROADMAP_OUTPUT || "output/implementation-2026-11-02/browser/roadmap";
mkdirSync(OUTPUT, { recursive: true });
async function loadPlaywright() {
  const module = process.env.AIRGLOW_PLAYWRIGHT_MODULE;
  if (module) return import(isAbsolute(module) ? pathToFileURL(module).href : module);
  try { return await import("playwright"); }
  catch (error) {
    if (error.code !== "ERR_MODULE_NOT_FOUND" || !process.env.APPDATA) throw error;
    return import(pathToFileURL(join(process.env.APPDATA, "npm/node_modules/playwright/index.mjs")).href);
  }
}
async function api(path, body) {
  const response = await fetch(BASE + path, body === undefined ? {} : {
    method: "POST", headers: { "Content-Type": "application/json", "X-Airglow": "1" }, body: JSON.stringify(body),
  });
  const result = await response.json();
  assert.ok(response.ok, `${path}: ${response.status} ${JSON.stringify(result)}`);
  return result;
}
let browser;
before(async () => {
  const settings = await api("/api/settings");
  assert.equal(settings.settings.lhm_url, "http://127.0.0.1:8791");
  await api("/api/onboarded", {});
  await api("/api/settings", { online_checks: false, design: "graphite" });
  await eventually(() => api("/api/state"), state => state.fans.channels.some(channel => channel.pump)
    && state.fans.channels.filter(channel => !channel.pump).length >= 2, "fixture fan discovery completed");
  const { chromium } = await loadPlaywright();
  browser = await chromium.launch({ channel: process.env.AIRGLOW_BROWSER || "msedge" });
});
after(async () => { await browser?.close(); });

async function pageFor(t, view, width = 1280, height = 840) {
  const page = await browser.newPage({ viewport: { width, height }, reducedMotion: "reduce" });
  page.setDefaultTimeout(10000);
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  t.after(async () => { await page.close(); assert.deepEqual(errors, [], "No browser exceptions"); });
  await page.goto(`${BASE}/#${view}`, { waitUntil: "domcontentloaded" });
  await page.locator(`[data-view="${view}"]`).waitFor({ state: "visible" });
  return page;
}

async function eventually(read, accept, message, timeout = 10000) {
  const until = Date.now() + timeout;
  let value;
  do {
    value = await read();
    if (accept(value)) return value;
    await new Promise(resolve => setTimeout(resolve, 100));
  } while (Date.now() < until);
  assert.fail(`${message}: ${JSON.stringify(value)}`);
}

async function posted(page, path, act) {
  const response = page.waitForResponse(r => r.url().endsWith(path) && r.request().method() === "POST");
  await act();
  const r = await response, result = await r.json();
  assert.ok(r.ok(), `${path}: ${JSON.stringify(result)}`);
  return result;
}

async function fits(page, selector = "body") {
  const sizes = await page.locator(selector).evaluate(e => ({ scroll: e.scrollWidth, width: e.clientWidth }));
  assert.ok(sizes.scroll <= sizes.width + 1, `${selector} overflows: ${JSON.stringify(sizes)}`);
}

test("narrow navigation reaches every section and curve points are keyboard editable drafts", async (t) => {
  const page = await pageFor(t, "overview", 390);
  for (const view of ["lighting", "devices", "automations", "settings", "studio", "fans", "overview"]) {
    await page.locator("#nav-select").selectOption(view);
    await page.locator(`[data-view="${view}"]`).waitFor({ state: "visible" });
    await fits(page);
  }
  await page.locator("#nav-select").selectOption("fans");
  const before = await api("/api/curves");
  await page.locator("#curve-add").focus();
  await page.locator("#curve-add").press("Enter");
  assert.equal(await page.evaluate(() => document.activeElement?.getAttribute("role")), "slider");
  assert.deepEqual(await api("/api/curves"), before, "adding a point must remain a draft");
  await page.screenshot({ path: join(OUTPUT, "390-fans-draft.png") });
});

test("fan copying names targets, leaves pumps unselected and changes only selected channels", async (t) => {
  const state = await api("/api/state"), channels = state.fans.channels;
  const source = channels.find(c => !c.pump), target = channels.find(c => !c.pump && c.id !== source.id);
  const pump = channels.find(c => c.pump);
  assert.ok(source && target && pump, "fixture has fans and a pump");
  t.after(async () => { for (const c of [source, target]) await api("/api/fans/channel", { id: c.id, mode: c.mode || "", duty: c.steady || 50 }); });
  const page = await pageFor(t, "fans");
  const row = page.locator(`[data-chan="${source.id}"]`);
  await posted(page, "/api/fans/channel", () => row.locator("[data-mode]").selectOption("steady"));
  await row.locator("[data-chan-all]").click();
  assert.equal(await page.locator(`[data-copy-target][value="${pump.id}"]`).isChecked(), false);
  for (const box of await page.locator("[data-copy-target]").all()) await box.setChecked((await box.getAttribute("value")) === target.id);
  await page.locator("#copy-apply").click();
  await eventually(() => page.locator("#copy-results").textContent(), s => /1 of 1 targets updated/.test(s), "copy result");
  const afterState = await api("/api/state");
  assert.equal(afterState.fans.channels.find(c => c.id === target.id).mode, "steady");
  for (const c of channels.filter(c => c.id !== source.id && c.id !== target.id))
    assert.equal(afterState.fans.channels.find(x => x.id === c.id).mode, c.mode, `${c.name} was not selected`);
  await page.screenshot({ path: join(OUTPUT, "fan-copy-targets.png") });
});

test("failed lighting writes remain visible and can be explicitly retried", async (t) => {
  const original = (await api("/api/state")).scene;
  t.after(() => api("/api/scene", original));
  const page = await pageFor(t, "lighting");
  let failed = false;
  await page.route("**/api/scene", route => {
    if (!failed && route.request().method() === "POST") {
      failed = true;
      return route.fulfill({ status: 500, contentType: "application/json", body: JSON.stringify({ error: "Fixture save unavailable" }) });
    }
    return route.continue();
  });
  await page.locator("#power").click();
  await eventually(() => page.locator("#mutation-status").textContent(), s => s.includes("Fixture save unavailable"), "persistent failure");
  assert.equal((await api("/api/state")).scene.on, original.on);
  await page.screenshot({ path: join(OUTPUT, "persistent-save-error.png") });
  await page.locator("#mutation-status").getByRole("button", { name: /Retry/ }).click();
  await eventually(() => api("/api/state"), s => s.scene.on !== original.on, "explicit retry saves the intended value");
  await eventually(() => page.locator("#mutation-status").textContent(), s => !s.includes("Fixture save unavailable"), "failure clears after success");
});

test("updating an adjusted profile keeps its ID and automation reference; save as new creates a separate profile", async (t) => {
  const oldPresets = await api("/api/presets"), oldRules = await api("/api/automations"), scene = (await api("/api/state")).scene;
  const made = await api("/api/presets/from-current", { name: "Roadmap profile", include_fans: false, include_areas: true });
  const rule = { id: "roadmap-reference", name: "Reference", enabled: false, when: { type: "idle", minutes: 13 }, then: { type: "preset", preset: made.id } };
  await api("/api/automations", { rules: [...oldRules, rule] });
  t.after(async () => {
    await api("/api/automations", { rules: oldRules });
    await api("/api/presets", { presets: oldPresets });
    await api("/api/scene", scene);
  });
  const page = await pageFor(t, "lighting");
  await posted(page, `/api/presets/${made.id}/apply`, () => page.locator(`.preset[data-preset="${made.id}"]`).click());
  await posted(page, "/api/scene", () => page.locator("#bri").fill("37"));
  await page.locator("#save-preset").click();
  assert.equal(await page.locator("#sp-update").isChecked(), true);
  const saved = await posted(page, "/api/presets/from-current", () => page.locator("#sp-submit").click());
  assert.equal(saved.id, made.id);
  assert.equal(saved.presets.length, oldPresets.length + 1);
  assert.equal(saved.presets.find(p => p.id === made.id).brightness, 37);
  assert.equal((await api("/api/automations")).find(r => r.id === rule.id).then.preset, made.id);
  await page.locator("#save-preset").click();
  await page.locator("#sp-new").check();
  await page.locator("#sp-name").fill("Roadmap copy");
  const copy = await posted(page, "/api/presets/from-current", () => page.locator("#sp-submit").click());
  assert.notEqual(copy.id, made.id);
  assert.equal(copy.presets.length, oldPresets.length + 2);
});

test("control center shows completed-work evidence and its optional tile survives reload and resize", async (t) => {
  const original = await api("/api/overview");
  t.after(() => api("/api/overview", original));
  const page = await pageFor(t, "overview");
  await page.locator("[data-control-center]").first().click();
  await eventually(() => page.locator("[data-control-loops]").textContent(), text => /Controller cycles/.test(text), "control rows");
  const status = await api("/api/control/status");
  assert.ok(status.controller.last_success_at && status.sensors.last_success_at);
  assert.ok(status.devices.length && status.events.length);
  const result = await posted(page, "/api/control/recheck", () => page.locator('[data-recheck="sensors"]').click());
  assert.deepEqual([result.requested, result.completed], [true, false]);
  await page.screenshot({ path: join(OUTPUT, "control-center-wide.png") });
  await page.setViewportSize({ width: 390, height: 840 });
  await fits(page, "#sheet");
  await page.screenshot({ path: join(OUTPUT, "control-center-390.png") });
  await page.locator("#sheet [data-close]").first().click();
  await page.setViewportSize({ width: 1280, height: 840 });
  await page.locator("#ov-edit").click();
  await page.locator("#ov-addtile").click();
  await page.locator('[data-ctl="control"]').click();
  const saved = await eventually(() => api("/api/overview"), s => s.tiles?.some(x => x.type === "control"), "optional tile persisted");
  await page.reload();
  await page.locator(".tile-control").waitFor();
  await page.setViewportSize({ width: 700, height: 840 });
  assert.deepEqual(await api("/api/overview"), saved, "viewport resize cannot rewrite saved layout");
});

test("cooling records two sessions, compares and replays drafts, and downloads without changing fan settings", async (t) => {
  const state = await api("/api/state"), before = await api("/api/curves");
  const page = await pageFor(t, "fans");
  await page.locator("#cooling-workbench").click();
  const ids = [];
  t.after(async () => {
    if ((await api("/api/cooling/sessions")).active) await api("/api/cooling/stop", {});
    for (const id of ids) await api("/api/cooling/delete", { id });
  });
  for (const label of ["Fixture idle", "Fixture workload"]) {
    await page.locator("#cooling-annotation").fill(label);
    const started = await posted(page, "/api/cooling/start", () => page.locator("[data-cooling-start]").click());
    ids.push(started.active.id);
    await eventually(() => api("/api/cooling/sessions"), s => s.active?.sample_count >= 2, "two recorded samples", 15000);
    const stopped = await posted(page, "/api/cooling/stop", () => page.locator("[data-cooling-stop]").click());
    assert.equal(stopped.saved, true);
  }
  await page.locator("[data-cooling-session]").selectOption(ids[1]);
  await page.locator("[data-cooling-compare-select]").selectOption(ids[0]);
  await posted(page, "/api/cooling/compare", () => page.locator("[data-cooling-compare]").click());
  assert.match(await page.locator("[data-cooling-comparison]").textContent(), /CPU mean/);
  await page.locator("[data-cooling-points]").fill("30: 30\n60: 55\n80: 100");
  const replay = await posted(page, "/api/cooling/replay", () => page.locator("[data-cooling-replay]").click());
  assert.equal(replay.applied, false);
  assert.ok(replay.samples.length >= 2);
  await page.locator("[data-cooling-chart] svg").first().waitFor();
  for (const format of ["json", "csv"]) {
    const downloaded = page.waitForEvent("download");
    await page.locator(`[data-cooling-export="${format}"]`).click();
    const file = await downloaded, dest = join(OUTPUT, `cooling-export.${format}`);
    await file.saveAs(dest);
    const text = readFileSync(dest, "utf8");
    if (format === "json") assert.equal(JSON.parse(text).annotation, "Fixture workload");
    else assert.match(text, /elapsed/);
  }
  await page.screenshot({ path: join(OUTPUT, "cooling-workbench-wide.png") });
  await page.setViewportSize({ width: 390, height: 840 }); await fits(page, "#sheet");
  await page.screenshot({ path: join(OUTPUT, "cooling-workbench-390.png") });
  const afterState = await api("/api/state");
  assert.equal(afterState.fans.base_curve, state.fans.base_curve);
  assert.deepEqual(afterState.fans.channels.map(c => [c.id, c.mode, c.steady]), state.fans.channels.map(c => [c.id, c.mode, c.steady]));
  assert.deepEqual(await api("/api/curves"), before);
});

test("rig export round-trips through manual mapping without activating profiles, automations or cooling", async (t) => {
  const page = await pageFor(t, "settings");
  const state = await api("/api/state"), profiles = await api("/api/presets"), rules = await api("/api/automations"), zones = await api("/api/zones");
  t.after(async () => { await api("/api/automations", { rules }); await api("/api/presets", { presets: profiles }); });
  const downloaded = page.waitForEvent("download");
  await page.locator("#s-rig-export").click();
  const file = await downloaded, dest = join(OUTPUT, "rig-bundle.json");
  await file.saveAs(dest);
  const bundle = JSON.parse(readFileSync(dest, "utf8"));
  assert.equal(bundle.format, "airglow-rig");
  await page.locator("#s-rig-import").click();
  await page.locator("#rig-file").setInputFiles(dest);
  await page.locator("[data-rig-source]").first().waitFor();
  assert.equal(await page.locator("#rig-apply").isDisabled(), true);
  const sources = await page.locator("[data-rig-source]").evaluateAll(els => els.map(e => e.dataset.rigSource));
  // Name one compatible target and explicitly skip the remaining source zones.
  for (let i = 0; i < sources.length; i++) {
    const selector = page.locator(`[data-rig-source="${sources[i]}"]`);
    const value = i === 0 ? `target:${zones.zones[0].key}` : "skip";
    await posted(page, "/api/rig/preview", () => selector.selectOption(value));
  }
  await page.screenshot({ path: join(OUTPUT, "rig-import-preview.png") });
  const imported = await posted(page, "/api/rig/apply", () => page.locator("#rig-apply").click());
  assert.equal(imported.applied, true);
  await page.getByRole("heading", { name: "Rig setup imported", exact: true }).waitFor();
  const afterState = await api("/api/state");
  assert.deepEqual(afterState.scene, state.scene);
  assert.equal(afterState.fans.base_curve, state.fans.base_curve);
  assert.deepEqual(afterState.fans.channels.map(c => [c.id, c.mode, c.steady]), state.fans.channels.map(c => [c.id, c.mode, c.steady]));
  for (const rule of (await api("/api/automations")).filter(r => !rules.some(o => o.id === r.id))) assert.equal(rule.enabled, false);
});

test("Studio palettes, focus, preview, save and apply work at wide and narrow sizes", async (t) => {
  const original = (await api("/api/state")).scene;
  const ids = [];
  t.after(async () => {
    await api("/api/studio/draft", { lights: false }); await api("/api/scene", original);
    for (const fx_id of ids) await api("/api/studio/delete", { fx_id });
  });
  const page = await pageFor(t, "studio");
  await page.locator("[data-tpl]").first().click();
  await page.locator("#st-name").fill("Roadmap lights");
  assert.equal(await page.locator("#st-output-state").textContent(), "Preview only");
  await page.locator('details[data-details="palettes"] summary').click();
  await page.locator("#st-palette-name").fill("Roadmap palette");
  await page.locator('[data-st="palette-save"]').click();
  assert.match(await page.locator('[data-details="palettes"]').textContent(), /Roadmap palette/);
  await page.locator('details[data-details="controls"] summary').click();
  const expose = page.locator('[data-mexpose="speed"]');
  await expose.setChecked(!(await expose.isChecked()));
  assert.equal(await expose.evaluate(el => document.activeElement === el), true);
  assert.equal(await page.locator('[data-details="controls"]').evaluate(el => el.open), true);
  await page.locator("#st-lights").focus();
  await posted(page, "/api/studio/draft", () => page.locator("#st-lights").press("Space"));
  await eventually(() => page.locator("#st-output-state").textContent(), s => s === "Live draft preview", "confirmed live draft");
  await posted(page, "/api/studio/draft", () => page.locator("#st-lights").press("Space"));
  const saved = await posted(page, "/api/studio/save", () => page.locator('[data-st="save"]').click());
  ids.push(saved.effect.fx_id);
  assert.notEqual((await api("/api/state")).scene.effect, saved.effect.fx_id, "saving does not apply");
  await posted(page, "/api/studio/use", () => page.locator('[data-st="use"]').click());
  await eventually(() => page.locator("#st-output-state").textContent(), s => s === "Saved effect applied", "confirmed saved effect");
  await page.screenshot({ path: join(OUTPUT, "studio-workflow-wide.png") });
  for (const width of [700, 390]) {
    await page.setViewportSize({ width, height: 840 });
    await page.locator('[data-st="add"]').click();
    await page.locator("#st-menu").waitFor({ state: "visible" });
    await fits(page); await fits(page, ".st-edit");
    const geometry = await page.evaluate(() => {
      const rect = s => document.querySelector(s).getBoundingClientRect().toJSON();
      return { left: rect(".st-left"), middle: rect(".st-mid"), right: rect(".st-right"), menu: rect("#st-menu") };
    });
    assert.ok(geometry.middle.bottom <= geometry.left.top + 1, `preview precedes editor panes: ${JSON.stringify(geometry)}`);
    assert.ok(geometry.menu.bottom <= geometry.left.bottom + 1, "the menu remains contained in its pane");
    await page.screenshot({ path: join(OUTPUT, `studio-menu-${width}.png`) });
    await page.locator('[data-add]').first().press("Escape");
    assert.equal(await page.locator('[data-st="add"]').evaluate(el => document.activeElement === el), true);
  }
  await page.reload();
  // A saved effect reopens from its card; favorite palettes survive page reload.
  await page.locator(`[data-edit="${saved.effect.fx_id}"]`).click();
  await page.locator('details[data-details="palettes"] summary').click();
  assert.match(await page.locator('[data-details="palettes"]').textContent(), /Roadmap palette/);
  await page.setViewportSize({ width: 1280, height: 840 });
  await page.locator('details[data-details="publishing"] summary').click();
  await page.locator('[data-st="pack-export"]').click();
  const download = page.waitForEvent("download");
  await page.locator("#st-pack-export .primary").click();
  const file = await download, dest = join(OUTPUT, "scene-pack.json"); await file.saveAs(dest);
  const pack = JSON.parse(readFileSync(dest, "utf8"));
  assert.equal(pack.format, "airglow-scene-pack");
  await page.locator("#sheet [data-close]").first().click();
  await page.getByRole("link", { name: "Back to the Studio" }).click();
  await page.locator("#st-pack-file").setInputFiles(dest);
  await page.locator("#st-pack-import").waitFor();
  const beforeImport = (await api("/api/state")).scene;
  const imported = await posted(page, "/api/studio/packs/import", () => page.locator("#st-pack-confirm").click());
  assert.equal(imported.applied, false);
  assert.deepEqual((await api("/api/state")).scene, beforeImport);
  await page.locator("#sheet [data-close]").first().click();
  await page.reload(); await page.locator(".st-pack-history").waitFor();
});

test("Studio save failure retains the draft and late pack replies cannot replace newer editing", async (t) => {
  const created = await api("/api/studio/save", { name: "Portable fixture", layers: [{ type: "solid" }] });
  const { pack } = await api("/api/studio/packs/export", { fx_ids: [created.effect.fx_id], name: "Portable test",
    areas: [{ area: "front", fx_id: created.effect.fx_id, values: {} }] });
  const page = await pageFor(t, "studio");
  const file = { name: "portable.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(pack)) };
  let releasePreview;
  const previewGate = new Promise(resolve => { releasePreview = resolve; });
  await page.route("**/api/studio/packs/preview", async route => { await previewGate; await route.continue(); }, { times: 1 });
  const previewRequest = page.waitForRequest(r => r.url().endsWith("/api/studio/packs/preview"));
  await page.locator("#st-pack-file").setInputFiles(file); await previewRequest;
  await page.locator("#sheet [data-close]").click();
  const previewResponse = page.waitForResponse(r => r.url().endsWith("/api/studio/packs/preview")); releasePreview(); await previewResponse;
  await page.evaluate(() => new Promise(requestAnimationFrame));
  assert.equal(await page.locator("#sheet").evaluate(el => el.open), false, "dismissed preview stays dismissed");
  await page.locator("#st-pack-file").setInputFiles(file);
  await page.locator("#st-pack-import").waitFor();
  await page.locator("[data-pack-choice]").selectOption("copy");
  let releaseImport;
  const importGate = new Promise(resolve => { releaseImport = resolve; });
  t.after(() => releaseImport());
  await page.route("**/api/studio/packs/import", async route => { await importGate; await route.continue(); }, { times: 1 });
  const importRequest = page.waitForRequest(r => r.url().endsWith("/api/studio/packs/import"));
  await page.locator("#st-pack-confirm").click(); await importRequest;
  await page.locator("#sheet [data-close]").click();
  await page.locator("[data-tpl]").first().click();
  await page.locator("#st-name").fill("New work while importing");
  const importResponse = page.waitForResponse(r => r.url().endsWith("/api/studio/packs/import")); releaseImport();
  const response = await importResponse; assert.ok(response.ok());
  await eventually(() => page.evaluate(() => localStorage.getItem("airglow.studio.packs.v1")), s => !!s, "import callback completed");
  assert.equal(await page.locator("#st-name").inputValue(), "New work while importing");
  assert.equal(await page.locator("#sheet").evaluate(el => el.open), false);
  const saveResponses = [];
  page.on("response", response => { if (response.url().endsWith("/api/studio/save")) saveResponses.push(response.status()); });
  // Keep this failure on the context while the earlier, delayed page routes finish cleaning up.
  await page.context().route("**/api/studio/save", route => route.fulfill({ status: 500, contentType: "application/json",
    body: JSON.stringify({ error: "Fixture disk full" }) }));
  const failedSave = page.waitForResponse(response => response.url().endsWith("/api/studio/save")
    && response.request().method() === "POST");
  await page.locator('[data-st="save"]').click();
  assert.equal((await failedSave).status(), 500, "the save request receives the injected disk failure");
  await eventually(async () => ({ error: await page.locator("#st-workflow-error").textContent(),
    saved: await page.locator("#st-saved").textContent(), saveResponses }), s => /Fixture disk full/.test(s.error), "persistent Studio save failure");
  assert.match(await page.locator("#st-saved").textContent(), /Save failed/);
  assert.equal(await page.locator("#st-name").inputValue(), "New work while importing");
  await page.screenshot({ path: join(OUTPUT, "studio-save-error.png") });
});

test("new tools and the narrow code editor remain usable in the paper theme", async (t) => {
  await api("/api/settings", { design: "paper" });
  t.after(() => api("/api/settings", { design: "graphite" }));
  const page = await pageFor(t, "overview", 700);
  await page.locator("[data-control-center]").first().click();
  await page.locator("[data-control-loops] li").first().waitFor();
  await fits(page, "#sheet");
  await page.screenshot({ path: join(OUTPUT, "control-center-paper.png") });
  await page.locator("#sheet [data-close]").click();
  await page.locator("#nav-select").selectOption("studio");
  await page.locator("[data-code-tpl]").first().click();
  await page.locator(".st-edit.is-code .CodeMirror").waitFor();
  for (const width of [700, 390]) {
    await page.setViewportSize({ width, height: 840 });
    await fits(page); await fits(page, ".st-edit");
    await page.locator(".CodeMirror").scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(OUTPUT, `studio-code-paper-${width}.png`) });
  }
});

test("automation value editing preserves its field and keyboard position", async (t) => {
  const page = await pageFor(t, "automations");
  await page.locator("#auto-add").click();
  await page.locator("#rb-when").selectOption("idle");
  const field = await page.locator('[data-w="minutes"]').elementHandle();
  await page.locator('[data-w="minutes"]').fill("13");
  await page.locator('[data-w="minutes"]').press("Tab");
  assert.equal(await field.evaluate((el) => el.isConnected), true, "editing a value must not replace the form");
  assert.equal(await page.locator("#rb-then").evaluate((el) => el === document.activeElement), true);
  await page.locator("#rb-when").selectOption("time");
  await page.locator('[data-day="0"]').click();
  assert.equal(await page.locator('[data-day="0"]').evaluate((el) => el === document.activeElement), true);
  await page.screenshot({ path: join(OUTPUT, "automation-focus.png") });
});
