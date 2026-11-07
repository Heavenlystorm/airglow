import assert from 'node:assert/strict';
import {before, after, test} from 'node:test';
import {mkdirSync} from 'node:fs';
import {join, isAbsolute} from 'node:path';
import {pathToFileURL} from 'node:url';
const BASE='http://127.0.0.1:8790';
const OUT=process.env.AIRGLOW_CANVAS_OUTPUT || 'output/overview-lighting-canvas/screenshots';
mkdirSync(OUT,{recursive:true});
let browser, original, settings;
const api=async(path,body)=>{
  const r=await fetch(BASE+path,body===undefined?{}:{method:'POST',headers:{'Content-Type':'application/json','X-Airglow':'1'},body:JSON.stringify(body)});
  const data=await r.json(); assert.ok(r.ok,JSON.stringify(data));return data;
};
const eventually=async(read, accept)=>{
  for(let n=0;n<80;n++){const value=await read();if(accept(value))return value;await new Promise(r=>setTimeout(r,100));}
  assert.fail('Expected state was not reached');
};
before(async()=>{
  settings=(await api('/api/settings')).settings;
  assert.equal(settings.lhm_url,'http://127.0.0.1:8791','Only the isolated fixture may be changed');
  original=await api('/api/overview'); await api('/api/onboarded',{});
  await api('/api/settings',{online_checks:false});
  let pw;const name=process.env.AIRGLOW_PLAYWRIGHT_MODULE;
  if(name)pw=await import(isAbsolute(name)?pathToFileURL(name).href:name);
  else try{pw=await import('playwright');}catch{pw=await import(pathToFileURL(join(process.env.APPDATA,'npm/node_modules/playwright/index.mjs')).href);}
  browser=await pw.chromium.launch({channel:process.env.AIRGLOW_BROWSER || 'msedge'});
});
after(async()=>{
  await browser?.close();
  if(original)await api('/api/overview',{...original,workspaces:original.workspaces||[],active_workspace:original.active_workspace||null});
  if(settings)await api('/api/settings',{design:settings.design,appearance:settings.appearance,online_checks:settings.online_checks});
});
async function pageFor(t,width=1440,height=960){
  const page=await browser.newPage({viewport:{width,height},reducedMotion:'reduce'}),errors=[];
  page.on('pageerror',e=>errors.push(e.message));page.setDefaultTimeout(10000);
  t.after(async()=>{await page.close();assert.deepEqual(errors,[],'No page exceptions');});
  await page.goto(BASE+'/#overview');await page.locator('#ov-grid .tile').first().waitFor();
  await page.evaluate(()=>document.fonts.ready);return page;
}
async function starter(page,kind,name){
  await page.locator('#ov-workspace-manage').click();await page.locator('#ov-workspace-new').click();
  await page.locator('#ov-workspace-name').fill(name);await page.locator('#ov-workspace-starter').selectOption(kind);
  await page.locator('#ov-workspace-form button.btn.primary').click();
  await eventually(()=>api('/api/overview'),v=>v.workspaces?.some(w=>w.name===name));
}
async function fits(page){
  const sizes=await page.evaluate(()=>({width:innerWidth,scroll:document.documentElement.scrollWidth}));
  assert.ok(sizes.scroll<=sizes.width+1,JSON.stringify(sizes));
  const boxes=await page.locator('.overview-rig-host, .lt-bri, .lt-on').evaluateAll(els=>els.map(el=>({w:el.getBoundingClientRect().width,h:el.getBoundingClientRect().height})));
  assert.ok(boxes.every(b=>b.w>0&&b.h>0),'Rig and direct controls remain visible');
}
test('showcase, presentation, undo and 3D persist without lighting writes',async t=>{
  const page=await pageFor(t),writes=[];
  page.on('request',r=>{if(r.method()==='POST')writes.push(new URL(r.url()).pathname);});
  await starter(page,'showcase','Canvas test');
  const saved=await api('/api/overview');
  assert.equal(saved.tiles.find(t=>t.type==='lighting').w,3);
  assert.ok(saved.tiles.filter(t=>t.type==='sensor').every(t=>t.style==='auto'));
  await page.locator('.overview-rig-view[aria-label="3D lighting view"]').click();
  await page.locator('.overview-rig [id$="-canvas"]').waitFor();
  await eventually(()=>api('/api/overview'),v=>v.tiles.find(t=>t.type==='lighting').view==='3d');
  await page.locator('#ov-edit').click();
  await page.locator('.tile-lighting [data-act="edit"]').click();
  await page.locator('#ts-emphasis').selectOption('quiet');
  await page.locator('[data-accent="#df93ae"]').click();
  await page.locator('#ts-form button.btn.primary').click();
  await eventually(()=>api('/api/overview'),v=>v.tiles.find(t=>t.type==='lighting').appearance?.accent==='#df93ae');
  await page.locator('#ov-undo').click();
  await eventually(()=>api('/api/overview'),v=>v.tiles.find(t=>t.type==='lighting').appearance?.emphasis==='focus');
  await page.locator('#ov-redo').click();
  await eventually(()=>api('/api/overview'),v=>v.tiles.find(t=>t.type==='lighting').appearance?.emphasis==='quiet');
  await page.locator('.tile-lighting [data-act="edit"]').click();
  await page.locator('[data-accent="#86c8c1"]').click();await page.keyboard.press('Escape');
  await page.locator('.tile-lighting [data-act="edit"]').click();
  await page.locator('[data-accent="#b4baff"]').click();await page.keyboard.press('Escape');
  await page.locator('#ov-undo').click();
  await eventually(()=>api('/api/overview'),v=>v.tiles.find(t=>t.type==='lighting').appearance?.accent==='#86c8c1');
  await page.locator('#ov-edit').click();
  await page.reload();await page.locator('.overview-rig [id$="-canvas"]').waitFor();
  assert.equal(await page.locator('.tile-lighting').getAttribute('data-emphasis'),'quiet');
  await page.locator('.overview-rig-view[aria-label="2D lighting view"]').click();
  await page.locator('.overview-rig-canvas').waitFor({state:'visible'});
  const choices=await page.locator('.overview-rig-zones option').evaluateAll(els=>els.map(e=>e.value).filter(Boolean));
  assert.ok(choices.length);await page.locator('.overview-rig-zones').selectOption(choices[0]);
  await page.locator('[data-view="lighting"]').waitFor({state:'visible'});
  assert.equal(await page.locator('#stage-svg .zone.selected').getAttribute('data-key'),choices[0]);
  await page.locator('#zone-done').waitFor({state:'visible'});
  assert.ok(writes.every(p=>p==='/api/overview'),'Presentation and preview cannot apply a scene, profile or fan change');
});
test('adaptive canvas fits themes, viewport sizes and 120% scale without layout writes',async t=>{
  const page=await pageFor(t);await starter(page,'balanced','Balanced test');
  const before=await api('/api/overview');
  for(const [design,width,height,scale] of [['graphite',1440,960,100],['darkroom',1440,960,100],['paper',1440,960,100],['graphite',2560,1440,100],['graphite',700,900,100],['graphite',390,844,100],['graphite',390,844,120]]){
    await api('/api/settings',{design,appearance:{...settings.appearance,scale,motion:'reduced'}});
    await page.setViewportSize({width,height});await page.reload();await page.locator('.s-auto').first().waitFor();
    await page.evaluate(()=>document.fonts.ready);await fits(page);
    await page.screenshot({path:join(OUT,`${design}-${width}-${scale}.png`),fullPage:true});
  }
  assert.deepEqual(await api('/api/overview'),before,'Viewport/theme changes preserve the exact saved layout');
});
test('large instruments expose comparison/history and missing values stay honest',async t=>{
  const layout=await api('/api/overview');const tile=layout.tiles.find(t=>t.type==='sensor');
  tile.style='auto';tile.w=2;tile.h=3;tile.sensors=['auto/cpu_temp','auto/gpu_temp'];
  await api('/api/overview',layout);await api('/api/settings',{design:'graphite',appearance:{...settings.appearance,scale:100,motion:'reduced'}});
  const page=await pageFor(t),sensor=page.locator(`.tile[data-id="${tile.id}"]`);
  await sensor.locator('.instrument-compare').waitFor({state:'visible'});
  assert.ok(await sensor.locator('.threshold-caption').textContent());
  await page.evaluate(async()=>{const {R}=await import('/js/readings.js');const {renderOverview}=await import('/js/overview.js');R.stale=true;renderOverview();});
  assert.equal(await sensor.locator('.readout .num').textContent(),'–');
  assert.match(await sensor.locator('.tile-note').textContent(),/reconnect/);
});

