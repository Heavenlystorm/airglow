import assert from "node:assert/strict";
import { test } from "node:test";
import { api, rigPage, until } from "./isolated-browser-rig.mjs";

async function quick(page, name) {
  await page.keyboard.press("Control+k");
  const search = page.getByRole("dialog", {name:"Quick switch"}).getByRole("combobox");
  await search.fill(name);
  await search.press("Enter");
}

async function action(page, path, click) {
  const completed = page.waitForResponse(r => new URL(r.url()).pathname === path && r.request().method() === "POST");
  await click();
  const response = await completed;
  assert.ok(response.ok(), await response.text());
  return response.json();
}

test("a focus session can run beside a marked recording while a personal lighting recipe is saved, applied and undone", async t => {
  const page = await rigPage(t,"overview");
  await page.locator(".tile-sensor").first().waitFor();
  const before = await api("/api/state");
  const context = page.context(), origin = new URL(page.url()).origin;
  const desk = await context.newPage(), recorder = await context.newPage();
  const errors = [];
  for (const p of [desk,recorder]) p.on("pageerror", e=>errors.push(e.message));
  t.after(async()=>{await Promise.all([desk.close(),recorder.close()]);assert.deepEqual(errors,[]);});

  await desk.goto(origin+"/desk.html");
  await desk.locator("#desk-timer-open").click();
  await desk.locator("[data-minutes='25']").click();
  await desk.getByRole("button",{name:"Start focus timer"}).click();
  const timerKey = "airglow.desk.timer.v1";
  const timerState = p=>p.evaluate(key=>JSON.parse(localStorage.getItem(key)),timerKey);
  const timer = await timerState(desk);
  assert.equal(timer.minutes,25);
  assert.deepEqual(await timerState(page),timer);

  await recorder.goto(origin+"/#overview");
  await recorder.locator("#ov-tools").click();
  await recorder.locator("[data-telemetry-sessions]").click();
  await recorder.locator("#cooling-annotation").fill("Evening lighting comparison");
  const recording = await action(recorder,"/api/cooling/start",()=>recorder.locator("[data-cooling-start]").click());
  await recorder.locator("#live-marker-label").fill("Before lookbook");
  await action(recorder,"/api/cooling/mark",()=>recorder.locator("[data-mark-now]").click());

  await quick(page,"Lighting lookbook");
  await page.locator(".lb-card").nth(1).click();
  await page.locator(".lb-bench [data-frame='ready']").waitFor();
  await page.locator("#lb-bright").focus();
  await page.keyboard.press("Home");
  for (let i=0;i<5;i++) await page.keyboard.press("ArrowRight");
  const name = "Quiet iris";
  await page.locator("#lb-name").fill(name);
  const imported = await action(page,"/api/profiles/import",()=>page.getByRole("button",{name:"Save to library",exact:true}).click());
  const preset = imported.presets.find(p=>p.id===imported.id);
  assert.equal(preset.brightness,5);
  assert.deepEqual((await api("/api/state")).scene,before.scene);
  await page.locator(".pl-title h3").filter({hasText:name}).waitFor();
  await action(page,"/api/presets/"+preset.id+"/apply",()=>page.locator("[data-pl-apply]").click());
  await until(()=>api("/api/state"),s=>s.active_preset===preset.id && s.scene.brightness===5);
  await page.getByRole("button",{name:"Undo",exact:true}).click();
  await until(()=>api("/api/state"),s=>s.active_preset===before.active_preset && s.scene.brightness===before.scene.brightness);
  const restored = await api("/api/state");
  assert.deepEqual(restored.scene,before.scene);
  assert.equal(restored.fans.base_curve,before.fans.base_curve);

  await quick(page,"Wind down");
  await page.getByLabel("Fade duration in minutes").fill("5");
  await action(page,"/api/lighting/wind-down",()=>page.getByRole("button",{name:"Start fade"}).click());
  await recorder.locator("#live-marker-label").fill("Fade started");
  await action(recorder,"/api/cooling/mark",()=>recorder.locator("[data-mark-now]").click());
  await action(page,"/api/lighting/wind-down",()=>page.getByRole("button",{name:/Cancel fade/i}).click());
  await until(()=>api("/api/cooling/trace?id="+recording.active.id),r=>r.samples.length>=3);
  await action(recorder,"/api/cooling/stop",()=>recorder.locator("[data-cooling-stop]").click());
  const trace = await api("/api/cooling/trace?id="+recording.active.id);
  assert.deepEqual(trace.markers.map(m=>m.label),["Before lookbook","Fade started"]);
  assert.equal((await api("/api/state")).fans.base_curve,before.fans.base_curve);
  await desk.reload();
  await desk.locator("#desk-timer-open").waitFor();
  assert.deepEqual(await timerState(desk),timer);
});
