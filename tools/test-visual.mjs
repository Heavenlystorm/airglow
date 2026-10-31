// Run against tools/dev_server.py --demo on 8790, never the installed app on 8787.
// AIRGLOW_PLAYWRIGHT_MODULE can point to playwright/index.mjs; Edge is used on Windows.
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const OUTPUT = process.env.AIRGLOW_VISUAL_OUTPUT ? resolve(process.env.AIRGLOW_VISUAL_OUTPUT) : join(ROOT, "output", "visual-overhaul", "checks");
const BASE = "http://127.0.0.1:8790";
const THEMES = ["graphite", "ascii", "paper", "amber", "contrast", "neon", "kiln", "lumen", "midnight", "spore", "rim", "darkroom"];
const VIEWS = ["overview", "lighting", "studio", "fans", "automations", "devices", "settings"];
const READY = {
  overview: "#ov-grid .tile-sensor", lighting: "#deck [data-effect]", studio: "#studio .st-home",
  fans: "#curve-seg button", automations: "#starters .starter, #rules > li",
  devices: "#dev-list [data-dev]", settings: "#s-design",
};
const result = { started: new Date().toISOString(), base: BASE, checks: [], pageErrors: [], failures: [] };
let current = "startup";

async function api(path, body) {
  const response = await fetch(BASE + path, body === undefined ? {} : {
    method: "POST", headers: { "Content-Type": "application/json", "X-Airglow": "1" }, body: JSON.stringify(body),
  });
  assert.ok(response.ok, `${path}: HTTP ${response.status}`);
  return response.json();
}

async function loadPlaywright() {
  const moduleName = process.env.AIRGLOW_PLAYWRIGHT_MODULE;
  if (moduleName) return import(isAbsolute(moduleName) ? pathToFileURL(moduleName).href : moduleName);
  try { return await import("playwright"); }
  catch (error) {
    if (error.code !== "ERR_MODULE_NOT_FOUND" || !process.env.APPDATA) throw error;
    return import(pathToFileURL(join(process.env.APPDATA, "npm", "node_modules", "playwright", "index.mjs")).href);
  }
}

async function check(name, action) {
  current = name;
  try {
    await action();
    result.checks.push(name);
    console.log(`PASS ${name}`);
  } catch (error) {
    result.failures.push({ check: name, message: error.message });
    console.error(`FAIL ${name}: ${error.message}`);
  }
}

