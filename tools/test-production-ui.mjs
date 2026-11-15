// Runs Overview against routed static assets and in-memory telemetry. No app server or hardware.
import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { mkdirSync, writeFileSync } from "node:fs";
import { isAbsolute, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const BASE = "https://airglow.test";
const WEB = new URL("../web/", import.meta.url);
const OUTPUT = process.env.AIRGLOW_PRODUCTION_UI_OUTPUT;
const styles = ["auto", "spark", "ring", "graph", "bar"];
const sensors = [
  { id: "auto/cpu_temp", name: "CPU temperature", unit: "°C", value: 63 },
  { id: "auto/ram_used", name: "Memory used", unit: "GB", value: 8, max: 32 },
  { id: "auto/cpu_load", name: "Processor load", unit: "%", value: 43 },
  { id: "auto/cpu_power", name: "CPU power", unit: "W", value: 165 },
  { id: "auto/net_down", name: "Download", unit: "B/s", value: 50000 },
];
const tile = (sensor, style, index) => ({
  id: `sensor-${style}-${index}`, type: "sensor", sensors: [sensor.id], style,
  w: style === "graph" ? 2 : 1, h: 1, appearance: { emphasis: "focus", accent: "#111111" },
});
let browser;
before(async () => {
  const name = process.env.AIRGLOW_PLAYWRIGHT_MODULE;
  const location = name || (process.env.APPDATA && join(process.env.APPDATA, "npm/node_modules/playwright/index.mjs")) || "playwright";
  let playwright;
  try { playwright = await import(name ? (isAbsolute(name) ? pathToFileURL(name).href : name) : "playwright"); }
  catch (error) {
    if (name || error.code !== "ERR_MODULE_NOT_FOUND" || location === "playwright") throw error;
    playwright = await import(pathToFileURL(location).href);
  }
  browser = await playwright.chromium.launch({ channel: process.env.AIRGLOW_BROWSER || "msedge" });
  if (OUTPUT) mkdirSync(OUTPUT, { recursive: true });
});
after(async () => { await browser?.close(); });

async function staticPage(t, mock = () => false) {
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 }, reducedMotion: "reduce" });
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  t.after(async () => { await page.close(); assert.deepEqual(errors, []); });
  const now = Date.now() / 1000;
  const offsets = Array.from({ length: 301 }, (_, index) => (index - 300) * 2);
  const history = { t: offsets.map(offset => now + offset),
    series: Object.fromEntries(sensors.map(sensor => [sensor.id, offsets.map((offset, index) => (
      offset >= -280 && offset <= -180 ? null : sensor.value * (.25 + (index % 47) / 64)
    ))])) };
  await page.route("**/*", async (route) => {
    const url = new URL(route.request().url());
    if (url.origin !== BASE) return route.abort();
    if (await mock(route, url)) return;
    if (url.pathname === "/api/sensors/history") return route.fulfill({ json: history });
    if (url.pathname === "/api/overview" && route.request().method() === "POST") {
      return route.fulfill({ json: route.request().postDataJSON() });
    }
    if (url.pathname.startsWith("/api/")) return route.abort();
    if (url.pathname === "/js/main.js") return route.fulfill({ contentType: "text/javascript", body: "" });
    const path = new URL(url.pathname.slice(1) || "index.html", WEB);
    if (!path.href.startsWith(WEB.href)) return route.abort();
    try { await route.fulfill({ path: fileURLToPath(path) }); }
    catch { await route.fulfill({ status: 404 }); }
  });
  await page.goto(BASE);
  return page;
}

async function overview(t, tiles) {
  const page = await staticPage(t);
  await page.evaluate(async ({ sensors, tiles }) => {
    const [{ initOverview }, { R }, { S }] = await Promise.all(
      ["/js/overview.js", "/js/readings.js", "/js/core.js"].map(path => import(path))
    );
    R.meta = Object.fromEntries(sensors.map(sensor => [sensor.id, sensor]));
    S.state = {};
    document.querySelector(".overview-view").hidden = false;
    document.querySelector('.tabs [data-tab="overview"]').setAttribute("aria-current", "page");
    await initOverview({ tiles, pins: [] });
  }, { sensors, tiles });
  return page;
}

