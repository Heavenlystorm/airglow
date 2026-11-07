// Exercises personal customization only against the isolated demo fixture.
import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { mkdirSync, readFileSync } from "node:fs";
import { isAbsolute, join } from "node:path";
import { pathToFileURL } from "node:url";

const BASE = "http://127.0.0.1:8790";
const OUTPUT = process.env.AIRGLOW_PERSONALIZATION_OUTPUT || "output/living-rig-implementation/browser/personalization";
const DEFAULT = { accent: null, density: "comfortable", scale: 100, rig: "frame", motion: "system" };
mkdirSync(OUTPUT, { recursive: true });
async function api(path, body) {
  const r = await fetch(BASE + path, body === undefined ? {} : {
    method: "POST", headers: { "Content-Type": "application/json", "X-Airglow": "1" }, body: JSON.stringify(body),
  });
  const data = await r.json();
  assert.ok(r.ok, `${path}: ${JSON.stringify(data)}`);
  return data;
}
async function eventually(read, accept, message) {
  const until = Date.now() + 10000;
  let value;
  do { value = await read(); if (accept(value)) return value; await new Promise(r => setTimeout(r, 100)); } while (Date.now() < until);
  assert.fail(`${message}: ${JSON.stringify(value)}`);
}
let browser, originalSettings, originalOverview;
before(async () => {
  originalSettings = (await api("/api/settings")).settings;
  assert.equal(originalSettings.lhm_url, "http://127.0.0.1:8791", "Only the isolated demo is allowed");
  originalOverview = await api("/api/overview");
  await api("/api/onboarded", {});
  await api("/api/settings", { appearance: DEFAULT, looks: [], design: "graphite", online_checks: false });
  let module;
  if (process.env.AIRGLOW_PLAYWRIGHT_MODULE) {
    const name = process.env.AIRGLOW_PLAYWRIGHT_MODULE;
    module = await import(isAbsolute(name) ? pathToFileURL(name).href : name);
  } else {
    try { module = await import("playwright"); }
    catch (e) { if (e.code !== "ERR_MODULE_NOT_FOUND" || !process.env.APPDATA) throw e;
      module = await import(pathToFileURL(join(process.env.APPDATA, "npm/node_modules/playwright/index.mjs")).href); }
  }
  browser = await module.chromium.launch({ channel: process.env.AIRGLOW_BROWSER || "msedge" });
});
after(async () => {
  await browser?.close();
  if (originalSettings) await api("/api/settings", { appearance: originalSettings.appearance || DEFAULT, looks: originalSettings.looks || [], design: originalSettings.design });
  if (originalOverview) await api("/api/overview", { ...originalOverview, workspaces: originalOverview.workspaces || [], active_workspace: originalOverview.active_workspace || null });
});
async function pageFor(t, view, width = 1440, height = 960) {
  const page = await browser.newPage({ viewport: { width, height }, reducedMotion: "reduce" });
  page.setDefaultTimeout(10000);
  const errors = [];
  page.on("pageerror", e => errors.push(e.message));
  t.after(async () => { await page.close(); assert.deepEqual(errors, [], "No browser exceptions"); });
  await page.goto(`${BASE}/#${view}`, { waitUntil: "domcontentloaded" });
  await page.locator(`[data-view="${view}"]`).waitFor({ state: "visible" });
  return page;
}
async function fits(page) {
  const sizes = await page.evaluate(() => ({ scroll: document.documentElement.scrollWidth, width: innerWidth }));
  assert.ok(sizes.scroll <= sizes.width + 1, `Page overflows: ${JSON.stringify(sizes)}`);
}
async function openAppearance(page) { await page.locator("#s-appearance").click(); await page.locator("#ap-form, #appearance-form").waitFor(); }
async function keep(page) {
  await page.locator("#ap-keep").click();
  await page.locator("#sheet").waitFor({ state: "hidden" });
}

