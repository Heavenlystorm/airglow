import test from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { api, rigPage, root, until } from "./isolated-browser-rig.mjs";

const output = process.env.AIRGLOW_COMPANIONS_OUTPUT || join(root,"output/experience-upgrade/overview-companions");
const collection = JSON.parse(readFileSync(join(root,"web/data/lookbook.json"),"utf8"));
const workspace = { id:"main", name:"Everyday", tiles:[{id:"profiles",type:"presets",w:2,h:1}], pins:[] };
const reset = () => api("/api/overview",{tiles:workspace.tiles,pins:[],active_workspace:"main",workspaces:[workspace]});
const readLayout = () => api("/api/overview");
const press = async button => { await button.focus(); await button.press("Enter"); };
const add = async (page, kind) => {
  await page.locator("#ov-addtile").click();
  await page.locator('[data-ctl="' + kind + '"]').click();
  await page.locator(".tile-" + kind).waitFor();
};

test("optional cards can be added, named, resized, browsed, opened, exported and imported", async t => {
  await reset(); mkdirSync(output, {recursive:true});
  const page = await rigPage(t,"overview"), posts = [];
  page.on("request", r => {if (r.method()==="POST") posts.push(new URL(r.url()).pathname);});
  assert.equal(await page.locator(".tile-lookbook,.tile-desk-display").count(), 0);
  await page.locator("#ov-edit").click();
  await add(page,"lookbook"); await add(page,"desk-display");
  const card = page.locator(".tile-lookbook"), desk = page.locator(".tile-desk-display");
  await card.locator('[data-act="edit"]').click();
  await page.getByLabel("Tile name").fill("Evening looks");
  await page.locator("#ts-form button.btn.primary").click();
  await card.focus(); await card.press("Shift+ArrowDown");
  await page.locator("#ov-edit").click();
  const forward = card.getByRole("button",{name:"Next lighting recipe"});
  const back = card.getByRole("button",{name:"Previous lighting recipe"});
  for (const item of collection) {
    const recipe = card.getByRole("button",{name:"Open recipe " + item.profile.name});
    await recipe.waitFor(); assert.equal(await card.locator(".ov-recipe-colors i").count(),item.colors.length);
    const swatches = await card.locator(".ov-recipe-colors i").evaluateAll(els=>els.map(el=>[...el.style.backgroundColor.matchAll(/\d+/g)].map(n=>Number(n[0]).toString(16).padStart(2,"0")).join("")));
    assert.deepEqual(swatches,item.colors.map(c=>c.slice(1)));
    await press(forward); assert.ok(await forward.evaluate(el=>el===document.activeElement));
  }
  await press(back); await press(back);
  const chosen = collection.at(-2);
  await press(card.getByRole("button",{name:"Open recipe " + chosen.profile.name}));
  await page.locator(".lb-cover").getByRole("heading",{name:chosen.profile.name,exact:true}).waitFor();
  await page.keyboard.press("Escape");
  const saved = await until(readLayout, d=>d.tiles.find(z=>z.type==="lookbook")?.recipe===chosen.tile);
  assert.equal(saved.tiles.find(z=>z.type==="lookbook").label,"Evening looks");
  assert.equal(saved.tiles.find(z=>z.type==="lookbook").h,2);
  await page.reload();
  await card.getByRole("button",{name:"Open recipe " + chosen.profile.name}).waitFor();
  assert.equal(await card.locator(".tile-name").textContent(),"Evening looks");
  await page.getByRole("button",{name:"Browse profile library"}).click(); await page.locator(".profile-library").waitFor();
  await page.keyboard.press("Escape");
  await page.getByRole("button",{name:"Manage workspaces"}).click();
  const downloading = page.waitForEvent("download"); await page.locator("#ov-workspace-export").click();
  const doc = JSON.parse(readFileSync(await (await downloading).path(),"utf8")); assert.deepEqual(doc.tiles,saved.tiles);
  await page.locator("#ov-workspace-file").setInputFiles({name:"custom.json",mimeType:"application/json",buffer:Buffer.from(JSON.stringify({...doc,name:"Imported desk"}))});
  const imported = await until(readLayout,d=>d.active_workspace==="imported-desk");
  assert.deepEqual(imported.tiles,saved.tiles); assert.deepEqual(imported.workspaces[0],saved.workspaces[0]);
  await page.locator("#ov-grid").screenshot({path:join(output,"companion-cards-desktop.png")});
  await page.setViewportSize({width:320,height:690});
  await page.evaluate(async ()=>(await import("/js/appearance.js")).applyAppearance({scale:120,motion:"reduced"},{persist:false}));
  await page.locator("#ov-edit").click(); await card.focus(); await card.press("Shift+ArrowUp"); await card.press("Shift+ArrowLeft");
  await desk.focus(); await desk.press("Shift+ArrowLeft"); await page.locator("#ov-edit").click();
  for (const tile of [desk,card]) {
    const hidden = await tile.evaluate(el=>{
      const b=el.getBoundingClientRect();
      return [...el.querySelectorAll(".tile-inner button:not([hidden])")].filter(c=>{
        const r=c.getBoundingClientRect();return r.width>0&&(r.left<b.left-1||r.right>b.right+1||r.bottom>b.bottom+1||r.top<b.top-1);
      }).map(c=>c.outerHTML.slice(0,100));
    });
    assert.deepEqual(hidden,[]);
  }
  await card.screenshot({path:join(output,"lookbook-card-compact.png")});
  await desk.screenshot({path:join(output,"desk-card-compact.png")});
  await page.emulateMedia({forcedColors:"active"});
  await press(forward); await card.getByRole("button",{name:"Open recipe " + collection.at(-1).profile.name}).waitFor();
  assert.ok(posts.every(p=>p==="/api/overview" || p.startsWith("/api/studio/preview/")),posts.join(", "));
});