test("interface zoom keeps the viewport and its scroll area aligned", async (t) => {
  const page = await overview(t, [tile(sensors[0], "auto", 0)]);
  await page.setViewportSize({ width: 390, height: 844 });
  for (const scale of [90, 100, 110, 120]) {
    const dimensions = await page.evaluate(async scale => {
      (await import("/js/appearance.js")).applyAppearance({ scale }, { persist: false });
      const main = document.querySelector("main").getBoundingClientRect(), view = document.querySelector(".overview-view");
      return { viewport: innerHeight, bottom: main.bottom, overflow: view.scrollWidth - view.clientWidth };
    }, scale);
    assert.ok(Math.abs(dimensions.bottom - dimensions.viewport) < 1, `${scale}%: ${JSON.stringify(dimensions)}`);
    assert.ok(dimensions.overflow <= 1);
  }
});

test("style radios keep one tab stop and retain focus and comparison across arrow keys", async (t) => {
  const initial = tile(sensors[0], "auto", 0);
  initial.sensors.push(sensors[1].id);
  const page = await overview(t, [initial]);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.evaluate(async () => (await import("/js/appearance.js")).applyAppearance({ scale: 120 }, { persist: false }));
  await page.locator(".tile-sensor").dblclick();
  const group = page.getByRole("radiogroup", { name: "Style" });
  await group.waitFor({ timeout: 1000 });
  const input = await page.locator("#ts-label").elementHandle();
  const instrument = group.getByRole("radio", { name: "Instrument" });
  const bar = group.getByRole("radio", { name: "Bar" });
  await instrument.focus();
  await page.keyboard.press("ArrowLeft");
  assert.equal(await bar.getAttribute("aria-checked"), "true");
  assert.equal(await bar.evaluate(el => el === document.activeElement), true);
  assert.equal(await group.locator('[tabindex="0"]').count(), 1);
  await page.keyboard.press("ArrowRight");
  assert.equal(await instrument.getAttribute("aria-checked"), "true");
  assert.equal(await instrument.evaluate(el => el === document.activeElement), true);
  assert.equal(await input.evaluate(el => el === document.querySelector("#ts-label")), true);
  assert.equal(await page.locator("#ts-compare b").textContent(), "Memory used");
  await page.keyboard.press("Tab");
  assert.equal(await input.evaluate(el => el === document.activeElement), true);
  if (OUTPUT) await page.screenshot({ path: join(OUTPUT, "style-radio-390-120.png") });
});

test("instrument families show scales and actual history in distinct forms", async (t) => {
  const page = await overview(t, sensors.map((sensor, index) => tile(sensor, "auto", index)));
  const thermal = page.locator('[data-family="thermal"]');
  assert.equal(await thermal.locator(".threshold-tick:not([hidden])").count(), 2);
  assert.equal(await thermal.locator(".thermal-marker").isVisible(), true);
  const capacity = page.locator('[data-family="capacity"]');
  assert.equal(await capacity.locator(".scale-hi").textContent(), "32.0 GiB");
  assert.equal(await capacity.locator(".instrument-track i").evaluate(el => el.style.width), "25%");
  const load = page.locator('[data-family="load"] .instrument-track i');
  assert.notEqual(await load.evaluate(el => getComputedStyle(el).maskImage), "none");
  const power = page.locator('[data-family="power"]');
  assert.ok((await power.locator(".instrument-history .area").evaluate(el => getComputedStyle(el).d)).match(/M/g).length >= 2);
  const network = page.locator('[data-family="network"]');
  assert.equal(await network.locator(".history-caption span").first().textContent(), "30 s peaks · 10 min");
  const bins = await network.locator(".network-bins rect").evaluateAll(rects => rects.map(el => ({
    x: +el.getAttribute("x"), height: +el.getAttribute("height"),
  })));
  assert.ok(bins.length > 1 && bins.length < 20, "missing buckets must leave blank columns");
  assert.ok(bins.some(({ x }, i) => i && x - bins[i - 1].x > 20));
  assert.ok(Math.max(...bins.map(({ height }) => height)) > 50, "low throughput remains visible on the observed scale");
  assert.equal(await power.locator(".history-empty").isVisible(), false);
  assert.equal(await network.locator(".history-empty").isVisible(), false);
});

