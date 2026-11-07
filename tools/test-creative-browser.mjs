// Creative workflows use the owned demo fixture only; no installed-app endpoints.
import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { mkdirSync } from "node:fs";
import { isAbsolute, join } from "node:path";
import { pathToFileURL } from "node:url";
import { installBrowserPreviewBridge } from "./browser-preview-bridge.mjs";
const BASE = "http://127.0.0.1:8790";
const OUT = process.env.AIRGLOW_CREATIVE_OUTPUT || "output/release-1.14.0/browser/creative";
let browser, endpoints, oldSettings;
const made = [];
async function api(path, body) {
  const r = await fetch(BASE + path, body === undefined ? {} : { method: "POST", headers: { "Content-Type": "application/json", "X-Airglow": "1" }, body: JSON.stringify(body) });
  const result = await r.json(); assert.ok(r.ok, `${path}: ${JSON.stringify(result)}`); return result;
}
async function eventually(read, accept, message) {
  const until = Date.now() + 10000; let value;
  do { value = await read(); if (accept(value)) return value; await new Promise(r => setTimeout(r, 80)); } while (Date.now() < until);
  assert.fail(`${message}: ${JSON.stringify(value)}`);
}
before(async () => {
  oldSettings = (await api("/api/settings")).settings;
  assert.equal(oldSettings.lhm_url, "http://127.0.0.1:8791");
  await api("/api/onboarded", {});
  await api("/api/settings", { design: "graphite", online_checks: false, appearance: { accent: null, scale: 100, density: "comfortable", rig: "frame", motion: "system" } });
  endpoints = [];
  for (const [name, color] of [["Creative A", "#ff0000"], ["Creative B", "#0000ff"]]) {
    const result = await api("/api/studio/save", { name, layers: [{ type: "solid", params: { color } }] });
    endpoints.push(result.effect); made.push(result.effect.fx_id);
  }
  let pw;
  if (process.env.AIRGLOW_PLAYWRIGHT_MODULE) { const p = process.env.AIRGLOW_PLAYWRIGHT_MODULE; pw = await import(isAbsolute(p) ? pathToFileURL(p).href : p); }
  else { try { pw = await import("playwright"); } catch(e) { if (e.code !== "ERR_MODULE_NOT_FOUND") throw e; pw = await import(pathToFileURL(join(process.env.APPDATA, "npm/node_modules/playwright/index.mjs")).href); } }
  browser = await pw.chromium.launch({ channel: process.env.AIRGLOW_BROWSER || "msedge" });
  mkdirSync(OUT, { recursive: true });
});
after(async () => {
  await browser?.close();
  for (const fx_id of made) await api("/api/studio/delete", { fx_id });
  if (oldSettings) await api("/api/settings", { design: oldSettings.design, appearance: oldSettings.appearance });
});
async function pageFor(t) {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 }, reducedMotion: "reduce" });
  const errors = [], writes = [];
  page.on("pageerror", e => errors.push(e.message));
  page.on("request", r => { if (r.method() === "POST") writes.push({ path: new URL(r.url()).pathname, body: r.postDataJSON() }); });
  page.setDefaultTimeout(10000);
  const previews = await installBrowserPreviewBridge(page, BASE);
  t.after(async () => {
    const previewStats = await previews.stats();
    await page.close(); await previews.close();
    assert.ok(previewStats.frames > 0, "The UI received real sandbox worker frames");
    t.diagnostic(`Real sandbox previews: ${previewStats.loads} loads, ${previewStats.frames} frames, ${previewStats.stops} explicit stops`);
    assert.deepEqual(errors, []);
  });
  await page.goto(BASE + "/#studio", { waitUntil: "domcontentloaded" });
  await page.locator(".st-home").waitFor();
  return { page, writes };
}
async function postResult(page, path, action) {
  const response = page.waitForResponse(r => r.request().method() === "POST" && r.url().endsWith(path));
  await action(); const r = await response, body = await r.json(); assert.ok(r.ok(), JSON.stringify(body)); return body;
}
function noHardware(writes) { assert.ok(!writes.some(x => x.path === "/api/studio/use" || x.path === "/api/scene" || (x.path === "/api/studio/draft" && x.body?.lights)), "Exploration and saving must not activate hardware"); }
async function fits(page) {
  const s = await page.evaluate(() => ({ width: innerWidth, scroll: document.documentElement.scrollWidth,
    overflow: [...document.querySelectorAll("body *")].filter(element => {
      const r = element.getBoundingClientRect(); return r.width && r.right > innerWidth + 1;
    }).slice(0, 12).map(element => ({ element: element.id || element.className || element.tagName,
      right: Math.round(element.getBoundingClientRect().right), width: Math.round(element.getBoundingClientRect().width) })) }));
  assert.ok(s.scroll <= s.width + 1, JSON.stringify(s));
}

