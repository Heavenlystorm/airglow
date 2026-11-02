// Fixture-only checks for Studio alignment, every catalog entry and truthful tile states.
import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { isAbsolute, join } from "node:path";
import { pathToFileURL } from "node:url";

const BASE = "http://127.0.0.1:8790";
const OUTPUT = process.env.AIRGLOW_TELEMETRY_OUTPUT || "output/telemetry-ui";
mkdirSync(OUTPUT, { recursive: true });
async function api(path, body) {
  const r = await fetch(BASE + path, body === undefined ? {} : { method: "POST",
    headers: { "Content-Type": "application/json", "X-Airglow": "1" }, body: JSON.stringify(body) });
  assert.ok(r.ok, `${path}: ${r.status}`); return r.json();
}
async function playwright() {
  const name = process.env.AIRGLOW_PLAYWRIGHT_MODULE;
  if (name) return import(isAbsolute(name) ? pathToFileURL(name).href : name);
  try { return await import("playwright"); }
  catch (e) {
    if (e.code !== "ERR_MODULE_NOT_FOUND" || !process.env.APPDATA) throw e;
    return import(pathToFileURL(join(process.env.APPDATA, "npm/node_modules/playwright/index.mjs")).href);
  }
}
let browser, original;
before(async () => {
  assert.equal((await api("/api/settings")).settings.lhm_url, "http://127.0.0.1:8791", "Only simulated hardware is allowed");
  original = await api("/api/overview");
  await api("/api/onboarded", {}); await api("/api/settings", { online_checks: false });
  const { chromium } = await playwright();
  browser = await chromium.launch({ channel: process.env.AIRGLOW_BROWSER || "msedge" });
});
after(async () => { await browser?.close(); if (original) await api("/api/overview", original); });
async function pageFor(t, view) {
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 }, reducedMotion: "reduce" });
  await page.addInitScript(() => {
    const Native = window.EventSource;
    window.__testStreams = [];
    window.EventSource = class extends Native { constructor(...args) { super(...args); window.__testStreams.push(this); } };
  });
  const errors = [];
  page.on("pageerror", e => errors.push(e.message));
  t.after(async () => { await page.close(); assert.deepEqual(errors, []); });
  await page.goto(`${BASE}/#${view}`);
  return page;
}
async function frames(page) { await page.evaluate(() => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)))); }

test("Studio shares one left edge at wide and narrow sizes in every theme", async (t) => {
  const page = await pageFor(t, "studio");
  await page.locator(".st-home .st-sec").first().waitFor();
  const themes = await page.evaluate(async () => (await import("/js/design.js")).DESIGNS.map(d => d.id));
  const geometry = [];
  for (const theme of themes) {
    await page.evaluate(async id => (await import("/js/design.js")).applyDesign(id), theme);
    for (const width of [3840, 2560, 1920, 960, 791, 700, 390]) {
      await page.setViewportSize({ width, height: 1080 }); await frames(page);
      const g = await page.evaluate(() => {
        const home = document.querySelector(".st-home"), box = home.getBoundingClientRect();
        const left = box.left + parseFloat(getComputedStyle(home).paddingLeft);
        return { left, width: innerWidth, documentWidth: document.documentElement.scrollWidth,
          overflow: home.scrollWidth - home.clientWidth,
          children: [...home.children].filter(e => e.matches(".page-head,.st-sec,.st-filter,.banner,.st-workflow"))
            .map(e => ({ kind: e.className, x: e.getBoundingClientRect().left })) };
      });
      assert.ok(g.children.length >= 4);
      for (const child of g.children) assert.ok(Math.abs(child.x - g.left) <= 1, `${theme} ${width}: ${JSON.stringify(child)} vs ${g.left}`);
      assert.ok(g.documentWidth <= width && g.overflow <= 1, `${theme} ${width}: overflow`);
      geometry.push({ theme, ...g });
      if ((theme === "amber" && [1920, 390].includes(width)) || (theme === "graphite" && width === 2560))
        await page.screenshot({ path: join(OUTPUT, `studio-${theme}-${width}.png`) });
    }
  }
  writeFileSync(join(OUTPUT, "studio-geometry.json"), JSON.stringify(geometry, null, 2));
});

test("every addable sensor renders in all four tile styles without invalid values", async (t) => {
  const page = await pageFor(t, "overview");
  await page.locator("#ov-grid .tile").first().waitFor();
  const captured = process.env.AIRGLOW_CATALOG_CAPTURE;
  const catalog = captured ? JSON.parse(readFileSync(captured, "utf8")).sensors : (await api("/api/sensors")).sensors;
  const styles = ["spark", "ring", "bar", "graph"];
  const inventory = [];
  // Limit each layout to 24 tiles. This covers actual raw sensors as well as quick picks and app readings.
  for (let start = 0; start < catalog.length; start += 6) {
    const sensors = catalog.slice(start, start + 6);
    const tiles = sensors.flatMap((m, i) => styles.map(style => ({ id: `audit-${i}-${style}`, type: "sensor",
      sensors: [m.id], style, w: style === "graph" ? 2 : 1, h: 1 })));
    await api("/api/overview", { tiles, pins: [] }); await page.reload();
    await page.waitForFunction(n => document.querySelectorAll("#ov-grid .tile-sensor").length === n, tiles.length);
    await frames(page);
    if (captured) await page.evaluate(async entries => {
      const { S } = await import("/js/core.js"), { R, pushReadings } = await import("/js/readings.js");
      const { renderOverview } = await import("/js/overview.js");
      // Replay a read-only hardware capture into the isolated UI. No requests go to real controllers.
      for (const m of entries) R.meta[m.id] = m;
      S.state.readings = Object.fromEntries(entries.map(m => [m.id, m.value]));
      pushReadings(S.state.readings); renderOverview();
    }, sensors);
    const rendered = await page.evaluate(() => [...document.querySelectorAll("#ov-grid .tile-sensor")].map(el => ({
      title: el.querySelector(".tile-name").textContent, text: el.innerText,
      paths: [...el.querySelectorAll("path")].map(p => p.style.d),
    })));
    assert.equal(rendered.length, tiles.length);
    for (const tile of rendered) {
      assert.ok(tile.title.trim()); assert.doesNotMatch(tile.text, /NaN|Infinity|undefined/);
      for (const path of tile.paths) assert.doesNotMatch(path, /NaN|Infinity|undefined/);
    }
    inventory.push(...sensors.map(m => ({ id: m.id, name: m.name, unit: m.unit, source: m.source || m.group,
      source_id: m.source_id, source_ids: m.source_ids, value: m.value, styles })));
  }
  assert.ok(inventory.some(m => m.id === "auto/cpu_load"));
  assert.ok(inventory.some(m => m.id === "auto/ssd_temp"));
  writeFileSync(join(OUTPUT, "tile-inventory.json"), JSON.stringify(inventory, null, 2));
  console.log(`Rendered ${catalog.length} catalog entries in ${styles.length} styles (${catalog.length * 4} tiles).`);
});

