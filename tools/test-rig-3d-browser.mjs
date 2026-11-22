// Read-only browser checks against the isolated demo fixture. Never use a hardware server.
import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { mkdirSync } from "node:fs";
import { isAbsolute, join } from "node:path";
import { pathToFileURL } from "node:url";

const BASE = process.env.AIRGLOW_FIXTURE_URL || "http://127.0.0.1:8790";
const OUTPUT = process.env.AIRGLOW_RIG3D_OUTPUT || "output/release-1.14.0/rig-3d";
let browser;
before(async () => {
  const settings = await fetch(`${BASE}/api/settings`).then(r => r.json());
  assert.equal(settings.settings.lhm_url, "http://127.0.0.1:8791", "An isolated fixture is required");
  let pw;
  const name = process.env.AIRGLOW_PLAYWRIGHT_MODULE;
  if (name) pw = await import(isAbsolute(name) ? pathToFileURL(name).href : name);
  else { try { pw = await import("playwright"); } catch (error) {
    if (error.code !== "ERR_MODULE_NOT_FOUND") throw error;
    pw = await import(pathToFileURL(join(process.env.APPDATA, "npm/node_modules/playwright/index.mjs")).href);
  } }
  const { chromium } = pw;
  browser = await chromium.launch({ channel: process.env.AIRGLOW_BROWSER || "msedge" });
  mkdirSync(OUTPUT, { recursive: true });
});
after(async () => { await browser?.close(); });

async function pageFor(t, width = 1440, init = null) {
  const page = await browser.newPage({ viewport: { width, height: 960 }, reducedMotion: "reduce" });
  page.setDefaultTimeout(10000);
  const errors = [], posts = [], modules = [];
  page.on("pageerror", e => errors.push(e.message));
  page.on("request", r => {
    const path = new URL(r.url()).pathname;
    // Deck stills use isolated preview messages, which do not mutate hardware or settings.
    if (r.method() === "POST" && !/^\/api\/studio\/preview\/(load|tick|stop)$/.test(path)) posts.push(r.url());
    if (r.url().endsWith("/rig-3d.js")) modules.push(r.url());
  });
  await page.addInitScript(() => {
    // The camera checks compare exact pixels. With Skia's Graphite GPU backend (on in Edge 158, still off in
    // Playwright's Chromium 145) two identical repaints of a GPU canvas differ by a brightness level or more in a few
    // dozen pixels where the lights' translucent glows are drawn over the fan surfaces. So this canvas is rasterized
    // in software, which repeats exactly. The app itself never reads this canvas back and keeps the GPU.
    const context = HTMLCanvasElement.prototype.getContext;
    HTMLCanvasElement.prototype.getContext = function (type, options) {
      return context.call(this, type, type === "2d" && this.id === "rig-3d-canvas" ? { ...options, willReadFrequently: true } : options);
    };
    window.rigPaints = 0;
    const fill = CanvasRenderingContext2D.prototype.fillRect;
    CanvasRenderingContext2D.prototype.fillRect = function (...args) {
      if (this.canvas.id === "rig-3d-canvas") window.rigPaints++;
      return fill.apply(this, args);
    };
  });
  if (init) await page.addInitScript(init);
  await page.route("**/api/state", async route => {
    const response = await route.fetch(), state = await response.json();
    await route.fulfill({ response, json: { ...state, onboarded: true, online_asked: true } });
  });
  await page.route("**/api/zones", async route => {
    const response = await route.fetch(), data = await response.json();
    // Simulated configured fans, only in this browser: never claim hardware evidence.
    for (const zone of data.zones) if (zone.key.startsWith("nzxt-hub/")) zone.device.online = true;
    await route.fulfill({ response, json: data });
  });
  // Stable frames make camera/reset comparisons meaningful, independently of the running scene.
  await page.route("**/api/stream?*", () => {}); // Keep the fixture stream pending until page close.
  await page.goto(`${BASE}/#lighting`, { waitUntil: "domcontentloaded" });
  await page.locator('[data-view="lighting"]').waitFor({ state: "visible" });
  await page.waitForFunction(async () => (await import("/js/core.js")).S.zones?.zones?.length > 0);
  // Zones load before boot finishes initializing the stage's frame listener.
  // Synthetic frames must not be emitted until the actual stage is ready.
  await page.locator("#stage-view-2d").waitFor({ state: "visible" });
  await page.evaluate(() => document.fonts.ready);
  t.after(async () => { await page.close(); assert.deepEqual(errors, [], "No browser exceptions"); });
  return { page, posts, modules };
}
async function stableFrames(page, color = "ff0000") {
  await page.evaluate(async color => {
    const { S, emit } = await import("/js/core.js");
    emit("frames:live", { zones: Object.fromEntries(S.zones.zones.map(z => [z.key, color.repeat(z.positions.length)])) });
  }, color);
  await flush(page);
}
async function flush(page) { await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))); }
async function pixels(page) { await flush(page); return page.locator("#rig-3d-canvas").evaluate(c => c.toDataURL()); }
// The reference picture for the exact comparisons: taken once an identical repaint reproduces it. If identical repaints
// never matched, every "the picture changed" check below would pass without testing anything, so that fails here.
async function settledPixels(page) {
  let last = await pixels(page);
  for (let i = 0; i < 8; i++) {
    await stableFrames(page);                        // repaint with the same frames; never with Home, which is under test
    const next = await pixels(page);
    if (next === last) return next;
    last = next;
  }
  assert.fail("The 3D picture does not settle: identical repaints keep differing");
}

