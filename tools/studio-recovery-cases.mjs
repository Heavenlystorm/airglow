import assert from "node:assert/strict";
import { test } from "node:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import vm from "node:vm";

// The routed fixture runs the shipped layer renderer; preview sessions have no output devices.
function fixture() {
  const ctx = vm.createContext({ self: {} });
  vm.runInContext(readFileSync(new URL("../web/runtime/layers.js", import.meta.url), "utf8"), ctx);
  const { types } = JSON.parse(readFileSync(new URL("../web/runtime/layers.json", import.meta.url), "utf8"));
  const defaults = Object.fromEntries(Object.entries(types).map(([key, t]) => [key, Object.fromEntries(t.params.map(p => [p.id, p.default]))]));
  let next = 0;
  const jobs = new Map(), writes = [];
  async function route(reply, url) {
    const path = url.pathname;
    if (path === "/api/studio/effects") { await reply.fulfill({ json: { effects: [] } }); return true; }
    if (reply.request().method() !== "POST") return false;
    const body = reply.request().postDataJSON();
    let json = {};
    if (path.endsWith("/preview/load")) {
      const id = String(++next);
      const layer = body.effect.kind === "layers" ? ctx.self.AirglowLayers.effect(body.effect.layers, defaults) : null;
      layer?.init({ params: body.effect.params });
      jobs.set(id, { layer, points: body.points }); json = { id };
    } else if (path.endsWith("/preview/tick")) {
      const job = jobs.get(body.id);
      if (job) {
        job.layer?.frame(body);
        const colors = job.points.flatMap(([px, py, area, local, fan, zone, position], index) =>
          job.layer ? Array.from(job.layer.point({ x: px / 320, y: py / 200, area, local, fan, zone, position, index }, body)) : [90, 120, 135]);
        json = { colors: Buffer.from(colors).toString("base64") };
      } else json = { colors: null, problem: { reason: "stopped" } };
    } else if (path.endsWith("/preview/stop")) jobs.delete(body.id);
    else if (path === "/api/studio/build") json = { effect: { params: [] }, source: `<html><title>${body.name || "Effect"}</title><script>${body.code || ""}</script></html>` };
    else if (path === "/api/studio/check") json = { flags: [] };
    else writes.push({ path, body });
    await reply.fulfill({ json }); return true;
  }
  return { route, writes, jobs };
}

async function enter(page) {
  await page.evaluate(async () => {
    (await import("/js/design.js")).applyDesign("firefly", { persist: false });
    document.querySelector(".studio-view").hidden = false;
    const { initStudio, renderStudio } = await import("/js/studio.js");
    initStudio(); await renderStudio();
  });
}
const home = page => page.evaluate(async () => (await import("/js/studio.js")).renderStudio());
const editor = page => page.locator("#st-name");
const checkpoint = page => page.locator("#st-editor-checkpoints");
const sheet = page => page.locator("#sheet[open]");

