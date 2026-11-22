import assert from "node:assert/strict";
import { test } from "node:test";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

export function helpCases(staticPage, output) {
  test("Help guides an unconfigured rig to Studio, retains keyboard filters and respects the active modal", async t => {
    const calls = [], page = await staticPage(t, async (route, url) => {
      if (!url.pathname.startsWith("/api/")) return false;
      calls.push([route.request().method(), url.pathname]);
      await route.fulfill({ json: {} }); return true;
    });
    await page.evaluate(async () => {
      const { S } = await import("/js/core.js"); S.zones = { zones: [] }; S.state = { devices: [], sensors: {} };
      (await import("/js/design.js")).applyDesign("firefly", { persist: false });
      document.querySelector(".overview-view").hidden = false;
      (await import("/js/help.js")).initShortcuts();
      (await import("/js/workspace-tools.js")).initWorkspaceTools();
    });
    const d = page.locator("#sheet[open]"), guide = d.getByRole("tab", { name: "Task guide" }), keys = d.getByRole("tab", { name: "Keyboard shortcuts" });
    await page.locator("#ov-tools").click(); await page.locator("#ov-help").click();
    await d.getByText("Studio has a sample case ready", { exact: false }).waitFor();
    assert.ok(await d.getByRole("searchbox", { name: "Search the task guide" }).evaluate(el => el === document.activeElement));
    if (output) await d.screenshot({ path: join(output, "help-guide-desktop.png") });
    await d.getByLabel("Filter by topic").selectOption("Create & arrange");
    const search = d.getByRole("searchbox"); await search.fill("recover");
    assert.equal(await d.locator(".help-task").count(), 1);
    await guide.press("ArrowRight");
    assert.equal(await keys.getAttribute("aria-selected"), "true");
    await d.getByLabel("Filter by topic").selectOption("Studio");
    await search.fill("control + z");
    assert.match(await d.locator(".help-keys").innerText(), /Undo the last document edit/);
    await search.fill("zzzz"); await d.getByText("No shortcuts match these filters.").waitFor();
    await d.getByRole("button", { name: "Clear filters" }).click();
    assert.ok(await search.evaluate(el => el === document.activeElement));
    await page.keyboard.press("F1");
    assert.equal(await guide.count(), 1);
    await guide.click();
    assert.equal(await search.inputValue(), "recover");
    assert.equal(await d.getByLabel("Filter by topic").inputValue(), "Create & arrange");
    await d.getByLabel("Filter by topic").selectOption(""); await search.fill("");
    await d.getByRole("link", { name: "Explore templates" }).click();
    await page.waitForURL("**/#studio/new");
    assert.equal(await d.count(), 0);
    await page.keyboard.press("F1");
    await page.getByRole("dialog", { name: "Keyboard shortcuts" }).waitFor();
    await page.setViewportSize({ width: 390, height: 844 });
    await d.getByLabel("Filter by topic").selectOption("Lighting");
    assert.ok(await d.evaluate(el => el.clientWidth >= el.scrollWidth));
    if (output) await d.screenshot({ path: join(output, "help-shortcuts-narrow.png") });
    await guide.click();
    if (output) await d.screenshot({ path: join(output, "help-guide-narrow.png") });
    await page.evaluate(async () => (await import("/js/appearance.js")).applyAppearance({ scale: 120 }, { persist: false }));
    assert.ok(await d.evaluate(el => {
      const frame = el.getBoundingClientRect(), results = el.querySelector(".help-content").getBoundingClientRect();
      return frame.left >= 0 && frame.right <= innerWidth && frame.bottom <= innerHeight && results.bottom <= frame.bottom && el.clientWidth >= el.scrollWidth;
    }));
    if (output) await d.screenshot({ path: join(output, "help-guide-narrow-120.png") });
    await page.keyboard.press("Escape");
    assert.deepEqual(calls, []);
  });

  test("Help opens the real read-only rule rehearsal and updates the sample-case guidance when discovery finishes", async t => {
    const writes = [];
    const page = await staticPage(t, async (route, url) => {
      if (route.request().method() !== "POST") return false;
      writes.push(url.pathname);
      const scene = { on: false, effect: "solid", brightness: 65 };
      await route.fulfill({ json: { rows: [], active: [], winners: { lighting: null, fans: null }, scene, curve: "balanced", base: { scene, curve: "balanced" } } });
      return true;
    });
    await page.evaluate(async () => {
      const { S } = await import("/js/core.js"); S.state = { devices: [], sensors: {} };
      (await import("/js/help.js")).initShortcuts();
      await (await import("/js/workspace-tools.js")).openWorkspaceTool("help");
    });
    const d = page.locator("#sheet[open]");
    await d.getByText("Studio can preview a template on a sample case", { exact: false }).waitFor();
    await page.evaluate(async () => {
      const { S, emit } = await import("/js/core.js");
      S.zones = { zones: [{ device: { online: true }, config: { enabled: true }, positions: [[0, 0]] }] }; emit("zones");
    });
    await d.getByText("See a template on your configured rig", { exact: false }).waitFor();
    await d.getByRole("searchbox").fill("priority");
    await d.getByRole("button", { name: /Rehearse an automation/ }).press("Enter");
    await page.getByRole("dialog", { name: "Rehearse rules" }).getByText("There are no saved rules.", { exact: false }).waitFor();
    assert.deepEqual(writes, ["/api/automations/rehearse"]);
  });

  test("Quick switch opens Help before discovery and its template link reaches the actual sample-rig Studio", async t => {
    const writes = [];
    const page = await staticPage(t, async (r, url) => {
      if (url.pathname === "/js/main.js") { await r.fulfill({ path: fileURLToPath(new URL("../web/js/main.js", import.meta.url)) }); return true; }
      if (url.pathname === "/api/studio/effects") { await r.fulfill({ json: { effects: [] } }); return true; }
      if (r.request().method() === "POST") { writes.push(url.pathname); await r.fulfill({ status: 400, json: { error: "Fixture is read only" } }); return true; }
      return false;
    });
    await page.keyboard.press("Control+k");
    const quick = page.getByRole("dialog", { name: "Quick switch" });
    await quick.getByRole("combobox").fill("Airglow help");
    await quick.getByRole("combobox").press("Enter");
    const help = page.getByRole("dialog", { name: "Airglow help" });
    await help.getByRole("link", { name: "Explore templates" }).click();
    await page.getByRole("heading", { name: "Effect templates" }).waitFor();
    await page.getByText("No lights are set up yet, so previews use a sample case.", { exact: false }).waitFor();
    assert.equal(await page.locator("a[data-tab=studio]").getAttribute("aria-current"), "page");
    assert.deepEqual(writes, []);
  });

  test("Help links open the bundled lookbook, saved profile library, recordings and control status without changing equipment", async t => {
    const writes = [], page = await staticPage(t, async (route, url) => {
      if (!url.pathname.startsWith("/api/")) return false;
      if (route.request().method() !== "GET") writes.push(url.pathname);
      await route.fulfill({ json: url.pathname === "/api/cooling/sessions" ? { sessions: [], active: null } : {} });
      return true;
    });
    for (const [task, heading] of [["Browse lighting looks", "Lighting lookbook"], ["Compare saved profiles", "Profile library"],
      ["Compare recorded sessions", "Telemetry sessions"], ["Check what controls your hardware", "Control center"]]) {
      await page.evaluate(async () => (await import("/js/help.js")).openGuide());
      await page.getByRole("dialog", { name: "Airglow help" }).getByRole("button", { name: new RegExp(task) }).click();
      const next = page.getByRole("dialog", { name: heading, exact: true });
      await next.waitFor();
      if (heading === "Lighting lookbook") {
        await next.locator("[data-plate]").first().waitFor();
        assert.equal(await next.locator("[data-plate]").count(), 6);
      }
      await page.keyboard.press("Escape"); await next.waitFor({ state: "hidden" });
    }
    assert.deepEqual(writes, []);
  });
}