mkdirSync(OUTPUT, { recursive: true });
const settings = await api("/api/settings");
assert.equal(settings.settings.lhm_url, "http://127.0.0.1:8791", "The fixture must use simulated sensors");
const playwright = await loadPlaywright();
const browser = await playwright.chromium.launch({ channel: process.env.AIRGLOW_BROWSER || "msedge" });
let overviewToRestore;
try {
  const context = await browser.newContext({ viewport: { width: 1280, height: 840 }, deviceScaleFactor: 1, reducedMotion: "reduce" });
  const page = await context.newPage();
  page.setDefaultTimeout(12000);
  page.on("pageerror", (error) => result.pageErrors.push({ check: current, message: error.message }));
  let overviewWrites = 0;
  page.on("request", (request) => {
    if (request.method() === "POST" && request.url() === BASE + "/api/overview") overviewWrites++;
  });
  await api("/api/onboarded", {});
  await api("/api/settings", { online_checks: false, design: "graphite" });

  async function settled() {
    await page.evaluate(async () => {
      await document.fonts.ready;
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    });
  }

  async function visit(view) {
    await page.locator(`[data-tab="${view}"]`).click();
    await page.locator(`[data-view="${view}"]`).waitFor({ state: "visible" });
    await page.locator(READY[view]).first().waitFor({ state: "visible" });
    assert.equal(await page.locator(`[data-tab="${view}"]`).getAttribute("aria-current"), "page");
    assert.equal(await page.locator(".view:visible").count(), 1, "Exactly one view should be visible");
    await page.locator(`[data-view="${view}"]`).evaluate((element) => { element.scrollTop = 0; });
    await settled();
  }

  async function layout(name, view, dialog = false) {
    const geometry = await page.evaluate(({ view, dialog }) => {
      const root = document.documentElement;
      const selectors = ["main", `.view[data-view="${view}"]`, ...(dialog ? ["#sheet"] : [])];
      return {
        document: { scroll: root.scrollWidth, width: root.clientWidth },
        elements: selectors.map((selector) => {
          const element = document.querySelector(selector), rect = element.getBoundingClientRect();
          return { selector, scroll: element.scrollWidth, width: element.clientWidth, left: rect.left, right: rect.right };
        }),
      };
    }, { view, dialog });
    await page.screenshot({ path: join(OUTPUT, `${name}.png`), animations: "disabled" });
    assert.ok(geometry.document.scroll <= geometry.document.width + 1,
      `Document overflows horizontally: ${JSON.stringify(geometry.document)}`);
    for (const element of geometry.elements) {
      assert.ok(element.width > 0, `${element.selector} must have a rendered width`);
      assert.ok(element.scroll <= element.width + 1, `Horizontal overflow: ${JSON.stringify(element)}`);
      assert.ok(element.left >= -1 && element.right <= geometry.document.width + 1,
        `Outside viewport: ${JSON.stringify(element)}`);
    }
  }

  async function keyboardFocus() {
    // Tab must move to a real, visible control with a painted keyboard indicator.
    await page.locator('[data-tab="overview"]').focus();
    await page.keyboard.press("Tab");
    const focus = await page.evaluate(() => {
      const element = document.activeElement, style = getComputedStyle(element), rect = element.getBoundingClientRect();
      return { tab: element.dataset.tab, visible: element.matches(":focus-visible"), width: rect.width, height: rect.height,
        outline: style.outlineStyle !== "none" && parseFloat(style.outlineWidth) > 0,
        shadow: style.boxShadow !== "none" };
    });
    assert.equal(focus.tab, "lighting", "Tab should advance through navigation");
    assert.ok(focus.visible && focus.width > 0 && focus.height > 0, `Keyboard focus is not visible: ${JSON.stringify(focus)}`);
    assert.ok(focus.outline || focus.shadow, `Keyboard focus has no painted indicator: ${JSON.stringify(focus)}`);
  }

  async function headerBounds() {
    const boxes = await page.evaluate(() => {
      const rect = (selector) => {
        const r = document.querySelector(selector).getBoundingClientRect();
        return { selector, left: r.left, right: r.right, top: r.top, bottom: r.bottom };
      };
      return { parent: rect(".appbar"), children: [rect(".wordmark"), rect(".tabs"), rect(".bar-right")],
        tabs: [...document.querySelectorAll(".tabs a")].map((a) => rect(`[data-tab="${a.dataset.tab}"]`)) };
    });
    for (const box of boxes.children) {
      assert.ok(box.left >= boxes.parent.left && box.right <= boxes.parent.right,
        `Header child is outside the header: ${JSON.stringify(box)}`);
    }
    for (let i = 1; i < boxes.children.length; i++) {
      assert.ok(boxes.children[i - 1].right <= boxes.children[i].left + 1,
        `Header sections overlap: ${JSON.stringify(boxes.children)}`);
    }
    const nav = boxes.children[1];
    for (const tab of boxes.tabs) {
      assert.ok(tab.left >= nav.left - 1 && tab.right <= nav.right + 1,
        `Navigation text escapes its header allocation: ${JSON.stringify({ tab, nav })}`);
    }
  }

  async function reachable(selector) {
    const control = page.locator(selector).first();
    await control.scrollIntoViewIfNeeded();
    const status = await control.evaluate((element) => {
      const r = element.getBoundingClientRect(), x = (r.left + r.right) / 2, y = (r.top + r.bottom) / 2;
      const hit = document.elementFromPoint(x, y);
      return { width: r.width, height: r.height, left: r.left, right: r.right, top: r.top, bottom: r.bottom,
        viewportWidth: innerWidth, viewportHeight: innerHeight, hit: hit === element || element.contains(hit),
        coveredBy: hit?.outerHTML.slice(0, 150) };
    });
    assert.ok(status.width > 0 && status.height > 0 && status.left >= -1 && status.right <= status.viewportWidth + 1
      && status.top >= -1 && status.bottom <= status.viewportHeight + 1 && status.hit,
    `Control is clipped or unreachable (${selector}): ${JSON.stringify(status)}`);
  }

  async function studioEditor(size, mode) {
    await visit("studio");
    await page.locator(mode === "layers" ? '[data-tpl="blank"]' : '[data-code-tpl="canvas"]').click();
    await page.locator(".st-edit").waitFor({ state: "visible" });
    if (mode === "code") await page.locator("#st-code .CodeMirror").waitFor({ state: "visible" });
    else await page.locator("#st-list [data-sel]").first().waitFor({ state: "visible" });
    for (const selector of ['#st-name', '[data-mode="layers"]', '[data-mode="code"]',
      '.st-top [data-st="check"]', '.st-top [data-st="save"]', '.st-top [data-st="use"]', '.st-top [data-st="share"]',
      mode === "layers" ? '[data-st="add"]' : '#st-code .CodeMirror', '#st-canvas']) await reachable(selector);
    await page.locator(".st-top").scrollIntoViewIfNeeded();
    await settled();
    await layout(`${size}-studio-${mode}-editor`, "studio");
    await page.locator('a[aria-label="Back to the Studio"]').click();
    await page.locator(READY.studio).waitFor({ state: "visible" });
  }

  async function fanControls(size) {
    await visit("fans");
    for (const selector of ["#curve-seg button", "#curve-reset", "#curve-save", "#channels select"])
      await reachable(selector);
    await page.screenshot({ path: join(OUTPUT, `${size}-fan-controls.png`), animations: "disabled" });
  }

  for (const viewport of [{ width: 1280, height: 840 }, { width: 1040, height: 720 }]) {
    await page.setViewportSize(viewport);
    const size = `${viewport.width}x${viewport.height}`;
    for (const theme of THEMES) {
      current = `${size}-${theme}-boot`;
      await api("/api/settings", { design: theme });
      // A unique query forces boot to read the saved design instead of only changing the hash.
      await page.goto(`${BASE}/?visual=${theme}-${size}#overview`, { waitUntil: "domcontentloaded" });
      await page.locator(READY.overview).first().waitFor({ state: "visible" });
      await page.waitForFunction((id) => document.documentElement.dataset.design === id, theme);
      for (const view of theme === "graphite" ? VIEWS : ["overview", "lighting", "settings"]) {
        await check(`${size}-${theme}-${view}`, async () => {
          await visit(view);
          assert.equal(await page.locator("html").getAttribute("data-design"), theme, "Navigation must retain the chosen design");
          await layout(`${size}-${theme}-${view}`, view);
        });
      }
      await check(`${size}-${theme}-keyboard-focus`, keyboardFocus);
      if (viewport.width === 1040 && ["ascii", "amber", "rim"].includes(theme))
        await check(`${size}-${theme}-header-bounds`, headerBounds);
    }

    await api("/api/settings", { design: "graphite" });
    await page.reload({ waitUntil: "domcontentloaded" });
    await page.locator(READY.settings).waitFor({ state: "visible" });
    await check(`${size}-customize-tile-settings`, async () => {
      await visit("overview");
      await page.locator("#ov-edit").click();
      assert.equal(await page.locator("#ov-edit").getAttribute("aria-pressed"), "true");
      assert.ok(await page.locator("#ov-editbar").isVisible());
      await settled();
      await layout(`${size}-customize`, "overview");
      await page.locator('.tile-sensor [data-act="edit"]').first().click();
      await page.locator("#ts-form").waitFor({ state: "visible" });
      assert.equal(await page.locator("#sheet h2").textContent(), "Tile settings");
      await page.locator("#ts-label").focus();
      await page.keyboard.press("Tab");
      assert.ok(await page.evaluate(() => document.querySelector("#sheet").contains(document.activeElement)));
      await settled();
      await layout(`${size}-tile-settings`, "overview", true);
      await page.keyboard.press("Escape");
      await page.locator("#sheet").waitFor({ state: "hidden" });
      await page.locator("#ov-edit").click();
      assert.equal(await page.locator("#ov-edit").getAttribute("aria-pressed"), "false");
    });
    // A failed geometry assertion must not leave its dialog blocking the next check.
    await page.keyboard.press("Escape");

    await check(`${size}-theme-dialog-persistence`, async () => {
      await visit("settings");
      await page.locator("#s-design").click();
      await page.locator(".design-gallery").waitFor({ state: "visible" });
      assert.equal(await page.locator("[data-design-card]").count(), 12);
      assert.equal(await page.locator('[data-design-card][aria-checked="true"]').getAttribute("data-design-card"), "graphite");
      const done = page.locator("#sheet [data-close]");
      await done.focus();
      await page.keyboard.press("Tab");
      assert.equal(await page.evaluate(() => document.activeElement.dataset.designCard), "graphite", "Dialog focus should wrap to its first choice");
      await page.keyboard.press("Shift+Tab");
      assert.ok(await done.evaluate((element) => element === document.activeElement), "Reverse Tab should stay inside the dialog");
      await settled();
      await layout(`${size}-theme-dialog`, "settings", true);
      const saved = page.waitForResponse((response) => response.url() === BASE + "/api/settings" && response.request().method() === "POST");
      await page.locator('[data-design-card="paper"]').click();
      assert.ok((await saved).ok(), "Design selection should persist successfully");
      await page.waitForFunction(() => document.documentElement.dataset.design === "paper");
      assert.equal((await api("/api/settings")).settings.design, "paper");
      await done.click();
      await page.reload({ waitUntil: "domcontentloaded" });
      await page.locator(READY.settings).waitFor({ state: "visible" });
      assert.equal(await page.locator("html").getAttribute("data-design"), "paper", "Reload should retain the saved theme");
      assert.equal(await page.evaluate(() => localStorage.getItem("airglow.design")), "paper");
    });
    await page.keyboard.press("Escape");

    await api("/api/settings", { design: "graphite" });
    await page.reload({ waitUntil: "domcontentloaded" });
    await page.locator(READY.settings).waitFor({ state: "visible" });
    for (const mode of ["layers", "code"])
      await check(`${size}-studio-${mode}-editor`, () => studioEditor(size, mode));
    await check(`${size}-fan-controls`, () => fanControls(size));
  }

  await api("/api/settings", { design: "graphite" });
  for (const viewport of [{ width: 390, height: 844 }, { width: 700, height: 900 }]) {
    const size = `${viewport.width}x${viewport.height}`;
    await page.setViewportSize(viewport);
    await page.goto(`${BASE}/?visual=graphite-${size}#overview`, { waitUntil: "domcontentloaded" });
    await page.locator(READY.overview).first().waitFor({ state: "visible" });
    for (const view of VIEWS) await check(`${size}-graphite-${view}`, async () => {
      await visit(view);
      await layout(`${size}-graphite-${view}`, view);
    });
    await check(`${size}-fan-controls`, () => fanControls(size));
  }

  await check("overview-responsive-size-preservation", async () => {
    overviewToRestore = await api("/api/overview");
    const wide = structuredClone(overviewToRestore), tile = wide.tiles.find((t) => t.type === "sensor");
    assert.ok(tile, "The fixture must contain a sensor tile");
    tile.w = 4;
    tile.h = 1;
    const stored = await api("/api/overview", wide);
    await page.setViewportSize({ width: 1280, height: 840 });
    await page.goto(`${BASE}/?visual=wide-tile#overview`, { waitUntil: "domcontentloaded" });
    const selector = `.tile[data-id="${tile.id}"]`;
    await page.locator(selector).waitFor({ state: "visible" });
    const before = overviewWrites;
    for (const [width, height, columns] of [[700, 900, 2], [390, 844, 1], [1280, 840, 4]]) {
      await page.setViewportSize({ width, height });
      await settled();
      const geometry = await page.locator(selector).evaluate((element) => {
        const grid = document.querySelector("#ov-grid"), style = getComputedStyle(grid);
        return { savedWidth: element.dataset.w, width: element.getBoundingClientRect().width,
          available: grid.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight),
          columns: style.gridTemplateColumns.split(" ").length };
      });
      assert.equal(geometry.savedWidth, "4", "Display clamping must preserve the tile's saved width");
      assert.equal(geometry.columns, columns);
      assert.ok(Math.abs(geometry.width - geometry.available) < 2, `A full-width tile must fit its grid: ${JSON.stringify(geometry)}`);
      await layout(`${width}x${height}-four-column-tile`, "overview");
    }
    await page.waitForTimeout(450); // Past the 350 ms autosave delay: resizing alone must never schedule a write.
    assert.equal(overviewWrites, before, "Viewport changes must not POST a new Overview layout");
    assert.deepEqual(await api("/api/overview"), stored, "Viewport changes must not alter persisted layout");
  });

  await check("overview-narrow-keyboard-and-pointer-resize", async () => {
    assert.ok(overviewToRestore, "The wide-tile fixture must have been created");
    await page.setViewportSize({ width: 700, height: 900 });
    await visit("overview");
    if (await page.locator("#ov-edit").getAttribute("aria-pressed") !== "true") await page.locator("#ov-edit").click();
    const tile = overviewToRestore.tiles.find((t) => t.type === "sensor");
    const locator = page.locator(`.tile[data-id="${tile.id}"]`);
    await locator.evaluate((element) => element.scrollIntoView({ block: "center" }));
    const verticalHandle = await locator.locator('[data-act="resize"]').boundingBox();
    const verticalSave = page.waitForResponse((response) => response.url() === BASE + "/api/overview" && response.request().method() === "POST");
    await page.mouse.move(verticalHandle.x + verticalHandle.width / 2, verticalHandle.y + verticalHandle.height / 2);
    await page.mouse.down();
    await page.mouse.move(verticalHandle.x + verticalHandle.width / 2, verticalHandle.y + verticalHandle.height / 2 + 164, { steps: 8 });
    await page.mouse.up();
    assert.ok((await verticalSave).ok());
    const taller = (await api("/api/overview")).tiles.find((t) => t.id === tile.id);
    assert.equal(taller.w, 4, "Vertical-only resizing must preserve the saved four-column width");
    assert.equal(taller.h, 2, "The pointer drag must actually change the tile height");
    async function resizeKey(key, width) {
      await locator.focus();
      const saved = page.waitForResponse((response) => response.url() === BASE + "/api/overview" && response.request().method() === "POST");
      await page.keyboard.press(key);
      assert.ok((await saved).ok());
      assert.equal((await api("/api/overview")).tiles.find((t) => t.id === tile.id).w, width);
    }
    await resizeKey("Shift+ArrowLeft", 1);
    await resizeKey("Shift+ArrowRight", 2);
    const handle = locator.locator('[data-act="resize"]');
    await handle.scrollIntoViewIfNeeded();
    const box = await handle.boundingBox();
    const saved = page.waitForResponse((response) => response.url() === BASE + "/api/overview" && response.request().method() === "POST");
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width / 2 - 300, box.y + box.height / 2, { steps: 8 });
    await page.mouse.up();
    assert.ok((await saved).ok());
    assert.equal((await api("/api/overview")).tiles.find((t) => t.id === tile.id).w, 1);
    await layout("700x900-resized-tile", "overview");
    await page.locator("#ov-edit").click();
  });
} catch (error) {
  result.failures.push({ check: current, message: error.stack || error.message });
} finally {
  if (overviewToRestore) {
    try { await api("/api/overview", overviewToRestore); }
    catch (error) { result.failures.push({ check: "restore Overview", message: error.message }); }
  }
  try { await api("/api/settings", { design: "graphite" }); }
  catch (error) { result.failures.push({ check: "restore graphite", message: error.message }); }
  await browser.close();
  result.finished = new Date().toISOString();
  writeFileSync(join(OUTPUT, "results.json"), JSON.stringify(result, null, 2) + "\n");
}

console.log(`${result.checks.length} checks passed, ${result.failures.length} failures, ${result.pageErrors.length} browser errors. Screenshots: ${OUTPUT}`);
for (const error of result.pageErrors) console.error(`PAGE ERROR ${error.check}: ${error.message}`);
process.exitCode = result.failures.length || result.pageErrors.length ? 1 : 0;