test("3D is lazy; real orbit, zoom, pan and Reset change the rendered scene without hardware writes", async t => {
  const { page, posts, modules } = await pageFor(t);
  assert.equal(await page.locator("#stage-view-2d").getAttribute("aria-pressed"), "true");
  assert.equal(modules.length, 0, "2D does not download the optional renderer");
  await page.locator("#stage-view-3d").click();
  await page.locator("#rig-3d-canvas").waitFor({ state: "visible" });
  await stableFrames(page);
  assert.equal(modules.length, 1);
  const original = await settledPixels(page), canvas = page.locator("#rig-3d-canvas");
  const red = await canvas.evaluate(c => {
    const p = c.getContext("2d").getImageData(0, 0, c.width, c.height).data;
    let count = 0; for (let i = 0; i < p.length; i += 4) if (p[i] === 255 && p[i + 1] === 0 && p[i + 2] === 0) count++;
    return count;
  });
  assert.ok(red > 20, "Live red frames reach actual LED cores");
  await canvas.press("ArrowRight"); assert.notEqual(await pixels(page), original, "Orbit changes perspective");
  await canvas.press("Home"); assert.ok(await pixels(page) === original, "Home returns to the same view");
  await canvas.press("Shift+ArrowLeft"); assert.notEqual(await pixels(page), original, "Keyboard pan works");
  await canvas.press("Home"); await canvas.press("+"); assert.notEqual(await pixels(page), original, "Keyboard zoom works");
  await page.locator("#rig-3d-reset").click(); assert.ok(await pixels(page) === original, "Reset restores the original camera");
  const box = await canvas.boundingBox(), x = box.x + box.width * .65, y = box.y + box.height * .35;
  await page.mouse.move(x, y); await page.mouse.down(); await page.mouse.move(x + 70, y + 25, { steps: 4 }); await page.mouse.up();
  assert.notEqual(await pixels(page), original, "Pointer dragging orbits the scene");
  await canvas.press("Home"); assert.ok(await pixels(page) === original, "Home undoes a dragged orbit");
  await page.mouse.move(x, y); await page.mouse.down({ button: "right" }); await page.mouse.move(x - 45, y + 20, { steps: 3 }); await page.mouse.up({ button: "right" });
  assert.notEqual(await pixels(page), original, "Right dragging pans the scene");
  await canvas.press("Home"); assert.ok(await pixels(page) === original, "Home undoes a dragged pan");
  await page.mouse.wheel(0, -120);
  assert.notEqual(await pixels(page), original, "Wheel zoom changes perspective");
  await canvas.press("Home"); assert.ok(await pixels(page) === original, "Home undoes a wheel zoom");
  await page.screenshot({ path: `${OUTPUT}/lighting-3d.png`, fullPage: true });
  await page.locator("#arrange").click();
  assert.equal(await page.locator("#stage-view-2d").getAttribute("aria-pressed"), "true");
  assert.equal(await page.locator("#arrange").getAttribute("aria-pressed"), "true");
  assert.ok(await page.locator("#stage-svg").isVisible());
  assert.deepEqual(posts, [], "Inspecting the rig cannot POST to hardware or settings");
});

