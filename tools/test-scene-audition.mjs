// Entirely routed browser fixture: no app process, helper, capture or hardware endpoints.
import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { join, isAbsolute } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
let browser;
before(async () => {
  const custom = process.env.AIRGLOW_PLAYWRIGHT_MODULE;
  let pw;
  try { pw = await import(custom ? (isAbsolute(custom) ? pathToFileURL(custom).href : custom) : "playwright"); }
  catch (error) { if (custom || error.code !== "ERR_MODULE_NOT_FOUND") throw error; pw = await import(pathToFileURL(join(process.env.APPDATA, "npm/node_modules/playwright/index.mjs")).href); }
  browser = await pw.chromium.launch({ channel: process.env.AIRGLOW_BROWSER || "msedge" });
});
after(async () => browser?.close());
async function fixture(t, { failLoad = false, holdFirstFrame = false } = {}) {
  const page = await browser.newPage({ reducedMotion: "reduce" });
  const calls = [], errors = [], sessions = new Map(); let seq = 0;
  let releaseFrame;
  const heldFrame = new Promise(resolve => { releaseFrame = resolve; });
  t.after(releaseFrame);
  page.on("pageerror", e => errors.push(e.message));
  t.after(async () => { await page.close(); assert.deepEqual(errors, []); });
  const web = new URL("../web/", import.meta.url);
  await page.route("**/*", async route => {
    const url = new URL(route.request().url());
    if (url.origin !== "https://airglow.test") return route.abort();
    if (url.pathname.startsWith("/api/")) {
      const body = route.request().method() === "POST" ? route.request().postDataJSON() : {};
      calls.push({ path: url.pathname, body });
      if (url.pathname.endsWith("preview/load")) {
        if (failLoad) { failLoad = false; return route.fulfill({ status: 500, json: { error: "Renderer unavailable" } }); }
        const id = `p${++seq}`; sessions.set(id, body); return route.fulfill({ json: { id } });
      }
      if (url.pathname.endsWith("preview/tick")) {
        const session = sessions.get(body.id);
        if (holdFirstFrame) { holdFirstFrame = false; await heldFrame; }
        const rgb = session?.effect.id === "off" ? [0, 0, 0] : [255, 0, 0];
        return route.fulfill({ json: { colors: Buffer.from(session.points.flatMap(() => rgb)).toString("base64") } });
      }
      if (url.pathname.endsWith("preview/stop")) { sessions.delete(body.id); return route.fulfill({ json: { ok: true } }); }
      if (url.pathname === "/api/studio/effects") return route.fulfill({ json: { effects: [
        { fx_id: "fx:me.fixture", kind: "code", code: "airglow.effect({point(){return [1,0,0]}})" },
      ] } });
      if (url.pathname === "/api/scene") return route.fulfill({ json: { scene: { effect: body.effect, params: body.params, on: true }, scene_status: { desired_revision: 1, active_revision: 1, status: "ready", undo_available: true } } });
      if (url.pathname === "/api/scene/undo") return route.fulfill({ json: { scene: { effect: "off", params: {} }, scene_status: { desired_revision: 2, active_revision: 2, status: "ready", undo_available: false } } });
      return route.abort();
    }
    if (url.pathname === "/js/main.js") return route.fulfill({ contentType: "text/javascript", body: "" });
    const path = new URL(url.pathname.slice(1) || "index.html", web);
    if (!path.href.startsWith(web.href)) return route.abort();
    return route.fulfill({ path: fileURLToPath(path) });
  });
  await page.goto("https://airglow.test/#lighting");
  await page.evaluate(async () => {
    const { S } = await import("/js/core.js");
    S.state = { scene: { effect: "off", params: {}, brightness: 100, on: true } };
    S.effects = [{ id: "solid", name: "Solid", description: "One colour", params: [{ id: "color", default: "#ff0000" }], needs: [] }, { id: "off", name: "Off", params: [], needs: [] }];
    S.zones = { zones: [] };
    document.querySelector('[data-view="lighting"]').hidden = false;
  });
  return { page, calls, sessions, releaseFrame };
}
test("audition uses sample rig and isolated renderer; only Apply changes lights and Undo restores them", async t => {
  const { page, calls, sessions } = await fixture(t);
  await page.evaluate(async () => (await import("/js/scene-audition.js")).openSceneAudition("solid"));
  await page.getByText("Preview ready", { exact: true }).waitFor();
  assert.match(await page.locator("#sheet").innerText(), /Sample rig/);
  assert.equal(calls.filter(c => c.path === "/api/scene").length, 0);
  assert.equal(calls.filter(c => c.path.endsWith("preview/tick")).length, 1, "Reduced motion renders one still");
  await page.getByRole("button", { name: "Apply lighting", exact: true }).click();
  await page.getByRole("button", { name: "Undo lighting", exact: true }).waitFor();
  assert.equal(calls.filter(c => c.path === "/api/scene").length, 1);
  assert.equal(calls.find(c => c.path === "/api/scene").body.expected_effect, "off", "Guard the scene being replaced, not the new effect");
  await page.getByRole("button", { name: "Undo lighting", exact: true }).click();
  await page.getByText("Previous lighting restored", { exact: true }).waitFor();
  await page.keyboard.press("Escape");
  await page.waitForFunction(() => !document.querySelector("#sheet").open);
  await assertEventually(() => assert.equal(sessions.size, 0));
});
test("deck stills come from rendered frames and reuse a bounded cache without hardware writes", async t => {
  const { page, calls } = await fixture(t);
  await page.evaluate(async () => {
    const deck = document.querySelector("#deck");
    deck.innerHTML = '<canvas width="120" height="60" data-preview-effect="solid"></canvas><canvas width="120" height="60" data-preview-effect="off"></canvas>';
    (await import("/js/scene-audition.js")).mountScenePreviews(deck);
  });
  await page.waitForFunction(() => [...document.querySelectorAll("#deck canvas")].every(c => c.dataset.previewReady === "true"));
  const loads = calls.filter(c => c.path.endsWith("preview/load")).length;
  await page.evaluate(async () => (await import("/js/scene-audition.js")).mountScenePreviews(document.querySelector("#deck")));
  assert.equal(calls.filter(c => c.path.endsWith("preview/load")).length, loads);
  assert.equal(calls.some(c => c.path === "/api/scene"), false);
});
test("preview failure is visible and retry recovers; navigation releases its lease", async t => {
  const { page, calls, sessions } = await fixture(t, { failLoad: true });
  await page.evaluate(async () => (await import("/js/scene-audition.js")).openSceneAudition("solid"));
  await page.getByText("Preview unavailable: Renderer unavailable", { exact: true }).waitFor();
  assert.equal(await page.getByRole("button", { name: "Apply lighting", exact: true }).isDisabled(), true);
  await page.getByRole("button", { name: "Retry preview", exact: true }).click();
  await page.getByText("Preview ready", { exact: true }).waitFor();
  await page.evaluate(() => { location.hash = "#overview"; });
  await page.waitForFunction(() => !document.querySelector("#sheet").open);
  await assertEventually(() => assert.equal(sessions.size, 0));
  assert.equal(calls.some(c => c.path === "/api/scene"), false);
});
test("replacing deck canvases cancels in-flight stills even with identical parameters", async t => {
  const { page, calls, releaseFrame } = await fixture(t, { holdFirstFrame: true });
  const draw = async () => page.evaluate(async () => {
    const root = document.querySelector("#deck");
    root.innerHTML = '<canvas data-preview-effect="solid"></canvas>';
    (await import("/js/scene-audition.js")).mountScenePreviews(root);
  });
  await draw();
  await assertEventually(() => assert.equal(calls.filter(c => c.path.endsWith("preview/tick")).length, 1));
  await draw();
  await assertEventually(() => assert.equal(calls.filter(c => c.path.endsWith("preview/load")).length, 2));
  releaseFrame();
  await page.waitForFunction(() => document.querySelector("#deck canvas").dataset.previewReady === "true");
});
test("audition Undo is bound to its own applied revision after an external scene change", async t => {
  const { page, calls } = await fixture(t);
  await page.evaluate(async () => (await import("/js/scene-audition.js")).openSceneAudition("solid"));
  await page.getByText("Preview ready", { exact: true }).waitFor();
  await page.getByRole("button", { name: "Apply lighting", exact: true }).click();
  await page.getByRole("button", { name: "Undo lighting", exact: true }).waitFor();
  await page.evaluate(async () => { (await import("/js/core.js")).S.state.scene_status.desired_revision = 9; });
  await page.getByRole("button", { name: "Undo lighting", exact: true }).click();
  await assertEventually(() => assert.equal(calls.find(c => c.path === "/api/scene/undo")?.body.expected_revision, 1));
});
test("a recovered thumbnail clears the failure overlay and title", async t => {
  const { page } = await fixture(t, { failLoad: true });
  await page.evaluate(async () => {
    const root = document.querySelector("#deck");
    root.innerHTML = '<canvas data-preview-effect="solid"></canvas>';
    (await import("/js/scene-audition.js")).mountScenePreviews(root);
  });
  const canvas = page.locator("#deck canvas");
  await page.locator("#deck canvas[data-preview-error]").waitFor();
  await page.evaluate(async () => (await import("/js/core.js")).emit("catalog"));
  await page.locator("#deck canvas[data-preview-ready]").waitFor();
  assert.equal(await canvas.getAttribute("data-preview-error"), null);
  assert.equal(await canvas.getAttribute("title"), null);
});
test("a Studio catalog change disposes the open audition before drawing new source", async t => {
  const { page, calls, sessions } = await fixture(t);
  await page.evaluate(async () => {
    const { S } = await import("/js/core.js");
    S.effects.push({ id: "fx:me.fixture", name: "Studio fixture", studio: {}, params: [], needs: [] });
    await (await import("/js/scene-audition.js")).openSceneAudition("fx:me.fixture");
  });
  await page.getByText("Preview ready", { exact: true }).waitFor();
  const first = [...sessions.keys()][0];
  await page.evaluate(async () => (await import("/js/core.js")).emit("catalog"));
  await assertEventually(() => assert.equal(calls.filter(c => c.path.endsWith("preview/load")).length, 2));
  await page.getByText("Preview ready", { exact: true }).waitFor();
  await assertEventually(() => assert.equal(sessions.has(first), false));
  assert.equal(calls.some(c => c.path === "/api/scene"), false);
});
test("an audition uses current brightness and prospective area and zone overrides", async t => {
  const { page, calls } = await fixture(t);
  await page.evaluate(async () => {
    const { S } = await import("/js/core.js"), { sampleZones } = await import("/js/fxrun.js");
    const [front, top, bottom] = sampleZones();
    bottom.config.override = { effect: "breathing", params: { speed: 20 } };
    S.zones = { zones: [front, top, bottom] };
    S.state.scene.brightness = 38;
    S.state.scene.areas = { top: { effect: "off", params: {} } };
    S.effects.push({ id: "breathing", name: "Breathing", params: [], needs: [] });
    await (await import("/js/scene-audition.js")).openSceneAudition("solid");
  });
  await page.getByText("Preview ready", { exact: true }).waitFor();
  const effect = calls.find(c => c.path.endsWith("preview/load")).body.effect;
  assert.equal(effect.kind, "scene");
  assert.equal(effect.brightness, 38);
  assert.deepEqual(effect.zones.map(z => z.id), ["solid", "off", "breathing"]);
  assert.match(await page.locator("#sheet").innerText(), /38% brightness/);
  assert.match(await page.locator("#sheet").innerText(), /area and zone effects/i);
  assert.equal(calls.some(c => c.path === "/api/scene"), false);
});
async function assertEventually(check) {
  for (let n = 0; n < 30; n++) { try { check(); return; } catch (error) { if (n === 29) throw error; await new Promise(resolve => setTimeout(resolve, 30)); } }
}
