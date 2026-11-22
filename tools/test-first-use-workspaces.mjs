import assert from "node:assert/strict";
import { test } from "node:test";
import { api, rigPage, until } from "./isolated-browser-rig.mjs";

// Keep the actual application and its bundled sample-rig renderer. Replace device/sensor
// discovery and history with the same empty responses a new, unconfigured rig can return.
async function emptyWorkspace(t) {
  const page = await rigPage(t, "overview");
  const origin = new URL(page.url()).origin;
  await page.goto("about:blank");
  const [baseState, defaults, presentation] = await Promise.all([api("/api/state"), api("/api/overview"), api("/api/settings")]);
  const fixture = {
    catalog: [], catalogRevision: 1, pins: defaults.pins, profiles: await api("/api/presets"), runs: [], active: null,
    fanChannels: [], onboarded: false, writes: [],
  };
  const state = () => ({ ...baseState, devices: [], setup_pending: 0, conflicts: [], readings: {}, catalog_rev: fixture.catalogRevision,
    onboarded: fixture.onboarded, online_asked: true, telemetry_stale: false, fps: 0,
    sensors: { online: fixture.catalog.length > 0, cpu_temp: null, gpu_temp: null, liquid_temp: null },
    fans: { ...baseState.fans, temp:null, target:null, duty:null, ports:[], channels:fixture.fanChannels, hub_online:false, failsafe:true },
    openrgb: { enabled:false, connected:false, installed:false, used:false, status:"Off" } });
  const forbidden = [];
  await page.addInitScript(() => {
    window.EventSource = class extends EventTarget {
      static CLOSED = 2;
      constructor() { super(); this.readyState = 1; window.fixtureStream = this; }
      close() { this.readyState = 2; }
    };
  });
  await page.route("**/api/**", async route => {
    const req = route.request(), url = new URL(req.url()), path = url.pathname;
    const json = body => route.fulfill({json:body});
    if (req.method() === "POST") {
      fixture.writes.push(path);
      if (path === "/api/onboarded") { fixture.onboarded = true; return json({ok:true}); }
      if (path === "/api/desk-display/open") return json({native:false,open:false,pinned:false});
      if (path.startsWith("/api/studio/preview/")) {
        if (path.endsWith("/load")) {
          const zones = req.postDataJSON().effect.zones;
          if (!zones?.length || !zones.every(z => z.key.startsWith("sample/"))) {
            forbidden.push("Attempted preview outside the sample rig");
            return route.fulfill({status:400,json:{error:"Only sample-rig zones are expected."}});
          }
        }
        return route.continue();
      }
      forbidden.push(path); return route.fulfill({status:403,json:{error:"Unexpected mutation during read-only first use."}});
    }
    if (path === "/api/state") return json(state());
    if (path === "/api/tools") return json({tools:[],job:{state:"idle"},installed_any:false});
    if (path === "/api/zones") return json({canvas:{w:1.6,h:1},zones:[]});
    if (path === "/api/sensors") return json({sensors:fixture.catalog,lhm_online:fixture.catalog.length>0,revision:fixture.catalogRevision});
    if (path === "/api/history") return json([]);
    if (path === "/api/sensors/history") return json({t:[],series:{}});
    if (path === "/api/presets") return json(fixture.profiles);
    if (path === "/api/cooling/sessions") return json({active:fixture.active,sessions:fixture.runs,saving:false});
    if (path === "/api/cooling/trace") {
      const run = [fixture.active,...fixture.runs].find(r => r?.id === url.searchParams.get("id")), offset = Number(url.searchParams.get("after") || 0);
      return json({...run,offset,samples:run.samples.slice(offset)});
    }
    if (path === "/api/overview") return json({...defaults,pins:fixture.pins});
    if (path === "/api/desk-display") {
      const pins = url.searchParams.get("source") === "desk" ? url.searchParams.getAll("reading") : fixture.pins;
      return json({design:"firefly",temp_unit:"C",appearance:presentation.settings.appearance,time:Date.now()/1000,times:[],
        window:{native:false,open:false,pinned:false},readings:pins.map(id=>({id,name:id,known:false,unit:"",value:null,samples:[]}))});
    }
    return route.continue();
  });
  t.after(() => assert.deepEqual(forbidden, []));
  await page.goto(origin + "/#overview");
  const welcome = page.locator(".welcome-sheet");
  await welcome.waitFor();
  assert.match(await welcome.innerText(), /No lighting devices found/);
  assert.match(await welcome.innerText(), /temperatures unavailable/);
  assert.doesNotMatch(await welcome.innerText(), /Fan failsafe is active/);
  await welcome.getByRole("button", {name:"Open Overview",exact:true}).click();
  await welcome.waitFor({state:"hidden"});
  return {page, fixture, async stateEvent() {
    await page.evaluate(snapshot => window.fixtureStream.dispatchEvent(new MessageEvent("state", {data:JSON.stringify(snapshot)})), state());
  }};
}

