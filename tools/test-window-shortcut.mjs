import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { rigPage } from "./isolated-browser-rig.mjs";

const output = process.env.AIRGLOW_SHORTCUT_OUTPUT || "output/windows-shortcut";
mkdirSync(output, { recursive: true });

test("Windows shortcut explains the browser-only fallback and opens without a write", async t => {
  const page = await rigPage(t, "settings"), writes = [];
  page.on("request", r => { if (r.method() === "POST") writes.push(r.url()); });
  await page.getByRole("button", { name: "Configure shortcut" }).click();
  const d = page.getByRole("dialog", { name: "Windows shortcut" });
  await d.getByRole("status").getByText(/available in the Airglow desktop app/).waitFor();
  assert.ok(await d.getByLabel("Key combination").isDisabled());
  assert.ok(await d.getByRole("button", { name: "Save shortcut" }).isDisabled());
  await d.getByRole("button", { name: "Refresh status" }).press("Enter");
  await page.keyboard.press("Escape");
  assert.equal(await page.locator("dialog[open]").count(), 0);
  assert.deepEqual(writes, []);
});

test("Windows shortcut retries a startup conflict, retains the old binding on a collision and disables explicitly", async t => {
  const page = await rigPage(t, "settings", 390, 844);
  const state = { available: true, registered: false, keys: ["F7", "F8", "F9"], session: "fixture", version: 1,
    saved: { key: "F7", action: "desk" }, error: "This shortcut is already in use. Choose another combination." };
  const commands = [];
  await page.route("**/api/windows-shortcut", async r => {
    if (r.request().method() === "GET") return r.fulfill({ json: state });
    const body = r.request().postDataJSON(); commands.push(body);
    assert.equal(body.session, state.session); assert.equal(body.version, state.version);
    if (body.choice.key === "F8") return r.fulfill({ status: 409, json: { error: "This shortcut is already in use. Choose another combination." } });
    state.saved = body.choice; state.registered = body.choice.key !== "off"; state.error = ""; state.version++;
    return r.fulfill({ json: state });
  });
  await page.getByRole("button", { name: "Configure shortcut" }).click();
  const d = page.getByRole("dialog", { name: "Windows shortcut" });
  const combo = d.getByLabel("Key combination"), action = d.getByLabel("Window to open");
  await d.getByRole("button", { name: "Try registration again" }).press("Enter");
  await d.getByRole("status").getByText("Registered with Windows. Ctrl + Shift + F7 opens Desk Display.").waitFor();
  await combo.selectOption("F8"); await action.selectOption("main");
  await d.getByRole("button", { name: "Save shortcut" }).press("Enter");
  await d.getByRole("alert").getByText(/already in use/).waitFor();
  assert.equal(await combo.inputValue(), "F7"); assert.equal(await action.inputValue(), "desk");
  await combo.selectOption("F9"); await action.selectOption("main");
  await d.getByRole("button", { name: "Save shortcut" }).press("Enter");
  await d.getByRole("status").getByText("Registered with Windows. Ctrl + Shift + F9 opens Airglow.").waitFor();
  assert.ok(await d.evaluate(el => el.scrollWidth <= el.clientWidth));
  await d.screenshot({ path: join(output, "windows-shortcut-narrow.png") });
  await combo.selectOption("off"); assert.ok(await action.isDisabled());
  await d.getByRole("button", { name: "Save shortcut" }).press("Enter");
  await d.getByRole("status").getByText("Off. No Windows shortcut is registered.").waitFor();
  assert.deepEqual(commands.map(c => c.choice.key), ["F7", "F8", "F9", "off"]);
});