test("a delayed lazy import cannot reopen 3D after the user returns to 2D", async t => {
  const { page } = await pageFor(t);
  let release, acknowledge;
  const requested = new Promise(resolve => { acknowledge = resolve; });
  const delayed = new Promise(resolve => { release = resolve; });
  await page.route("**/js/rig-3d.js", async route => {
    const response = await route.fetch(); acknowledge(); await delayed; await route.fulfill({ response });
  });
  await page.locator("#stage-view-3d").click(); await requested;
  await page.locator("#stage-view-2d").click();
  const loaded = page.waitForResponse(response => response.url().endsWith("/js/rig-3d.js"));
  release(); await loaded;
  await page.waitForFunction(() => document.querySelector("#stage-view-3d").getAttribute("aria-busy") !== "true");
  await flush(page);
  assert.equal(await page.locator("#stage-view-2d").getAttribute("aria-pressed"), "true");
  assert.equal(await page.locator("#rig-3d").count(), 0);
});

test("depth is local presentation state, survives reload, and does not change configured positions", async t => {
  const { page, posts } = await pageFor(t);
  await page.locator("#stage-view-3d").click();
  await page.locator("#rig-3d-canvas").waitFor({ state: "visible" });
  const before = await page.evaluate(async () => JSON.stringify((await import("/js/core.js")).S.zones));
  const key = await page.locator("#rig-3d-zone option").nth(1).getAttribute("value");
  await page.locator("#rig-3d-zone").selectOption(key);
  await stableFrames(page);
  const flat = await settledPixels(page);
  await page.locator("#rig-3d-depth").fill("35");
  await page.locator("#rig-3d-depth").dispatchEvent("input");
  assert.notEqual(await pixels(page), flat, "Depth changes projected geometry");
  assert.equal(await page.evaluate(async () => JSON.stringify((await import("/js/core.js")).S.zones)), before);
  assert.equal(await page.evaluate(key => JSON.parse(localStorage.getItem("airglow.rig3d.depth.v1"))[key], key), .35);
  assert.deepEqual(posts, []);
  await page.reload({ waitUntil: "domcontentloaded" });
  assert.equal(await page.locator("#stage-view-2d").getAttribute("aria-pressed"), "true", "Reload keeps the default view in 2D");
  await page.locator("#stage-view-3d").click();
  await page.locator("#rig-3d-zone").selectOption(key);
  assert.equal(await page.locator("#rig-3d-depth").inputValue(), "35");
});

test("depth sorting lets a front panel occlude a live LED behind it", async t => {
  const { page } = await pageFor(t);
  await page.evaluate(async () => {
    const { S, emit } = await import("/js/core.js");
    const zone = (key, shape) => ({ key, positions: [[.8, .5]], device: { online: true },
      config: { name: key, enabled: true, layout: { shape, x: .8, y: .5, angle: 0, length: .8, leds: 1, cols: 4, rows: 4 } } });
    S.zones = { zones: [zone("front", "matrix"), zone("rear", "strip")] };
    localStorage.setItem("airglow.rig3d.depth.v1", JSON.stringify({ front: .2, rear: -.2 }));
    emit("zones"); emit("frames:live", { zones: { front: "0000ff", rear: "ff0000" } });
  });
  await page.locator("#stage-view-3d").click();
  await page.locator("#rig-3d-canvas").waitFor({ state: "visible" });
  await page.waitForFunction(() => window.rigPaints > 0);
  await flush(page);
  const colors = await page.locator("#rig-3d-canvas").evaluate(c => {
    const data = c.getContext("2d").getImageData(0, 0, c.width, c.height).data;
    let red = 0, blue = 0;
    for (let i = 0; i < data.length; i += 4) {
      if (data[i] === 255 && data[i + 1] === 0 && data[i + 2] === 0) red++;
      if (data[i] === 0 && data[i + 1] === 0 && data[i + 2] === 255) blue++;
    }
    return { red, blue, width: c.width, height: c.height, paints: window.rigPaints };
  });
  assert.equal(colors.red, 0, "Rear red light is behind the configured panel");
  assert.ok(colors.blue > 0, `Front blue LED core stays visible at the current display scale: ${JSON.stringify(colors)}`);
});

