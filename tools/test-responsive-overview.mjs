// Run against tools/dev_server.py --demo on 8790, never the installed app.
// node --test tools/test-responsive-overview.mjs
import assert from "node:assert/strict";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { isAbsolute, join } from "node:path";
import { test } from "node:test";
import { pathToFileURL } from "node:url";

const BASE = "http://127.0.0.1:8790";
async function loadPlaywright() {
  const moduleName = process.env.AIRGLOW_PLAYWRIGHT_MODULE;
  if (moduleName) return import(isAbsolute(moduleName) ? pathToFileURL(moduleName).href : moduleName);
  try { return await import("playwright"); }
  catch (error) {
    if (error.code !== "ERR_MODULE_NOT_FOUND" || !process.env.APPDATA) throw error;
    return import(pathToFileURL(join(process.env.APPDATA, "npm", "node_modules", "playwright", "index.mjs")).href);
  }
}
const { chromium } = await loadPlaywright();

async function api(path, body) {
  const response = await fetch(BASE + path, body === undefined ? {} : {
    method: "POST", headers: { "Content-Type": "application/json", "X-Airglow": "1" }, body: JSON.stringify(body),
  });
  assert.ok(response.ok, `${path}: HTTP ${response.status}`);
  return response.json();
}

test("Overview uses large displays and keeps responsive tile editing accurate", async () => {
  const settings = await api("/api/settings");
  assert.equal(settings.settings.lhm_url, "http://127.0.0.1:8791", "The fixture must use simulated sensors");
  const original = await api("/api/overview");
  const browser = await chromium.launch({ channel: process.env.AIRGLOW_BROWSER || "msedge" });
  const screenshots = mkdtempSync(join(tmpdir(), "airglow-responsive-"));
  try {
    await api("/api/onboarded", {});
    await api("/api/settings", { online_checks: false, design: "darkroom" });
    const page = await browser.newPage({ viewport: { width: 1920, height: 1080 }, reducedMotion: "reduce" });
    const errors = [];
    let writes = 0;
    page.on("pageerror", (error) => errors.push(error.message));
    page.on("request", (request) => {
      if (request.method() === "POST" && request.url() === BASE + "/api/overview") writes++;
    });
    await page.goto(`${BASE}/#overview`, { waitUntil: "domcontentloaded" });
    await page.locator("#ov-grid .tile-sensor").first().waitFor({ state: "visible" });
    const initialWrites = writes;
    let previousHeight = 0;
    for (const [width, height, columns] of [[1920, 1080, 4], [2560, 1440, 4], [3840, 2160, 4],
      [3440, 1440, 4], [1080, 1920, 4], [700, 900, 2], [390, 844, 1]]) {
      await page.setViewportSize({ width, height });
      await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
      const geometry = await page.evaluate(() => {
        const grid = document.querySelector("#ov-grid"), view = document.querySelector('[data-view="overview"]');
        const r = grid.getBoundingClientRect(), style = getComputedStyle(grid);
        return { width: r.width, top: r.top, bottom: r.bottom, columns: style.gridTemplateColumns.split(" ").length,
          rowHeight: parseFloat(style.gridAutoRows), scrollWidth: view.scrollWidth, viewWidth: view.clientWidth,
          headerSize: parseFloat(getComputedStyle(grid.querySelector(".tile-head")).fontSize),
          navSize: parseFloat(getComputedStyle(document.querySelector('.tabs a')).fontSize) };
      });
      assert.equal(geometry.columns, columns, `${width}x${height}: wrong column count`);
      assert.ok(geometry.scrollWidth <= geometry.viewWidth + 1, `${width}x${height}: horizontal overflow`);
      if (width >= 1920) {
        assert.ok(geometry.width >= width * 0.94, `${width}x${height}: dashboard too narrow: ${JSON.stringify(geometry)}`);
        assert.ok(Math.min(geometry.bottom, height) - geometry.top >= height * 0.74,
          `${width}x${height}: dashboard leaves too much vertical space: ${JSON.stringify(geometry)}`);
        if (width !== 3440) {
          assert.ok(geometry.rowHeight > previousHeight, `Tiles should grow with the display: ${JSON.stringify(geometry)}`);
          previousHeight = geometry.rowHeight;
        }
        if (width === 3840) {
          assert.ok(geometry.headerSize >= 20, "Large-screen labels must remain readable");
          assert.ok(geometry.navSize >= 18, "Large-screen navigation must remain readable");
        }
        await page.screenshot({ path: join(screenshots, `darkroom-${width}x${height}.png`), animations: "disabled" });
      }
    }
    await page.waitForTimeout(450);
    assert.equal(writes, initialWrites, "Resizing the window must not save a new tile layout");
    assert.deepEqual(await api("/api/overview"), original, "Window resizing must preserve saved tile sizes");

    await page.setViewportSize({ width: 3840, height: 2160 });
    for (const design of ["ascii", "paper", "amber", "contrast", "neon", "kiln", "lumen", "midnight", "spore", "rim", "darkroom", "graphite"]) {
      await api("/api/settings", { design });
      await page.reload({ waitUntil: "domcontentloaded" });
      await page.locator("#ov-grid .tile-sensor").first().waitFor({ state: "visible" });
      await page.waitForFunction((chosen) => document.documentElement.dataset.design === chosen, design);
      const bounds = await page.evaluate(() => {
        const rect = (selector) => document.querySelector(selector).getBoundingClientRect();
        return { wordmark: rect(".wordmark").right, tabsLeft: rect(".tabs").left, tabsRight: rect(".tabs").right,
          controls: rect(".bar-right").left, viewWidth: document.querySelector('[data-view="overview"]').clientWidth,
          scrollWidth: document.querySelector('[data-view="overview"]').scrollWidth };
      });
      assert.ok(bounds.wordmark <= bounds.tabsLeft && bounds.tabsRight <= bounds.controls,
        `${design}: navigation overlaps at 4K: ${JSON.stringify(bounds)}`);
      assert.ok(bounds.scrollWidth <= bounds.viewWidth + 1, `${design}: Overview overflows at 4K`);
    }
    await page.screenshot({ path: join(screenshots, "graphite-3840x2160.png"), animations: "disabled" });
    await page.locator("#ov-edit").click();
    const tile = page.locator(".tile-sensor[data-h='1']").first();
    const id = await tile.getAttribute("data-id");
    const rowStep = await page.locator("#ov-grid").evaluate((grid) => {
      const style = getComputedStyle(grid);
      return parseFloat(style.gridAutoRows) + parseFloat(style.rowGap);
    });
    const handle = tile.locator('[data-act="resize"]');
    let box = await handle.boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2 + rowStep * 0.3, { steps: 5 });
    await page.mouse.up();
    assert.equal((await api("/api/overview")).tiles.find((item) => item.id === id).h, 1,
      "Dragging less than half a tall row must not resize the tile");
    box = await handle.boundingBox();
    const saved = page.waitForResponse((response) => response.url() === BASE + "/api/overview" && response.request().method() === "POST");
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2 + rowStep, { steps: 8 });
    await page.mouse.up();
    assert.ok((await saved).ok());
    assert.equal((await api("/api/overview")).tiles.find((item) => item.id === id).h, 2,
      "Dragging one rendered row must add one row");
    assert.deepEqual(errors, [], "The responsive Overview must not produce browser errors");
    console.log(`Responsive screenshots: ${screenshots}`);
  } finally {
    await api("/api/overview", original);
    await api("/api/settings", { design: settings.settings.design, online_checks: settings.settings.online_checks });
    await browser.close();
  }
});