test("paint supports freehand, gesture undo, numeric edits and saved-mask recovery without activation", async t => {
  const { page, writes } = await pageFor(t);
  await page.locator('[data-tpl="blank"]').click();
  await page.locator('[data-sel="0"]').click();
  await page.locator('[data-paint="toggle"]').click();
  await page.locator('[data-paint="clear"]').click();
  await eventually(() => page.locator("#st-paint-status").textContent(), x => x.startsWith("0 LEDs"), "Clear creates an empty mask");
  await page.locator("#st-paint-radius-number").fill("80");
  const canvas = page.locator("#st-canvas"); await canvas.scrollIntoViewIfNeeded();
  await eventually(() => page.locator("#st-meter").textContent(), x => x.includes("LEDs"), "Editor preview has rendered before drawing");
  const r = await canvas.boundingBox();
  // A configured LED need not be on the canvas centerline. One continuous sweep covers the visible rig.
  await page.mouse.move(r.x + 12, r.y + 12); await page.mouse.down();
  let right = true;
  for (let y = 12; y < r.height; y += 80) {
    await page.mouse.move(r.x + (right ? r.width - 12 : 12), r.y + y, { steps: 8 });
    right = !right;
  }
  await page.mouse.up();
  await eventually(() => page.locator("#st-paint-status").textContent(), x => !x.startsWith("0 LEDs"), "Freehand stroke selects LEDs");
  await page.keyboard.press("Control+z");
  await eventually(() => page.locator("#st-paint-status").textContent(), x => x.startsWith("0 LEDs"), "One undo reverts the stroke");
  await page.keyboard.press("Control+Shift+z");
  await eventually(() => page.locator("#st-paint-status").textContent(), x => !x.startsWith("0 LEDs"), "Redo restores the stroke");
  await page.locator('[data-paint="clear"]').click();
  await page.locator('details[data-details="paint-numeric"] summary').click();
  await page.locator("#st-paint-led").fill("1"); await page.locator('[data-paint="add-led"]').click();
  await page.locator("#st-name").fill("Painted fixture");
  const saved = await postResult(page, "/api/studio/save", () => page.locator('[data-st="save"]').click());
  made.push(saved.effect.fx_id);
  assert.equal(saved.effect.layers[0].paint.v, 1); assert.equal(saved.effect.layers[0].paint.points.length, 1);
  await page.screenshot({ path: join(OUT, "paint-desktop.png") });
  await page.reload(); await page.locator(`[data-edit="${saved.effect.fx_id}"]`).click();
  await page.locator('[data-sel="0"]').click();
  assert.match(await page.locator("#st-paint-status").textContent(), /1\/2048 positions/);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.locator(".st-paint").scrollIntoViewIfNeeded(); await fits(page);
  await page.screenshot({ path: join(OUT, "paint-narrow.png") });
  noHardware(writes);
});

test("cross-effect blend swaps endpoints, recovers drafts, retries failed saves and remains screen-only", async t => {
  await api("/api/settings", { design: "darkroom" });
  const { page, writes } = await pageFor(t);
  await page.getByRole("button", { name: "Blend saved effects", exact: true }).click();
  await page.locator("#st-crossblend-a").selectOption({ label: endpoints[0].name });
  await page.locator("#st-crossblend-b").selectOption({ label: endpoints[1].name });
  await page.locator("#st-crossblend-number").fill("25"); await page.locator("#st-crossblend-number").press("Tab");
  await page.locator("#st-crossblend-name").fill("Creative blend");
  await page.locator("#st-crossblend-swap").click();
  assert.equal(await page.locator("#st-crossblend-number").inputValue(), "75");
  await page.getByRole("button", { name: "Close blend", exact: true }).click();
  await page.getByRole("button", { name: "Blend saved effects", exact: true }).click();
  assert.equal(await page.locator("#st-crossblend-number").inputValue(), "75");
  assert.equal(await page.locator("#st-crossblend-name").inputValue(), "Creative blend");
  await page.route("**/api/studio/save", route => route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ error: "Fixture disk unavailable" }) }));
  await page.locator("#st-crossblend-save").click();
  await eventually(() => page.locator("#st-crossblend-status").textContent(), x => x.includes("Could not save"), "Save failure retains controls");
  assert.equal(await page.locator("#st-crossblend-number").inputValue(), "75");
  await page.unroute("**/api/studio/save");
  const saved = await postResult(page, "/api/studio/save", () => page.locator("#st-crossblend-save").click());
  made.push(saved.effect.fx_id);
  assert.equal(saved.effect.kind, "code"); assert.match(saved.effect.source, /Airglow saved-effects blend v1/);
  await eventually(() => page.locator("#st-crossblend-meter").textContent(), x => x.includes("ms / frame"), "Blend renders a real preview frame");
  await page.screenshot({ path: join(OUT, "blend-desktop.png") });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.evaluate(async () => (await import("/js/appearance.js")).applyAppearance({ scale: 120 }, { persist: false }));
  await page.locator("#st-crossblend-open").scrollIntoViewIfNeeded(); await fits(page);
  const box = await page.locator("#sheet").boundingBox(); assert.ok(box.x >= -1 && box.x + box.width <= 391);
  await page.screenshot({ path: join(OUT, "blend-narrow-120.png") });
  await page.locator("#st-crossblend-open").click(); await page.locator(".st-edit").waitFor();
  assert.equal(await page.locator("#st-name").inputValue(), "Creative blend");
  noHardware(writes);
});
