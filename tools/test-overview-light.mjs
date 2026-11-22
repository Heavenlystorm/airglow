// The Overview's light graphs, the pulse beside the workspace title and the detail view, in a real browser against
// the isolated fixture (tools/run-ui-checks.py --suite overview-light).
import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { mkdirSync } from "node:fs";
import { isAbsolute, join } from "node:path";
import { pathToFileURL } from "node:url";

const BASE = process.env.AIRGLOW_FIXTURE_URL || "http://127.0.0.1:8790";
const OUTPUT = process.env.AIRGLOW_LIGHT_OUTPUT || "output/overview-light";
mkdirSync(OUTPUT, { recursive: true });
let browser, original, settings;

async function api(path, body) {
  const response = await fetch(BASE + path, body === undefined ? {} : {
    method: "POST", headers: { "Content-Type": "application/json", "X-Airglow": "1" }, body: JSON.stringify(body),
  });
  const data = await response.json();
  assert.ok(response.ok, `${path}: ${JSON.stringify(data)}`);
  return data;
}

const LAYOUT = { tiles: [
  { id: "cpu", type: "sensor", sensors: ["auto/cpu_temp"], style: "spark", w: 1, h: 1 },
  { id: "load", type: "sensor", sensors: ["auto/cpu_load"], style: "ring", w: 1, h: 1 },
  { id: "power", type: "sensor", sensors: ["auto/cpu_power", "auto/gpu_power"], style: "graph", w: 2, h: 1 },
  { id: "gpu", type: "sensor", sensors: ["auto/gpu_temp"], style: "auto", w: 1, h: 2 },
  { id: "ram", type: "sensor", sensors: ["auto/ram_load"], style: "bar", w: 1, h: 1 },
], pins: ["auto/cpu_temp", "auto/gpu_temp"] };

before(async () => {
  settings = (await api("/api/settings")).settings;
  assert.equal(settings.lhm_url, "http://127.0.0.1:8791", "Only the isolated fixture may be changed");
  original = await api("/api/overview");
  await api("/api/onboarded", {});
  await api("/api/settings", { online_checks: false, design: "firefly" });
  const name = process.env.AIRGLOW_PLAYWRIGHT_MODULE;
  let playwright;
  if (name) playwright = await import(isAbsolute(name) ? pathToFileURL(name).href : name);
  else try { playwright = await import("playwright"); } catch {
    playwright = await import(pathToFileURL(join(process.env.APPDATA, "npm/node_modules/playwright/index.mjs")).href);
  }
  browser = await playwright.chromium.launch({ channel: process.env.AIRGLOW_BROWSER || "msedge" });
});

after(async () => {
  await browser?.close();
  if (original) await api("/api/overview", { ...original, workspaces: original.workspaces || [], active_workspace: original.active_workspace || null });
  if (settings) await api("/api/settings", { design: settings.design, appearance: settings.appearance, online_checks: settings.online_checks });
});

async function pageFor(t, { layout = LAYOUT, reducedMotion = "reduce", width = 1440, height = 900 } = {}) {
  await api("/api/overview", layout);
  const page = await browser.newPage({ viewport: { width, height }, reducedMotion }), errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.setDefaultTimeout(10000);
  t.after(async () => { await page.close(); assert.deepEqual(errors, [], "No page exceptions"); });
  await page.goto(`${BASE}/#overview`);
  await page.locator("#ov-grid .tile-sensor").first().waitFor();
  return page;
}

// How many pixels of a canvas carry any paint.
const painted = (locator) => locator.evaluate((canvas) => {
  const data = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data;
  let count = 0;
  for (let i = 3; i < data.length; i += 4) if (data[i] > 8) count++;
  return count;
});

