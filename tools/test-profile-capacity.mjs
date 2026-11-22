import test from "node:test";
import assert from "node:assert/strict";
import { api, rigPage, until, withPreviewPage } from "./isolated-browser-rig.mjs";

// A private Python PreviewHost and its production HTML worker page exchange the real frame packets.
withPreviewPage();
const sleep = delay => new Promise(resolve => setTimeout(resolve, delay));
const colour = bytes => Buffer.from(bytes, "base64").subarray(0, 3).toString("hex");
const stop = id => api("/api/studio/preview/stop", { id });
const solid = () => api("/api/studio/preview/load", {
  effect: { kind: "builtin", id: "solid", params: { color: "#2468ac" } }, points: [[0, 0]],
});

function expectedRig(colors) {
  const counts = [72, 36, 30, 18, 20, 32];
  return colors.map((color, index) => color.repeat(counts[index])).join("");
}

async function presets() {
  const colors = ["#ff0000", "#00ff00", "#0000ff", "#ff00ff"];
  const ids = [];
  for (const color of colors) {
    const { effect } = await api("/api/studio/save", {
      name: "Fixture studio " + color.slice(1), layers: [{ type: "solid", params: { color } }],
    });
    ids.push(effect.fx_id);
  }
  const order = ["front", "top", "bottom", "rear"], result = [];
  for (const [name, brightness, offset] of [["Warm comparison", 100, 0], ["Cool comparison", 50, 1]]) {
    const areas = Object.fromEntries(order.map((spot, i) => [spot, { effect: ids[(i + offset) % ids.length], params: {} }]));
    const { id } = await api("/api/profiles/import", { profile: {
      airglow_profile: 1, id: "capacity-" + offset, name, areas,
      lighting: { effect: "solid", params: { color: "#285078" }, brightness },
    } });
    result.push(id);
  }
  return result;
}

async function assertFourFree() {
  const ids = [];
  try {
    for (let i = 0; i < 4; i++) ids.push((await solid()).id);
    await assert.rejects(solid(), /Too many screen previews/);
  } finally { await Promise.all(ids.map(stop)); }
}