async function tool(page, selector) {
  await page.locator("#ov-tools").press("Enter"); await page.locator(selector).press("Enter");
}

test("a first-time user can keep a clock and timer on an empty desk and return to Overview", async t => {
  const {page,fixture} = await emptyWorkspace(t);
  await tool(page, "[data-desk-display]");
  await page.waitForURL("**/desk.html");
  await page.locator(".desk-meter").first().waitFor();
  assert.deepEqual(await page.locator(".desk-reading strong").allTextContents(), fixture.pins.map(() => "–"));
  assert.ok((await page.locator(".desk-extrema").allTextContents()).every(label => label === "No recent samples"));
  assert.ok((await page.locator(".desk-meter path").evaluateAll(nodes => nodes.map(n => n.getAttribute("d")))).every(d=>d===""));
  assert.equal(await page.locator("#desk-pin").isVisible(), false);
  await page.locator("#desk-timer-open").press("Enter");
  await page.locator("[data-minutes='15']").press("Enter");
  await page.getByRole("button",{name:"Start focus timer"}).press("Enter");
  await page.getByRole("button",{name:"Focus timer running"}).press("Enter");
  await page.locator("#desk-end").press("Enter");
  await page.locator("#desk-choose-readings").press("Enter"); await page.locator("#desk-pick-mode").selectOption("desk");
  while (await page.locator("[data-remove]").count()) await page.locator("[data-remove]").first().press("Enter");
  await page.locator("#desk-pick-save").press("Enter");
  await page.locator("#desk-empty").waitFor();
  assert.match(await page.locator("#desk-empty").innerText(), /Choose display readings/);
  await page.getByRole("button",{name:"Display settings"}).press("Enter");
  await page.locator("#desk-main").press("Enter"); await page.waitForURL("**/#overview");
  assert.ok(fixture.onboarded);
});

test("the native desk waits for its window mode before handling Open Airglow", async t => {
  const {page} = await emptyWorkspace(t);
  const origin = new URL(page.url()).origin;
  const received = Promise.withResolvers(), respond = Promise.withResolvers();
  await page.route("**/api/desk-display", async route => {
    received.resolve(); await respond.promise;
    await route.fulfill({json:{design:"firefly",temp_unit:"C",time:Date.now()/1000,times:[],readings:[],
      window:{native:true,open:true,pinned:false}}});
  });
  let opened = 0;
  await page.route("**/api/window/show", async route => { opened++; await route.fulfill({json:{ok:true}}); });
  await page.goto(origin + "/desk.html");
  await received.promise;
  await page.getByRole("button",{name:"Display settings"}).press("Enter");
  try {
    const main = page.locator("#desk-main");
    if (await main.isEnabled()) await main.press("Enter");
    assert.equal(new URL(page.url()).pathname, "/desk.html", "The companion must never navigate itself to the main app");
  } finally { respond.resolve(); }
  await page.locator("#desk-pin").waitFor();
  if (!opened) await page.locator("#desk-main").press("Enter");
  await until(() => Promise.resolve(opened), n => n === 1);
  assert.equal(new URL(page.url()).pathname, "/desk.html");
});

