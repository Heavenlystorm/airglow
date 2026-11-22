// Runs Overview against routed static assets and in-memory telemetry. No app server or hardware.
import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { mkdirSync, writeFileSync } from "node:fs";
import { isAbsolute, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const BASE = "https://airglow.test";
const WEB = new URL("../web/", import.meta.url);

test("welcome describes partial readings and keeps expanded discoveries through a poll", async t => {
  const page = await staticPage(t);
  await page.evaluate(async () => {
    const { S } = await import("/js/core.js");
    S.state = { setup_pending: 0, online_asked: true, devices: [{ online: true, type: "Lighting", name: "Desk hub", zones: [1] }],
      sensors: { online: true, cpu_temp: 54, cpu_name: "My CPU", gpu_temp: null, gpu_name: "My GPU" }, fans: { failsafe: false } };
    (await import("/js/onboarding.js")).openWelcome();
  });
  const status = page.locator("#onb-status");
  assert.match(await status.innerText(), /graphics card temperature.*(unavailable|missing)/i);
  assert.doesNotMatch(await status.innerText(), /reading .* and .* through|safe speed/i);
  const summary = status.locator("summary");
  await summary.press("Enter");
  assert.ok(await status.getByText("Desk hub").isVisible());
  await page.evaluate(async () => {
    const { S, emit } = await import("/js/core.js");
    S.state.sensors.gpu_temp = 47;
    emit("state");
  });
  assert.ok(await status.getByText("Desk hub").isVisible());
  assert.ok(await summary.evaluate(el => el === document.activeElement));
  assert.doesNotMatch(await status.innerText(), /graphics card temperature.*unavailable/i);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.evaluate(async () => {
    (await import("/js/design.js")).applyDesign("firefly", { persist: false });
    (await import("/js/appearance.js")).applyAppearance({ scale: 120 }, { persist: false });
    const { S, emit } = await import("/js/core.js"); S.state.setup_pending = 9; emit("state");
    await document.fonts.ready;
  });
  assert.ok(await page.locator(".welcome-heading h2").evaluate(el =>
    el.getBoundingClientRect().width >= el.closest(".sheet-head").getBoundingClientRect().width - 2), "The welcome heading owns the full width at 120%");
});

test("device wiring starts collapsed and retains a focused draft through discovery", async t => {
  const writes = [];
  let zones;
  const page = await staticPage(t, async (route, url) => {
    if (url.pathname === "/api/zones") { await route.fulfill({ json: { zones } }); return true; }
    if (decodeURIComponent(url.pathname) !== "/api/zones/hub/a") return false;
    const config = route.request().postDataJSON(); writes.push(config);
    zones[0].config = config;
    await route.fulfill({ json: { config } });
    return true;
  });
  await page.evaluate(async () => {
    const { S } = await import("/js/core.js");
    S.state = { openrgb: {} };
    S.zones = { zones: ["a", "b"].map(key => ({ key: "hub/" + key, device: { id: "hub", name: "Desk hub", kind: "native", online: true },
      config: { name: "Channel " + key, enabled: true, order: "RGB", layout: { shape: "strip", leds: 8 } },
      zone: { max_leds: 40, default_leds: 8 } })) };
    document.querySelector(".devices-view").hidden = false;
    const devices = await import("/js/devices.js");
    devices.initDevices(); devices.renderDevices();
  });
  const first = page.locator('[data-key="hub/a"]'), second = page.locator('[data-key="hub/b"]');
  assert.equal(await first.getByLabel("Shape").isVisible(), false);
  await first.getByRole("button", { name: /configure/i }).press("Enter");
  const name = first.getByLabel("Zone name");
  zones = await page.evaluate(async () => (await import("/js/core.js")).S.zones.zones);
  await name.fill("Desk strip");
  await page.evaluate(async () => {
    const { S, emit } = await import("/js/core.js");
    S.zones.zones[1].config.name = "Renamed elsewhere";
    emit("zones");
  });
  assert.equal(await name.inputValue(), "Desk strip");
  assert.ok(await name.evaluate(el => el === document.activeElement));
  assert.equal(writes.length, 0, "Discovery cannot commit an unfinished name");
  const saved = page.waitForResponse(response => decodeURIComponent(new URL(response.url()).pathname) === "/api/zones/hub/a", { timeout: 2000 });
  await name.press("Tab");
  await saved;
  assert.equal(writes.length, 1);
  assert.equal(writes[0].name, "Desk strip");
  await page.evaluate(async () => {
    const d = await import("/js/devices.js");
    d.showZone("hub/b"); d.renderDevices();
  });
  assert.ok(await second.getByLabel("Shape").isVisible());
});

test("every automation starter previews before writing, saves once and permits retry", { timeout: 15000 }, async t => {
  const writes = [];
  const { promise: received, resolve: markReceived } = Promise.withResolvers();
  const { promise: held, resolve: release } = Promise.withResolvers();
  const page = await staticPage(t, async (route, url) => {
    if (url.pathname === "/api/apps") { await route.fulfill({ json: [] }); return true; }
    if (url.pathname !== "/api/automations") return false;
    writes.push(route.request().postDataJSON());
    if (writes.length === 1) {
      markReceived();
      await held;
      await route.fulfill({ status: 503, json: { error: "Fixture save failed" } });
    } else await route.fulfill({ json: writes.at(-1).rules });
    return true;
  });
  await page.evaluate(async () => {
    const { S } = await import("/js/core.js");
    S.state = { temp_unit: "F" }; S.rules = []; S.presets = [{ id: "custom", name: "My profile" }]; S.curves = { curves: { performance: {}, silent: {} } };
    document.querySelector(".auto-view").hidden = false;
    const auto = await import("/js/automations.js"); auto.initAutomations(); auto.renderAutomations();
  });
  for (const starter of await page.locator("[data-starter]").all()) {
    await starter.press("Enter");
    assert.equal(writes.length, 0);
    await page.locator("#rb-form").waitFor({ timeout: 1400 });
    if ((await starter.textContent()).includes("Game mode")) {
      assert.match(await starter.textContent(), /My profile/);
      assert.equal(await page.getByLabel("Profile", { exact: true }).inputValue(), "custom");
    }
    if ((await starter.textContent()).includes("Cool down")) assert.equal(await page.getByLabel("Temperature", { exact: true }).inputValue(), "176");
    await page.keyboard.press("Escape");
    assert.ok(await starter.evaluate(el => el === document.activeElement));
  }
  await page.locator("[data-starter]").first().click();
  await page.locator("#rb-form").waitFor();
  await page.locator("#rb-form").evaluate(form => { form.requestSubmit(); form.requestSubmit(); });
  await received;
  assert.equal(await page.locator("#rb-form").getAttribute("aria-busy"), "true");
  release();
  await page.locator("#rb-form").getByText("Fixture save failed").waitFor();
  assert.equal(writes.length, 1);
  await page.getByRole("button", { name: "Add automation" }).click();
  await page.locator("#sheet").waitFor({ state: "hidden" });
  assert.equal(writes.length, 2);
  assert.equal(writes[1].rules.length, 1);
});

test("automation search keeps priority and edited copies save beside their source", async t => {
  const writes = [];
  const page = await staticPage(t, async (route, url) => {
    if (url.pathname === "/api/apps") { await route.fulfill({ json: [] }); return true; }
    if (url.pathname !== "/api/automations") return false;
    const { rules } = route.request().postDataJSON();
    writes.push(rules);
    await route.fulfill({ json: rules });
    return true;
  });
  await page.evaluate(async () => {
    const { S } = await import("/js/core.js");
    S.state = { override: { active: ["night"], lighting: "night" } };
    S.rules = [
      { id: "lock", enabled: false, when: { type: "locked" }, then: { type: "lights_off" } },
      { id: "night", enabled: true, when: { type: "time", start: "22:30", end: "06:00", days: [0] }, then: { type: "dim", brightness: 15 } },
      { id: "game", enabled: true, when: { type: "app_running", exe: "Game.exe" }, then: { type: "lights_off" } },
    ];
    S.presets = []; S.curves = { curves: {} };
    document.querySelector(".auto-view").hidden = false;
    const auto = await import("/js/automations.js"); auto.initAutomations(); auto.renderAutomations();
  });
  const rows = page.locator("#rules li"), search = page.getByLabel("Search automations"), status = page.getByLabel("Filter rules by status");
  await search.fill("GaMe.exe");
  assert.equal(await rows.count(), 1);
  assert.equal(await rows.first().getAttribute("value"), "3");
  assert.ok(await rows.locator("[data-up]").isDisabled());
  await status.selectOption("disabled");
  assert.equal(await rows.count(), 0);
  assert.equal(await page.locator("#rule-count").innerText(), "No matching rules");
  await search.fill("");
  assert.equal(await rows.first().getAttribute("data-id"), "lock");
  await status.selectOption("active");
  assert.equal(await rows.first().getAttribute("data-id"), "night");
  await page.getByRole("button", { name: "Clear filters" }).click();
  assert.equal(await rows.count(), 3);
  const duplicate = page.locator("#copy-night");
  await duplicate.press("Enter");
  const enabled = page.getByLabel("Enable rule");
  assert.equal(await enabled.isChecked(), false);
  await page.keyboard.press("Escape");
  assert.equal(writes.length, 0);
  await duplicate.press("Enter");
  const detail = page.locator("#rb-schedule-detail");
  assert.match(await detail.innerText(), /7h 30m.*following day/);
  await page.getByRole("button", { name: "Weekends", exact: true }).click();
  await page.getByRole("button", { name: "Sat", exact: true }).click();
  await page.getByRole("button", { name: "Sun", exact: true }).click();
  const save = page.getByRole("button", { name: "Add automation" });
  await save.click();
  assert.equal(writes.length, 0);
  assert.match(await detail.innerText(), /at least one/);
  await page.getByRole("button", { name: "Weekdays", exact: true }).click();
  const end = page.getByLabel("End time");
  await end.fill("22:30");
  await save.click();
  assert.equal(writes.length, 0);
  assert.match(await detail.innerText(), /different start and end/);
  await end.fill("23:15");
  assert.match(await detail.innerText(), /45m on each selected day/);
  await save.click();
  await page.locator("#sheet").waitFor({ state: "hidden" });
  assert.equal(writes.length, 1);
  const [first, original, copy, last] = writes[0];
  assert.deepEqual([first.id, original.id, last.id], ["lock", "night", "game"]);
  assert.equal(original.enabled, true);
  assert.equal(original.when.end, "06:00");
  assert.notEqual(copy.id, original.id);
  assert.equal(copy.enabled, false);
  assert.deepEqual(copy.when.days, [0, 1, 2, 3, 4]);
  assert.equal(copy.when.end, "23:15");
  await status.selectOption("disabled");
  await page.locator(`#enable-${copy.id}`).press("Space");
  await page.locator(`#enable-${copy.id}`).waitFor({ state: "detached" });
  assert.ok(await search.evaluate(el => el === document.activeElement), "focus remains in the rules view when a changed rule leaves the filter");
  await page.getByRole("button", { name: "Clear filters", exact: true }).click();
  await page.locator(`#edit-${copy.id}`).press("Enter");
  assert.ok(await page.getByLabel("Enable rule").isChecked());
  await page.keyboard.press("Escape");
  await page.setViewportSize({ width: 360, height: 780 });
  assert.ok(await page.locator(".auto-view").evaluate(el => el.scrollWidth <= el.clientWidth + 1));
});

test("automation numeric drafts stay invalid across unrelated selector changes", async t => {
  const writes = [];
  const page = await staticPage(t, async (route, url) => {
    if (url.pathname === "/api/apps") { await route.fulfill({ json: [] }); return true; }
    if (url.pathname !== "/api/automations") return false;
    const { rules } = route.request().postDataJSON();
    writes.push(rules);
    await route.fulfill({ json: rules });
    return true;
  });
  await page.evaluate(async () => {
    const { S } = await import("/js/core.js");
    S.state = { temp_unit: "F" }; S.rules = []; S.presets = []; S.curves = { curves: {} };
    document.querySelector(".auto-view").hidden = false;
    const auto = await import("/js/automations.js"); auto.initAutomations(); auto.renderAutomations();
  });
  await page.locator("#auto-add").click();
  const when = page.locator("#rb-when"), then = page.locator("#rb-then"), save = page.locator("#rb-save");
  const brightness = page.getByLabel("Brightness"), temperature = page.getByLabel("Temperature", { exact: true });
  const assertInvalid = async (input, raw, message) => {
    assert.equal(await input.inputValue(), raw);
    assert.equal(await input.evaluate(el => el.validity.valid), false);
    assert.match(await page.locator("#rb-preview").innerText(), message);
    await save.click();
    assert.equal(writes.length, 0);
  };
  await then.selectOption("dim");
  await brightness.fill("");
  await when.selectOption("temp_above");
  await assertInvalid(brightness, "", /brightness between 0 and 100/i);
  await brightness.fill("0");
  await temperature.fill("");
  await then.selectOption("lights_off");
  await assertInvalid(temperature, "", /temperature between 86 and 212/i);
  await temperature.fill("250");
  await then.selectOption("dim");
  await assertInvalid(temperature, "250", /temperature between 86 and 212/i);
  await temperature.fill("");
  await temperature.press("-");
  assert.ok(await temperature.evaluate(el => el.validity.badInput));
  await then.selectOption("lights_off");
  assert.ok(await temperature.evaluate(el => el.validity.badInput), "an unfinished number remains in the native input");
  await assertInvalid(temperature, "", /temperature between 86 and 212/i);
  await then.selectOption("dim");
  await temperature.fill("176");
  await brightness.fill("0");
  await save.click();
  await page.locator("#sheet").waitFor({ state: "hidden" });
  assert.equal(writes.length, 1);
  assert.equal(writes[0][0].when.temp, 80, "Fahrenheit input saves in whole Celsius");
  assert.equal(writes[0][0].then.brightness, 0, "an explicit zero remains valid");
});

test("short Overview shares its section and reading row and keeps the lighting controls compact", async t => {
  const page = await overview(t, [{ id: "lighting", type: "lighting", w: 2, h: 2 }, tile(sensors[0], "auto", 0)]);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.evaluate(() => { document.querySelector("#ambient").innerHTML = "<span>CPU 52°</span><span>GPU 48°</span>"; });
  const nav = await page.locator("#nav-select").boundingBox(), pins = await page.locator("#ambient").boundingBox();
  assert.ok(Math.min(nav.y + nav.height, pins.y + pins.height) > Math.max(nav.y, pins.y), "normal-length pins share the Section row");
  const rig = await page.locator(".tile-lighting").boundingBox();
  assert.ok(rig.height >= 180 && rig.height < 265, "the normal rig fits a compact but usable mobile frame");
  assert.equal(await page.locator(".tile-lighting").getAttribute("data-h"), "2");
  await page.evaluate(async () => {
    (await import("/js/appearance.js")).applyAppearance({ scale: 120 }, { persist: false });
    document.querySelector(".lt-effect").textContent = "Gradient";
  });
  assert.ok(await page.locator(".lt-effect").evaluate(el => el.scrollWidth <= el.clientWidth + 1), "The active Gradient name fits at 120%");
  await page.setViewportSize({ width: 320, height: 640 });
  await page.evaluate(async () => {
    (await import("/js/appearance.js")).applyAppearance({ scale: 120 }, { persist: false });
    const readings = document.querySelector("#ambient");
    readings.innerHTML = '<span class="pin">GraphicsCardPower <b>143.7</b><i> W</i></span><span class="pin">Temperature <b>103.9</b><i> °F</i></span>';
  });
  assert.ok(await page.locator("#ambient").evaluate(el => el.scrollWidth <= el.clientWidth + 1), "long pin labels and units fit at 120%");
  assert.ok(await page.locator(".appbar").evaluate(el => el.scrollWidth <= el.clientWidth + 1));
});

test("one Appearance entry describes the saved look and its design preview cancels", async t => {
  const page = await staticPage(t);
  await page.evaluate(async () => {
    const { S } = await import("/js/core.js");
    S.state = {};
    S.settings = { settings: { design: "paper", appearance: { scale: 110, density: "compact", accent: "#65ab85" } } };
    document.querySelector(".settings-view").hidden = false;
    const settings = await import("/js/settings.js");
    settings.initSettings(); settings.renderSettings();
  });
  assert.equal(await page.locator("#s-design, #s-appearance").count(), 1);
  const editor = page.getByRole("button", { name: "Edit appearance" });
  const row = editor.locator("xpath=ancestor::div[contains(@class,'set-row')]");
  assert.match(await row.innerText(), /paper/i);
  assert.match(await row.innerText(), /110%/);
  assert.match(await row.innerText(), /compact/i);
  assert.match(await row.innerText(), /#65ab85/i);
  await editor.press("Enter");
  const design = page.locator("#ap-design");
  assert.ok(await design.evaluate(el => el === document.activeElement));
  await design.selectOption("firefly");
  await page.keyboard.press("Escape");
  assert.equal(await page.locator("html").getAttribute("data-design"), "paper");
  assert.ok(await editor.evaluate(el => el === document.activeElement));
});
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
    // live values come only from the state readings, never from sensor metadata
    S.state = { scene: { on: true, brightness: 75 }, effective: { on: true, effect: "solid" },
      readings: Object.fromEntries(sensors.map(sensor => [sensor.id, sensor.value])) };
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

test("Lighting scopes a tile to the selected zone and disconnects never redirect it to the whole rig", async t => {
  const writes = [];
  const config = { name: "Front", enabled: true, layout: { shape: "strip", x: .4, y: .5, angle: 0, length: .2 }, override: null };
  const page = await staticPage(t, async (route, url) => {
    if (route.request().method() !== "POST") return false;
    const body = route.request().postDataJSON();
    writes.push([url.pathname, body]);
    const previous = config.override;
    config.override = body.override;
    await route.fulfill({ json: { config, previous_override: previous } });
    return true;
  });
  await page.evaluate(async () => {
    const { S, on } = await import("/js/core.js");
    const { initLighting, renderLighting } = await import("/js/lighting.js");
    S.state = { scene: { effect: "solid", params: {}, brightness: 65 }, scene_status: { status: "ready", undo_available: true } };
    S.effects = ["solid", "cycle"].map(id => ({ id, name: id, params: [], needs: [] }));
    S.presets = [{ id: "desk", name: "Desk" }];
    S.zones = { zones: [{ key: "hub/front", device: { name: "Desk", online: true }, positions: [[.4, .5]], config: {
      name: "Front", enabled: true, layout: { shape: "strip", x: .4, y: .5, angle: 0, length: .2 }, override: null,
    } }] };
    window.chosenEffects = []; on("effect:applied", id => window.chosenEffects.push(id));
    document.querySelector(".lighting-view").hidden = false;
    (await import("/js/stage.js")).initStage(); initLighting(); renderLighting();
  });
  const picker = page.locator("#zone-pick"), cycle = page.locator('[data-effect="cycle"]');
  assert.equal(await page.locator(".lighting-saved [data-scene-undo]").count(), 1);
  assert.equal(await page.locator("#stage #stage-hint").count(), 0);
  assert.equal(await page.locator('.preset-choice > [data-favorite-id="desk"]').count(), 1);
  assert.equal(await page.locator('.fx-choice > [data-favorite-id="cycle"]').count(), 1);
  await picker.selectOption("hub/front");
  await cycle.press("Enter");
  await page.waitForFunction(() => window.chosenEffects.length > 0, null, { timeout: 4000 }).catch(async error => {
    throw new Error(`${error.message}; writes ${JSON.stringify(writes)}, scope ${await picker.inputValue()}, inspector ${await page.locator("#inspector").innerText()}`);
  });
  assert.equal(await picker.inputValue(), "hub/front");
  assert.equal(await cycle.getAttribute("aria-checked"), "true");
  assert.ok(await cycle.evaluate(el => el === document.activeElement));
  assert.deepEqual(writes.map(([path]) => path), ["/api/zones/hub%2Ffront"]);
  assert.deepEqual(writes[0][1].override, { effect: "cycle", params: {} });
  const undo = page.getByRole("button", {name:"Undo zone lighting"});
  await undo.press("Enter");
  await undo.waitFor({state:"hidden"});
  assert.deepEqual(writes[1][1], { override: null, expected_override: {effect:"cycle",params:{}} });
  assert.deepEqual(await page.evaluate(() => window.chosenEffects), ["cycle"]);
  await cycle.press("Enter");
  await undo.waitFor();
  await page.locator('[data-mode="follow"]').press("Enter");
  await page.waitForFunction(() => document.querySelector('[data-mode="follow"]').getAttribute("aria-checked") === "true");
  assert.deepEqual(await page.evaluate(() => window.chosenEffects), ["cycle","cycle"]);
  await page.evaluate(async () => { const { S, emit } = await import("/js/core.js"); S.zones.zones = []; emit("zones"); });
  assert.equal(await picker.inputValue(), "hub/front");
  assert.equal(await cycle.isDisabled(), true);
  await page.locator("#zone-done").press("Enter");
  assert.equal(await picker.inputValue(), "");
  assert.equal(await cycle.isDisabled(), false);
  assert.ok(await picker.evaluate(el => el === document.activeElement));
  assert.deepEqual(writes.map(([path]) => path), Array(4).fill("/api/zones/hub%2Ffront"));
});

test("Fans keeps exact live and focused draft readings outside the plot during keyboard and pointer edits", async t => {
  const writes = [];
  const page = await staticPage(t, async (route, url) => {
    if (route.request().method() !== "POST") return false;
    writes.push(url.pathname); await route.abort(); return true;
  });
  await page.evaluate(async () => {
    const { S } = await import("/js/core.js");
    const { initFans, renderFans } = await import("/js/fans.js");
    S.state = { temp_unit: "C", sensors: {}, fans: { curve: "balanced", base_curve: "balanced", ports: [], temp: 50, duty: 37, target: 65 } };
    const balanced = [[30, 30], [60, 50], [80, 100]];
    S.curves = { curves: { balanced }, defaults: { balanced }, min_duty: 20, safety_temp: 90 };
    document.querySelector(".fans-view").hidden = false;
    initFans(); renderFans();
  });
  const point = page.locator('#editor .pt[data-i="1"]'), legend = page.locator("#curve-point");
  await point.focus();
  assert.equal(await legend.textContent(), "60°C · 50%");
  await page.keyboard.press("Shift+ArrowUp");
  assert.equal(await legend.textContent(), "60°C · 55%");
  assert.ok(await point.evaluate(el => el === document.activeElement));
  assert.equal(await page.locator("#curve-live").textContent(), "50°C · 37%");
  const coord = await point.boundingBox();
  await page.mouse.move(coord.x + coord.width / 2, coord.y + coord.height / 2);
  await page.mouse.down();
  await page.mouse.move(coord.x + coord.width / 2 + 20, coord.y + coord.height / 2 - 20);
  const dragged = await legend.textContent();
  assert.notEqual(dragged, "60°C · 55%");
  await page.mouse.up();
  assert.equal(await legend.textContent(), dragged);
  assert.equal(await page.locator("#curve-live").textContent(), "50°C · 37%");
  assert.ok(await page.locator("#curve-save").isEnabled() && await page.locator("#curve-discard").isEnabled());
  assert.equal(await page.locator("#editor .now-label, #editor .tip").count(), 0);
  assert.ok(await page.getByText("Always full speed").isVisible());
  assert.deepEqual(writes, []);
});

test("Studio puts saved work before templates and shows a single reduced-motion explanation", async t => {
  let effects = [];
  const page = await staticPage(t, async (route, url) => {
    if (url.pathname !== "/api/studio/effects") return false;
    await route.fulfill({ json: { effects } }); return true;
  });
  await page.evaluate(async () => {
    const { initStudio, renderStudio } = await import("/js/studio.js");
    document.querySelector(".studio-view").hidden = false;
    initStudio(); await renderStudio();
  });
  const sections = () => page.locator("#st-start, #st-saved").evaluateAll(nodes => nodes.map(n => n.id));
  assert.deepEqual(await sections(), ["st-start", "st-saved"]);
  assert.equal(await page.locator("#st-motion-note:visible").count(), 1);
  assert.equal(await page.locator("[data-preview]:visible").count(), 0);
  effects = [{ fx_id: "fx:mine", name: "Apricot", origin: "mine", kind: "layers", layers: [] }];
  await page.evaluate(async () => { await (await import("/js/studio.js")).renderStudio(); });
  assert.deepEqual(await sections(), ["st-saved", "st-start"]);
  assert.ok(await page.locator('[data-edit="fx:mine"]').isVisible());
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.locator('#st-start [data-preview]').first().waitFor();
  assert.equal(await page.locator("#st-motion-note").isVisible(), false);
});

test("Studio templates have a bundled sample still before playback and keep it through a preview error", async (t) => {
  const posts = [];
  const page = await staticPage(t, async (route, url) => {
    if (url.pathname === "/api/studio/effects") {
      await route.fulfill({ json: { effects: [] } });
      return true;
    }
    if (route.request().method() === "POST") {
      posts.push(url.pathname);
      await route.fulfill({ status: 503, json: { error: "Preview temporarily unavailable" } });
      return true;
    }
    return false;
  });
  await page.evaluate(async () => {
    const { initStudio, renderStudio } = await import("/js/studio.js");
    document.querySelector(".studio-view").hidden = false;
    initStudio();
    await renderStudio();
    await Promise.all([...document.querySelectorAll(".st-poster")].map(img => img.decode()));
  });
  const posters = page.locator(".st-tpl .st-poster");
  const templates = await page.evaluate(async () => {
    const { LAYER_TEMPLATES, CODE_TEMPLATES } = await import("/js/studio-start.js");
    return LAYER_TEMPLATES.length + CODE_TEMPLATES.length;
  });
  assert.ok(templates > 0);
  assert.equal(await posters.count(), templates);
  assert.equal(await posters.evaluateAll(images => images.every(img => img.naturalWidth > 0 && img.naturalHeight > 0)), true);
  assert.deepEqual(posts, []);
  const play = page.locator('[data-card="aurora"] [data-preview]');
  assert.equal(await play.isDisabled(), true);
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await play.click();
  await page.getByText("Couldn't play this preview.", { exact: false }).waitFor();
  assert.deepEqual(posts, ["/api/studio/preview/load"]);
  assert.equal(await play.getAttribute("aria-pressed"), "false");
  assert.equal(await page.locator('[data-card="aurora"] .has-frame').count(), 0);
  const retry = page.waitForResponse(response => response.url() === BASE + "/api/studio/preview/load");
  await page.locator('[data-tpl="aurora"]').focus();
  await retry;
  assert.deepEqual(posts, ["/api/studio/preview/load", "/api/studio/preview/load"]);
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

test("presentation accents remain legible in every theme and all five styles across supported widths and zooms", async (t) => {
  const tiles = [...sensors.map((sensor, index) => tile(sensor, "auto", index)),
    ...styles.slice(1).map(style => tile(sensors[0], style, 0)),
    { ...tile(sensors[0], "auto", 5), appearance: { emphasis: "quiet", accent: "#eeeeee" } },
    { ...tile(sensors[0], "spark", 6), appearance: { emphasis: "standard", accent: null } }];
  const page = await overview(t, tiles);
  const themes = await page.evaluate(async () => (await import("/js/design.js")).DESIGNS.map(({ id }) => id));
  assert.ok(themes.includes("firefly"));
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

test("a delayed Settings read retains an acknowledged favorite and a later read can update it", async t => {
  const started = Promise.withResolvers(), delayed = Promise.withResolvers();
  const pinned = { kind: "effect", id: "solid" };
  let reads = 0;
  const page = await staticPage(t, async (route, url) => {
    if (url.pathname === "/api/quick-actions") {
      await route.fulfill({ json: { favorites: [pinned], recents: [] } });
      return true;
    }
    if (url.pathname !== "/api/settings") return false;
    if (++reads === 1) { started.resolve(); await delayed.promise; }
    await route.fulfill({ json: { settings: { design: "firefly", quick_actions: { favorites: [], recents: [] } } } });
    return true;
  });
  await page.evaluate(async () => {
    const { S } = await import("/js/core.js");
    S.settings = { settings: { design: "firefly" } }; S.state = {};
    const { loadSettings } = await import("/js/settings.js");
    window.pendingSettings = loadSettings();
  });
  await started.promise;
  await page.evaluate(async () => (await import("/js/quick-preferences.js")).setFavorite("effect", "solid", true));
  const favorite = () => page.evaluate(async () => (await import("/js/quick-preferences.js")).isFavorite("effect", "solid"));
  assert.equal(await favorite(), true);
  delayed.resolve();
  await page.evaluate(() => window.pendingSettings);
  assert.equal(await favorite(), true, "an older snapshot cannot replace the pin");
  await page.evaluate(async () => (await import("/js/settings.js")).loadSettings());
  assert.equal(await favorite(), false, "a fresh server response can reflect another client's unpin");
});

test("a favorite save failure stays visible after closing Quick switch", async t => {
  const started = Promise.withResolvers(), delayed = Promise.withResolvers();
  const page = await switcher(t, async (route, url) => {
    if (url.pathname !== "/api/quick-actions") return false;
    started.resolve(); await delayed.promise;
    await route.fulfill({ status: 503, json: { error: "Fixture disk unavailable" } });
    return true;
  });
  await page.keyboard.press("Control+k");
  const dialog = page.getByRole("dialog", { name: "Quick switch" });
  await dialog.getByRole("button", { name: "Edit favorites" }).click();
  await dialog.getByRole("combobox").fill("night");
  await dialog.getByRole("combobox").press("Enter");
  await started.promise;
  await page.keyboard.press("Escape"); delayed.resolve();
  await page.getByText(/Fixture disk unavailable/).waitFor({ timeout: 2000 });
  assert.equal(await dialog.count(), 0);
});

test("keyboard pinning is separate from hardware and resolves renamed or deleted favorites", async t => {
  const writes = [];
  const favorites = [];
  const recents = [{ kind: "effect", id: "solid" }];
  const page = await switcher(t, async (route, url) => {
    if (route.request().method() !== "POST") return false;
    writes.push(url.pathname);
    if (url.pathname !== "/api/quick-actions") return false;
    const { kind, id, operation } = route.request().postDataJSON();
    assert.equal(operation, "pin");
    favorites.push({ kind, id });
    await route.fulfill({ json: { favorites, recents } });
    return true;
  });
  await page.evaluate(async recents => {
    const { S } = await import("/js/core.js");
    S.settings = { settings: { quick_actions: { favorites: [], recents } } };
  }, recents);
  await page.keyboard.press("Control+k");
  const dialog = page.getByRole("dialog", { name: "Quick switch" });
  const input = dialog.getByRole("combobox");
  await dialog.getByRole("button", { name: "Edit favorites" }).press("Enter");
  await input.fill("night");
  await input.press("Enter");
  await dialog.getByRole("option", { name: /Unpin favorite.*Lighting and cooling/ }).waitFor();
  await dialog.getByRole("button", { name: "Done editing favorites" }).press("Enter");
  await input.fill("");
  const groups = dialog.getByRole("listbox").getByRole("group");
  assert.equal(await groups.nth(0).getAttribute("aria-label"), "Favorites");
  assert.equal(await groups.nth(1).getAttribute("aria-label"), "Recently used");
  await page.evaluate(async () => {
    const { S, emit } = await import("/js/core.js");
    S.presets.find(p => p.id === "night").name = "Quiet evening";
    emit("state");
  });
  await groups.first().getByRole("option", { name: /Quiet evening/ }).waitFor();
  await page.evaluate(async () => {
    const { S, emit } = await import("/js/core.js");
    S.presets = S.presets.filter(p => p.id !== "night");
    emit("state");
  });
  assert.equal(await dialog.getByRole("group", { name: "Favorites", exact: true }).count(), 0);
  assert.deepEqual(writes, ["/api/quick-actions"]);
});

test("Recents accepts only confirmed manual effects and profiles", async t => {
  const writes = [];
  const page = await switcher(t, async (route, url) => {
    if (url.pathname === "/api/quick-actions") {
      writes.push(route.request().postDataJSON());
      await route.fulfill({ json: { favorites: [], recents: [] } });
      return true;
    }
    if (url.pathname === "/api/scene" || url.pathname.endsWith("/apply")) {
      const failed = url.pathname.includes("night");
      await route.fulfill({ json: { ok: !failed, scene_status: { status: "pending", desired_revision: 5, session: "demo" } } });
      return true;
    }
    return false;
  });
  await page.evaluate(async () => {
    const { post, settleSceneMutation } = await import("/js/core.js");
    const status = { status: "ready", desired_revision: 5, session: "demo" };
    settleSceneMutation(status); // An automation or an unsolicited live poll.
    await post("/api/scene", { effect: "spectrum" });
    settleSceneMutation({ ...status, status: "failed" });
    settleSceneMutation(status); // Background recovery cannot turn a failed choice into a recent.
    await post("/api/scene", { effect: "spectrum" });
    settleSceneMutation({ ...status, overridden: true });
    await post("/api/presets/night/apply"); // A partial profile failure.
    settleSceneMutation(status);
    await post("/api/scene", { effect: "solid" });
    settleSceneMutation(status);
    settleSceneMutation(status); // Repeating a poll must never create another recent entry.
    await post("/api/presets/movie-time/apply");
    settleSceneMutation(status);
  });
  await page.waitForFunction(() => performance.getEntriesByType("resource").filter(e => e.name.endsWith("/api/quick-actions")).length >= 2);
  assert.deepEqual(writes, [
    { operation: "use", kind: "effect", id: "solid" },
    { operation: "use", kind: "profile", id: "movie-time" },
  ]);
});

test("Quick switch finds settings by topic and zones by unaccented device or zone name without a write", async t => {
  const page = await switcher(t);
  const writes = [];
  page.on("request", request => { if (request.method() === "POST") writes.push(request.url()); });
  await page.evaluate(async () => {
    const { S } = await import("/js/core.js");
    S.zones = { zones: [
      { key: "hub/desk #1", config: { name: "Café-strip" }, device: { name: "Desk hub", online: true } },
      { key: "old", config: { name: "Café lamp" }, device: { name: "Desk hub", online: false } },
    ] };
  });
  const input = page.getByRole("combobox", { name: "Search actions" });
  const dialog = page.getByRole("dialog", { name: "Quick switch" });
  await page.keyboard.press("Control+k");
  await input.fill("coolant ramp");
  assert.match(await dialog.getByRole("option", { selected: true }).textContent(), /Fans.*Open settings/);
  await input.press("Enter");
  await page.waitForURL("**/#settings/fans");
  await page.keyboard.press("Control+k");
  await dialog.getByRole("button", { name: "Zones", exact: true }).click();
  await input.fill("desk cafe strip");
  assert.equal(await dialog.getByRole("option").count(), 1);
  assert.match(await dialog.getByRole("option").textContent(), /Café-strip.*Desk hub/);
  await input.press("Enter");
  await page.waitForFunction(() => decodeURIComponent(location.hash) === "#lighting/hub/desk #1");
  assert.deepEqual(writes, []);
});

test("Pinned search includes only saved favorites and Page keys move a full screen without applying", async t => {
  const page = await switcher(t);
  await page.evaluate(async () => {
    const { S } = await import("/js/core.js");
    S.settings = { settings: { quick_actions: { favorites: [{ kind: "effect", id: "solid" }], recents: [] } } };
  });
  await page.keyboard.press("Control+k");
  const dialog = page.getByRole("dialog", { name: "Quick switch" });
  const input = dialog.getByRole("combobox", { name: "Search actions" });
  await dialog.getByRole("button", { name: "Pinned", exact: true }).click();
  assert.equal(await dialog.getByRole("option").count(), 1);
  assert.match(await dialog.getByRole("option").textContent(), /Solid/);
  await input.fill("Movie");
  await dialog.getByText("No matching actions.", { exact: true }).waitFor();
  await dialog.getByRole("button", { name: "Show all actions" }).click();
  await input.press("PageDown");
  assert.ok(Number(await dialog.getByRole("option", { selected: true }).getAttribute("data-index")) > 1);
  await input.press("PageUp");
  assert.equal(await input.getAttribute("aria-activedescendant"), "qs-option-0");
  await input.press("Escape");
});

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
  const input = dialog.getByRole("combobox", { name: "Search actions" });
  const list = dialog.getByRole("listbox", { name: "Matching actions" });
  assert.equal(await input.evaluate(el => el === document.activeElement), true);
  await input.fill("  EfFecT  ");
  assert.equal(await list.getByRole("option").count(), 2);
  await list.getByRole("option").last().hover();
  await page.keyboard.press("ArrowDown");
  assert.equal(await list.getByRole("option", { selected: true }).textContent().then(text => text.includes("Music Spectrum")), true);
  assert.deepEqual(requests, []);
  await input.fill("ProFiLe night");
  assert.match(await list.getByRole("option", { selected: true }).textContent(), /Apply profile.*cooling/i);
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
  assert.match(await dialog.getByRole("option", { selected: true }).textContent(), /Settings\s*Open section/i);
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

test("Quick switch opens from the header, filters actions and recovers from an empty search", async (t) => {
  const page = await switcher(t);
  const launch = page.getByRole("button", { name: "Quick switch", exact: true });
  const dialog = page.getByRole("dialog", { name: "Quick switch" });
  const input = dialog.getByRole("combobox", { name: "Search actions" });
  await page.setViewportSize({ width: 390, height: 700 });
  await page.evaluate(async () => (await import("/js/appearance.js")).applyAppearance({ scale: 120 }, { persist: false }));
  await launch.click();
  const filter = dialog.getByRole("combobox", { name: "Search in" });
  const results = dialog.getByRole("listbox", { name: "Matching actions" });
  await filter.selectOption("profile");
  assert.equal(await results.getByRole("option").count(), 3);
  await input.fill("Solid");
  assert.equal(await results.getByRole("option").count(), 0);
  await dialog.getByRole("button", { name: "Show all actions" }).click();
  assert.equal(await input.inputValue(), "");
  assert.equal(await filter.inputValue(), "all");
  assert.equal(await input.evaluate(el => el === document.activeElement), true);
  await filter.selectOption("workspace");
  assert.match(await results.getByRole("option").last().textContent(), /Media Wall/);
  assert.equal(await dialog.evaluate(el => el.scrollWidth <= el.clientWidth + 1), true);
  await page.keyboard.press("Escape");
  assert.equal(await launch.evaluate(el => el === document.activeElement), true);
});

test("header search, navigation, pinned readings and power remain disjoint across window sizes", async (t) => {
  const page = await switcher(t);
  await page.locator("#ambient").evaluate(el => {
    el.innerHTML = ["CPU", "GPU", "CPU load", "Memory"].map(name => `<span class="pin">${name} <b>64</b><i>%</i></span>`).join("");
  });
  const clipped = [];
  for (const width of [390, 540, 640, 701, 800, 980, 1024, 1180, 1280, 1440]) {
    await page.setViewportSize({ width, height: 844 });
    for (const scale of [100, 120]) {
      const collisions = await page.evaluate(async scale => {
        (await import("/js/appearance.js")).applyAppearance({ scale }, { persist: false });
        const controls = () => [...document.querySelectorAll(".wordmark, .tabs a, #nav-select, #ambient, #power, #quick-open")]
          .filter(el => el.getClientRects().length).map(el => ({ id: el.id || el.textContent.trim(), rect: el.getBoundingClientRect() }));
        const problems = [];
        for (const theme of ["firefly", "graphite", "ascii"]) {
          document.documentElement.dataset.design = theme;
          await document.fonts.ready;
          const boxes = controls();
          for (const [index, control] of boxes.entries()) {
            const a = control.rect;
            if (a.right > innerWidth + 1 || a.left < 0) problems.push(`${theme}: ${control.id}`);
            for (const other of boxes.slice(index + 1)) {
              const b = other.rect;
              if (a.left < b.right - 1 && a.right > b.left + 1 && a.top < b.bottom - 1 && a.bottom > b.top + 1)
                problems.push(`${theme}: ${control.id} / ${other.id}`);
            }
          }
        }
        return problems;
      }, scale);
      if (collisions.length) clipped.push({ width, scale, collisions });
    }
  }
  assert.deepEqual(clipped, []);
});

test("an updated catalog retains the highlighted command and a removed result cannot run", async (t) => {
  const page = await switcher(t);
  const mutations = [];
  page.on("request", request => { if (request.method() === "POST") mutations.push(request.url()); });
  await page.keyboard.press("Control+k");
  const dialog = page.getByRole("dialog", { name: "Quick switch" });
  await dialog.getByRole("button", { name: "Effects", exact: true }).click();
  await page.keyboard.press("ArrowDown");
  await page.evaluate(async () => {
    const { S, emit } = await import("/js/core.js");
    S.effects.reverse();
    emit("state");
  });
  assert.match(await dialog.getByRole("option", { selected: true }).textContent(), /Music Spectrum/);
  await page.evaluate(async () => { (await import("/js/core.js")).S.effects.shift(); });
  await page.keyboard.press("Enter");
  await dialog.getByRole("status").getByText(/no longer available/).waitFor();
  assert.deepEqual(mutations, []);
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

async function colourEditorPage(t, mock) {
  const page = await staticPage(t, mock);
  await page.evaluate(async () => {
    const { S } = await import("/js/core.js");
    const { initLighting, renderLighting } = await import("/js/lighting.js");
    const colors = ["#112233", "#abcdef"];
    S.state = { scene: { effect: "gradient", params: { colors, speed: 20 }, brightness: 60 },
      scene_status: { status: "ready", desired_revision: 4, session: "fixture" } };
    S.effects = [
      { id: "gradient", name: "Gradient", needs: [], params: [{ id: "colors", type: "colors", label: "Colours",
        min: 2, max: 4, default: ["#ff0000", "#0000ff"] }] },
      { id: "solid", name: "Solid", needs: [], params: [{ id: "color", type: "color", label: "Colour", default: "#778899" }] },
    ];
    S.zones = { zones: [{ key: "hub/front", device: { name: "Desk hub", online: true }, positions: [[.4, .5]],
      config: { name: "Front strip", enabled: true, layout: { shape: "strip", x: .4, y: .5, angle: 0, length: .2 },
        override: { effect: "solid", params: { color: "#123456" } } } }] };
    document.querySelector(".lighting-view").hidden = false;
    (await import("/js/stage.js")).initStage();
    initLighting(); renderLighting();
  });
  return page;
}

test("Lighting palette edits stay local until applied and share saved palettes with Studio", async t => {
  const writes = [];
  const page = await colourEditorPage(t, async (route, url) => {
    if (url.pathname !== "/api/scene") return false;
    const body = route.request().postDataJSON(); writes.push(body);
    await route.fulfill({ json: { scene: { effect: "gradient", params: body.params, brightness: 60 },
      scene_status: { desired_revision: 5, session: "fixture", status: "ready" } } });
    return true;
  });
  await page.setViewportSize({ width: 360, height: 720 });
  const opener = page.getByRole("button", { name: "Edit palette" });
  await opener.click();
  const dialog = page.getByRole("dialog", { name: "Edit palette" });
  const input = dialog.getByLabel("Hex colours");
  const apply = dialog.getByRole("button", { name: "Apply palette" });
  await input.fill("#ggg");
  assert.ok(await apply.isDisabled());
  await dialog.getByRole("button", { name: "Polar night" }).click();
  await dialog.getByRole("button", { name: "Reverse order" }).click();
  assert.equal(await input.inputValue(), "#83e6cc #4473bd #10244d");
  await dialog.getByRole("button", { name: "Use effect default" }).click();
  assert.equal(await input.inputValue(), "#ff0000 #0000ff");
  await input.fill("dca, #12e");
  await dialog.locator("summary").click();
  const name = dialog.getByLabel("Palette name");
  await name.fill("My dusk");
  await name.press("Enter");
  assert.match(await dialog.getByRole("status").textContent(), /Saved My dusk/);
  const shared = await page.evaluate(async () => {
    const { PALETTE_KEY, cleanPalettes } = await import("/js/studio-document.js");
    return cleanPalettes(JSON.parse(localStorage.getItem(PALETTE_KEY)));
  });
  assert.deepEqual(shared, [{ name: "My dusk", colors: ["#ddccaa", "#1122ee"] }]);
  assert.ok(await dialog.evaluate(el => el.scrollWidth <= el.clientWidth + 1));
  await page.keyboard.press("Escape");
  assert.equal(writes.length, 0);
  await opener.press("Enter");
  await dialog.getByRole("button", { name: "My dusk" }).click();
  await apply.press("Enter");
  await dialog.waitFor({ state: "hidden" });
  assert.equal(writes.length, 1);
  assert.deepEqual(writes[0].params, { colors: shared[0].colors, speed: 20 });
  assert.equal(writes[0].expected_revision, 4);
  assert.equal(writes[0].expected_effect, "gradient");
  assert.ok(await opener.evaluate(el => document.activeElement === el));
});

test("Lighting colour tools remember picks and preserve zone scope through conflict and retry", async t => {
  const writes = [];
  const page = await colourEditorPage(t, async (route, url) => {
    if (route.request().method() !== "POST") return false;
    const body = route.request().postDataJSON(); writes.push([url.pathname, body]);
    if (writes.length === 1) await route.fulfill({ status: 409, json: { error: "Zone changed" } });
    else await route.fulfill({ json: { previous_override: body.expected_override,
      config: { name: "Front strip", enabled: true, layout: { shape: "strip", x: .4, y: .5, angle: 0, length: .2 }, override: body.override } } });
    return true;
  });
  await page.locator("#zone-pick").selectOption("hub/front");
  const opener = page.getByRole("button", { name: "Edit colour" });
  await opener.click();
  const dialog = page.getByRole("dialog", { name: "Edit colour" }), input = dialog.getByLabel("Hex colour");
  await input.fill("83f");
  await dialog.getByRole("button", { name: "Apply colour" }).click();
  await dialog.getByText("Zone changed", { exact: true }).waitFor();
  assert.equal(await input.inputValue(), "83f");
  await dialog.getByRole("button", { name: "Apply colour" }).click();
  await dialog.waitFor({ state: "hidden" });
  assert.deepEqual(writes.map(([url]) => url), ["/api/zones/hub%2Ffront", "/api/zones/hub%2Ffront"]);
  assert.deepEqual(writes[1][1], { override: { effect: "solid", params: { color: "#8833ff" } },
    expected_override: { effect: "solid", params: { color: "#123456" } } });
  await opener.click();
  await dialog.getByRole("button", { name: "Use effect default" }).click();
  await dialog.getByRole("button", { name: "Use #8833ff" }).click();
  assert.equal(await input.inputValue(), "#8833ff");
  await page.evaluate(async () => { (await import("/js/core.js")).S.zones.zones[0].device.online = false; });
  await input.fill("fed");
  await dialog.getByRole("button", { name: "Apply colour" }).click();
  await dialog.getByText("This zone is no longer available.").waitFor();
  assert.equal(writes.length, 2);
});

test("Lighting keeps an in-flight palette draft through Escape, a failed write and renderer startup", async t => {
  const writes = [], firstWrite = Promise.withResolvers(), failFirst = Promise.withResolvers();
  const poll = Promise.withResolvers(), ready = Promise.withResolvers();
  const page = await colourEditorPage(t, async (route, url) => {
    if (url.pathname === "/api/scene") {
      const body = route.request().postDataJSON();
      writes.push(body);
      if (writes.length === 1) {
        firstWrite.resolve();
        await failFirst.promise;
        await route.fulfill({ status: 503, json: { error: "Fixture save failed" } });
      } else {
        await route.fulfill({ json: { scene: { effect: "gradient", params: body.params, brightness: 60 },
          scene_status: { desired_revision: 5, session: "fixture", status: "pending" } } });
      }
      return true;
    }
    if (url.pathname !== "/api/state") return false;
    poll.resolve();
    await ready.promise;
    await route.fulfill({ json: { scene_status: { desired_revision: 5, session: "fixture", status: "ready" } } });
    return true;
  });
  const opener = page.getByRole("button", { name: "Edit palette" });
  await opener.click();
  const dialog = page.getByRole("dialog", { name: "Edit palette" });
  const hex = dialog.getByLabel("Hex colours");
  await hex.fill("abc 123");
  await dialog.getByRole("button", { name: "Apply palette" }).click();
  await firstWrite.promise;
  await page.keyboard.press("Escape");
  assert.ok(await dialog.isVisible());
  failFirst.resolve();
  await dialog.getByText("Fixture save failed").waitFor();
  assert.equal(await hex.inputValue(), "abc 123");
  await dialog.getByRole("button", { name: "Apply palette" }).press("Enter");
  await poll.promise;
  await page.keyboard.press("Escape");
  assert.ok(await dialog.isVisible());
  assert.ok(await dialog.getByRole("button", { name: "Applying…" }).isDisabled());
  ready.resolve();
  await dialog.waitFor({ state: "hidden" });
  assert.equal(writes.length, 2);
  assert.deepEqual(writes[0].params, writes[1].params);
  await opener.click();
  await page.keyboard.press("Escape");
  await dialog.waitFor({ state: "hidden" });
});

test("a replaced colour sheet reports a late error and can retry its original draft", async t => {
  const firstWrite = Promise.withResolvers(), failFirst = Promise.withResolvers(), writes = [];
  const page = await colourEditorPage(t, async (route, url) => {
    if (url.pathname !== "/api/scene") return false;
    const body = route.request().postDataJSON();
    writes.push(body);
    if (writes.length === 1) {
      firstWrite.resolve();
      await failFirst.promise;
      await route.fulfill({ status: 503, json: { error: "Palette save failed" } });
    } else await route.fulfill({ json: { scene: { effect: "gradient", params: body.params, brightness: 60 },
      scene_status: { desired_revision: 5, session: "fixture", status: "ready" } } });
    return true;
  });
  await page.getByRole("button", { name: "Edit palette" }).click();
  await page.getByLabel("Hex colours").fill("#abc #fed");
  await page.getByRole("button", { name: "Apply palette" }).click();
  await firstWrite.promise;
  await page.evaluate(async () => (await import("/js/help.js")).openShortcuts());
  const replacement = page.getByRole("dialog", { name: "Keyboard shortcuts" });
  await replacement.waitFor();
  await page.keyboard.press("Escape");
  await replacement.waitFor({ state: "hidden" });
  failFirst.resolve();
  const toast = page.locator("#toast");
  await toast.getByText(/Palette save failed/).waitFor();
  await toast.getByRole("button", { name: "Retry" }).click();
  await toast.getByText("Palette applied").waitFor();
  assert.equal(writes.length, 2);
  assert.deepEqual(writes[1].params, writes[0].params);
});