test("mixed-zone portraits share all four renderer slots, survive rapid replacement, and release pending loads on close", async t => {
  const [first, second] = await presets(), page = await rigPage(t, "overview");
  const pending = new Set(), frames = [], errors = [], writes = [];
  let highWater = 0, loads = 0, stops = 0, tickCount = 0;
  let delayLoad = 80, delayStop = 80, delayFrame = 0, rejectNext = false;
  page.on("request", request => {
    const path = new URL(request.url()).pathname;
    if (request.method() === "POST" && !path.startsWith("/api/studio/preview/")) writes.push(path);
  });
  await page.route("**/api/studio/preview/*", async route => {
    const body = route.request().postDataJSON(), action = route.request().url().split("/").pop();
    if (action === "stop") await sleep(delayStop);
    if (rejectNext && action === "tick") {
      rejectNext = false;
      return route.fulfill({ status: 503, json: { error: "Fixture portrait interrupted" } });
    }
    const response = await route.fetch(), result = await response.json();
    if (!response.ok()) errors.push(result.error);
    if (action === "load" && response.ok()) {
      loads++;
      pending.add(result.id);
      highWater = Math.max(highWater, pending.size);
      await sleep(delayLoad);
    }
    if (action === "tick" && result.colors) {
      const bytes = Buffer.from(result.colors, "base64");
      tickCount++;
      frames.push({ pixels: bytes.toString("hex"), time: body.time, count: bytes.length / 3 });
      if (delayFrame) await sleep(delayFrame);
    }
    if (action === "stop") { stops++; pending.delete(body.id); }
    await route.fulfill({ response });
  });
  const prepare = id => page.evaluate(async id => {
    const { S } = await import("/js/core.js");
    S.zones = { zones: (await import("/js/fxrun.js")).sampleZones() };
    (await import("/js/profile-library.js")).openProfileLibrary(id);
  }, id);
  const settled = async count => {
    await page.waitForFunction(n => {
      const panels = [...document.querySelectorAll(".pl-portrait")];
      return panels.length === n && panels.every(p => ["ready", "error"].includes(p.dataset.frame));
    }, count, { timeout: 22000 });
    return page.locator(".pl-portrait").evaluateAll(figures => figures.map(p => [p.dataset.frame, p.querySelector("output").textContent]));
  };
  const expectReady = async count => assert.deepEqual(await settled(count), Array.from({ length: count }, () => ["ready", ""]));
  const drained = () => until(() => pending.size, n => n === 0);
  const retry = () => page.locator("[data-pl-retry]").click();
  // Each composed portrait needs four separate Studio groups, plus built-in zones, on the same rig.
  await prepare(first);
  await expectReady(1);
  await drained();
  await page.locator("#pl-compare").selectOption(second);
  await expectReady(2);
  const warm = expectedRig(["ff0000", "00ff00", "0000ff", "ff00ff", "285078", "285078"]);
  const cool = expectedRig(["008000", "000080", "800080", "800000", "14283c", "14283c"]);
  assert.deepEqual(frames.slice(-2).map(f => f.pixels), [warm, cool]);
  assert.ok(frames.every(f => f.time === 2 && f.count === 208));
  await drained();
  // Delay both directions around a real server allocation. Re-selecting during load or stop must not start a competing lease.
  delayLoad = 180;
  delayStop = 160;
  const before = loads;
  await retry();
  await until(() => loads, n => n > before);
  for (let i = 0; i < 36; i++) await page.locator('[data-pl-id="' + (i % 2 ? first : second) + '"]').click();
  await page.keyboard.press("Escape");
  await prepare(second);
  await expectReady(1);
  assert.equal(await page.locator(".pl-title h3").textContent(), "Cool comparison");
  await drained();
  rejectNext = true;
  await retry();
  assert.deepEqual(await settled(1), [["error", "Fixture portrait interrupted"]]);
  await retry();
  await expectReady(1);
  delayFrame = 550;
  const oldFrames = tickCount;
  await retry();
  await until(() => tickCount, n => n > oldFrames);
  await page.keyboard.press("Escape");
  await prepare(first);
  await expectReady(1);
  assert.equal(await page.locator(".pl-title h3").textContent(), "Warm comparison");
  assert.equal(frames.at(-1).pixels, warm);
  delayFrame = 0;
  const oldLoads = loads;
  await retry();
  await until(() => loads, n => n > oldLoads);
  await page.keyboard.press("Escape");
  await drained();
  await sleep(200);
  assert.equal(highWater, 1);
  assert.deepEqual(errors, []);
  assert.equal(loads, stops);
  assert.deepEqual(writes, []);
  await assertFourFree();
  t.diagnostic(`${loads} real leases, ${stops} acknowledged stops, ${tickCount} frames. Peak simultaneous portrait leases: ${highWater}.`);
});

test("occupied external preview slots stay untouched and a refresh succeeds after one is freed", async t => {
  const ids = [], page = await rigPage(t, "overview");
  try {
    for (let i = 0; i < 4; i++) ids.push((await solid()).id);
    const builtin = (await api("/api/presets")).find(p => !p.effect.startsWith("fx:") && !p.areas);
    await page.evaluate(async id => (await import("/js/profile-library.js")).openProfileLibrary(id), builtin.id);
    await page.locator('.pl-portrait[data-frame="error"]').waitFor();
    assert.match(await page.locator(".pl-portrait output").textContent(), /Too many screen previews/);
    await stop(ids.pop());
    await page.locator("[data-pl-retry]").click();
    await page.locator('.pl-portrait[data-frame="ready"]').waitFor();
    await page.keyboard.press("Escape");
    for (const id of ids) {
      const frame = await api("/api/studio/preview/tick", { id, time: 2 });
      assert.equal(colour(frame.colors), "2468ac");
    }
  } finally { await Promise.all(ids.map(stop)); }
  await assertFourFree();
});