test("presentation accents remain legible in all 12 themes and all five styles across supported widths and zooms", async (t) => {
  const tiles = [...sensors.map((sensor, index) => tile(sensor, "auto", index)),
    ...styles.slice(1).map(style => tile(sensors[0], style, 0)),
    { ...tile(sensors[0], "auto", 5), appearance: { emphasis: "quiet", accent: "#eeeeee" } },
    { ...tile(sensors[0], "spark", 6), appearance: { emphasis: "standard", accent: null } }];
  const page = await overview(t, tiles);
  const themes = await page.evaluate(async () => (await import("/js/design.js")).DESIGNS.map(({ id }) => id));
  assert.equal(themes.length, 12);
  for (const theme of themes) {
    await page.evaluate(async theme => (await import("/js/design.js")).applyDesign(theme, { persist: false }), theme);
    if (theme === "neon" || theme === "midnight" || theme === "spore") {
      const stroke = await page.locator('[data-id="sensor-spark-6"] .spark .line').evaluate(el => getComputedStyle(el).stroke);
      assert.equal(stroke, { neon: "rgb(47, 230, 255)", midnight: "rgb(107, 203, 247)", spore: "rgb(232, 159, 146)" }[theme]);
    }
    if (theme === "ascii") {
      const background = await page.locator(".s-bar .fill").evaluate(el => getComputedStyle(el).backgroundImage);
      assert.match(background, /repeating-linear-gradient/);
    }
    for (const width of [390, 700, 1280, 1920, 3840]) {
      const height = width === 390 ? 844 : width === 3840 ? 2160 : 1080;
      await page.setViewportSize({ width, height });
      for (const scale of [90, 100, 120]) {
        const result = await page.evaluate(async scale => {
          const { applyAppearance, contrastRatio } = await import("/js/appearance.js");
          applyAppearance({ scale }, { persist: false });
          const view = document.querySelector(".overview-view");
          const tileStats = [...view.querySelectorAll(".tile")].map(el => {
            const css = getComputedStyle(el);
            const accent = css.getPropertyValue("--instrument-ink").trim();
            const bg = css.getPropertyValue(el.dataset.emphasis === "quiet" ? "--well" : "--panel").trim();
            const right = el.getBoundingClientRect().right;
            const reads = [...el.querySelectorAll(".readout .unit, .g-read .unit")].filter(unit => unit.getClientRects().length);
            return { id: el.dataset.id, contrast: contrastRatio(accent, bg), width: el.clientWidth,
              clipped: reads.some(unit => unit.getBoundingClientRect().right > right + 1) };
          });
          return { bottom: view.getBoundingClientRect().bottom, height: innerHeight,
            overflow: view.scrollWidth - view.clientWidth, tileStats };
        }, scale);
        const label = `${theme} ${width}px at ${scale}%`;
        assert.ok(Math.abs(result.bottom - result.height) < 1, `${label}: ${JSON.stringify(result)}`);
        assert.ok(result.overflow <= 1, `${label}: view overflow ${result.overflow}`);
        for (const { id, contrast, width, clipped } of result.tileStats) {
          assert.ok(width > 0, `${label}: ${id} collapsed`);
          assert.ok(contrast >= 4.5, `${label}: ${id} contrast ${contrast}`);
          assert.ok(!clipped, `${label}: ${id} reading clipped`);
        }
      }
    }
    if (OUTPUT) {
      await page.setViewportSize({ width: 1280, height: 900 });
      await page.evaluate(async () => (await import("/js/appearance.js")).applyAppearance({ scale: 100 }, { persist: false }));
      await page.screenshot({ path: join(OUTPUT, `${theme}-1280.png`) });
    }
  }
});