test("appearance preview cancels, failed Keep retries, and saved recipes survive reload", async t => {
  const page = await pageFor(t, "settings");
  await openAppearance(page);
  await page.locator("#ap-density").selectOption("compact");
  await page.locator("#ap-scale").selectOption("120");
  const marker = await page.locator(".tab-ink").boundingBox(), tab = await page.locator('.tabs a[aria-current]').boundingBox();
  assert.ok(Math.abs(marker.x - tab.x - 14.4) < 1 && Math.abs(marker.width - tab.width + 28.8) < 1, "Tab marker follows the current scale immediately");
  await page.locator("#ap-rig").selectOption("minimal");
  await page.locator("#ap-motion").selectOption("reduced");
  await page.locator("#ap-hex").fill("#45d6bc");
  await page.locator("#ap-hex").press("Tab");
  assert.equal(await page.locator("html").getAttribute("data-scale"), "120");
  assert.deepEqual((await api("/api/settings")).settings.appearance, DEFAULT, "Preview cannot persist");
  await page.keyboard.press("Escape");
  await eventually(() => page.locator("html").getAttribute("data-scale"), x => x === "100", "Escape restores baseline");
  await openAppearance(page);
  await page.locator("#ap-density").selectOption("compact");
  await page.locator("#ap-hex").fill("#45d6bc");
  await page.locator("#ap-hex").press("Tab");
  await page.locator("#ap-name").fill("Evening desk");
  await page.locator("#ap-save-look").click();
  assert.equal(await page.locator("[data-look-apply]").count(), 1);
  await page.route("**/api/settings", async route => {
    if (route.request().method() === "POST") await route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ error: "Fixture disk unavailable" }) });
    else await route.continue();
  });
  await page.locator("#ap-keep").click();
  await eventually(() => page.locator("#ap-keep").isEnabled(), Boolean, "Keep returns after failed save");
  assert.deepEqual((await api("/api/settings")).settings.appearance, DEFAULT, "Failed Keep cannot persist");
  await page.unroute("**/api/settings");
  await keep(page);
  await page.reload();
  await eventually(() => page.locator("html").getAttribute("data-density"), x => x === "compact", "Saved density reloads");
  await openAppearance(page);
  await page.locator("[data-look-rename]").click();
  await page.locator("#ap-name").fill("Night desk");
  await page.locator("#ap-save-look").click();
  await page.locator("[data-look-apply]").click();
  const downloadEvent = page.waitForEvent("download");
  await page.locator("#ap-export").click();
  const download = await downloadEvent;
  const recipe = readFileSync(await download.path(), "utf8");
  assert.ok(JSON.parse(recipe));
  await page.locator("[data-look-delete]").click();
  await page.locator("#ap-file").setInputFiles({ name: "look.json", mimeType: "application/json", buffer: Buffer.from(recipe) });
  await page.locator("#ap-save-look").click();
  await page.screenshot({ path: join(OUTPUT, "appearance-desktop.png") });
  await keep(page);
  assert.equal((await api("/api/settings")).settings.looks[0].name, "Night desk");
  await api("/api/settings", { appearance: DEFAULT, looks: [], design: "graphite" });
});

test("appearance scales fit narrow windows and all designs", async t => {
  const page = await pageFor(t, "settings", 390, 844);
  await openAppearance(page);
  for (const scale of ["90", "120"]) {
    await page.locator("#ap-scale").selectOption(scale);
    await fits(page);
    const box = await page.locator("#sheet").boundingBox();
    assert.ok(box.x >= -1 && box.x + box.width <= 391 && box.y >= -1 && box.y + box.height <= 845, `Sheet must fit at ${scale}%: ${JSON.stringify(box)}`);
    await page.locator("#ap-keep").scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(OUTPUT, `appearance-narrow-${scale}.png`) });
  }
  await page.locator("#ap-scale").selectOption("100");
  for (const design of ["graphite", "ascii", "paper", "amber", "contrast", "neon", "kiln", "lumen", "midnight", "spore", "rim", "darkroom"]) {
    await page.locator("#ap-design").selectOption(design);
    await fits(page);
  }
  await page.locator("#ap-cancel").click();
});