test('configured fixture fan geometry fills the canvas and snap guides track explicit edits',async t=>{
  const page=await pageFor(t);await starter(page,'showcase','Showcase fixture');
  // A representative, browser-only hardware fixture; these images are not a hardware test.
  await page.route('**/api/stream?*',()=>{});
  await page.reload();await page.locator('.overview-rig-view[aria-label="2D lighting view"]').click();await page.locator('.overview-rig-canvas').waitFor();
  await page.evaluate(async()=>{
    const {S,emit}=await import('/js/core.js');
    const colors=['78d9d3','9bbfff','db9eb7','e7c794'];
    S.zones={...S.zones,zones:S.zones.zones.map(z=>z.key.startsWith('nzxt-hub/')?{...z,device:{...z.device,online:true}}:z)};
    emit('zones');emit('frames:live',{zones:Object.fromEntries(S.zones.zones.map((z,n)=>[z.key,z.positions.map((_,i)=>colors[(n+Math.floor(i/6))%colors.length]).join('')]))});
  });
  await page.evaluate(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));
  await page.screenshot({path:join(OUT,'showcase-fixture-2d.png')});
  await page.locator('.overview-rig-view[aria-label="3D lighting view"]').click();
  await page.locator('.overview-rig [id$="-canvas"]').waitFor();
  await page.evaluate(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));
  await page.screenshot({path:join(OUT,'showcase-fixture-3d.png')});
  await page.locator('#ov-edit').click();
  const tile=page.locator('.tile-lighting'),handle=await tile.locator('[data-act="resize"]').boundingBox();
  await page.mouse.move(handle.x+handle.width/2,handle.y+handle.height/2);await page.mouse.down();
  await page.mouse.move(handle.x-200,handle.y-180,{steps:8});
  await page.locator('#ov-snap-guide').waitFor({state:'visible'});
  assert.match(await page.locator('#ov-snap-guide').textContent(),/\d × \d/);
  await page.mouse.up();await page.locator('#ov-snap-guide').waitFor({state:'detached'});
  await eventually(()=>page.locator('#ov-workspace-status').textContent(),v=>v==='Saved');
});