test("overlapping layout and reading changes retarget one animation and stop when hidden", async (t) => {
  const page = await overview(t, []);
  await page.emulateMedia({ reducedMotion: "no-preference" });
  const result = await page.evaluate(async () => {
    const { flip, countTo } = await import("/js/motion.js");
    const parent = document.createElement("div"), item = document.createElement("div"), number = document.createElement("span");
    parent.append(item);
    item.append(number);
    document.body.append(parent);
    item.style.width = "100px";
    item.style.height = "30px";
    flip(parent, () => { item.style.marginLeft = "100px"; });
    const first = item.getAnimations()[0];
    first.currentTime = 70;
    const renderedX = item.getBoundingClientRect().left;
    flip(parent, () => { item.style.marginLeft = "200px"; });
    const retargetX = item.getBoundingClientRect().left;
    const layout = { count: item.getAnimations().length, canceled: first.playState === "idle",
      error: Math.abs(renderedX - retargetX), duration: item.getAnimations()[0].effect.getTiming().duration };
    countTo(number, "75");
    const reading = number.getAnimations()[0];
    reading.currentTime = 70;
    const previousOpacity = +getComputedStyle(number).opacity;
    countTo(number, "80");
    const sample = { value: number.textContent, count: number.getAnimations().length, canceled: reading?.playState === "idle" };
    const readingRetargetError = Math.abs(+getComputedStyle(number).opacity - previousOpacity);
    const finish = () => [item.getAnimations().length, number.getAnimations().length];
    (await import("/js/appearance.js")).applyAppearance({ motion: "reduced" }, { persist: false });
    const reduced = finish();
    (await import("/js/appearance.js")).applyAppearance({}, { persist: false });
    const hidden = (await import("/js/motion.js"));
    countTo(number, "90");
    parent.hidden = true;
    hidden.enter(document.querySelector(".overview-view"));
    countTo(number, "91");
    return { layout, sample, readingRetargetError, reduced, hidden: finish(), latest: number.textContent };
  });
  assert.equal(result.layout.canceled, true);
  assert.equal(result.layout.count, 1);
  assert.ok(result.layout.error < 1);
  assert.ok(result.layout.duration >= 120 && result.layout.duration <= 220);
  assert.deepEqual(result.sample, { value: "80", count: 1, canceled: true });
  assert.ok(result.readingRetargetError < .03);
  assert.deepEqual(result.reduced, [0, 0]);
  assert.deepEqual(result.hidden, [0, 0]);
  assert.equal(result.latest, "91");
});