test("hidden view stops painting; live frames and reduced motion do not start automatic orbit", async t => {
  const { page } = await pageFor(t);
  await page.locator("#stage-view-3d").click();
  await page.locator("#rig-3d-canvas").waitFor({ state: "visible" }); await stableFrames(page);
  const count = await page.evaluate(() => window.rigPaints);
  await page.waitForTimeout(250);
  assert.equal(await page.evaluate(() => window.rigPaints), count, "No idle animation under reduced motion");
  await page.evaluate(() => { location.hash = "#settings"; });
  await page.locator('[data-view="lighting"]').waitFor({ state: "hidden" });
  const hiddenCount = await page.evaluate(() => window.rigPaints);
  await stableFrames(page, "00ff00");
  assert.equal(await page.evaluate(() => window.rigPaints), hiddenCount, "Hidden scene does not paint incoming frames");
  await page.evaluate(() => { location.hash = "#lighting"; });
  await page.locator("#rig-3d-canvas").waitFor({ state: "visible" }); await flush(page);
  assert.ok(await page.evaluate(() => window.rigPaints) > hiddenCount, "Returning draws the latest frame");
});

test("context loss releases the renderer and returns to the usable 2D stage", async t => {
  const { page } = await pageFor(t);
  await page.locator("#stage-view-3d").click();
  await page.locator("#rig-3d-canvas").waitFor({ state: "visible" });
  await page.locator("#rig-3d-canvas").dispatchEvent("contextlost");
  assert.equal(await page.locator("#stage-view-2d").getAttribute("aria-pressed"), "true");
  assert.equal(await page.locator("#rig-3d").count(), 0);
  assert.ok(await page.locator("#stage-svg").isVisible());
  await page.locator("#stage-view-3d").click();
  await page.locator("#rig-3d-canvas").waitFor({ state: "visible" });
  assert.ok(await page.locator("#rig-3d-canvas").isVisible(), "A fresh renderer can be requested");
});

test("unavailable drawing leaves 2D intact", async t => {
  const { page } = await pageFor(t, 1440, () => {
    const get = HTMLCanvasElement.prototype.getContext;
    HTMLCanvasElement.prototype.getContext = function (...args) { return this.id === "rig-3d-canvas" ? null : get.apply(this, args); };
  });
  await page.locator("#stage-view-3d").click();
  await page.waitForFunction(() => document.querySelector("#stage-view-3d").getAttribute("aria-busy") !== "true");
  assert.equal(await page.locator("#stage-view-2d").getAttribute("aria-pressed"), "true");
  assert.equal(await page.locator("#rig-3d").count(), 0);
  assert.ok(await page.locator("#stage-svg").isVisible());
});

test("narrow windows keep camera and zone controls inside the stage", async t => {
  const { page } = await pageFor(t, 390);
  await page.locator("#stage-view-3d").click();
  await page.locator("#rig-3d-canvas").waitFor({ state: "visible" }); await stableFrames(page);
  const stage = await page.locator("#stage").boundingBox();
  for (const id of ["#stage-view-3d", "#rig-3d-reset", "#rig-3d-zone", "#rig-3d-depth"]) {
    const box = await page.locator(id).boundingBox();
    assert.ok(box.x >= stage.x && box.x + box.width <= stage.x + stage.width + 1, `${id} fits horizontally`);
  }
  await page.screenshot({ path: `${OUTPUT}/lighting-3d-narrow.png`, fullPage: true });
});

