import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { openFixtureBrowser } from "./fixture-browser.mjs";

const base = process.env.AIRGLOW_FIXTURE_URL || "http://127.0.0.1:8790";
const output = process.env.AIRGLOW_DESK_OUTPUT || "output/desk-display";
let browser;
before(async () => {
  browser = await openFixtureBrowser(base);
  mkdirSync(output, { recursive: true });
});
after(async () => { await browser?.close(); });

async function face(t, width, height) {
  const context = await browser.newContext({ viewport: { width,height }, reducedMotion: "reduce" });
  const page = await context.newPage();
  const errors = [], writes = [];
  page.on("pageerror", e => errors.push(e.message));
  page.on("request", req => { if (req.method() === "POST") writes.push(new URL(req.url()).pathname); });
  t.after(async () => { await context.close(); assert.deepEqual(errors, []); assert.ok(writes.every(p => p === "/api/desk-display/open"), JSON.stringify(writes)); });
  return page;
}

test("Desk Display opens from workspace tools and reports actual pinned measurements", async t => {
  const page = await face(t, 1440,900);
  await page.route(base + "/api/desk-display", async route => {
    const response = await route.fetch();
    await route.fulfill({ json: { ...await response.json(), design:"firefly" } });
  });
  await page.goto(base + "/#overview");
  await page.locator("#ov-tools").press("Enter");
  await page.getByRole("button", { name: /Desk Display/ }).click();
  await page.waitForURL(base + "/desk.html");
  const pinned = await fetch(base+"/api/desk-display").then(r=>r.json());
  await page.waitForFunction(() => document.querySelector("#desk-feedback").textContent === "");
  assert.equal(await page.locator(".desk-meter").count(),pinned.readings.length);
  assert.equal(await page.locator("#desk-pin").isVisible(),false, "Pinning requires a native companion window");
  await page.screenshot({path:join(output,"desk-glasshouse-desktop.png")});
  await page.getByRole("button",{name:"Display settings"}).click();
  await page.getByLabel("Layout").selectOption("instruments");
  await page.getByLabel("Scenery").selectOption("dusk");
  await page.getByLabel("Clock format").selectOption("24");
  await page.keyboard.press("Escape");
  await page.reload();
  await page.waitForFunction(()=>document.body.dataset.layout==="instruments" && !!document.querySelector(".desk-meter"));
  assert.equal(await page.locator("#desk-period").textContent(),"");
  await page.screenshot({path:join(output,"desk-instruments-desktop.png")});
});

test("the focus timer survives reload, pauses, finishes, and remains independent of hardware", async t => {
  const page=await face(t,660,440);
  await page.goto(base + "/desk.html");
  await page.locator("#desk-timer-open").click();
  await page.getByRole("button",{name:"15 min",exact:true}).click();
  await page.getByRole("button",{name:"Start focus timer"}).click();
  assert.match(await page.locator("#desk-time").innerText(),/^1[45]:/);
  await page.locator("#desk-timer-open").click(); await page.locator("#desk-pause").click();
  const left=await page.locator("#desk-time").innerText();
  await page.reload();
  assert.equal(await page.locator("#desk-time").innerText(),left);
  assert.equal(await page.locator("#desk-timer-open").innerText(),"Timer paused");
  await page.locator("#desk-timer-open").click(); await page.getByRole("button",{name:"Resume",exact:true}).click();
  await page.evaluate(()=>localStorage.setItem("airglow.desk.timer.v1",JSON.stringify({mode:"running",minutes:15,deadline:Date.now()-1000})));
  await page.reload(); await page.waitForFunction(()=>document.querySelector("#desk-timer-open").textContent==="Timer finished");
  await page.locator("#desk-timer-open").click(); await page.getByRole("button",{name:"Done",exact:true}).click();
  assert.equal(await page.locator("#desk-timer-open").innerText(),"Focus timer");
});

test("a companion can choose and order its own measurements independently of Overview", async t => {
  const page = await face(t, 740,560);
  const overview = await fetch(base + "/api/desk-display").then(r=>r.json());
  await page.goto(base + "/desk.html");
  await page.locator(".desk-meter").first().waitFor();
  await page.locator("#desk-choose-readings").click();
  await page.locator("#desk-pick-mode").selectOption("desk");
  await page.locator("#desk-pick-list input").first().waitFor();
  while(await page.locator("[data-remove]").count()) await page.locator("[data-remove]").first().click();
  const choices = page.locator("#desk-pick-list input");
  const ids = await choices.evaluateAll(inputs=>inputs.slice(0,4).map(i=>i.dataset.meterId));
  assert.equal(ids.length,4);
  for(const id of ids) await page.locator("input[data-meter-id]").evaluateAll((inputs, id)=>inputs.find(i=>i.dataset.meterId===id).click(),id);
  assert.ok(await page.locator("#desk-pick-list input:disabled").count()>0);
  await page.locator("[data-up='1']").click();
  const expected=[ids[1],ids[0],ids[2],ids[3]];
  const request=page.waitForRequest(r=>r.url().includes("/api/desk-display?source=desk"));
  await page.locator("#desk-pick-save").click();
  assert.deepEqual(new URL((await request).url()).searchParams.getAll("reading"),expected);
  await page.locator(".desk-meter").nth(3).waitFor(); await page.reload(); await page.locator(".desk-meter").nth(3).waitFor();
  assert.equal(await page.locator("#desk-reading-title").innerText(),"Display readings");
  const actual = await fetch(base+"/api/desk-display").then(r=>r.json());
  assert.deepEqual(actual.readings.map(x=>x.id),overview.readings.map(x=>x.id));
  await page.locator("#desk-choose-readings").click(); await page.locator("#desk-pick-mode").selectOption("overview");
  await page.locator(".desk-picker .desk-close").click(); assert.equal(await page.locator("#desk-reading-title").innerText(),"Display readings");
  await page.locator("#desk-choose-readings").click(); await page.locator("#desk-pick-mode").selectOption("overview"); await page.locator("#desk-pick-save").click();
  await page.waitForFunction(()=>document.querySelector("#desk-reading-title").textContent==="Pinned readings");
});

