// An opt-in, timed usage check on an owned demo server. Run with node --test tools/check-lighting-usage.mjs.
import test from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { rigPage, root } from "./isolated-browser-rig.mjs";

const duration = Math.max(5, Number(process.env.AIRGLOW_USAGE_SECONDS) || 180);
const out = process.env.AIRGLOW_USAGE_OUTPUT || join(root, "output/experience-upgrade/usage");
mkdirSync(out, { recursive: true });
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
const config = [[320,568,120],[390,844,120],[1280,840,100]];
const tick = page => page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));

test("repeated keyboard use, reduced motion, modal disposal and narrow layouts on an isolated rig", { timeout: (duration + 90) * 1000 }, async t => {
  const page = await rigPage(t, "lighting"), cdp = await page.context().newCDPSession(page);
  await cdp.send("Performance.enable");
  const start = Date.now(), findings = new Map(), writes = [], samples = [], layouts = new Map();
  let cycles = 0, hiddenFrames = 0, hiddenRepaints = 0, heldChanges = 0, reloads = 0, checkedFocus = 0;
  let studioOpens = 0, studioHidden = false, hiddenStudioTicks = 0;
  page.setDefaultTimeout(5000);
  page.on("request", r => {
    const p = new URL(r.url()).pathname;
    if (studioHidden && p === "/api/studio/preview/tick") hiddenStudioTicks++;
    if (r.method() === "POST" && !p.startsWith("/api/studio/preview/")) writes.push(p);
  });
  async function instrument() {
    await page.evaluate(() => {
      window.usage = { refs: [], paints: 0, long: [] };
      const clear = CanvasRenderingContext2D.prototype.clearRect;
      CanvasRenderingContext2D.prototype.clearRect = function(...args) {
        if (this.canvas.matches("[data-emitters]")) window.usage.paints++;
        return clear.apply(this,args);
      };
      new PerformanceObserver(list => { window.usage.long.push(...list.getEntries().map(e => Math.round(e.duration))); })
        .observe({ type: "longtask" });
    });
  }
  await instrument();
  async function openVia(control) { await control.focus(); await control.press("Enter"); }
  function issue(key, detail) { const prior = findings.get(key); findings.set(key, { count: (prior?.count || 0) + 1, detail }); }
  async function layout(name, locator, tag) {
    const data = await locator.evaluate(el => {
      const r = el.getBoundingClientRect();
      const width = innerWidth, overflow = Math.round(el.scrollWidth - el.clientWidth);
      const offscreen = [...el.querySelectorAll("button,input,select,a,summary")].filter(c => {
        const b = c.getBoundingClientRect(); return b.width && b.height && (b.right > width + 2 || b.left < -2);
      }).slice(0,6).map(c => c.id || c.getAttribute("aria-label") || c.textContent.trim().slice(0,35));
      const wide = overflow > 2 ? [...el.querySelectorAll("*")].filter(c => c.getBoundingClientRect().right > r.right + 2).slice(0,8).map(c=>c.id || c.className?.baseVal || String(c.className) || c.tagName) : [];
      return { width, overflow, right: Math.ceil(r.right), left: Math.floor(r.left), offscreen, wide };
    });
    const id = tag + "/" + name;
    layouts.set(id, data);
    if (data.overflow > 2 || data.right > data.width + 2 || data.left < -2) issue(id + "-overflow",data);
    if (data.offscreen.length) issue(id + "-controls", data.offscreen);
  }
  async function focus(name, selector) {
    checkedFocus++;
    if (!(await page.evaluate(s => document.activeElement?.matches(s), selector))) {
      issue("focus/" + name, await page.evaluate(() => document.activeElement?.outerHTML.slice(0,170)));
    }
  }
  async function sample(label) {
    await cdp.send("HeapProfiler.collectGarbage");
    const [{ nodes, jsEventListeners, documents }, {metrics}, web, {result: importListeners}] = await Promise.all([
      cdp.send("Memory.getDOMCounters"), cdp.send("Performance.getMetrics"), page.evaluate(() => ({
        detachedTargets: window.usage.refs.filter(r => r.deref() && !r.deref().isConnected).length,
        liveCanvases: document.querySelectorAll("canvas").length,
        liveElements: document.getElementsByTagName("*").length,
        longTasks: window.usage.long.length, worstTaskMs: Math.max(0, ...window.usage.long),
      })), cdp.send("Runtime.evaluate",{includeCommandLineAPI:true,returnByValue:true,
        expression:'["st-pack-file","st-import"].map(id=>{const el=document.getElementById(id);return el?(getEventListeners(el).change||[]).length:0})'}),
    ]);
    const m = Object.fromEntries(metrics.map(row => [row.name,row.value]));
    const s = { label, cycles, seconds: Math.round((Date.now()-start)/1000), documents,nodes,listeners:jsEventListeners,
      heapMiB: +(m.JSHeapUsedSize/1048576).toFixed(2), taskSeconds: +m.TaskDuration.toFixed(3), studioImportListeners: importListeners.value, ...web };
    samples.push(s); console.log(JSON.stringify(s));
  }
  async function mode(index) {
    const [w,h,scale] = config[index];
    await page.setViewportSize({width:w,height:h});
    await page.evaluate(async scale => (await import("/js/appearance.js")).applyAppearance({scale,motion:"reduced"},{persist:false}),scale);
    return w + "@" + scale;
  }
  const report = () => ({ durationSeconds: +((Date.now()-start)/1000).toFixed(2), cycles, reloads, checkedFocus,
    hiddenFrames, hiddenRepaints, heldChanges, studioOpens, hiddenStudioTicks, writes: [...new Set(writes)], samples, findings: Object.fromEntries(findings), layouts:Object.fromEntries(layouts) });
  try {
    await page.getByRole("button",{name:"Edit colour",exact:true}).waitFor();
    await sample("initial");
    let lastMode = -1, tag;
    while ((Date.now()-start) < duration * 1000 || cycles < 3) {
      const index = cycles % 3;
      if (index !== lastMode) { tag = await mode(index); lastMode = index; }
      const lighting = page.locator('[data-view="lighting"]');
      await layout("lighting",lighting,tag);
      await openVia(page.locator("#arrange"));
      const panel = page.locator(".arrange-panel");
      await layout("arrange",panel,tag);
      const pick = panel.locator(".arrange-picks input:enabled").first();
      await pick.focus(); await pick.press("Space");
      await openVia(panel.getByRole("button",{name:"Nudge right"}));
      await page.keyboard.press("Control+z"); await page.keyboard.press("Escape");
      if (await panel.isVisible()) await openVia(panel.getByRole("button",{name:"Cancel"}));
      await focus("arrange","#arrange");
      await openVia(page.getByRole("button",{name:"Edit colour",exact:true}));
      await layout("manual-colour", page.locator("#sheet"),tag);
      await openVia(page.getByRole("button",{name:"From a photo…"}));
      const photo = page.locator(".photo-palette");
      await photo.getByText("Firefly dusk · Built-in sample").waitFor();
      await layout("photo",page.locator("#sheet"),tag);
      await photo.evaluate(el=>window.usage.refs.push(new WeakRef(el),new WeakRef(el.querySelector("canvas"))));
      await photo.getByRole("radio",{checked:true}).focus(); await page.keyboard.press("ArrowRight");
      const pixel = photo.locator(".photo-image"); await pixel.focus(); await pixel.press("ArrowRight"); await pixel.press("Enter");
      await page.keyboard.press("Escape"); await focus("photo","[data-photo-open]");
      await page.keyboard.press("Escape"); await focus("manual-colour","[data-edit-colour]");
      await openVia(page.locator("#stage-theatre"));
      const theatre = page.locator(".light-theatre"), screen = theatre.locator("[data-emitters]");
      await theatre.getByText("Frame held",{exact:true}).waitFor();
      await tick(page); await layout("theatre",page.locator("#sheet"),tag);
      await theatre.evaluate(el=>window.usage.refs.push(new WeakRef(el),new WeakRef(el.querySelector("canvas"))));
      const before = await screen.evaluate(c => c.toDataURL());
      await page.evaluate(async () => {
        const { S, emit } = await import("/js/core.js");
        emit("frames:live",{zones:Object.fromEntries(S.zones.zones.map(z=>[z.key,"ff0055".repeat(z.positions.length)]))});
      });
      await tick(page); if (before !== await screen.evaluate(c => c.toDataURL())) heldChanges++;
      await openVia(theatre.getByRole("button",{name:"Capture frame"}));
      const capture = page.locator(".theatre-capture");
      await layout("capture",page.locator("#sheet"),tag); await layout("capture-controls",capture.locator(".capture-tools"),tag);
      await capture.evaluate(el=>window.usage.refs.push(new WeakRef(el),new WeakRef(el.querySelector("canvas"))));
      await page.keyboard.press("Shift+Tab"); await focus("capture-last","[data-save]");
      await page.keyboard.press("Tab"); await focus("capture-first","[data-back]");
      await page.keyboard.press("Escape"); await focus("capture-back","[data-capture]");
      await openVia(theatre.getByRole("button",{name:"Resume live"})); await tick(page);
      const n = await page.evaluate(async () => {
        const { S,emit } = await import("/js/core.js"), start = window.usage.paints;
        Object.defineProperty(document,"hidden",{configurable:true,value:true});
        const zones = Object.fromEntries(S.zones.zones.map(z=>[z.key,"578ddd".repeat(z.positions.length)]));
        for(let i=0;i<16;i++) emit("frames:live",{zones});
        await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
        delete document.hidden;
        return window.usage.paints-start;
      });
      hiddenFrames += 16; hiddenRepaints += n;
      await page.keyboard.press("Escape"); await focus("theatre","#stage-theatre");
      await page.keyboard.press("Control+3");
      const studio = page.locator('[data-view="studio"]');
      await studio.locator(".st-home").waitFor(); await layout("studio-home",studio,tag);
      await openVia(studio.locator('[data-st="learn"]').first()); await page.locator("#sheet[open]").waitFor();
      await layout("studio-guide",page.locator("#sheet"),tag);
      await page.keyboard.press("Escape");
      if (cycles % 4 === 0) {
        const draft = studio.locator('[data-draft="resume"]').first();
        await openVia(await draft.count() ? draft : studio.locator('[data-st="new"]')); const editor = studio.locator(".st-edit"); await editor.waitFor();
        studioOpens++;
        if (!await editor.evaluate(el => el.contains(document.activeElement))) issue("focus/studio-editor", "New effect left focus outside the editor");
        await layout("studio-editor",studio,tag);
        const playback = editor.locator('[data-dbg="pause"]');
        await tick(page);
        if (await playback.getAttribute("aria-pressed") !== "true") issue("reduced/studio-preview", "Preview starts playing with system and app reduced motion");
        await page.evaluate(() => {
          Object.defineProperties(document,{hidden:{value:true,configurable:true},visibilityState:{value:"hidden",configurable:true}});
          document.dispatchEvent(new Event("visibilitychange"));
        });
        studioHidden = true; await pause(230); studioHidden = false;
        await page.evaluate(() => { delete document.hidden; delete document.visibilityState; document.dispatchEvent(new Event("visibilitychange")); });
        await openVia(editor.getByRole("link",{name:"Back to the Studio"})); await studio.locator(".st-home").waitFor();
      }
      await page.keyboard.press("Control+2"); await lighting.waitFor(); await tick(page);
      cycles++;
      if (cycles === 1 || cycles % 8 === 0) await sample("after-use");
      if (!reloads && Date.now()-start > duration*610) {
        await sample("before-reload"); await page.reload({waitUntil:"domcontentloaded"}); await instrument();
        await page.getByRole("button",{name:"Edit colour",exact:true}).waitFor();
        reloads++; await sample("after-reload");
      }
      await pause(100);
    }
    await sample("final");
    assert.equal(hiddenRepaints,0); assert.equal(heldChanges,0); assert.equal(writes.length,0);
    assert.equal(hiddenStudioTicks,0); assert.deepEqual(Object.fromEntries(findings),{});
    assert.equal(samples.at(-1).detachedTargets,0);
  } finally { writeFileSync(join(out,"lighting-usage.json"), JSON.stringify(report(),null,2)); }
});