test("Light theatre projects delivered frame colours, holds motion and never changes the rig", async t => {
  const { page, posts } = await pageFor(t, 1440);
  const launcher = page.getByRole("button", { name: "Open Light theatre" });
  await page.evaluate(async () => {
    const { S, emit } = await import("/js/core.js");
    // Use the fixture's actual configured geometry; supply known engine RGB to check the optical layer precisely.
    S.state.effective = { ...S.state.effective, effect: "gradient", on: true, brightness: 80 };
    const rgb = ["ffc492", "ffa96f", "df9ee0", "b092e0", "92d8d3", "79c8c2"];
    emit("frames:live", { zones: Object.fromEntries(S.zones.zones.map(z => [z.key,
      z.positions.map((p, i) => rgb[i % rgb.length]).join("")])) });
  });
  await launcher.press("Enter");
  const dialog = page.getByRole("dialog", { name: "Light theatre" });
  await dialog.getByText("Frame held", { exact: true }).waitFor();
  const lights = dialog.locator("[data-emitters]");
  async function sample() {
    await flush(page);
    return lights.evaluate(c => {
      const data = c.getContext("2d").getImageData(0, 0, c.width, c.height).data;
      let found = 0, red = 0, blue = 0;
      for (let i = 0; i < data.length; i += 4) {
        if (data[i + 3] > 200) found++;
        if (data[i] === 255 && !data[i + 1] && !data[i + 2] && data[i + 3] === 255) red++;
        if (data[i + 2] === 255 && !data[i] && !data[i + 1] && data[i + 3] === 255) blue++;
      }
      return { found, red, blue };
    });
  }
  assert.ok((await sample()).found > 200);
  await page.screenshot({ path: `${OUTPUT}/light-theatre.png` });
  await stableFrames(page, "ff0000");
  assert.equal((await sample()).red, 0, "Reduced motion holds the first received frame");
  await dialog.getByRole("button", { name: "Resume live" }).press("Enter");
  assert.ok((await sample()).red > 50, "Emitter cores retain frame RGB");
  await stableFrames(page, "0000ff");
  assert.ok((await sample()).blue > 50);
  const camera = dialog.getByRole("button", { name: "Face-on" });
  await camera.click();
  assert.equal(await camera.getAttribute("aria-pressed"), "true");
  const zone = dialog.getByLabel("Focus a lighting zone");
  const first = await page.evaluate(async () => (await import("/js/core.js")).S.zones.zones.find(z => z.device.online && z.config.enabled && z.positions.length).key);
  await zone.selectOption(first);
  assert.match(await dialog.locator(".theatre-meter").textContent(), /^1 zone\s+·/);
  await page.setViewportSize({ width: 390, height: 720 });
  await flush(page);
  assert.ok(await dialog.evaluate(el => el.scrollWidth <= el.clientWidth + 1));
  await page.screenshot({ path: `${OUTPUT}/light-theatre-narrow.png` });
  await page.evaluate(async () => { const { S, emit } = await import("/js/core.js"); S.state.effective.on = false; emit("state"); });
  await dialog.getByText("Lights off", { exact: true }).waitFor();
  assert.equal((await sample()).found, 0);
  await page.evaluate(async first => {
    const { S, emit } = await import("/js/core.js"); S.zones.zones.find(z => z.key === first).device.online = false; emit("zones");
  }, first);
  await dialog.getByText("Devices offline", { exact: true }).waitFor();
  await page.evaluate(async () => { const { S, emit } = await import("/js/core.js"); S.zones.zones = []; emit("zones"); });
  await dialog.getByText("No configured LEDs", { exact: true }).waitFor();
  await page.keyboard.press("Escape");
  await dialog.waitFor({ state: "hidden" });
  assert.ok(await launcher.evaluate(el => document.activeElement === el));
  await page.locator(".light-theatre").waitFor({ state: "detached" });
  assert.deepEqual(posts, []);
});
