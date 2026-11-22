import assert from "node:assert/strict";
import { before,after,test } from "node:test";
import { mkdirSync,writeFileSync } from "node:fs";
import { join } from "node:path";
import { openFixtureBrowser } from "./fixture-browser.mjs";
const base=process.env.AIRGLOW_FIXTURE_URL || "http://127.0.0.1:8790";
const out=process.env.AIRGLOW_SPLIT_OUTPUT || "output/split-workspace";
let browser;
before(async()=>{ browser=await openFixtureBrowser(base);mkdirSync(out,{recursive:true}); });
after(async()=>{await browser?.close();});

test("every main section stays reachable at the compact Windows size and 120% interface scale",async()=>{
  // The native minimum is the outer window. Leave room for Windows title bar and borders.
  const page=await browser.newPage({viewport:{width:544,height:398},reducedMotion:"reduce"});
  const errors=[],geometry={};page.on("pageerror",e=>errors.push(e.message));
  try{
    await page.route(base+"/api/settings",async route=>{
      if(route.request().method()!=="GET")return route.continue();
      const response=await route.fetch(), data=await response.json();
      data.settings.design="firefly";
      data.settings.appearance={...data.settings.appearance,scale:120};
      await route.fulfill({json:data});
    });
    await page.goto(base+"/#overview");await page.locator(".tile-sensor").first().waitFor();
    await page.evaluate(async()=>{
      const {applyDesign}=await import("/js/design.js");applyDesign("firefly",{persist:false});
      const {applyAppearance}=await import("/js/appearance.js");applyAppearance({scale:120},{persist:false});
    });
    const tabs=["overview","lighting","studio","fans","automations","devices","settings"];
    for(const tab of tabs){
      await page.locator("#nav-select").selectOption(tab);const view=page.locator('[data-view="'+tab+'"]');await view.waitFor();
      await page.waitForTimeout(160);
      geometry[tab]=await view.evaluate(el=>({ viewport:innerWidth, page:document.documentElement.scrollWidth,scale:document.documentElement.dataset.scale,
        client:el.clientWidth,content:el.scrollWidth,selector:document.querySelector("#nav-select").getBoundingClientRect().toJSON(),
        oversize:[...el.querySelectorAll("button,input,select,summary")].filter(e=>{const r=e.getBoundingClientRect();return r.width && r.left>=innerWidth;}).slice(0,6).map(e=>e.id||e.textContent.trim().slice(0,36)) }));
      assert.ok(geometry[tab].page<=geometry[tab].viewport+1,tab+" document exceeds window");
      assert.equal(geometry[tab].scale,"120",tab+" changed UI scale");
      assert.ok(geometry[tab].selector.width>=44 && geometry[tab].selector.right<=544,tab+" section chooser is obscured");
    }
    await page.keyboard.press("Control+1");await page.locator('[data-view="overview"]').waitFor();
    await page.screenshot({path:join(out,"compact-overview-120.png")});
    await page.locator("#ov-tools").click();await page.locator("#ov-profile-library").click();
    await page.locator("#pl-lookbook").click();await page.locator(".lb-card").first().waitFor();
    assert.ok(await page.locator(".lookbook").evaluate(el=>el.scrollWidth<=el.clientWidth+1));
    await page.screenshot({path:join(out,"compact-lookbook-120.png")});
    assert.deepEqual(errors,[]);
  } finally {writeFileSync(join(out,"compact-routes.json"),JSON.stringify(geometry,null,2));await page.close();}
});

test("companion typography and controls follow all installed interface designs and forced colors",async()=>{
  const page=await browser.newPage({viewport:{width:660,height:440},reducedMotion:"reduce"});
  const errors=[],seen=[];let design="firefly";page.on("pageerror",e=>errors.push(e.message));
  try{
    await page.route(base+"/api/desk-display",async route=>{
      const response=await route.fetch(),data=await response.json();await route.fulfill({json:{...data,design}});
    });
    await page.goto(base+"/desk.html"); const themes=await page.evaluate(async()=>(await import("/js/design.js")).DESIGNS.map(d=>d.id));
    assert.ok(themes.length>=13);
    for(const id of themes){
      design=id;await page.reload();await page.waitForFunction(id=>document.documentElement.dataset.design===id && document.querySelector(".desk-meter"),id);
      const readable=await page.locator("#desk-time").evaluate(el=>{
        const r=el.getBoundingClientRect(),s=getComputedStyle(el);return r.width>35 && r.height>15 && r.right<=innerWidth+1 && s.color!=="rgba(0, 0, 0, 0)";
      });
      assert.ok(readable,id);seen.push(id);
      if(id==="paper" || id==="contrast")await page.screenshot({path:join(out,"desk-"+id+".png")});
    }
    await page.emulateMedia({forcedColors:"active",reducedMotion:"reduce"}); await page.locator("#desk-choose-readings").click();
    await page.locator("#desk-pick-mode").selectOption("desk");await page.locator("#desk-pick-list input").first().waitFor();
    await page.keyboard.press("Escape");await page.waitForFunction(()=>document.activeElement?.id==="desk-choose-readings");
    assert.deepEqual(errors,[]);
  } finally {writeFileSync(join(out,"verified-designs.json"),JSON.stringify(seen));await page.close();}
});
