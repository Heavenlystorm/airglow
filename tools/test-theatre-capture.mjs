import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";
import { rigPage, root } from "./isolated-browser-rig.mjs";

const out = process.env.AIRGLOW_CAPTURE_OUTPUT || join(root, "output/experience-upgrade/theatre-capture");
mkdirSync(out, { recursive: true });

async function theatre(t) {
  const page = await rigPage(t, "lighting", 1280, 840), writes = [];
  await page.route("**/api/stream?*", () => {}); // Supply known optical frames to the fixture's actual zone geometry.
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.locator("#stage-view-2d").waitFor();
  page.on("request", r => {
    const path = new URL(r.url()).pathname;
    if (r.method() !== "GET" && !path.startsWith("/api/studio/preview/")) writes.push(path);
  });
  t.after(() => assert.deepEqual(writes, [], "Capture never changes the scene, zones or hardware"));
  await page.evaluate(async () => {
    const { S } = await import("/js/core.js");
    S.state.effective = { on: true, brightness: 72, effect: "gradient" };
    (await import("/js/design.js")).applyDesign("firefly", { persist: false });
  });
  return page;
}
async function frame(page, colors) {
  await page.evaluate(async rgb => {
    const { S, emit } = await import("/js/core.js");
    emit("frames:live", { zones: Object.fromEntries(S.zones.zones.map(z =>
      [z.key, z.positions.map((_p, i) => rgb[i % rgb.length]).join("")])) });
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  }, colors);
}
async function pixels(page, base64) {
  return page.evaluate(async data => {
    const bytes = Uint8Array.from(atob(data), c => c.charCodeAt(0));
    const bmp = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
    const canvas = document.createElement("canvas"); canvas.width = bmp.width; canvas.height = bmp.height;
    const ctx = canvas.getContext("2d", { willReadFrequently: true }); ctx.drawImage(bmp, 0, 0); bmp.close();
    const rgba = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
    let red = 0, blue = 0, transparent = 0;
    for (let i = 0; i < rgba.length; i += 4) {
      if (rgba[i + 3] === 0) transparent++;
      if (rgba[i] === 255 && rgba[i + 1] === 0 && rgba[i + 2] === 0 && rgba[i + 3] === 255) red++;
      if (rgba[i] === 0 && rgba[i + 1] === 0 && rgba[i + 2] === 255 && rgba[i + 3] === 255) blue++;
    }
    return { w: canvas.width, h: canvas.height, corner: [...rgba.slice(0, 4)], red, blue, transparent };
  }, base64);
}