test("normal-motion Overview layout editing measures frame cadence and interaction stalls", async (t) => {
  const page = await overview(t, sensors.flatMap((sensor, index) => styles.map(style => tile(sensor, style, index))));
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.locator("#ov-edit").click();
  const result = await page.evaluate(async () => {
    const tile = document.querySelector('[data-id="sensor-auto-0"]'), grid = document.querySelector("#ov-grid");
    tile.focus();
    const intervals = [], steady = [], operations = [], longTasks = [];
    const supportsLongTasks = PerformanceObserver.supportedEntryTypes.includes("longtask");
    const observer = supportsLongTasks ? new PerformanceObserver(entries => longTasks.push(...entries.getEntries())) : null;
    observer?.observe({ type: "longtask" });
    const actions = [["ArrowRight", false], ["ArrowLeft", false], ["ArrowRight", true], ["ArrowLeft", true]];
    let previous, animated = 0;
    await new Promise(resolve => {
      function frame(time) {
        if (previous !== undefined) {
          const frames = steady.length < 30 ? steady : intervals;
          frames.push(time - previous);
          if (frames === intervals && intervals.length % 12 === 1) {
            const [key, shiftKey] = actions[operations.length % actions.length];
            const start = performance.now();
            const event = new KeyboardEvent("keydown", { key, shiftKey, bubbles: true, cancelable: true });
            tile.dispatchEvent(event);
            operations.push({ start, end: performance.now(), changed: event.defaultPrevented });
            if (grid.getAnimations({ subtree: true }).length) animated++;
          }
        }
        previous = time;
        if (intervals.length < 180) requestAnimationFrame(frame);
        else resolve();
      }
      requestAnimationFrame(frame);
    });
    await new Promise(resolve => setTimeout(resolve, 0));
    if (observer) { longTasks.push(...observer.takeRecords()); observer.disconnect(); }
    const quantile = (values, q) => values.slice().sort((a, b) => a - b)[Math.ceil(values.length * q) - 1];
    const overlapping = longTasks.filter(task => operations.some(operation => task.startTime <= operation.end
      && task.startTime + task.duration >= operation.start));
    return { viewport: [innerWidth, innerHeight], frames: intervals.length, refreshPeriodMs: quantile(steady, .5),
      frameP95Ms: quantile(intervals, .95), worstInteractionMs: Math.max(...operations.map(({ start, end }) => end - start)),
      operations: operations.filter(operation => operation.changed).length, animated, supportsLongTasks,
      interactionLongTasksMs: overlapping.map(task => task.duration) };
  });
  const is60Hz = result.refreshPeriodMs >= 15 && result.refreshPeriodMs <= 18.5;
  const evidence = { ...result, frameBudgetEvaluatedAt60Hz: is60Hz };
  t.diagnostic(JSON.stringify(evidence));
  if (OUTPUT) writeFileSync(join(OUTPUT, "overview-frame-performance.json"), JSON.stringify(evidence, null, 2) + "\n");
  assert.equal(result.frames, 180);
  assert.equal(result.operations, 15);
  assert.ok(result.animated >= 12);
  if (is60Hz) assert.ok(result.frameP95Ms <= 25, `p95 frame interval: ${result.frameP95Ms} ms`);
  assert.ok(result.worstInteractionMs <= 100, `Interaction took ${result.worstInteractionMs} ms`);
  assert.ok(result.interactionLongTasksMs.every(duration => duration <= 100), "Interaction overlapped a task longer than 100 ms");
});

async function switcher(t, mock) {
  const page = await staticPage(t, async (route, url) => {
    if (url.pathname === "/js/scene-commands.js") {
      await route.fulfill({ contentType: "text/javascript", body: `
        export async function applyLightingScene(patch, {wait}) {
          const response = await fetch("/api/test-scene-command", { method: "POST", body: JSON.stringify({ patch, wait }) });
          if (!response.ok) throw new Error("Renderer unavailable");
          return response.json();
        }
        export async function waitForLightingScene(revision) {
          const response = await fetch("/api/test-scene-status/" + revision);
          return response.json();
        }` });
      return true;
    }
    return mock?.(route, url) ?? false;
  });
  await page.evaluate(async () => {
    const [{ initQuickSwitcher }, { S }, { initOverview }] = await Promise.all(
      ["/js/quick-switcher.js", "/js/core.js", "/js/overview.js"].map(path => import(path))
    );
    S.state = {};
    S.effects = [{ id: "solid", name: "Solid" }, { id: "spectrum", name: "Music Spectrum" }];
    S.presets = [{ id: "movie-time", name: "Movie Time" },
      { id: "night", name: "Night fan curve", fan_points: [[20, 40], [80, 100]] },
      { id: "long", name: "LongName".repeat(20) }];
    document.querySelector(".overview-view").hidden = false;
    await initOverview({ tiles: [], pins: [], active_workspace: "desk", workspaces: [
      { id: "desk", name: "Desk", tiles: [], pins: [] },
      { id: "media", name: "Media Wall", tiles: [], pins: [] },
    ] });
    initQuickSwitcher(["overview", "lighting", "studio", "fans", "automations", "devices", "settings"], () => window.__catalogState ?? "ready");
  });
  return page;
}

