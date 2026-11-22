import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { api, rigPage, until } from "./isolated-browser-rig.mjs";

const endpoint = "/api/lighting/wind-down";
const output = process.env.AIRGLOW_WIND_DOWN_OUTPUT || "output/wind-down";
mkdirSync(output, { recursive: true });

test("Wind down opens read-only, validates duration and releases to an edited scene on the running demo", async t => {
  await api("/api/scene", { on: true, effect: "solid", params: { color: "#ff9944" }, brightness: 80 });
  const before = await until(() => api("/api/state"), s => s.scene.brightness === 80 && s.scene_status.status === "ready");
  const page = await rigPage(t, "lighting"), writes = [];
  page.on("request", r => { if (r.method() === "POST") writes.push([new URL(r.url()).pathname, r.postDataJSON()]); });
  await page.keyboard.press("Control+k");
  const input = page.getByRole("dialog", { name: "Quick switch" }).getByRole("combobox");
  await input.fill("Wind down"); await input.press("Enter");
  const d = page.getByRole("dialog", { name: "Wind down" }), mins = d.getByLabel("Fade duration in minutes");
  await d.getByRole("button", { name: "Start fade" }).waitFor();
  await page.waitForFunction(() => !document.querySelector("[data-wd-start]")?.disabled);
  for (const invalid of ["", "121", "4", "5.5"]) {
    await mins.fill(invalid); await mins.press("Enter");
    assert.equal(await mins.evaluate(el => el.checkValidity()), false);
  }
  assert.deepEqual(writes, []);
  await mins.fill("5"); await mins.press("Enter");
  const cancel = d.getByRole("button", { name: "Cancel fade" });
  await cancel.waitFor();
  assert.ok(await cancel.evaluate(el => el === document.activeElement));
  const active = await api(endpoint);
  assert.equal(active.phase, "fading"); assert.equal(active.initial, 80); assert.equal(active.duration, 300);
  await until(() => d.getByRole("timer").textContent(), time => /^4:5\d$/.test(time));
  await d.screenshot({ path: join(output, "wind-down-desktop.png") });
  await page.keyboard.press("Escape");
  await api("/api/scene", { brightness: 25, params: { color: "#8855ff" } });
  const changed = await until(() => api("/api/state"), s => s.effective.brightness === 25 && s.scene_status.status === "ready");
  assert.equal(changed.wind_down.id, active.id);
  assert.equal(changed.scene_status.desired_revision, before.scene_status.desired_revision + 1);
  await page.locator("#wind-down-open").click();
  await d.getByText("25%", { exact: true }).waitFor();
  await cancel.press("Enter"); await mins.waitFor();
  assert.ok(await mins.evaluate(el => el === document.activeElement));
  const after = await api("/api/state");
  assert.equal(after.wind_down.phase, "idle"); assert.deepEqual(after.scene, changed.scene);
  assert.deepEqual(after.scene_status, changed.scene_status);
  assert.equal(after.fans.curve, before.fans.curve);
  assert.deepEqual(writes.map(([path, body]) => [path, body.action]), [[endpoint, "start"], [endpoint, "cancel"]]);
  assert.equal(writes[1][1].id, active.id);
});

