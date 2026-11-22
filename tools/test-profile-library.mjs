import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { mkdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { openFixtureBrowser } from "./fixture-browser.mjs";

const base = process.env.AIRGLOW_FIXTURE_URL || "http://127.0.0.1:8790";
const output = process.env.AIRGLOW_LIBRARY_OUTPUT || "output/profile-library";
const api = async(path,body) => {
  const r = await fetch(base+path,body===undefined ? {} : {method:"POST",headers:{"X-Airglow":"1","Content-Type":"application/json"},body:JSON.stringify(body)});
  assert.ok(r.ok,`${path} returned ${r.status}`); return r.json();
};
let browser;
before(async () => {
  browser = await openFixtureBrowser(base);
  await api("/api/onboarded", {});
  await api("/api/settings", { online_checks: false });
  mkdirSync(output, { recursive: true });
});
after(async () => { await browser?.close(); });

async function open(t,viewport={width:1380,height:900}, expectedConflictPath=null) {
  const page=await browser.newPage({viewport,reducedMotion:"reduce"});
  await page.addInitScript(()=>Object.defineProperty(window,"showSaveFilePicker",{value:undefined,configurable:true}));
  const errors=[],writes=[];
  page.on("pageerror",e=>errors.push(e.message));
  page.on("request",r=>{if(r.method()==="POST")writes.push(new URL(r.url()).pathname);});
  t.after(async()=>{await page.close();assert.deepEqual(errors,[]);assert.ok(writes.every(p=>p===expectedConflictPath || /^\/api\/(studio\/preview\/|profiles\/(export|import|inspect-file)$)/.test(p)),writes.join(", "));});
  await page.goto(base+"/#overview"); await page.locator("#ov-tools").click(); await page.locator("#ov-profile-library").click();
  await page.locator("#pl-list .pl-card").first().waitFor(); return page;
}

test("two saved looks render on the same rig with search, cooling filters, timing and a settings comparison",async t=>{
  const page=await open(t), records=await api("/api/presets"), [a,b]=records.slice(0,2);
  await page.locator("#pl-search").fill(a.name);
  assert.ok(await page.locator(".pl-card").count()>=1);
  await page.locator('[data-pl-id="'+a.id+'"]').click();
  await page.locator("#pl-search").fill("");
  await page.locator("#pl-compare").selectOption(b.id);
  await page.waitForFunction(()=>document.querySelectorAll('.pl-portrait[data-frame="ready"]').length===2,{},{timeout:22000});
  assert.equal(await page.locator(".pl-portrait canvas").count(),2);
  const pictures=await page.locator(".pl-portrait canvas").evaluateAll(c=>c.map(x=>x.toDataURL()));
  assert.notEqual(pictures[0],pictures[1],"Different saved looks should actually render different frames");
  await page.locator("#pl-diffs").check();
  await page.locator("#pl-time").focus(); await page.keyboard.press("ArrowRight");
  assert.equal(await page.locator('output[for="pl-time"]').textContent(),"2.5s");
  await page.locator("#pl-time").blur();await page.waitForTimeout(350);
  await page.waitForFunction(()=>document.querySelectorAll('.pl-portrait[data-frame="ready"]').length===2);
  await page.screenshot({path:join(output,"profile-comparison.png")});
  await page.locator("#pl-filter").selectOption("cooling");
  const ids=await page.locator(".pl-card").evaluateAll(bs=>bs.map(b=>b.dataset.plId));
  assert.deepEqual(ids,records.filter(p=>p.fan_points?.length || p.fan_curve || p.fan_profile).map(p=>p.id));
  await page.locator("#pl-search").fill("nothing-matches-this-5555");
  assert.equal(await page.locator(".pl-card").count(),0);
  await page.keyboard.press("Escape");assert.equal(await page.locator("#pl-search").inputValue(),"");
  await page.keyboard.press("Escape");await page.waitForFunction(()=>document.activeElement?.id==="ov-tools");
});

test("export and explicitly reviewed import round trip through a local file without activating it",async t=>{
  const originalState=await api("/api/state");
  const page=await open(t,{width:390,height:820});
  await page.locator(".pl-card").first().click();
  const dl=page.waitForEvent("download"); await page.locator("[data-pl-export]").click();
  const download=await dl, path=await download.path(), shared=JSON.parse(readFileSync(path,"utf8"));
  assert.equal(shared.airglow_profile,1);assert.match(download.suggestedFilename(),/\.airglow-profile\.json$/);
  const name="File test "+Date.now();
  const buffer=Buffer.from(JSON.stringify({...shared,name}));
  await page.locator("#pl-file").setInputFiles({name:"portable.json",mimeType:"application/json",buffer});
  await page.locator("[data-pl-install]").waitFor();
  assert.equal((await api("/api/presets")).some(p=>p.name===name),false);
  await page.locator("[data-pl-back]").click();
  await page.locator("#pl-file").setInputFiles({name:"portable.json",mimeType:"application/json",buffer});
  await page.locator("[data-pl-install]").click();
  await page.waitForFunction(n=>document.querySelector(".pl-title h3")?.textContent===n,name);
  const after=await api("/api/presets"), added=after.find(p=>p.name===name);
  t.after(async()=>{if(added){const all=await api("/api/presets");await api("/api/presets",{presets:all.filter(p=>p.id!==added.id)});}});
  assert.ok(added);assert.equal((await api("/api/state")).active_preset,originalState.active_preset);
  await page.locator("#pl-compare").selectOption(after.find(p=>p.id!==added.id).id);
  await page.waitForFunction(()=>document.querySelectorAll('.pl-portrait[data-frame="ready"]').length===2);
  assert.ok(await page.locator(".profile-library").evaluate(n=>n.scrollWidth<=n.clientWidth+1));
  await page.screenshot({path:join(output,"profile-comparison-compact.png")});
  await page.locator("#pl-file").setInputFiles({name:"broken.json",mimeType:"application/json",buffer:Buffer.from("garbled")});
  assert.match(await page.locator(".pl-feedback").innerText(),/valid JSON/);
});

test("every illustrated look previews offline and an adjusted copy saves without applying lighting or fans",async t=>{
  const page=await open(t), state=await api("/api/state");
  await page.locator("#pl-lookbook").click();
  const cards=page.locator(".lb-card");
  await cards.first().waitFor();
  assert.equal(await cards.count(),6);
  await page.screenshot({path:join(output,"lighting-lookbook.png")});
  for(let tile=0;tile<6;tile++){
    await cards.nth(tile).click();
    await page.locator('.lb-bench [data-frame="ready"]').waitFor({timeout:20000});
    assert.ok(await page.locator(".lb-cover h3").innerText());
    await page.locator("[data-lb-back]").click();
    assert.equal(await page.evaluate(()=>document.activeElement?.dataset.plate),String(tile));
  }
  await cards.nth(3).click(); await page.locator("#lb-areas").uncheck();
  const brightness=page.getByRole("dialog",{name:"Lighting lookbook"}).getByRole("slider",{name:"Brightness",exact:true});
  await brightness.focus();await page.keyboard.press("Home");await page.keyboard.press("ArrowRight");
  assert.equal(await brightness.getAttribute("aria-valuetext"),"1%");
  assert.equal(await page.locator('output[for="lb-bright"]').innerText(),"1%");
  const compare=(await api("/api/presets"))[0];await page.locator("#lb-compare").selectOption(compare.id);
  await page.waitForFunction(()=>document.querySelectorAll('.lb-bench [data-frame="ready"]').length===2);
  const name="Lookbook test "+Date.now();await page.locator("#lb-name").fill(name);
  await page.getByRole("button",{name:"Save to library",exact:true}).click();
  await page.waitForFunction(n=>document.querySelector(".pl-title h3")?.textContent===n,name);
  const saved=(await api("/api/presets")).find(p=>p.name===name);
  t.after(async()=>{if(saved){const all=await api("/api/presets");await api("/api/presets",{presets:all.filter(p=>p.id!==saved.id)});}});
  assert.equal(saved.brightness,1);assert.deepEqual(saved.areas||{},{});assert.ok(!saved.fan_curve && !saved.fan_profile);
  const current=await api("/api/state");assert.equal(current.active_preset,state.active_preset);assert.deepEqual(current.scene,state.scene);
  await page.setViewportSize({width:390,height:820}); await page.locator("#pl-lookbook").click();
  await cards.nth(2).click();await page.locator('.lb-bench [data-frame="ready"]').waitFor();
  assert.ok(await page.locator(".lookbook").evaluate(n=>n.scrollWidth<=n.clientWidth+1));
  await page.screenshot({path:join(output,"lighting-lookbook-compact.png")});
});

test("a stale preview refuses to apply, and reopening fetches the latest profile",async t=>{
  const records=await api("/api/presets"), old=records[0], state=await api("/api/state"), path="/api/presets/"+old.id+"/apply";
  const page=await open(t,undefined,path);await page.locator('[data-pl-id="'+old.id+'"]').click();
  await page.locator('.pl-portrait[data-frame="ready"]').waitFor();
  const name=old.name+" revised";await api("/api/presets",{presets:records.map(p=>p.id===old.id?{...p,name}:p)});
  t.after(async()=>{const all=await api("/api/presets");await api("/api/presets",{presets:all.map(p=>p.id===old.id?old:p)});});
  const reply=page.waitForResponse(r=>r.url().endsWith(path));await page.locator("[data-pl-apply]").click();assert.equal((await reply).status(),409);
  assert.deepEqual((await api("/api/state")).scene,state.scene);
  await page.keyboard.press("Escape"); await page.locator("#ov-tools").click();await page.locator("#ov-profile-library").click();
  await page.waitForFunction(n=>document.querySelector(".pl-title h3")?.textContent===n,name);
});

test("comparison shows exact named curves and stale points block both Apply and portable export", async t => {
  const presets = await api("/api/presets"), { curves } = await api("/api/curves");
  const a = presets.find(p => p.fan_curve && !p.fan_points?.length);
  const b = presets.find(p => p.fan_curve && !p.fan_points?.length && p.fan_curve !== a.fan_curve);
  const apply = "/api/presets/" + a.id + "/apply", curve = "/api/curves/" + a.fan_curve;
  const before = curves[a.fan_curve], changed = structuredClone(before);
  changed[0][1] = before[0][1] === 20 ? 21 : before[0][1] - 1;
  const page = await open(t, undefined, apply);
  await page.locator('[data-pl-id="' + a.id + '"]').click();
  await page.locator("#pl-compare").selectOption(b.id);
  const format = points => page.evaluate(async ps => {
    const { fmtTemp } = await import("/js/core.js");
    return ps.map(([temp,duty]) => fmtTemp(temp) + " → " + duty + "%").join(" · ");
  }, points);
  const cells = page.getByRole("row", { name: /Curve points/ }).getByRole("cell");
  assert.deepEqual(await cells.allInnerTexts(), [await format(before), await format(curves[b.fan_curve])]);
  const state = await api("/api/state");
  await api(curve, { points: changed, expected_points: before });
  t.after(() => api(curve, { points: before, expected_points: changed }));

  for (const [path,button] of [[apply,"[data-pl-apply]"], ["/api/profiles/export","[data-pl-export]"]]) {
    const reply = page.waitForResponse(r => r.url().endsWith(path) && r.request().method() === "POST");
    await page.locator(button).click();
    const response = await reply;
    assert.deepEqual(response.request().postDataJSON().reviewed_curve_points, before);
    assert.equal(response.status(), 409);
    assert.match((await response.json()).error, /fan curve changed/i);
  }
  assert.deepEqual((await api("/api/state")).scene, state.scene);

  await page.locator(".profile-library [data-close]").click();
  await page.locator("#ov-tools").click(); await page.locator("#ov-profile-library").click();
  const expected = await format(changed);
  await page.waitForFunction(label => Array.from(document.querySelectorAll(".pl-table-body td"))
    .some(cell => cell.textContent === label), expected);
  const download = page.waitForEvent("download");
  await page.locator("[data-pl-export]").click();
  const exported = JSON.parse(readFileSync(await (await download).path(), "utf8"));
  assert.deepEqual(exported.fans.curve, changed);
});

test("a missing named curve is shown and its absence is checked when exporting", async t => {
  const records = await api("/api/presets"), p = records.find(row => row.fan_curve && !row.fan_points?.length);
  const page = await open(t);
  await page.locator(".profile-library [data-close]").click();
  await page.route("**/api/curves", async route => {
    const response = await route.fetch(), data = await response.json();
    delete data.curves[p.fan_curve];
    await route.fulfill({ json: data });
  });
  await page.locator("#ov-tools").click(); await page.locator("#ov-profile-library").click();
  await page.locator('[data-pl-id="' + p.id + '"]').click();
  await page.locator(".pl-impact").filter({ hasText: /unavailable/ }).waitFor();
  await page.locator(".pl-settings summary").click();
  assert.equal(await page.getByRole("row", { name: /Curve points/ }).getByRole("cell").innerText(), "Unavailable");
  // The fixture still has the curve, as if it became available after review.
  const reply = page.waitForResponse(r => r.url().endsWith("/api/profiles/export"));
  await page.locator("[data-pl-export]").click();
  const response = await reply;
  assert.equal(response.request().postDataJSON().reviewed_curve_points, null);
  assert.equal(response.status(), 409);
});

async function rulesFixture(t) {
  const original = await api("/api/automations");
  const id = "library-edit-" + Date.now();
  await api("/api/automations", { rules: [...original, { id, enabled: false, when: { type: "locked" }, then: { type: "lights_off" } }] });
  const profile = { airglow_profile: 1, id: "local-import", name: "Linked " + Date.now(),
    lighting: { effect: "solid", params: {}, brightness: 20 },
    automations: [{ when: { type: "locked" }, then: { type: "preset", preset: "self" } }] };
  t.after(async () => {
    await api("/api/automations", { rules: original });
    const all = await api("/api/presets");
    await api("/api/presets", { presets: all.filter(p => p.name !== profile.name) });
  });
  return { id, profile };
}
async function importRules(page, profile) {
  await page.locator("#pl-file").setInputFiles({ name: "linked.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(profile)) });
  await page.locator("[data-pl-rules]").check();
  await page.locator("[data-pl-install]").click();
  await page.waitForFunction(name => document.querySelector(".pl-title h3")?.textContent === name, profile.name);
  await page.locator("[data-pl-export-rules]").waitFor();
}
function latch() {
  let signal;
  const wait = new Promise(resolve => { signal = resolve; });
  return { wait, signal };
}

test("an import survives queued edits from a stale second window and the writer recovers", async t => {
  const { id, profile } = await rulesFixture(t);
  const importer = await open(t), writer = await browser.newPage();
  t.after(() => writer.close());
  await writer.goto(base + "/#automations");
  const control = writer.locator("#enable-" + id);
  await control.waitFor();
  const started = latch(), release = latch(), replies = [];
  await writer.route("**/api/automations", async route => {
    if (route.request().method() !== "POST") return route.continue();
    started.signal(); await release.wait; await route.continue();
  });
  writer.on("response", r => { if (r.url().endsWith("/api/automations") && r.request().method() === "POST") replies.push(r.status()); });
  const conflict = writer.waitForResponse(r => r.url().endsWith("/api/automations") && r.status() === 409);
  try {
    await control.press("Space"); await started.wait;
    await control.press("Space");
    await importRules(importer, profile);
  } finally { release.signal(); }
  await conflict;
  const importedId = (await api("/api/presets")).find(p => p.name === profile.name).id;
  const importedRule = (await api("/api/automations")).find(r => r.then.preset === importedId);
  assert.ok(importedRule && !importedRule.enabled);
  await writer.locator("#enable-" + importedRule.id).waitFor();
  assert.ok((await writer.locator('[data-id="' + importedRule.id + '"] .sentence').innerText()).includes(profile.name));
  assert.equal(await control.isChecked(), false);
  const saved = writer.waitForResponse(r => r.url().endsWith("/api/automations") && r.request().method() === "POST" && r.ok());
  await control.press("Space"); await saved;
  const rules = await api("/api/automations");
  assert.equal(rules.find(r => r.id === id).enabled, true);
  assert.deepEqual(rules.find(r => r.id === importedRule.id), importedRule);
  assert.deepEqual(replies, [409, 200]);
});

test("a late successful write response cannot replace a subsequently imported list", async t => {
  const { id, profile } = await rulesFixture(t);
  const page = await browser.newPage();
  t.after(() => page.close());
  await page.goto(base + "/#automations");
  const captured = latch(), release = latch();
  await page.route("**/api/automations", async route => {
    if (route.request().method() !== "POST") return route.continue();
    const response = await route.fetch();
    captured.signal(); await release.wait; await route.fulfill({ response });
  });
  try {
    await page.locator("#enable-" + id).press("Space"); await captured.wait;
    await page.locator('[data-tab="overview"]').click();
    await page.locator("#ov-tools").click(); await page.locator("#ov-profile-library").click();
    await importRules(page, profile);
  } finally { release.signal(); }
  await page.locator(".profile-library [data-close]").click();
  await page.locator('[data-tab="automations"]').click();
  const current = await api("/api/automations");
  const added = current.find(r => r.then.preset === (profile.name.toLowerCase().replace(/ /g, "-")));
  assert.ok(added && !added.enabled);
  await page.locator("#enable-" + added.id).waitFor();
  assert.equal(await page.locator("#enable-" + id).isChecked(), true);
  assert.deepEqual(await page.evaluate(async () => (await import("/js/core.js")).S.rules), current);
});
