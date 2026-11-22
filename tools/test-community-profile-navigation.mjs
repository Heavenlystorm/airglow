import { before, after, test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { openFixtureBrowser } from "./fixture-browser.mjs";

const base=process.env.AIRGLOW_FIXTURE_URL || "http://127.0.0.1:8790";
const read=name=>JSON.parse(readFileSync(new URL("../profiles/"+name+".json",import.meta.url),"utf8"));
const listing={profiles:read("index").profiles,featured:read("featured")};
const gate=()=>{let release;const wait=new Promise(r=>{release=r});return {wait,release};};
let browser;
before(async()=>{
  browser=await openFixtureBrowser(base);
  for (const [path,body] of [["/api/onboarded",{}],["/api/settings",{online_checks:false}]]) {
    const response=await fetch(base+path,{method:"POST",headers:{"X-Airglow":"1","Content-Type":"application/json"},body:JSON.stringify(body)});
    assert.ok(response.ok);
  }
});
after(async()=>{await browser?.close();});

async function ready(t){
  const page=await browser.newPage({viewport:{width:1150,height:810},reducedMotion:"reduce"}), errors=[];
  page.on("pageerror",e=>errors.push(e.message));
  t.after(async()=>{await page.close();assert.deepEqual(errors,[]);});
  await page.goto(base+"/#lighting");await page.locator("#browse-profiles").waitFor();
  return page;
}

test("a delayed community listing cannot reopen itself over the saved profile library",async t=>{
  const page=await ready(t), requested=gate(), proceed=gate();
  await page.route(base+"/api/gallery",async route=>{requested.release();await proceed.wait;await route.fulfill({json:listing});});
  await page.locator("#browse-profiles").click();await requested.wait;
  await page.locator(".profile-gallery [data-close]").click();
  await page.locator(".lighting-view [data-profile-library]").click();await page.locator(".profile-library").waitFor();
  const replied=page.waitForResponse(r=>r.url().endsWith("/api/gallery"));proceed.release();await replied;
  await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
  assert.equal(await page.locator(".profile-library").isVisible(),true);
  assert.equal(await page.locator(".profile-gallery").count(),0);
});

test("search, filters and keyboard picks keep the preview attached to the visible community profile",async t=>{
  const page=await ready(t), delayed=gate(), release=gate();let picked=0;
  await page.route(base+"/api/gallery",r=>r.fulfill({json:listing}));
  await page.route(url=>url.pathname==="/api/gallery/profile",async route=>{
    const id=new URL(route.request().url()).searchParams.get("id");
    if(++picked===1){delayed.release();await release.wait;}
    await route.fulfill({json:read(id)});
  });
  await page.locator("#browse-profiles").click();await delayed.wait;
  await page.locator("#gl-search").fill("neon");
  await page.locator("#gl-detail h2").waitFor();assert.equal(await page.locator("#gl-detail h2").innerText(),"Neon Dusk");
  release.release();await page.locator("#gl-search").fill("");
  await page.locator('[role="option"][aria-selected="true"]').focus();await page.keyboard.press("Home");
  await page.waitForFunction(()=>document.querySelector("#gl-detail h2")?.textContent==="Ember");
  await page.locator('[data-filter="fans"]').click();
  await page.waitForFunction(()=>document.querySelector("#gl-detail h2")?.textContent==="Heat Map");
  assert.equal(await page.locator('[role="option"][aria-selected="true"]').innerText().then(s=>s.includes("Heat Map")),true);
  await page.locator("#gl-search").fill("not-an-existing-profile");
  assert.equal(await page.locator("#gl-try").count(),0);assert.equal(await page.locator(".gl-card").count(),0);
  await page.keyboard.press("Tab");await page.keyboard.press("Escape");
});

test("an offline community gallery offers the installed lighting collection without another network attempt",async t=>{
  const page=await ready(t); let requests=0;
  await page.route(base+"/api/gallery",r=>{requests++;return r.fulfill({status:503,json:{error:"The community gallery is offline."}});});
  await page.locator("#browse-profiles").click();await page.locator("#gl-offline").click();
  await page.locator(".lb-card").first().waitFor();assert.equal(await page.locator(".lb-card").count(),6);assert.equal(requests,1);
});