test("setup activates detected fans and Control center opens the selected channel", async (t) => {
  const page = await pageFor(t, "fans");
  await page.locator("#channels [data-mode]").first().waitFor();
  const state = await api("/api/state");
  const connected = state.fans.channels.filter(c => c.rpm > 0 && !c.pump);
  assert.ok(connected.length);
  for (const c of connected) assert.equal(c.mode, "curve");
  await page.locator('.tabs a[data-tab="overview"]').click();
  await page.locator("[data-control-center]").first().click();
  const id = connected[0].id, href = `#fans/${encodeURIComponent(id)}`;
  await page.locator(`[data-control-link][href="${href}"]`).click();
  await page.waitForFunction(sid => document.activeElement?.closest("[data-chan]")?.dataset.chan === sid, id);
  const select = page.locator(`[data-chan="${id}"] [data-mode]`);
  await Promise.all([
    page.waitForResponse(r => r.url().endsWith("/api/fans/channel") && r.request().method() === "POST"),
    select.selectOption("bios"),
  ]);
  await page.reload(); await select.waitFor();
  assert.equal(await select.inputValue(), "bios");
  await Promise.all([
    page.waitForResponse(r => r.url().endsWith("/api/fans/channel") && r.request().method() === "POST"),
    select.selectOption("curve"),
  ]);
  await page.waitForFunction(sid => document.querySelector(`[data-chan="${sid}"] [data-mode]`)?.value === "curve", id);
});

test("mixed-unit graphs label both scales, missing values stay empty, and disconnects clear current numbers", async (t) => {
  await api("/api/overview", { tiles: [
    { id: "mixed", type: "sensor", sensors: ["auto/cpu_temp", "auto/cpu_load"], style: "graph", w: 2, h: 2 },
    { id: "cpu", type: "sensor", sensors: ["auto/cpu_temp"], style: "spark", w: 1, h: 1 },
    { id: "fans", type: "fans", w: 1, h: 1 }, { id: "control", type: "control", w: 2, h: 1 },
  ], pins: ["auto/cpu_temp"] });
  const page = await pageFor(t, "overview");
  await page.locator(".mixed-units").waitFor();
  assert.match(await page.locator(".ax-hi:not(.ax-right)").textContent(), /°C/);
  assert.match(await page.locator(".ax-hi.ax-right").textContent(), /%/);
  await page.evaluate(async () => {
    const { S } = await import("/js/core.js"), { renderOverview } = await import("/js/overview.js");
    S.state.temp_unit = "F"; S.state.readings["auto/cpu_temp"] = 0;
    renderOverview();
  });
  assert.equal(await page.locator('[data-id="cpu"] .num').textContent(), "32");
  assert.equal(await page.locator('[data-id="cpu"] .unit').textContent(), "°F");
  await page.evaluate(async () => {
    const { S } = await import("/js/core.js"), { renderOverview } = await import("/js/overview.js");
    S.state.readings["auto/cpu_temp"] = null; S.state.readings["auto/cpu_load"] = null;
    renderOverview();
  });
  assert.equal(await page.locator('[data-id="cpu"] .num').textContent(), "–");
  assert.match(await page.locator('[data-id="mixed"] .tile-note').textContent(), /No reading/);
  const note = await page.locator('[data-id="mixed"] .tile-note').boundingBox();
  const time = await page.locator('[data-id="mixed"] .ax-time').boundingBox();
  assert.ok(note.y + note.height + 4 <= time.y, "Unavailable note must not overlap the time axis");
  await page.reload(); await page.locator('[data-id="cpu"] .num').waitFor();
  await page.context().setOffline(true);
  // Chromium offline emulation need not sever an already-open event stream. Reproduce a silently lost stream.
  await page.evaluate(() => window.__testStreams.forEach(s => s.close()));
  await page.waitForFunction(() => document.querySelector('[data-id="cpu"] .tile-note')?.textContent.includes("reconnect"));
  assert.equal(await page.locator('[data-id="cpu"] .num').textContent(), "–");
  assert.match(await page.locator(".control-tile-state").textContent(), /Waiting/);
  await page.screenshot({ path: join(OUTPUT, "disconnected-tiles.png") });
  await page.context().setOffline(false);
  await page.waitForFunction(() => document.querySelector('[data-id="cpu"] .num')?.textContent !== "–");
});