test("small and disconnected displays keep their clock, legible controls and truthful reading state", async t => {
  const page=await face(t,360,320);
  let offline=false, empty=false;
  await page.route(base+"/api/desk-display",async route=>{
    if(offline) return route.fulfill({status:503,json:{error:"unavailable"}});
    const response=await route.fetch(), data=await response.json();
    if(empty) data.readings=[];
    else data.readings=Array.from({length:4},(_,i)=>({ id:`probe${i}`,short:`Sensor ${i+1}`,name:"A long sensor title",unit:"°C",value:40+i,samples:data.times.map(()=>40+i) }));
    data.design="firefly"; data.appearance.scale=120;
    await route.fulfill({json:data});
  });
  await page.goto(base + "/desk.html");
  await page.locator(".desk-meter").nth(3).waitFor();
  assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1));
  assert.ok(await page.locator(".desk-reading").last().evaluate(el=>el.getBoundingClientRect().bottom <= innerHeight+1), "All four values fit the compact face");
  await page.screenshot({path:join(output,"desk-compact.png")});
  await page.setViewportSize({width:320,height:212});
  await page.getByRole("button",{name:"Display settings"}).click();
  await page.getByLabel("Show seconds").check();
  await page.getByLabel("Scenery").selectOption("daily");
  await page.keyboard.press("Escape");
  assert.ok(await page.locator(".desk-plate").isVisible());
  assert.ok(await page.locator(".desk-reading").last().evaluate(el=>el.getBoundingClientRect().bottom<=innerHeight+1),
    "Four values fit the smallest native client with seconds at 120%");
  await page.screenshot({path:join(output,"desk-minimum-120.png")});
  offline=true;
  await page.waitForFunction(()=>document.querySelector(".desk-reading strong").textContent==="–");
  assert.match(await page.locator("#desk-feedback").innerText(),/offline/);
  assert.ok(await page.locator("#desk-time").innerText());
  offline=false; empty=true;
  await page.locator("#desk-empty").waitFor(); assert.equal(await page.locator(".desk-meter").count(),0);
  await page.emulateMedia({forcedColors:"active"});
  await page.getByRole("button",{name:"Display settings"}).press("Enter");
  await page.getByLabel("Layout").selectOption("quiet"); await page.keyboard.press("Escape");
  assert.equal(await page.locator(".desk-readings").isVisible(),false);
  assert.ok(await page.locator("#desk-options-open").evaluate(el=>el===document.activeElement));
});

test("reading chooser retries discovery and protects a selection saved by another window", async t=>{
  const page=await face(t,720,560);
  const other=await page.context().newPage();
  let requests=0;
  await page.route(base+"/api/sensors",async route=>{
    if(++requests===1)return route.fulfill({status:503,json:{error:"Sensor catalog unavailable"}});
    return route.continue();
  });
  await Promise.all([page.goto(base+"/desk.html"),other.goto(base+"/desk.html")]);
  await page.locator("#desk-choose-readings").click();
  assert.equal(requests,0,"Following Overview pins does not request the entire sensor catalog");
  await page.locator("#desk-pick-mode").selectOption("desk");
  await page.getByRole("status").filter({hasText:"Could not load available readings"}).waitFor();
  await page.getByRole("button",{name:"Retry loading readings"}).press("Enter");
  await page.locator("#desk-pick-list input").first().waitFor();
  assert.equal(requests,2);
  assert.ok(await page.locator("#desk-pick-search").evaluate(e=>e===document.activeElement));
  await other.locator("#desk-choose-readings").click();
  await other.locator("#desk-pick-mode").selectOption("desk");
  await other.locator("#desk-pick-list input").first().waitFor();
  while(await other.locator("[data-remove]").count())await other.locator("[data-remove]").first().click();
  await other.locator("#desk-pick-list input").first().check();
  await other.locator("#desk-pick-save").click();
  await page.waitForFunction(()=>document.querySelector("#desk-reading-title").textContent==="Display readings");
  await page.locator("#desk-pick-save").click();
  await page.getByRole("status").filter({hasText:"changed in another window"}).waitFor();
  await page.keyboard.press("Escape");
  await page.locator("#desk-choose-readings").click();
  assert.equal(await page.locator("[data-remove]").count(),1);
  assert.equal(await page.locator("#desk-pick-mode").inputValue(),"desk");
});
