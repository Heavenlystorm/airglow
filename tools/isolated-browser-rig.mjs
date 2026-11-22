// An owned demo server with a fresh data directory and free loopback ports. No installed app or hardware.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { basename, dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { after, before } from "node:test";

export const root = fileURLToPath(new URL("../", import.meta.url));
let browser, server, base, fixture, serverLog = "";
let renderPreview = false;
export function withPreviewPage() { renderPreview = true; }
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
async function freePort() {
  const sock = createServer();
  await new Promise((resolve, reject) => { sock.once("error", reject); sock.listen(0, "127.0.0.1", resolve); });
  const port = sock.address().port;
  await new Promise(resolve => sock.close(resolve));
  return port;
}
export async function api(path, body) {
  const res = await fetch(base + path, body === undefined ? {} : { method: "POST",
    headers: { "X-Airglow": "1", "Content-Type": "application/json" }, body: JSON.stringify(body) });
  const data = await res.json();
  assert.ok(res.ok, `${path} failed: ${JSON.stringify(data)}`); return data;
}
export async function until(read, accept, delayMs = 12000) {
  const end = Date.now() + delayMs;
  let result;
  do {
    result = await read(); if (accept(result)) return result;
    await delay(150);
  } while (Date.now() < end);
  assert.fail(`Timed out. Last value: ${JSON.stringify(result)}`);
}
before(async () => {
  fixture = mkdtempSync(join(tmpdir(), "airglow-session-test-"));
  const port = await freePort(); let sensorPort = port;
  while (sensorPort === port) sensorPort = await freePort();
  base = `http://127.0.0.1:${port}`;
  server = spawn(process.env.PYTHON || "python", ["-u", "tools/dev_server.py", "--demo"], {
    cwd: root, windowsHide: true, env: { ...process.env, APPDATA: join(fixture, "roaming"), TEMP: fixture, TMP: fixture,
      AIRGLOW_FIXTURE: "1", AIRGLOW_FIXTURE_DATA: join(fixture, "data"), AIRGLOW_FIXTURE_PREVIEW_PAGE: renderPreview ? "1" : "0", AIRGLOW_DEMO_PORT: String(port), AIRGLOW_DEMO_LHM_PORT: String(sensorPort) },
    stdio: ["ignore", "pipe", "pipe"],
  });
  for (const stream of [server.stdout, server.stderr]) stream.on("data", c => { serverLog = (serverLog + c).slice(-2500); });
  await until(async () => {
    if (server.exitCode !== null) throw Error(`Demo exited: ${serverLog}`);
    try { return await api("/api/settings"); } catch { return null; }
  }, s => s?.settings?.lhm_url === `http://127.0.0.1:${sensorPort}`, 25000);
  await api("/api/onboarded", {}); await api("/api/settings", { design: "firefly", online_checks: false });
  let pw;
  if (process.env.AIRGLOW_PLAYWRIGHT_MODULE) {
    const path = process.env.AIRGLOW_PLAYWRIGHT_MODULE;
    pw = await import(isAbsolute(path) ? pathToFileURL(path).href : path);
  } else {
    try { pw = await import("playwright"); }
    catch { pw = await import(pathToFileURL(join(process.env.APPDATA, "npm/node_modules/playwright/index.mjs")).href); }
  }
  browser = await pw.chromium.launch({ channel: process.env.AIRGLOW_BROWSER || "msedge" });
  if (renderPreview) {
    const token = readFileSync(join(fixture,"data","preview-page.token"),"ascii");
    const hostPage = await browser.newPage();
    await hostPage.goto(base + "/runtime/host.html#token=" + encodeURIComponent(token) + "&preview=1", {waitUntil:"domcontentloaded"});
  }
});
after(async () => {
  await browser?.close();
  if (server && server.exitCode === null) {
    const done = new Promise(resolve => server.once("exit", resolve)); server.kill(); await done;
  }
  if (fixture && dirname(resolve(fixture)) === resolve(tmpdir()) && basename(fixture).startsWith("airglow-session-test-")) {
    rmSync(fixture, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 });
  }
});

export async function rigPage(t, view, width = 1360, height = 900) {
  const context = await browser.newContext({ viewport: { width, height }, reducedMotion: "reduce" });
  const page = await context.newPage();
  const errors = []; page.on("pageerror", e => errors.push(e.message));
  t.after(async () => { await context.close(); assert.deepEqual(errors, []); });
  await page.goto(base + "/#" + view, { waitUntil: "domcontentloaded" });
  await page.locator(`[data-view="${view}"]`).waitFor();
  return page;
}