test("Ctrl+K opens the search, arrows change only selection, and Escape restores focus", async (t) => {
  const page = await switcher(t);
  const requests = [];
  page.on("request", request => { if (request.url().startsWith(BASE + "/api/")) requests.push(request.url()); });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.evaluate(async () => (await import("/js/appearance.js")).applyAppearance({ scale: 120 }, { persist: false }));
  const shortcutOrigin = page.locator("#ov-edit");
  await shortcutOrigin.click();
  await page.keyboard.press("Control+k");
  const dialog = page.getByRole("dialog", { name: "Quick switch" });
  const input = dialog.getByRole("combobox");
  assert.equal(await input.evaluate(el => el === document.activeElement), true);
  await input.fill("  EfFecT  ");
  assert.equal(await dialog.getByRole("option").count(), 2);
  await dialog.getByRole("option").last().hover();
  await page.keyboard.press("ArrowDown");
  assert.equal(await dialog.getByRole("option", { selected: true }).textContent().then(text => text.includes("Music Spectrum")), true);
  assert.deepEqual(requests, []);
  await input.fill("ProFiLe night");
  assert.match(await dialog.getByRole("option", { selected: true }).textContent(), /Apply profile.*cooling/i);
  if (OUTPUT) await page.screenshot({ path: join(OUTPUT, "quick-switch-390-120.png") });
  await input.fill("LongName");
  assert.equal(await dialog.evaluate(el => el.scrollWidth <= el.clientWidth + 1), true);
  await input.fill("no such action");
  assert.match(await dialog.getByRole("status").textContent(), /No matching actions/i);
  await page.keyboard.press("Enter");
  assert.deepEqual(requests, []);
  await page.keyboard.press("Escape");
  assert.equal(await dialog.isVisible(), false);
  assert.equal(await shortcutOrigin.evaluate(el => el === document.activeElement), true);
  assert.equal(await shortcutOrigin.getAttribute("aria-pressed"), "true", "Escape leaves the underlying Customize mode active");
});

test("quick switch routes navigation and saves an explicitly selected workspace", async (t) => {
  const mutations = [];
  const page = await switcher(t, route => {
    if (route.request().method() === "POST") mutations.push(route.request().url().split(BASE)[1]);
    return false;
  });
  await page.keyboard.press("Control+k");
  const dialog = page.getByRole("dialog", { name: "Quick switch" });
  const input = dialog.getByRole("combobox", { name: "Search actions" });
  await input.fill("Settings");
  assert.match(await dialog.getByRole("option", { selected: true }).textContent(), /Open settings/i);
  await page.keyboard.press("Enter");
  await page.waitForURL("**/#settings");
  assert.deepEqual(mutations, []);
  await page.keyboard.press("Control+k");
  await input.fill("workspace media Wall");
  assert.match(await dialog.getByRole("option", { selected: true }).textContent(), /Switch workspace/i);
  const saved = page.waitForResponse(response => response.url() === BASE + "/api/overview" && response.request().method() === "POST");
  await page.keyboard.press("Enter");
  await page.waitForURL("**/#overview");
  assert.equal(await page.locator("#ov-workspace").inputValue(), "media");
  await saved;
  assert.deepEqual(mutations, ["/api/overview"]);
});