test("sensor tiles draw on canvas, keep their SVG for forced colours, and stay inside their tile", async (t) => {
  const page = await pageFor(t);
  for (const id of ["cpu", "power", "gpu"]) {
    const canvas = page.locator(`[data-id="${id}"] .light-canvas`);
    await page.waitForFunction((tile) => document.querySelector(`[data-id="${tile}"] .light-canvas`)?.dataset.series > 0, id);
    assert.ok(await painted(canvas) > 40, `${id}: the line is drawn`);
    const boxes = await page.evaluate((tile) => {
      const el = document.querySelector(`[data-id="${tile}"]`), a = el.getBoundingClientRect(), b = el.querySelector(".light-canvas").getBoundingClientRect();
      return { inside: b.left >= a.left - 1 && b.right <= a.right + 1 && b.top >= a.top - 1 && b.bottom <= a.bottom + 1, width: b.width, height: b.height };
    }, id);
    assert.ok(boxes.inside && boxes.width > 60 && boxes.height > 24, `${id}: ${JSON.stringify(boxes)}`);
  }
  assert.equal(await page.locator('[data-id="power"] .light-canvas').getAttribute("data-series"), "2", "both lines have a live head");
  await page.waitForFunction(() => document.querySelector('[data-id="load"] .light-canvas')?.dataset.fraction != null);
  assert.ok(await painted(page.locator('[data-id="load"] .light-canvas')) > 200, "the gauge is drawn");
  assert.equal(await page.locator('[data-id="ram"] .light-canvas').count(), 0, "a bar needs no canvas");
  assert.equal(await page.locator('[data-id="cpu"] .spark').evaluate((el) => getComputedStyle(el).visibility), "hidden");
  assert.match(await page.locator('[data-id="cpu"] .spark .line').evaluate((el) => el.style.d), /^path\("M/, "the fallback path is still maintained");

  await page.emulateMedia({ forcedColors: "active", reducedMotion: "reduce" });
  assert.equal(await page.locator('[data-id="cpu"] .light-host').evaluate((el) => getComputedStyle(el).display), "none");
  assert.equal(await page.locator('[data-id="cpu"] .spark').evaluate((el) => getComputedStyle(el).visibility), "visible");
  assert.equal(await page.locator('[data-id="load"] .g-fill').evaluate((el) => getComputedStyle(el).visibility), "visible");
});

test("moving along a graph reads the value it showed, and leaving puts the reading away", async (t) => {
  const page = await pageFor(t);
  await page.waitForFunction(() => document.querySelector('[data-id="power"] .light-canvas')?.dataset.series > 0);
  // the fixture has only seconds of history at first: wait until the line reaches back to the point we hover
  const chart = page.locator('[data-id="power"] .chart');
  await page.waitForFunction(() => {
    const el = document.querySelector('[data-id="power"]'), chart = el.querySelector(".chart").getBoundingClientRect();
    const canvas = el._light.canvas, box = canvas.getBoundingClientRect(), x = chart.left + chart.width - 56 - 18;
    return el._light.probe((x - box.left) * canvas.clientWidth / box.width)?.values.some(Number.isFinite);
  }, null, { timeout: 60000 });
  // hover() measures at the moment it acts, so a notice that opens above the grid cannot move the tile away
  const box = await chart.boundingBox();
  await chart.hover({ position: { x: box.width - 56 - 18, y: (box.height - 18) / 2 } });
  const tip = page.locator('[data-id="power"] .light-tip');
  await tip.waitFor({ state: "visible" });
  assert.match(await tip.textContent(), /\d+ W.*\d+ W.*(now|s ago)/);
  const edge = await page.evaluate(() => {
    const tile = document.querySelector('[data-id="power"]').getBoundingClientRect(), tip = document.querySelector('[data-id="power"] .light-tip').getBoundingClientRect();
    return tip.left >= tile.left && tip.right <= tile.right && tip.top >= tile.top;
  });
  assert.ok(edge, "the reading stays inside its tile");
  await page.mouse.move(4, 4);
  await tip.waitFor({ state: "hidden" });
});

test("the pulse follows the pins, keeps its time span on this PC and steps aside for Customize", async (t) => {
  const page = await pageFor(t);
  const pins = page.locator("#ov-pulse .pulse-pin");
  assert.equal(await pins.count(), 2);
  const shown = await page.evaluate(async () => {
    const { R, fmt, value } = await import("/js/readings.js");
    return [...document.querySelectorAll("#ov-pulse .pulse-pin")].map((el) => ({
      text: el.querySelector(".num").textContent + el.querySelector(".unit").textContent,
      expected: Object.values(fmt(R.meta[el.dataset.pin], value(el.dataset.pin))).join(""),
    }));
  });
  for (const pin of shown) assert.equal(pin.text, pin.expected);
  await page.waitForFunction(() => document.querySelector("#ov-pulse .light-canvas")?.dataset.series > 0);
  assert.ok(await painted(page.locator("#ov-pulse .light-canvas")) > 40);
  assert.equal(await page.locator('#ov-pulse [data-span="60"]').getAttribute("aria-checked"), "true", "one minute by default");
  await page.locator('#ov-pulse [data-span="300"]').click();
  assert.equal(await page.locator('#ov-pulse [data-span="300"]').getAttribute("aria-checked"), "true");
  await page.reload();
  await page.locator("#ov-pulse .pulse-pin").first().waitFor();
  assert.equal(await page.locator('#ov-pulse [data-span="300"]').getAttribute("aria-checked"), "true", "the span is remembered");
  const overflow = await page.evaluate(() => { const view = document.querySelector(".overview-view"); return view.scrollWidth - view.clientWidth; });
  assert.ok(overflow <= 1, `the title row fits: ${overflow}`);

  await page.locator("#ov-edit").click();
  await page.locator("#ov-pulse").waitFor({ state: "hidden" });
  await page.locator("#ov-edit").click();
  await page.locator("#ov-pulse").waitFor({ state: "visible" });
  await page.screenshot({ path: join(OUTPUT, "pulse.png") });
});

test("without pins the pulse says how to get one", async (t) => {
  const page = await pageFor(t, { layout: { ...LAYOUT, pins: [] } });
  assert.equal(await page.locator("#ov-pulse").getAttribute("data-empty"), "true");
  assert.match(await page.locator(".pulse-empty").textContent(), /Pin a reading/);
  assert.equal(await page.locator(".pulse-empty").isVisible(), true);
  assert.equal(await page.locator("#ov-pulse .pulse-span").isVisible(), false);
});

test("a tile opens its detail view by click or Enter, and a double click still opens its settings", async (t) => {
  const page = await pageFor(t);
  const focus = page.locator("#light-focus");
  await page.locator('[data-id="cpu"] .tile-name').click();
  await focus.locator(".focus-panel").waitFor();
  assert.equal(await focus.evaluate((el) => el.open && el.matches(":modal")), true);
  assert.equal(await focus.locator("#focus-title").textContent(), await page.locator('[data-id="cpu"] .tile-name').textContent());
  assert.match(await focus.locator(".focus-read").textContent(), /\d+\s*°C/);
  assert.deepEqual(await focus.locator(".focus-stats dt").allTextContents(), ["Low", "Average", "Peak", "Airglow warning", "Airglow high"]);
  assert.equal(await focus.locator('[data-span="300"]').getAttribute("aria-checked"), "true", "five minutes by default");
  await page.waitForFunction(() => document.querySelector("#light-focus .light-canvas")?.dataset.series > 0);
  assert.ok(await painted(focus.locator(".light-canvas")) > 60);
  await focus.locator('[data-span="60"]').click();
  assert.equal(await focus.locator(".focus-then").textContent(), "1 min ago");
  await page.screenshot({ path: join(OUTPUT, "focus.png") });
  await page.keyboard.press("Escape");
  await focus.locator(".focus-panel").waitFor({ state: "detached" });
  assert.equal(await focus.evaluate((el) => el.open), false);

  await page.locator('[data-id="cpu"]').focus();
  await page.keyboard.press("Enter");
  await focus.locator(".focus-panel").waitFor();
  assert.equal(await focus.locator('[data-span="60"]').getAttribute("aria-checked"), "true", "the chosen span is remembered");
  await focus.locator("[data-settings]").click();
  await page.getByRole("radiogroup", { name: "Style" }).waitFor();
  assert.equal(await focus.evaluate((el) => el.open), false, "settings replace the detail view");
  await page.keyboard.press("Escape");

  await page.locator('[data-id="power"] .tile-name').dblclick();
  await page.getByRole("radiogroup", { name: "Style" }).waitFor();
  assert.equal(await focus.evaluate((el) => el.open), false);
  await page.keyboard.press("Escape");

  await page.locator("#ov-pulse .pulse-pin").nth(1).click();
  await focus.locator(".focus-panel").waitFor();
  assert.equal(await focus.locator("[data-settings]").count(), 0, "a pin has no tile settings");
  assert.match(await focus.locator("#focus-title").textContent(), /graphics|gpu/i);
  await focus.locator("[data-close]").click();
  await focus.locator(".focus-panel").waitFor({ state: "detached" });
});

test("history inspection holds recorded samples, preserves gaps and can be copied", async (t) => {
  const page = await pageFor(t, { width: 480, height: 640 });
  await page.waitForFunction(() => document.querySelector('[data-id="power"] .light-canvas')?.dataset.series > 0);
  const focus = page.locator("#light-focus");
  // Freeze before the next fixture poll. GPU samples straddle the CPU peak without sharing its timestamp.
  await page.evaluate(async () => {
    const { R } = await import("/js/readings.js");
    const now = Date.now() / 1000;
    R.series["auto/cpu_power"] = [[now - 61, 20], [now - 8, 85], [now - 6, 40], [now - 4, 70]];
    R.series["auto/gpu_power"] = [[now - 61, 11], [now - 10, 53], [now - 6, 55], [now - 4, 52]];
    document.querySelector('[data-id="power"] .tile-name').click();
    document.querySelector("#light-focus [data-hold]").click();
  });
  const cursor = focus.getByRole("slider", { name: "Inspect recorded sample" });
  assert.match(await focus.locator(".focus-sample-info").textContent(), /Sample 5 of 5/);
  assert.match(await cursor.getAttribute("aria-valuetext"), /70 W.*52 W/);
  assert.equal(await cursor.evaluate((el) => el.style.getPropertyValue("--fill")), "100%");
  await focus.locator(".light-canvas").evaluate(canvas => {
    const ctx = canvas.getContext("2d"), arc = ctx.arc.bind(ctx), clear = ctx.clearRect.bind(ctx);
    ctx.clearRect = (...args) => { canvas.dataset.markers = "0"; return clear(...args); };
    ctx.arc = (x, y, radius, ...args) => {
      if (radius === 3.6) canvas.dataset.markers = String(Number(canvas.dataset.markers) + 1);
      return arc(x, y, radius, ...args);
    };
  });
  await focus.locator('[data-jump="peak"]').click();
  await page.waitForFunction(() => document.querySelector("#light-focus .light-canvas").dataset.markers === "1");
  assert.equal(await cursor.evaluate((el) => el === document.activeElement), true);
  assert.equal(await focus.locator(".focus-read .num").textContent(), "85");
  assert.equal(await focus.locator('[data-tone="second"] dd').textContent(), "–");
  await cursor.press("ArrowRight");
  assert.match(await cursor.getAttribute("aria-valuetext"), /40 W.*55 W/);
  await page.waitForFunction(() => document.querySelector("#light-focus .light-canvas").dataset.markers === "2");
  await focus.locator('[data-jump="low"]').click();
  assert.equal(await focus.locator(".focus-read .num").textContent(), "20");

  // Pruning the live ring buffer must leave the held samples intact.
  await page.evaluate(async () => {
    const { R } = await import("/js/readings.js");
    const { emit } = await import("/js/core.js");
    R.series["auto/cpu_power"].splice(0);
    emit("state");
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async () => { throw Error("denied"); } } });
  });
  await focus.locator('[data-span="60"]').click();
  assert.match(await focus.locator(".focus-sample-info").textContent(), /Sample 4 of 4/);
  await cursor.press("Home");
  assert.equal(await cursor.evaluate((el) => el.style.getPropertyValue("--fill")), "0%");
  await cursor.press("ArrowRight");
  assert.equal(await focus.locator(".focus-read .num").textContent(), "85");
  await focus.locator("[data-copy-summary]").click();
  const text = await focus.getByRole("textbox", { name: "Sensor summary to copy" }).inputValue();
  assert.match(text, /3 recorded samples\nLow: 40 W at .*\nAverage: 65 W\nPeak: 85 W at .*\nSelected: 85 W at /);
  assert.match(text, /3 recorded samples[\s\S]*Selected: – at /);
  await page.evaluate(() => { navigator.clipboard.writeText = async (summary) => { window.copiedSummary = summary; }; });
  await focus.locator("[data-copy-summary]").click();
  assert.equal(await page.evaluate(() => window.copiedSummary), text);
  await focus.getByRole("button", { name: "Resume live" }).click();
  assert.equal(await cursor.count(), 0);
  assert.equal(await focus.locator(".focus-now").textContent(), "now");
  assert.ok(await focus.locator(".focus-panel").evaluate((el) => el.scrollWidth <= el.clientWidth + 1));
});