test("Wind down confirms uncertain starts, ignores stale fading events and releases the finished hold by keyboard", async t => {
  const page = await rigPage(t, "lighting", 390, 844);
  const idle = await api(endpoint);
  const state = { ...idle, phase: "idle", id: null, version: idle.version + 10, underlying: 65 };
  const commands = [];
  let getFails = true;
  await page.route("**/api/lighting/wind-down", async r => {
    if (r.request().method() === "GET") return r.fulfill(getFails ? { status: 503, json: { error: "Offline fixture" } } : { json: state });
    const body = r.request().postDataJSON(); commands.push(body);
    if (body.action === "start") {
      Object.assign(state, { phase: "fading", id: "timer-a", version: state.version + 1, remaining: 299, duration: 300, initial: 65, cap: 64.78 });
      return r.abort();  // The server accepted the command, but the browser lost its acknowledgement.
    }
    Object.assign(state, { phase: "idle", id: null, version: state.version + 1 });
    return r.fulfill({ json: state });
  });
  await page.evaluate(async () => (await import("/js/appearance.js")).applyAppearance({ scale: 120 }, { persist: false }));
  const toolbar = page.locator(".stage-tools");
  await toolbar.locator("#stage-theatre").waitFor();
  assert.ok(await toolbar.evaluate(el => {
    const stage = el.closest(".stage").getBoundingClientRect(), buttons = [...el.querySelectorAll("button")];
    return buttons.every(b => { const r = b.getBoundingClientRect(); return r.left >= stage.left && r.right <= stage.right; });
  }));
  await toolbar.screenshot({ path: join(output, "wind-down-stage-narrow.png") });
  await page.locator("#wind-down-open").click();
  const d = page.getByRole("dialog", { name: "Wind down" });
  await d.getByRole("alert").getByText(/Could not check the timer/).waitFor();
  getFails = false; await d.getByRole("button", { name: "Refresh timer status" }).click();
  await d.getByText("65%", { exact: true }).waitFor();
  await d.getByLabel("Fade duration in minutes").fill("5");
  await d.getByRole("button", { name: "Start fade" }).press("Enter");
  await d.getByRole("button", { name: "Cancel fade" }).waitFor();
  await d.getByRole("alert").getByText(/could not be confirmed/).waitFor();
  const fading = { ...state };
  Object.assign(state, { phase: "held", remaining: 0, cap: 0 });
  // Advance the status event directly so a real five-minute wall-clock wait is unnecessary.
  // The production monotonic engine and the physical output at this boundary are covered in Python.
  await page.evaluate(async ([held, late]) => {
    const { S, emit } = await import("/js/core.js");
    for (const wind_down of [held, late]) { S.state.wind_down = wind_down; emit("state"); }
  }, [state, fading]);
  const release = d.getByRole("button", { name: "Release hold" });
  await release.waitFor();
  await d.getByRole("button", { name: "Refresh timer status" }).click();
  await d.getByRole("alert").waitFor({ state: "hidden" });
  assert.ok(await d.evaluate(el => {
    const rect = el.getBoundingClientRect(); return el.scrollWidth <= el.clientWidth && rect.left >= 0 && rect.right <= innerWidth + 1;
  }));
  await d.screenshot({ path: join(output, "wind-down-held-narrow.png") });
  await release.press("Enter");
  await d.getByRole("button", { name: "Start fade" }).waitFor();
  assert.deepEqual(commands.map(c => c.action), ["start", "cancel"]);
  assert.equal(commands[1].id, "timer-a");
});