test("execution waits for renderer readiness and reports partial cooling application", async (t) => {
  let profileCalls = 0;
  const command = Promise.withResolvers(), sceneResponse = Promise.withResolvers();
  const page = await switcher(t, async (route, url) => {
    if (url.pathname === "/api/test-scene-command") {
      command.resolve(route.request().postDataJSON());
      await sceneResponse.promise;
      await route.fulfill({ json: { scene_status: { status: "ready" } } });
      return true;
    }
    if (url.pathname === "/api/presets/night/apply") {
      profileCalls++;
      await route.fulfill({ json: { ok: false, steps: [
        { step: "lighting", ok: true }, { step: "FanControl", ok: false, error: "profile utility unavailable" },
      ], scene_status: { status: "pending", desired_revision: 12 } } });
      return true;
    }
    if (url.pathname === "/api/test-scene-status/12") {
      await route.fulfill({ json: { scene_status: { status: "ready" } } });
      return true;
    }
    return false;
  });
  await page.keyboard.press("Control+k");
  const dialog = page.getByRole("dialog", { name: "Quick switch" });
  const input = dialog.getByRole("combobox");
  await input.fill("music spectrum");
  await page.keyboard.press("Enter");
  await dialog.getByText(/Applying .*Music Spectrum/i).waitFor({ timeout: 1000 });
  assert.equal(await dialog.isVisible(), true);
  assert.deepEqual(await command.promise, { patch: { effect: "spectrum" }, wait: true });
  await page.keyboard.press("Enter");
  sceneResponse.resolve();
  await dialog.waitFor({ state: "hidden", timeout: 2000 });
  assert.match(await page.locator("#toast").textContent(), /Music Spectrum applied/i);
  await page.keyboard.press("Control+k");
  await input.fill("night");
  await dialog.getByRole("option", { selected: true }).click();
  await dialog.getByText(/FanControl.*profile utility unavailable/i).waitFor({ timeout: 1000 });
  assert.equal(profileCalls, 1);
  assert.equal(await input.evaluate(el => el === document.activeElement), true);
});

test("Airglow boot installs the Quick Switch shortcut and lists it in keyboard help", async (t) => {
  const page = await staticPage(t, async (route, url) => {
    if (url.pathname !== "/js/main.js") return false;
    await route.fulfill({ path: fileURLToPath(new URL("js/main.js", WEB)) });
    return true;
  });
  await page.keyboard.press("F1");
  const help = page.getByRole("dialog", { name: "Keyboard shortcuts" });
  await help.getByText(/search and switch effects/i).waitFor({ timeout: 1500 });
  await page.keyboard.press("Control+k");
  assert.equal(await help.isVisible(), true, "the global shortcut must not replace another modal");
  await page.keyboard.press("Escape");
  await page.keyboard.press("Control+k");
  const quick = page.getByRole("dialog", { name: "Quick switch" });
  await quick.getByText(/workspaces are unavailable/i).waitFor({ timeout: 1500 });
});

test("quick search distinguishes initial loading, unavailable catalog, and renderer failure", async (t) => {
  const page = await switcher(t, async (route, url) => {
    if (url.pathname !== "/api/test-scene-command") return false;
    await route.fulfill({ status: 503 });
    return true;
  });
  const dialog = page.getByRole("dialog", { name: "Quick switch" });
  const input = dialog.getByRole("combobox");
  for (const [state, message] of [["loading", /Loading effects/i], ["unavailable", /Unavailable/i], ["ready", null]]) {
    await page.evaluate(state => { window.__catalogState = state; }, state);
    await page.keyboard.press("Control+k");
    await input.fill("music");
    if (message) {
      assert.match(await dialog.getByRole("status").textContent(), message);
      assert.equal(await dialog.getByRole("option").count(), 0);
      await page.keyboard.press("Escape");
    }
  }
  await page.keyboard.press("Enter");
  await dialog.getByText("Renderer unavailable", { exact: false }).waitFor({ timeout: 1000 });
  assert.equal(await dialog.isVisible(), true);
  assert.equal(await input.evaluate(el => el === document.activeElement), true);
});

test("quick switch reports effects saved under a lighting override or while lights are off", async (t) => {
  let overridden = true;
  const page = await switcher(t, async (route, url) => {
    if (url.pathname !== "/api/test-scene-command") return false;
    await route.fulfill({ json: { scene: { on: overridden },
      scene_status: { status: "ready", overridden } } });
    return true;
  });
  const input = page.getByRole("combobox", { name: "Search actions" });
  for (const feedback of [/Solid saved.*overridden/i, /Solid saved.*lights are off/i]) {
    await page.keyboard.press("Control+k");
    await input.fill("solid");
    await page.keyboard.press("Enter");
    await page.locator("#toast").getByText(feedback).waitFor({ timeout: 1000 });
    overridden = false;
  }
});
