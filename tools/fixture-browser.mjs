import assert from "node:assert/strict";
import { isAbsolute, join } from "node:path";
import { pathToFileURL } from "node:url";

// Shared browser checks run only against the paired loopback demo server, including its isolated sensor port.
export async function openFixtureBrowser(base) {
  const url = new URL(base);
  assert.equal(url.hostname,"127.0.0.1");
  const settings = await fetch(base + "/api/settings").then(r => r.json());
  assert.equal(settings.settings.lhm_url, "http://127.0.0.1:" + (Number(url.port) + 1), "Use the isolated Airglow fixture");
  let module = process.env.AIRGLOW_PLAYWRIGHT_MODULE || "playwright";
  try { module = await import(isAbsolute(module) ? pathToFileURL(module).href : module); }
  catch (e) {
    if (e.code !== "ERR_MODULE_NOT_FOUND" || !process.env.APPDATA) throw e;
    module = await import(pathToFileURL(join(process.env.APPDATA, "npm/node_modules/playwright/index.mjs")).href);
  }
  return module.chromium.launch({ channel: process.env.AIRGLOW_BROWSER || "msedge" });
}