test("every graph draws on one shared beat, so the window is not presented once per canvas", async (t) => {
  const page = await pageFor(t, { reducedMotion: "no-preference" });
  await page.waitForFunction(() => document.querySelector('[data-id="cpu"] .light-canvas')?.dataset.series > 0);
  // Each canvas that draws makes the whole window composite again. Count the display refreshes on which any canvas
  // drew: staggered canvases would need one each, a shared beat needs at most 30 a second however many there are.
  const SECONDS = 4;
  await page.waitForTimeout(1200);                  // past the lines' arrival
  const seen = await page.evaluate((ms) => new Promise((resolve) => {
    const proto = CanvasRenderingContext2D.prototype, clear = proto.clearRect, frames = new Set();
    let frame = 0, draws = 0, running = true;
    const count = () => { frame++; if (running) requestAnimationFrame(count); };
    requestAnimationFrame(count);
    proto.clearRect = function (...box) { if (this.canvas.matches(".light-canvas")) { draws++; frames.add(frame); } return clear.apply(this, box); };
    setTimeout(() => { running = false; proto.clearRect = clear; resolve({ draws, frames: frames.size, canvases: document.querySelectorAll(".light-canvas").length }); }, ms);
  }), SECONDS * 1000);
  assert.ok(seen.canvases >= 5, `several canvases are on screen: ${seen.canvases}`);
  assert.ok(seen.draws > seen.frames, `canvases draw together: ${JSON.stringify(seen)}`);
  assert.ok(seen.frames <= SECONDS * 30 + 4, `at most one beat's worth of frames: ${JSON.stringify(seen)}`);
  // at rest most of the time: a reading every two seconds, a short ring at the head, then only the slow glide
  assert.ok(seen.frames <= SECONDS * 24, `the graphs rest between readings: ${JSON.stringify(seen)}`);
});