test("workspace starters preserve the old layout, switch, undo, and import", async t => {
  const page = await pageFor(t, "overview", 1920, 1080);
  const original = await api("/api/overview");
  await page.locator("#ov-workspace-manage").click();
  await page.locator("#ov-workspace-new").click();
  await page.locator("#ov-workspace-name").fill("Living Rig");
  await page.locator("#ov-workspace-starter").selectOption("lighting");
  await page.locator("#ov-workspace-form button[type=submit], #ov-workspace-form .primary").click();
  const created = await eventually(() => api("/api/overview"), x => x.workspaces?.length >= 2, "Workspace creation saved");
  const old = created.workspaces.find(x => x.id !== created.active_workspace);
  assert.deepEqual(old.tiles, original.tiles, "First workspace preserves original tiles");
  const newId = created.active_workspace;
  await page.screenshot({ path: join(OUTPUT, "living-rig-desktop.png") });
  await page.locator("#ov-workspace").selectOption(old.id);
  await eventually(() => api("/api/overview"), x => x.active_workspace === old.id, "Switch persists");
  await page.locator("#ov-undo").click();
  await eventually(() => api("/api/overview"), x => x.active_workspace === newId, "Undo restores workspace");
  await page.reload();
  await eventually(() => page.locator("#ov-workspace").inputValue(), x => x === newId, "Workspace survives reload");
  await page.locator("#ov-workspace-manage").click();
  const downloadEvent = page.waitForEvent("download");
  await page.locator("#ov-workspace-export").click();
  const recipe = readFileSync(await (await downloadEvent).path(), "utf8");
  await page.locator("#ov-workspace-file").setInputFiles({ name: "workspace.json", mimeType: "application/json", buffer: Buffer.from(recipe) });
  await eventually(() => api("/api/overview"), x => x.workspaces?.length === 3, "Imported workspace saved");
  await page.setViewportSize({ width: 390, height: 844 });
  await fits(page);
  await page.screenshot({ path: join(OUTPUT, "living-rig-narrow.png") });
});

test("Studio variations hold controls, blend and undo without enabling hardware preview", async t => {
  await api("/api/settings", { appearance: DEFAULT, design: "graphite" });
  const page = await pageFor(t, "studio");
  await page.locator("[data-tpl]").first().click();
  await page.locator("#st-variation-name").waitFor();
  const posts = [];
  page.on("request", req => { if (req.method() === "POST") posts.push({ path: new URL(req.url()).pathname, body: req.postDataJSON() }); });
  await page.locator("#st-variation-name").fill("First mood");
  await page.locator('[data-var="capture"]').click();
  assert.equal(await page.locator("[data-vrestore]").count(), 1);
  for (const key of ["palette", "speed", "brightness"]) await page.locator(`[data-vlock="${key}"]`).click();
  await page.locator('details[data-details="comparison"] summary').click();
  await page.locator('[data-vcapture="a"]').click();
  await page.locator('[data-var="vary"]').click();
  for (const key of ["palette", "speed", "brightness"]) assert.equal(await page.locator(`[data-vlock="${key}"]`).getAttribute("aria-pressed"), "true");
  await page.locator("#mm-react-num").fill("22");
  await page.locator("#mm-react-num").press("Tab");
  await page.locator('[data-vcapture="b"]').click();
  await page.locator("#st-variation-blend-number").fill("50");
  await page.locator("#st-variation-blend-number").press("Tab");
  assert.equal(await page.locator("#mm-react-num").inputValue(), "36");
  await page.locator('[data-var="vary"]').focus();
  await page.keyboard.press("Control+z");
  assert.equal(await page.locator("#mm-react-num").inputValue(), "22", "Blend is one undoable edit");
  await page.keyboard.press("Control+Shift+z");
  assert.equal(await page.locator("#mm-react-num").inputValue(), "36");
  await page.locator("[data-vrestore]").click();
  assert.equal(await page.locator("#mm-react-num").inputValue(), "50", "Recall restores captured controls");
  await page.keyboard.press("Control+z");
  assert.equal(await page.locator("#mm-react-num").inputValue(), "36", "Recall is undoable");
  await page.screenshot({ path: join(OUTPUT, "studio-variations-desktop.png") });
  await page.setViewportSize({ width: 390, height: 844 });
  await fits(page);
  await page.locator("#st-instrument").scrollIntoViewIfNeeded();
  await page.screenshot({ path: join(OUTPUT, "studio-variations-narrow.png") });
  assert.ok(!posts.some(({path, body}) => path === "/api/studio/use" || (path === "/api/studio/draft" && body?.lights)), `Variation exploration cannot enable hardware: ${JSON.stringify(posts)}`);
});