test("physical power stays editable during the hold and lock or suspend is explained ahead of the timer", async t => {
  const page = await rigPage(t, "lighting");
  // Feed deterministic status events into the ordinary live-stream listeners. Power commands still go to the demo.
  await page.addInitScript(() => {
    window.EventSource = class extends EventTarget {
      static CLOSED = 2;
      readyState = 1;
      constructor() { super(); window.fixtureStream = this; }
      close() { this.readyState = 2; }
    };
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForFunction(() => window.fixtureStream?.readyState === 1);
  const real = await api("/api/state");
  const timer = { ...real.wind_down, phase: "held", id: "finished-timer", version: real.wind_down.version + 1,
    underlying: real.scene.brightness, cap: 0, duration: 300, remaining: 0, initial: real.scene.brightness };
  const overlay = (state, locked = false, power = false) => ({ ...state, wind_down: timer,
    effective: { ...state.effective, on: locked || power ? false : state.effective.on, brightness: 0 },
    override: { ...state.override, locked, power } });
  const push = (state) => page.evaluate(info => window.fixtureStream.dispatchEvent(new MessageEvent("state", { data: JSON.stringify(info) })), state);
  await page.route("**/api/state", async r => {
    const raw = await r.fetch(); await r.fulfill({ json: overlay(await raw.json()) });
  });
  await page.route("**/api/lighting/wind-down", r => r.fulfill({ json: timer }));
  await push(overlay(real));
  const why = page.locator("#why"), power = page.locator("#power");
  await why.getByRole("button", { name: "Manage timer" }).click();
  await page.getByRole("dialog", { name: "Wind down" }).getByRole("button", { name: "Release hold" }).waitFor();
  await page.keyboard.press("Escape");
  await push(overlay(real, true));
  await why.getByText(/off while the PC is locked/).waitFor();
  await push(overlay(real, true, true));
  await why.getByText(/Windows power/).waitFor();
  await push(overlay(real));
  await why.getByRole("button", { name: "Manage timer" }).waitFor();
  await power.click();
  await until(() => api("/api/state"), s => !s.scene.on);
  assert.equal(await page.locator("dialog[open]").count(), 0);
  await push(overlay(await api("/api/state")));
  assert.match(await power.getAttribute("title"), /Turn the lights on.*hold the output/);
  await power.click();
  await until(() => api("/api/state"), s => s.scene.on);
  await push(overlay(await api("/api/state")));
  await why.getByRole("button", { name: "Manage timer" }).waitFor();
});

test("a lost cancellation is reconciled in a reopened Wind down sheet without a duplicate command or stale read", async t => {
  const page = await rigPage(t, "lighting");
  const live = await api(endpoint);
  let state = { ...live, version: live.version + 20, phase: "fading", id: "interrupted", underlying: 80,
    remaining: 180, duration: 300, initial: 80, cap: 48 };
  let cancelReached, finishCancel, readReached, finishOldRead, readReturned;
  const pendingCancel = new Promise(resolve => { finishCancel = resolve; }), cancelled = new Promise(resolve => { cancelReached = resolve; });
  const pendingRead = new Promise(resolve => { finishOldRead = resolve; }), reopening = new Promise(resolve => { readReached = resolve; });
  const returned = new Promise(resolve => { readReturned = resolve; });
  const posts = [];
  let reads = 0;
  await page.route("**/api/lighting/wind-down", async route => {
    if (route.request().method() === "GET") {
      const captured = { ...state };
      const old = ++reads === 2;
      if (old) { readReached(); await pendingRead; }
      await route.fulfill({ json: captured });
      if (old) readReturned();
      return;
    }
    const body = route.request().postDataJSON(); posts.push(body);
    if (body.action === "cancel") {
      cancelReached(); await pendingCancel;
      state = { phase: "idle", id: null, session: state.session, version: state.version + 1, underlying: 80 };
      return route.abort(); // The old sheet never receives the successful acknowledgement.
    }
    state = { ...state, version: state.version + 1, phase: "fading", id: "new-timer", remaining: 300, duration: 300, initial: 80, cap: 80 };
    return route.fulfill({ json: state });
  });
  t.after(() => { finishCancel(); finishOldRead(); });
  const d = page.getByRole("dialog", { name: "Wind down" });
  await page.locator("#wind-down-open").click();
  await d.getByRole("button", { name: "Cancel fade" }).click(); await cancelled;
  await page.keyboard.press("Escape");
  await page.locator("a[data-tab=fans]").click();
  await page.keyboard.press("Control+k");
  const input = page.getByRole("dialog", { name: "Quick switch" }).getByRole("combobox");
  await input.fill("Wind down"); await input.press("Enter");
  await reopening; finishCancel();
  const minutes = d.getByLabel("Fade duration in minutes");
  await minutes.waitFor({ timeout: 3500 });  // A read that starts after the uncertain write owns the confirmation.
  await minutes.fill("5"); await minutes.press("Enter");
  await d.getByRole("button", { name: "Cancel fade" }).waitFor();
  finishOldRead(); await returned;
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  assert.ok(parseFloat(await d.locator("[data-wd-cap]").textContent()) > 78, "a late 48% response belongs to the previous timer");
  await page.keyboard.press("Escape");
  assert.equal(await page.locator("a[data-tab=fans]").getAttribute("aria-current"), "page");
  assert.deepEqual(posts.map(p => p.action), ["cancel", "start"]);
  assert.equal(posts[0].id, "interrupted"); assert.equal(posts[1].version, live.version + 21);
});

test("Wind down distinguishes disconnected lights from its software ceiling and keeps time without temperature readings", async t => {
  const page = await rigPage(t, "lighting"), live = await api(endpoint), posts = [];
  await page.waitForFunction(async () => Boolean((await import("/js/core.js")).S.zones?.zones?.length));
  const timer = { ...live, version: live.version + 30, phase: "fading", id: "offline-test", underlying: 80,
    remaining: 225, duration: 300, initial: 80, cap: 60 };
  await page.route("**/api/lighting/wind-down", r => { if (r.request().method() !== "GET") posts.push(r.request().method()); return r.fulfill({ json: timer }); });
  await page.evaluate(async () => {
    const { S, emit } = await import("/js/core.js");
    window.fixtureZones = S.zones;
    S.zones = { ...S.zones, zones: S.zones.zones.map(z => ({ ...z, device: { ...z.device, online: false } })) };
    S.state.sensors = { online: false, cpu_temp: null, gpu_temp: null };
    emit("zones");
  });
  await page.locator("#wind-down-open").click();
  const d = page.getByRole("dialog", { name: "Wind down" });
  await d.getByText(/No lighting zones are active/).waitFor();
  await d.getByRole("button", { name: "Cancel fade" }).waitFor();
  assert.equal(await d.locator("[data-wd-under]").textContent(), "80%");
  await until(async () => parseFloat(await d.locator("[data-wd-cap]").textContent()), n => n < 59.9, 3500);
  assert.match(await d.locator("[data-wd-detail]").textContent(), /Fan curves and temperature safeguards run independently/);
  await page.evaluate(async () => { const { S, emit } = await import("/js/core.js"); S.zones = window.fixtureZones; emit("zones"); });
  await d.getByText(/No lighting zones are active/).waitFor({ state: "hidden" });
  assert.deepEqual(posts, []);
});