test("Capture exports exact held LED colours, correct PNG size and transparent pixels from the real theatre renderer", async t => {
  const page = await theatre(t);
  const theme = ["ffa968", "ff9100", "f99bd4", "bd9cfe", "a8e6c3", "80c5d2"];
  await frame(page, theme);
  await page.getByRole("button", { name: "Open Light theatre" }).click();
  const live = page.getByRole("dialog", { name: "Light theatre" });
  await live.getByText("Frame held", { exact: true }).waitFor();
  await live.getByRole("button", { name: "Resume live" }).click();
  await frame(page, theme);
  await live.getByRole("button", { name: "Capture frame" }).click();
  const capture = page.getByRole("dialog", { name: "Frame capture" });
  await capture.getByText("1920 × 1080 px", { exact: true }).waitFor();
  await page.screenshot({ path: join(out, "capture-desktop.png") });
  await page.evaluate(() => { Object.defineProperty(window, "showSaveFilePicker", { value: undefined, configurable: true }); });
  const landscape = page.waitForEvent("download");
  await capture.getByRole("button", { name: "Save PNG…" }).click();
  const full = await landscape; await full.saveAs(join(out, "firefly-rig-landscape.png"));
  const wide = readFileSync(await full.path());
  assert.equal(wide.readUInt32BE(16), 1920); assert.equal(wide.readUInt32BE(20), 1080);
  await page.setViewportSize({ width: 390, height: 844 });
  await capture.getByLabel("Framing", { exact: true }).selectOption("portrait");
  await capture.getByLabel("Backdrop", { exact: true }).selectOption("transparent");
  await page.screenshot({ path: join(out, "capture-portrait-narrow.png") });
  assert.ok(await capture.evaluate(el => el.scrollWidth <= el.clientWidth + 1));
  await capture.getByRole("button", { name: "Back to theatre" }).click();
  await frame(page, ["ff0000"]);
  await new Promise(resolve => setTimeout(resolve, 275));
  await frame(page, ["ff0000"]);
  await new Promise(resolve => setTimeout(resolve, 275));
  await frame(page, ["0000ff"]);
  await live.getByRole("button", { name: "Capture frame" }).click();
  const recent = capture.getByLabel("Recent frames");
  await recent.focus(); await recent.press("End"); await recent.press("ArrowLeft");
  await capture.getByLabel("Framing", { exact: true }).selectOption("square");
  await capture.getByLabel("Camera", { exact: true }).selectOption("front");
  await capture.getByLabel("Backdrop", { exact: true }).selectOption("transparent");
  const downloaded = page.waitForEvent("download");
  await capture.getByRole("button", { name: "Save PNG…" }).click();
  const file = await downloaded; assert.match(file.suggestedFilename(), /^airglow-gradient-.+\.png$/);
  await file.saveAs(join(out, "captured-square.png"));
  const png = readFileSync(await file.path());
  assert.deepEqual([...png.subarray(0, 8)], [137,80,78,71,13,10,26,10]);
  assert.equal(png.readUInt32BE(16), 1080); assert.equal(png.readUInt32BE(20), 1080);
  const sample = await pixels(page, png.toString("base64"));
  assert.ok(sample.red > 60 && sample.blue === 0, JSON.stringify(sample));
  assert.equal(sample.corner[3], 0); assert.ok(sample.transparent > 50000);
  await page.evaluate(async () => {
    const { S, emit } = await import("/js/core.js"); S.state.effective.on = false; emit("state");
  });
  await capture.getByText(/Lights off.*keeps the captured frame/).waitFor();
  await page.keyboard.press("Escape");
  await live.getByText("Lights off", { exact: true }).waitFor();
  assert.ok(await live.getByRole("button", { name: "Capture frame" }).isDisabled());
  await page.evaluate(async () => { const { S, emit } = await import("/js/core.js"); S.state.effective.on = true; emit("state"); });
  await live.getByText("Waiting for frames", { exact: true }).waitFor();
  assert.ok(await live.getByRole("button", { name: "Capture frame" }).isDisabled());
  await page.keyboard.press("Escape");
  assert.ok(await page.getByRole("button", { name: "Open Light theatre" }).evaluate(el => el === document.activeElement));
});

test("Capture handles a cancelled Save As, writes a full opaque portrait and keeps its preview while the picker is pending", async t => {
  const page = await theatre(t);
  await frame(page, ["0000ff"]);
  await page.getByRole("button", { name: "Open Light theatre" }).click();
  await page.getByRole("button", { name: "Capture frame" }).click();
  const capture = page.getByRole("dialog", { name: "Frame capture" });
  await capture.getByLabel("Framing", { exact: true }).selectOption("portrait");
  await page.evaluate(() => {
    window.captureChoices = [];
    window.showSaveFilePicker = async options => {
      window.captureChoices.push(options);
      if (window.captureChoices.length === 1) throw new DOMException("cancelled", "AbortError");
      await new Promise(resolve => { window.releaseCapturePicker = resolve; });
      return { createWritable: async () => ({ write: async blob => { window.capturedPNG = await blob.arrayBuffer(); }, close: async () => {} }) };
    };
  });
  const save = capture.getByRole("button", { name: "Save PNG…" });
  await save.click(); await capture.getByText("Save cancelled.").waitFor();
  await save.click(); await page.waitForFunction(() => !!window.releaseCapturePicker);
  await page.keyboard.press("Escape"); assert.ok(await capture.isVisible());
  await frame(page, ["ff0000"]);
  await page.evaluate(() => window.releaseCapturePicker());
  await capture.getByText("PNG saved.", { exact: true }).waitFor();
  const encoded = await page.evaluate(async () => {
    const reader = new FileReader();
    const done = new Promise(resolve => { reader.onload = () => resolve(reader.result.split(",")[1]); });
    reader.readAsDataURL(new Blob([window.capturedPNG])); return await done;
  });
  const sample = await pixels(page, encoded);
  assert.equal(sample.w, 1080); assert.equal(sample.h, 1920);
  assert.equal(sample.transparent, 0); assert.ok(sample.blue > 60 && sample.red === 0);
  await page.keyboard.press("Escape"); await page.keyboard.press("Escape");
});