test("scene playback is explicit, keyboard accessible and stops for reduced motion", async t => {
  const page = await pageFor(t, "studio");
  const play = page.locator("[data-preview]").first();
  await play.waitFor();
  assert.equal(await play.isDisabled(), true, "OS reduced motion keeps still previews");
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await eventually(() => play.isEnabled(), Boolean, "Playback becomes available");
  await play.focus();
  await play.press("Enter");
  await eventually(() => page.locator(".st-card.is-playing").count(), x => x === 1, "Only one scene plays");
  await page.screenshot({ path: join(OUTPUT, "scene-collection.png") });
  await page.keyboard.press("Escape");
  assert.equal(await page.locator(".st-card.is-playing").count(), 0);
  await page.locator("[data-tpl]").first().focus();
  await eventually(() => page.locator(".st-card.is-playing").count(), x => x === 1, "Keyboard focus previews a scene");
  await page.evaluate(async () => {
    const { applyAppearance } = await import("/js/appearance.js");
    applyAppearance({ motion: "reduced" }, { persist: false });
  });
  await eventually(() => page.locator(".st-card.is-playing").count(), x => x === 0, "App reduced motion stops preview");
  assert.equal(await play.isDisabled(), true);
});

test("configured fan geometry renders framed and minimal portraits in dark and light designs", async t => {
  // This representative rendering fixture supplies simulated device state and colors in the browser only.
  // It neither changes device configuration nor claims that physical lights were tested.
  const page = await pageFor(t, "overview", 1920, 1080);
  const sample = await api("/api/zones");
  sample.zones = sample.zones.filter(z => z.key.startsWith("nzxt-hub/") && z.positions.length);
  for (const zone of sample.zones) zone.device.online = true;
  await page.route("**/api/zones", route => route.fulfill({ contentType: "application/json", body: JSON.stringify(sample) }));
  await page.route("**/api/stream?*", route => route.abort());
  await page.reload();
  await page.locator("#ov-grid canvas").first().waitFor();
  for (const [design, rig] of [["graphite", "frame"], ["paper", "frame"], ["graphite", "minimal"]]) {
    await page.evaluate(async ({ design, rig, sample }) => {
      const { S, emit } = await import("/js/core.js");
      const { applyDesign } = await import("/js/design.js");
      const { applyAppearance } = await import("/js/appearance.js");
      const { renderOverview } = await import("/js/overview.js");
      S.zones = sample;
      applyDesign(design, { persist: false }); applyAppearance({ rig }, { persist: false });
      await document.fonts.ready;
      await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      renderOverview();
      const colors = ["35d9cb", "5a8dff", "bc6be8"];
      emit("frames:live", { zones: Object.fromEntries(sample.zones.map((z, n) => [z.key, z.positions.map((_, i) => colors[(Math.floor(i / 4) + n) % colors.length]).join("")])) });
    }, { design, rig, sample });
    await fits(page);
    await page.locator(".tile-lighting").screenshot({ path: join(OUTPUT, `simulated-fan-rig-${design}-${rig}.png`) });
  }
});