test("an empty library leads to the sample-rig lookbook without applying its lighting", async t => {
  const {page,fixture} = await emptyWorkspace(t);
  fixture.profiles = [];
  await tool(page, "[data-profile-library]");
  await page.getByRole("heading", {name:"Your saved looks"}).waitFor();
  assert.equal(await page.locator("[data-pl-apply]").count(), 0);
  await page.locator("#pl-lookbook").press("Enter");
  const plate = page.locator(".lb-card").first(); await plate.waitFor();
  assert.equal(await page.locator(".lb-card").count(), 6);
  await plate.press("Enter");
  await page.locator('.lb-bench [data-frame="ready"]').waitFor();
  assert.match(await page.locator("[data-rig-source]").innerText(), /Sample rig.*simulated inputs/i);
  await page.locator("#lb-bright").press("Home");
  await page.waitForFunction(() => document.querySelector('output[for="lb-bright"]')?.textContent === "0%");
  await page.locator('[data-lb-back]').press("Enter");
  assert.equal(await page.evaluate(()=>document.activeElement?.dataset.plate),"0");
  await page.keyboard.press("Escape");
  assert.ok(fixture.writes.some(p => p === "/api/studio/preview/tick"));
});

test("the supplied profiles can be compared on a sample rig before any devices connect", async t => {
  const {page,fixture} = await emptyWorkspace(t);
  await tool(page, "[data-profile-library]");
  const [first,second] = fixture.profiles;
  await page.locator('[data-pl-id="'+first.id+'"]').press("Enter");
  await page.locator("#pl-compare").selectOption(second.id);
  await page.waitForFunction(()=>document.querySelectorAll('.pl-portrait[data-frame="ready"]').length === 2);
  assert.ok((await page.locator(".pl-portraits").innerText()).includes(first.name));
  assert.match(await page.locator("[data-rig-source]").innerText(), /Sample rig.*simulated inputs/i);
  const images = await page.locator(".pl-portraits canvas").evaluateAll(cs=>cs.map(c=>c.toDataURL()));
  assert.notEqual(images[0],images[1]);
});

test("a recording with missing samples stays empty through inspection and export", async t => {
  const {page,fixture} = await emptyWorkspace(t);
  const run = {id:"empty-run",annotation:"No sensors yet",started_at:1,ended_reason:"stopped",duration_seconds:4,
    sample_count:3,sensors:[],markers:[],samples:[0,2,4].map(elapsed=>({elapsed,cpu:null,gpu:null,liquid:null,duty:null,rpm:null}))};
  fixture.runs = [run];
  await tool(page, "[data-telemetry-sessions]");
  await page.locator("[data-session-signal]").selectOption("cpu");
  await page.getByText("No measurements for this reading.", {exact:true}).waitFor();
  assert.equal(await page.locator(".session-line").count(), 0);
  assert.equal(await page.locator("[data-report-save]").isDisabled(), true);
  assert.match(await page.locator("[data-session-stats]").innerText(), /0 \/ 3/);
  await page.locator("#session-time").press("Home");
  assert.match(await page.locator("[data-session-probes]").innerText(), /Missing reading/);
});

test("the open recorder offers a sensor discovered after a first run with no readings", async t => {
  const {page,fixture,stateEvent} = await emptyWorkspace(t);
  await tool(page,"[data-telemetry-sessions]");
  await page.locator(".session-empty").waitFor();
  assert.match(await page.locator(".session-empty").innerText(), /Start a recording/);
  assert.equal(await page.locator("[data-report-tools]").isVisible(), false);
  assert.equal(await page.locator("[data-cooling-start]").isEnabled(), true);
  assert.equal(await page.locator(".session-line").count(), 0);
  await page.locator(".session-picker summary").press("Enter");
  assert.equal(await page.locator("[data-session-add]").isDisabled(), true);
  const id = "cpu/package-power";
  fixture.catalog = [{id,name:"CPU package power",short:"CPU power",group:"Processor",unit:"W",kind:"power",value:15}];
  fixture.catalogRevision++;
  await stateEvent();
  await until(() => page.evaluate(async id => !!(await import("/js/readings.js")).R.meta[id], id), Boolean);
  await until(() => page.locator("#session-catalog option").evaluateAll((options,id)=>options.some(o=>o.value===id),id), Boolean, 5500);
  await page.locator("#session-catalog").selectOption(id);
  await page.locator("[data-session-add]").press("Enter");
  assert.equal(await page.locator("[data-session-remove]").getAttribute("data-session-remove"), id);
  const sensor = fixture.catalog[0];
  fixture.catalog = []; fixture.catalogRevision++; await stateEvent();
  await until(() => page.locator("[data-session-remove]").count(), n => n === 0);
  assert.match(await page.locator("[data-session-picker-note]").innerText(), /CPU package power/);
  fixture.active = {id:"other-window-run", annotation:"Started with package power", started_at:1, ended_reason:null,
    duration_seconds:0,sample_count:0,sensors:[sensor],markers:[],samples:[]};
  await page.locator("[data-cooling-refresh]").press("Enter");
  const recorded = page.locator("[data-session-remove]"); await recorded.waitFor();
  assert.equal(await recorded.getAttribute("data-session-remove"), id);
  assert.equal(await recorded.isDisabled(), true);
});

