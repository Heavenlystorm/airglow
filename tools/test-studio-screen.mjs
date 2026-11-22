import test from "node:test";
import assert from "node:assert/strict";
import { rigPage, until } from "./isolated-browser-rig.mjs";

const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
const press = async button => { await button.focus(); await button.press("Enter"); };

async function visibility(page, hidden) {
  await page.evaluate(value => {
    if (value) Object.defineProperties(document, {
      hidden: { configurable: true, value: true },
      visibilityState: { configurable: true, value: "hidden" },
    });
    else { delete document.hidden; delete document.visibilityState; }
    document.dispatchEvent(new Event("visibilitychange"));
  }, hidden);
}

test("Studio starts with a still under reduced motion, updates paused edits, and resumes only the chosen playback", async t => {
  const page = await rigPage(t, "studio", 320, 568);
  const leases = new Map(), writes = [];
  await page.route("**/api/studio/preview/*", async route => {
    const action = route.request().url().split("/").pop();
    const data = route.request().postDataJSON();
    if (action === "load" && data.effect?.layers?.layers?.[0]?.params?.color === "#0b2a4a") {
      const id = "screen-test-" + leases.size;
      leases.set(id, { ticks: [], stop: 0, colors: Buffer.alloc(3 * data.points.length, 156).toString("base64") });
      return route.fulfill({ json: { id } });
    }
    const lease = leases.get(data.id);
    if (!lease) return route.continue();
    if (action === "stop") { lease.stop++; return route.fulfill({ json: {} }); }
    if (action === "tick") {
      lease.ticks.push(data);
      return route.fulfill({ json: { colors: lease.colors, ms: 1 } });
    }
    await route.continue();
  });
  page.on("request", req => {
    if (req.method() === "POST" && !req.url().includes("/api/studio/preview/")) writes.push(new URL(req.url()).pathname);
  });
  await page.evaluate(async () => (await import("/js/appearance.js")).applyAppearance({ scale: 120 }, { persist: false }));
  await press(page.locator('[data-st="new"]'));
  const play = page.locator('[data-dbg="pause"]'), beat = page.locator("#st-beat");
  const first = await until(() => [...leases.values()].at(-1), lease => lease?.ticks.length === 1);
  assert.equal(first.ticks[0].time, 0);
  assert.equal(first.ticks[0].dt, 0);
  await until(() => page.locator("#st-meter").textContent(), s => s.includes("Frame held") && s.includes("LEDs"));
  assert.equal(await page.evaluate(() => document.activeElement.id), "st-name");
  assert.equal(await play.textContent(), "Play");
  await wait(250);
  assert.equal(first.ticks.length, 1);
  await beat.focus(); await beat.press("Space");
  await until(() => first.ticks.length, n => n === 2);
  assert.equal(first.ticks[1].time, 0);
  assert.ok(first.ticks[1].audio);
  const canvas = page.locator("#st-canvas");
  const initial = await canvas.evaluate(c => c.toDataURL());
  await press(page.locator('[data-dbg="overlay"]'));
  await until(() => canvas.evaluate(c => c.toDataURL()), value => value !== initial);
  assert.equal(first.ticks.length, 2);
  await press(play);
  await until(() => first.ticks.filter(frame => frame.dt > 0).length, n => n >= 3);
  await visibility(page, true);
  await until(() => first.stop, n => n === 1);
  const hiddenCount = first.ticks.length;
  await wait(300);
  assert.equal(first.ticks.length, hiddenCount);
  await visibility(page, false);
  const resumed = await until(() => [...leases.values()].at(-1), s => s !== first && s?.ticks.length >= 3);
  assert.ok(resumed.ticks[0].dt > 0);
  await press(play);
  await visibility(page, true);
  await until(() => resumed.stop, n => n === 1);
  await visibility(page, false);
  const still = await until(() => [...leases.values()].at(-1), s => s !== resumed && s?.ticks.length === 1);
  assert.equal(still.ticks[0].dt, 0);
  await wait(220);
  assert.equal(still.ticks.length, 1);
  await press(page.getByRole("link", { name: "Back to the Studio" }));
  await until(() => still.stop, n => n === 1);
  await page.locator(".st-home").waitFor();
  assert.equal(await page.evaluate(() => document.activeElement.dataset.st), "new");
  assert.deepEqual(writes, []);
});

test("leaving the code editor releases its widget and restores the template button", async t => {
  const page = await rigPage(t, "studio");
  const cdp = await page.context().newCDPSession(page);
  const template = page.locator('[data-code-tpl="canvas"]');
  await press(template);
  await page.locator(".CodeMirror").waitFor();
  await page.evaluate(() => { window.codeWidget = new WeakRef(document.querySelector(".CodeMirror")); });
  await press(page.getByRole("link", { name: "Back to the Studio" }));
  await template.waitFor();
  assert.equal(await page.evaluate(() => document.activeElement.dataset.codeTpl), "canvas");
  // Playwright retains the most recently actioned element for its trace. Advance past the old Back link.
  await template.press("Tab");
  await until(async () => {
    await cdp.send("HeapProfiler.collectGarbage");
    return page.evaluate(() => !!window.codeWidget.deref());
  }, retained => !retained, 2500);
});

test("Studio stops its on-screen playback when the app or system starts requesting reduced motion", async t => {
  const page = await rigPage(t, "studio");
  await page.emulateMedia({ reducedMotion: "no-preference" });
  const motion = choice => page.evaluate(async value => {
    (await import("/js/appearance.js")).applyAppearance({ motion: value }, {persist:false});
  }, choice);
  await motion("system");
  await press(page.locator('[data-st="new"]'));
  const play = page.locator('[data-dbg="pause"]');
  assert.equal(await play.textContent(), "Pause");
  await motion("reduced");
  await until(() => play.textContent(), s => s === "Play");
  await press(play);
  assert.equal(await play.textContent(), "Pause");
  await motion("system");
  await page.emulateMedia({ reducedMotion: "reduce" });
  await until(() => play.textContent(), s => s === "Play");
});

test("effect and pack import each handle one file after repeatedly revisiting Studio", async t => {
  const page = await rigPage(t, "studio");
  for (let i = 0; i < 4; i++) {
    await page.keyboard.press("Control+2");
    await page.locator('[data-view="lighting"]').waitFor();
    await page.keyboard.press("Control+3");
    await page.locator("#st-import").waitFor({ state: "attached" });
  }
  let effectChecks = 0, packChecks = 0;
  await page.route("**/api/studio/check", route => {
    effectChecks++;
    return route.fulfill({ json: { effect: { name: "Imported light", kind: "layers", layers: [] } } });
  });
  await page.route("**/api/studio/packs/preview", route => {
    packChecks++;
    return route.fulfill({ json: { name: "Imported pack", effects: [], areas: [] } });
  });
  await page.locator("#st-pack-file").setInputFiles({ name: "colours.json", mimeType: "application/json", buffer: Buffer.from("{}") });
  await page.getByRole("heading", { name: "Imported pack" }).waitFor();
  await page.keyboard.press("Escape");
  await page.locator("#st-import").setInputFiles({ name: "colours.html", mimeType: "text/html", buffer: Buffer.from("<title>Imported light</title>") });
  await page.locator("#st-name").waitFor();
  assert.equal(await page.locator("#st-name").inputValue(), "Imported light");
  assert.equal(effectChecks, 1); assert.equal(packChecks, 1);
});
