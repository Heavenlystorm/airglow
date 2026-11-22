import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const evaluate = `import json, sys
from tests.test_automation_rehearsal import RULES, PRESETS, SCENE, simulate
version = json.load(sys.stdin)
print(json.dumps(simulate(**version) if version is not None else {"rules": RULES, "presets": list(PRESETS.values()), "scene": SCENE}))`;
function rehearsal(body) {
  const run = spawnSync(process.env.AIRGLOW_PYTHON || "python", ["-c", evaluate], { cwd: root, encoding: "utf8", input: JSON.stringify(body) });
  assert.equal(run.status, 0, run.stderr);
  return JSON.parse(run.stdout);
}

export function rehearsalCase(staticPage, output) {
  test("Automations rehearses overnight priority, missing readings, thermal memory and return to base through the production matcher", async t => {
    const calls = [], docs = rehearsal(null); let failOnce = false;
    const page = await staticPage(t, async (r, url) => {
      if (r.request().method() !== "POST") return false;
      calls.push(url.pathname);
      if (url.pathname !== "/api/automations/rehearse") return false;
      if (failOnce) { failOnce = false; await r.fulfill({ status: 503, json: { error: "Try again." } }); }
      else await r.fulfill({ json: rehearsal(r.request().postDataJSON()) });
      return true;
    });
    await page.evaluate(async ({ rules, presets, scene }) => {
      const { S } = await import("/js/core.js");
      S.rules = rules; S.presets = presets; S.state = { scene, temp_unit: "F", telemetry_stale: true, sensors: { cpu_temp: 66 }, override: {} };
      S.effects = [{ id: "solid", name: "Solid colour" }, { id: "comet", name: "Comet" }];
      document.querySelector(".auto-view").hidden = false;
      (await import("/js/design.js")).applyDesign("firefly", { persist: false });
      const a = await import("/js/automations.js"); a.initAutomations(); a.renderAutomations();
    }, docs);
    const opener = page.getByRole("button", { name: "Rehearse rules" });
    await opener.press("Enter");
    const d = page.locator("#sheet[open]"), run = d.getByRole("button", { name: "Run rehearsal" });
    const result = d.locator(".rehearse-result:visible"), status = d.getByRole("status");
    const row = i => result.locator(".rehearse-ranks li").nth(i);
    await result.waitFor();
    assert.equal(await d.getByLabel("Processor", { exact: true }).inputValue(), "");
    assert.match(await row(2).innerText(), /Reading unavailable/);
    await d.getByLabel("Day", { exact: true }).selectOption("6");
    await d.getByLabel("Foreground app").fill("GAME.exe");
    await d.getByLabel("Idle minutes").fill("3");
    await d.getByLabel("Processor", { exact: true }).fill("176");
    await d.getByLabel("Schedule boundary").selectOption("0-start");
    await result.waitFor();
    assert.match(await status.innerText(), /Sunday at 23:00 · 5 matching rules/);
    assert.match(await result.locator('[data-channel="lighting"]').innerText(), /Lights off.*Rule #1 wins/s);
    assert.match(await result.locator('[data-channel="fans"]').innerText(), /Game profile.*Rule #2 wins/s);
    assert.match(await row(1).innerText(), /#1 wins lighting · Wins cooling/);
    assert.match(await row(2).innerText(), /#2 wins cooling/);
    if (output) await d.screenshot({ path: join(output, "automation-rehearsal.png") });
    await d.getByLabel("Schedule boundary").selectOption("0-end");
    await result.waitFor(); assert.match(await status.innerText(), /Monday at 07:00/);
    assert.match(await result.locator('[data-channel="lighting"]').innerText(), /Comet · 89%/);
    await d.getByRole("button", { name: "−1 minute" }).press("Enter");
    await result.waitFor(); assert.match(await status.innerText(), /Monday at 06:59/);
    assert.match(await result.locator('[data-channel="lighting"]').innerText(), /Lights off/);
    await d.getByRole("button", { name: "+1 minute" }).press("Enter");
    await result.waitFor();
    await d.getByLabel("Foreground app").fill("");
    await d.getByLabel("Processor", { exact: true }).fill("171.5");
    await d.getByLabel("Carry temperature matches").check();
    await run.click(); await result.waitFor();
    assert.match(await row(2).innerText(), /Wins cooling.*threshold 170.6°F/s);
    assert.match(await result.locator('[data-channel="lighting"]').innerText(), /Solid colour · 36%/);
    await d.getByLabel("Processor", { exact: true }).fill(""); await d.getByLabel("Idle minutes").fill("0");
    failOnce = true; await run.click(); await status.getByText("Rehearsal failed.", { exact: false }).waitFor();
    assert.equal(await result.count(), 0);
    await page.setViewportSize({ width: 390, height: 844 });
    await run.click(); await result.waitFor();
    assert.match(await status.innerText(), /0 matching rules/);
    assert.equal(await result.getByText("Base setting", { exact: true }).count(), 2);
    assert.ok(await status.evaluate(s => s === document.activeElement));
    assert.ok(await d.evaluate(s => s.scrollWidth <= s.clientWidth + 1));
    await d.getByRole("button", { name: "Edit scenario" }).click();
    assert.ok(await d.getByLabel("Day", { exact: true }).evaluate(s => s === document.activeElement));
    await page.keyboard.press("Escape");
    assert.ok(await opener.evaluate(b => b === document.activeElement));
    assert.deepEqual([...new Set(calls)], ["/api/automations/rehearse"]);
    assert.deepEqual(await page.evaluate(async () => (await import("/js/core.js")).S.rules), docs.rules);
  });
}