test("Capture requires complete output and keeps an offline, disabled or empty focus clear", async t => {
  const page = await theatre(t);
  const first = await page.evaluate(async () => {
    const { S, emit } = await import("/js/core.js");
    const z = S.zones.zones.find(z => z.device.online && z.config.enabled && z.positions.length);
    emit("frames:live", { zones: { [z.key]: "00ffaa".repeat(z.positions.length) } }); return z.key;
  });
  await page.getByRole("button", { name: "Open Light theatre" }).click();
  const live = page.getByRole("dialog", { name: "Light theatre" }), capture = live.getByRole("button", { name: "Capture frame" });
  await live.getByText("Partial frame held", { exact: true }).waitFor();
  assert.ok(await capture.isDisabled());
  await live.getByLabel("Focus a lighting zone").selectOption(first);
  await live.getByText("Frame held", { exact: true }).waitFor();
  assert.ok(await capture.isEnabled());
  await page.evaluate(async first => {
    const { S, emit } = await import("/js/core.js"); S.zones.zones.find(z => z.key === first).config.enabled = false; emit("zones");
  }, first);
  await live.getByText("Zones disabled", { exact: true }).waitFor(); assert.ok(await capture.isDisabled());
  await page.evaluate(async first => {
    const { S, emit } = await import("/js/core.js"); S.zones.zones.find(z => z.key === first).device.online = false; emit("zones");
  }, first);
  await live.getByText("Devices offline", { exact: true }).waitFor(); assert.ok(await capture.isDisabled());
  await page.evaluate(async () => { const { S, emit } = await import("/js/core.js"); S.zones.zones = []; emit("zones"); });
  await live.getByText("No configured LEDs", { exact: true }).waitFor(); assert.ok(await capture.isDisabled());
});

test("an older sheet's queued close cannot dispose a newly opened theatre", async t => {
  const page = await theatre(t);
  const result = await page.evaluate(async () => {
    const { openSheet, closeSheet } = await import("/js/sheet.js");
    const { openTheatre } = await import("/js/light-theatre.js");
    openSheet("<h2>Previous sheet</h2>"); closeSheet();
    let disposed = false;
    openTheatre(null, () => { disposed = true; });
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    return { open: document.querySelector("#sheet").open, theatre: !!document.querySelector(".light-theatre"), disposed };
  });
  assert.deepEqual(result, { open:true, theatre:true, disposed:false });
});

test("stage controls stay visible beside the rig caption when Wind down has a longer status", async t => {
  const page = await theatre(t);
  const issues = [];
  for (const [width, scale] of [[320,120],[390,100],[430,100],[540,120],[800,100],[1280,100]]) {
    await page.setViewportSize({width,height:800});
    const detail = await page.evaluate(async scale => {
      (await import("/js/appearance.js")).applyAppearance({scale},{persist:false});
      const stage = document.querySelector("#stage"), wind = stage.querySelector("#wind-down-open span");
      wind.textContent = "Wind down · holding";
      const buttons = [...stage.querySelectorAll(".stage-tools button")].map(el=>el.getBoundingClientRect());
      const bounds = stage.getBoundingClientRect(), caption = stage.querySelector(".stage-identity");
      const captionBounds = caption.getBoundingClientRect();
      return {
        stage: [bounds.left,bounds.right], controls: buttons.map(b=>[b.left,b.right]),
        caption: [captionBounds.left,captionBounds.right],
        overflow: buttons.some(b => b.left < bounds.left || b.right > bounds.right),
        collision: getComputedStyle(caption).visibility !== "hidden" && captionBounds.width > 2 &&
          buttons.some(b => b.right > captionBounds.left && b.left < captionBounds.right && b.bottom > captionBounds.top),
      };
    },scale);
    if (detail.overflow || detail.collision) issues.push({width,scale,...detail});
  }
  assert.deepEqual(issues, []);
});