test("first-use warnings appear for a managed output missing its temperature", async t => {
  const {page,fixture,stateEvent} = await emptyWorkspace(t);
  assert.equal(await page.locator("#fan-alert").isVisible(), false);
  assert.notEqual(await page.locator(".fan-curve").innerText(), "Failsafe");
  fixture.fanChannels = [{id:"/fan/0",name:"Case fan",mode:"curve",pump:false,duty:null,rpm:null,target:30}];
  await stateEvent();
  await page.locator("#fan-alert").waitFor();
  assert.match(await page.locator("#fan-alert").innerText(), /Fan failsafe/);
  await until(() => page.locator(".fan-curve").innerText(), value => value === "Failsafe");
  await page.evaluate(async () => (await import("/js/onboarding.js")).openWelcome());
  await page.getByText("Fan failsafe is active", {exact:true}).waitFor();
});

test("the offline curve sandbox handles missing temperatures and named curves with zero fan outputs", async t => {
  const {page} = await emptyWorkspace(t);
  const requested = Promise.withResolvers(), release = Promise.withResolvers();
  await page.route("**/js/fans.js", async route => { requested.resolve(); await release.promise; await route.continue(); });
  await page.locator('[data-tab="fans"]').press("Enter"); await requested.promise;
  const button = page.locator("#curve-sandbox");
  let openedWhileLoading = false;
  try {
    if (await button.isVisible() && await button.isEnabled() && !await button.evaluate(n=>!!n.closest("[inert]"))) {
      await button.click(); openedWhileLoading = true;
    }
    await page.locator("#view-loading").waitFor();
    assert.match(await page.locator("#view-loading").innerText(), /Loading Fans/i);
  } finally { release.resolve(); }
  await page.locator("#curve-seg [data-c]").first().waitFor();
  if (!openedWhileLoading) await button.click();
  await page.locator(".curve-sandbox").waitFor({timeout:3000});
  await page.locator("[data-sb-draft]").selectOption("silent");
  await page.locator("[data-sb-comparison]").selectOption("balanced");
  await page.getByLabel("Simulated temperature").press("End");
  assert.equal(await page.locator("[data-sb-target]").innerText(),"100%");
  await page.locator("[data-sb-mode]").selectOption("missing");
  assert.ok(await page.getByLabel("Simulated temperature").isDisabled());
  assert.equal(await page.locator("[data-sb-command]").innerText(), "30%");
});

test("a missing first-load module offers a working recovery path to the fan sandbox", async t => {
  const {page} = await emptyWorkspace(t);
  await page.route("**/js/fans.js", route => route.abort(), {times:1});
  await page.locator('[data-tab="fans"]').press("Enter");
  const notice = page.locator("#view-loading");
  await notice.locator("button:visible").waitFor();
  assert.match(await notice.innerText(), /open Fans/i);
  await notice.locator("button").press("Enter");
  await page.locator("#curve-seg [data-c]").first().waitFor();
  assert.equal(await notice.isVisible(), false);
  await page.locator("#curve-sandbox").click();
  await page.locator(".curve-sandbox").waitFor();
});