export function studioRecoveryCases(staticPage, output) {
  test("Studio compares real checkpoint stills, opens independent copies and resumes several drafts after reload", async t => {
    const live = fixture(), page = await staticPage(t, live.route);
    await enter(page);
    await page.locator('[data-tpl="lantern-walk"]').click();
    await editor(page).fill("Lantern study");
    await checkpoint(page).click();
    await sheet(page).getByLabel("Name a checkpoint", { exact: false }).fill("Full light");
    await page.keyboard.press("Control+s");
    await sheet(page).getByRole("button", { name: "Keep checkpoint" }).press("Enter");
    await sheet(page).getByText("This version matches the current edit.").waitFor();
    await sheet(page).getByRole("button", { name: "Close", exact: true }).click();
    await page.locator("#mm-brightness").fill("28");
    await checkpoint(page).click();
    await sheet(page).getByText("Brightness: 100 → 28", { exact: true }).waitFor();
    await page.waitForFunction(() => [...document.querySelectorAll("#sheet canvas")].length === 2 &&
      [...document.querySelectorAll("#sheet canvas")].every(c => c.nextElementSibling.hidden));
    const frames = await sheet(page).locator("canvas").evaluateAll(cs => cs.map(c => c.toDataURL()));
    assert.notEqual(frames[0], frames[1], "the same rig at the same instant has a different brightness");
    if (output) await sheet(page).screenshot({ path: join(output, "studio-checkpoint-comparison.png") });
    await page.setViewportSize({ width: 390, height: 844 });
    assert.ok(await sheet(page).evaluate(d => d.scrollWidth <= d.clientWidth + 1));
    await sheet(page).getByRole("button", { name: "Open as a copy" }).press("Enter");
    assert.equal(await editor(page).inputValue(), "Lantern study (copy)");
    assert.equal(await page.locator("#mm-brightness").inputValue(), "100");
    assert.equal(await page.locator("#st-lights").isChecked(), false);
    await home(page);
    assert.equal(await page.getByRole("button", { name: /^Resume Lantern study/ }).count(), 2);
    await page.reload(); await enter(page);
    await page.getByRole("button", { name: "Resume Lantern study", exact: true }).click();
    assert.equal(await page.locator("#mm-brightness").inputValue(), "28");
    await home(page);
    await page.setViewportSize({ width: 1280, height: 900 });
    if (output) await page.locator("#st-draft").screenshot({ path: join(output, "studio-draft-shelf.png") });
    await page.getByRole("button", { name: "Browse checkpoints" }).click();
    assert.equal(await sheet(page).locator("canvas").count(), 1);
    await page.keyboard.press("Escape"); await sheet(page).waitFor({ state: "hidden" });
    assert.ok(await page.locator("#st-home-checkpoints").evaluate(b => b === document.activeElement));
    assert.deepEqual(live.writes, []);
  });

  test("Studio keeps painted source layers through Code edits, checkpoint copies, mode switches and storage failures", async t => {
    const live = fixture(), page = await staticPage(t, live.route);
    await enter(page);
    await page.locator('[data-tpl="lantern-walk"]').click();
    await page.locator('li[data-sel="1"]').click();
    await page.locator('[data-details="paint-numeric"] summary').click();
    await page.locator('[data-paint="add-led"]').click();
    await page.locator('[data-mode="code"]').click();
    await page.locator(".CodeMirror").waitFor();
    await page.locator(".CodeMirror").evaluate(el => el.CodeMirror.setValue("<html><title>Code study</title><script>airglow.effect({point(){return [1,2,3]}});</script></html>"));
    await checkpoint(page).click();
    const field = sheet(page).locator("#st-checkpoint-name");
    await field.fill("Code and paint");
    await page.evaluate(() => {
      const original = Storage.prototype.setItem;
      window.repairStudioStorage = () => { Storage.prototype.setItem = original; };
      Storage.prototype.setItem = () => { throw new DOMException("Full", "QuotaExceededError"); };
    });
    await sheet(page).getByRole("button", { name: "Keep checkpoint" }).click();
    await sheet(page).getByRole("alert").getByText("Local storage is unavailable or full.", { exact: false }).waitFor();
    assert.equal(await field.inputValue(), "Code and paint");
    await page.evaluate(() => window.repairStudioStorage());
    await sheet(page).getByRole("button", { name: "Keep checkpoint" }).click();
    await sheet(page).getByRole("button", { name: "Open as a copy" }).click();
    await page.locator(".CodeMirror").waitFor();
    const source = await page.locator(".CodeMirror").evaluate(el => el.CodeMirror.getValue());
    assert.match(source, /return \[1,2,3\]/);
    page.once("dialog", dialog => dialog.accept());
    await page.locator('[data-mode="layers"]').click();
    await page.locator('li[data-sel="1"]').click();
    await page.locator("#st-paint-status").getByText("1/2048 positions", { exact: false }).waitFor();
    await page.locator('li[data-sel="1"]').focus();
    await page.keyboard.press("Control+z");
    await page.locator(".CodeMirror").waitFor();
    await page.locator("#st-editor-checkpoints").focus();
    await page.keyboard.press("Control+Shift+z");
    await page.locator('li[data-sel="1"]').waitFor();
    assert.equal(await page.locator(".st-edit.is-code").count(), 0);
    assert.deepEqual(live.writes, []);
  });
}