test("Desk card follows the companion's timer across windows without writing it or querying the display", async t => {
  await reset(); await api("/api/overview",{...workspace,tiles:[{id:"desk",type:"desk-display",w:2,h:1}]});
  const page=await rigPage(t,"overview"), tile=page.locator(".tile-desk-display");
  await tile.locator(".ov-desk-time").waitFor(); assert.ok(await tile.locator(".ov-desk-time").textContent());
  const traffic=[];
  page.on("request", r=>{const path=new URL(r.url()).pathname;if(path.startsWith("/api/desk-display"))traffic.push([r.method(),path]);});
  const [other]=await Promise.all([page.waitForEvent("popup"),page.evaluate(()=>{window.open("/desk.html","_blank");})]);
  await other.locator("#desk-timer-open").click(); await other.getByRole("button",{name:"15 min",exact:true}).click();
  await other.getByRole("button",{name:"Start focus timer"}).click();
  await until(()=>tile.locator(".ov-desk-status").textContent(),s=>s==="Focus · 15 min left");
  await other.locator("#desk-timer-open").click(); await other.locator("#desk-pause").click();
  await until(()=>tile.locator(".ov-desk-status").textContent(),s=>s==="Paused · 15 min left");
  await other.evaluate(async () => {const {DESK_TIMER_KEY}=await import("/js/desk-state.js");localStorage.setItem(DESK_TIMER_KEY,JSON.stringify({mode:"running",minutes:15,deadline:Date.now()-1000}));});
  await until(()=>tile.locator(".ov-desk-status").textContent(),s=>s==="Focus complete");
  assert.deepEqual(traffic,[]);
  await press(tile.getByRole("button",{name:"Open Desk Display"})); await page.waitForURL("**/desk.html");
  await page.getByRole("button",{name:"Timer finished"}).waitFor();
  assert.equal(traffic.filter(([method])=>method==="POST").length,1);
  await page.locator("#desk-timer-open").click(); await page.locator("#desk-end").click();
  await page.goBack(); await tile.waitFor();
  await until(()=>tile.locator(".ov-desk-status").textContent(),s=>s && !s.includes("Focus") && !s.includes("Paused"));
});

test("a full workspace keeps every existing tile when Add a tile is opened", async t => {
  const tiles = Array.from({length:32},(_,i)=>({id:"reading-"+i,type:"sensor",sensors:["auto/cpu_temp"],style:"spark",w:1,h:1}));
  await api("/api/overview",{tiles,pins:[],active_workspace:"full",workspaces:[{id:"full",name:"Full workspace",tiles,pins:[]}]});
  const page=await rigPage(t,"overview"), writes=[];
  page.on("request",r=>{if(r.method()==="POST")writes.push(r.url());});
  await page.locator("#ov-edit").click(); await page.locator("#ov-addtile").click();
  await page.getByText("This workspace has 32 tiles. Remove a tile to add another.",{exact:true}).waitFor();
  assert.equal(await page.locator("#sheet[open]").count(),0);
  assert.equal((await readLayout()).tiles.length,32); assert.deepEqual(writes,[]);
});