test("Customize keeps a click for arranging, and tiles animate only when motion is allowed", async (t) => {
  const page = await pageFor(t, { reducedMotion: "no-preference" });
  await page.waitForFunction(() => document.querySelector('[data-id="cpu"] .light-canvas')?.dataset.series > 0);
  // with motion the line keeps redrawing between readings; the head glides, a few frames a second at rest
  const redraws = await page.evaluate(() => new Promise((resolve) => {
    const canvas = document.querySelector("#ov-pulse .light-canvas"), context = canvas.getContext("2d");
    const clear = context.clearRect.bind(context);
    let count = 0;
    context.clearRect = (...box) => { count++; clear(...box); };
    setTimeout(() => { context.clearRect = clear; resolve(count); }, 2400);
  }));
  assert.ok(redraws > 8, `the pulse flows: ${redraws} frames in 2.4 s`);
  await page.locator("#ov-edit").click();
  await page.locator('[data-id="load"] .tile-name').click();
  await page.locator('[data-id="load"]').focus();
  await page.keyboard.press("ArrowRight");          // moves the tile; it does not open anything
  assert.equal(await page.locator("#light-focus[open]").count(), 0, "arranging never opens the detail view");
  await page.locator('[data-id="load"] [data-act="edit"]').click();
  await page.locator("#ts-form").waitFor();
  await page.keyboard.press("Escape");
  await page.keyboard.press("Escape");

  await page.emulateMedia({ reducedMotion: "reduce" });
  // leaving Customize and switching motion off each redraw once; count only after they have settled
  await page.evaluate(() => new Promise((resolve) => setTimeout(resolve, 800)));
  const still = await page.evaluate(() => new Promise((resolve) => {
    const canvas = document.querySelector('[data-id="cpu"] .light-canvas'), context = canvas.getContext("2d");
    const clear = context.clearRect.bind(context);
    let count = 0;
    context.clearRect = (...box) => { count++; clear(...box); };
    setTimeout(() => { context.clearRect = clear; resolve(count); }, 1500);
  }));
  assert.ok(still <= 2, `Reduce motion draws once per reading: ${still} frames in 1.5 s`);
});
