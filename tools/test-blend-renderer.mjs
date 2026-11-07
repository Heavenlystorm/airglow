// Real OffscreenCanvas, WebGL and worker.js. No API server or physical lighting is used.
// AIRGLOW_PLAYWRIGHT may point to an installed Playwright module; Windows defaults to installed Edge.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";
import { isAbsolute, join } from "node:path";

const root = fileURLToPath(new URL("..", import.meta.url));
const runtime = readFileSync(new URL("../web/runtime/worker.js", import.meta.url), "utf8");
const layers = readFileSync(new URL("../web/runtime/layers.js", import.meta.url), "utf8");
const solid = (name, color) => ({ name, layers: [{ type: "solid", params: { color } }] });
const point = (name, value) => ({ name, code: `airglow.effect({point(){return ${value}}});` });
const cases = [
  { name: "layers", a: solid("Red", "#ff0000"), b: solid("Blue", "#0000ff") },
  { name: "point conversion", a: point("Point A", "[300.7,-1.9,NaN]"), b: point("Point B", "[20.5,Infinity,120]") },
  { name: "2D alpha", a: { name: "Half red", code: `airglow.effect({render(ctx){const c=ctx.canvas.getContext('2d');c.clearRect(0,0,320,200);c.fillStyle='rgba(255,0,0,0.5)';c.fillRect(0,0,320,200)}});` }, b: point("Blue point", "[0,0,240]") },
  { name: "WebGL alpha", a: { name: "GL green", code: `let gl;airglow.effect({init(ctx){gl=ctx.canvas.getContext('webgl2',{preserveDrawingBuffer:true});if(!gl)throw Error('WebGL2 unavailable');},render(){gl.clearColor(0,1,0,.5);gl.clear(gl.COLOR_BUFFER_BIT);}});` }, b: solid("Red layers", "#ff0000") },
  { name: "frame and params", a: { name: "Stateful", code: `let p,v;airglow.effect({init(ctx){p=ctx.params},frame(f){v=f.time*10+p.offset},point(){return [v,0,0]}});`, params: [{ id: "offset", label: "Offset", type: "number", min: 0, max: 100, step: 1, default: 20 }] }, b: solid("Green", "#00ff00") },
];
const python = String.raw`
import json,sys
from airglow import fxformat,fxblend
out=[]
for case in json.load(sys.stdin):
    a=fxformat.to_html(case['a']); b=fxformat.to_html(case['b'])
    def run(source):
        e=fxformat.validate(source)
        code=e['code'] if e['kind']=='code' else 'airglow.effect(AirglowLayers.effect('+json.dumps({'v':2,'layers':e['layers'],'master':e['master'],'expose':e['expose']})+',{}));'
        return {'code':code,'params':{p['id']:p['default'] for p in e['params']}}
    out.append({'name':case['name'],'a':run(a),'b':run(b),'blend':run(fxblend.build({'a':{'source':a},'b':{'source':b},'name':'Mix','amount':50}))})
print(json.dumps(out))
`;
const generated = spawnSync(process.env.PYTHON || "python", ["-c", python], { cwd: root, input: JSON.stringify(cases), encoding: "utf8" });
assert.equal(generated.status, 0, generated.stderr);
const entries = JSON.parse(generated.stdout);
let pw;
const moduleName = process.env.AIRGLOW_PLAYWRIGHT_MODULE || process.env.AIRGLOW_PLAYWRIGHT;
if (moduleName) pw = await import(isAbsolute(moduleName) ? pathToFileURL(moduleName).href : moduleName);
else { try { pw = await import("playwright"); } catch (error) {
  if (error.code !== "ERR_MODULE_NOT_FOUND") throw error;
  pw = await import(pathToFileURL(join(process.env.APPDATA, "npm/node_modules/playwright/index.mjs")).href);
} }
const { chromium } = pw;
const channel = process.env.AIRGLOW_BROWSER || (process.platform === "win32" ? "msedge" : undefined);
const browser = await chromium.launch(channel ? { channel } : {});
try {
  const page = await browser.newPage();
  await page.setContent("<!doctype html><title>Blend renderer verification</title>");
  const results = await page.evaluate(async ({ prefix, entries }) => {
    async function render(effect, values) {
      const blob = URL.createObjectURL(new Blob([prefix, effect.code], { type: "text/javascript" }));
      const worker = new Worker(blob); URL.revokeObjectURL(blob);
      const errors = [], messages = [], awaiting = [];
      worker.onmessage = event => { if (event.data.type === "log") return; const resolve = awaiting.shift(); if (resolve) resolve(event.data); else messages.push(event.data); };
      worker.onerror = event => { const data = { type: "error", message: event.message }; const resolve = awaiting.shift(); if (resolve) resolve(data); else messages.push(data); };
      async function send(message) {
        const answer = messages.length ? Promise.resolve(messages.shift()) : new Promise(resolve => awaiting.push(resolve));
        worker.postMessage(message);
        let timer;
        const result = await Promise.race([answer, new Promise(resolve => { timer = setTimeout(() => resolve({ type: "timeout" }), 4000); })]);
        clearTimeout(timer); if (result.type === "error" || result.type === "timeout") throw new Error(result.message || "Worker timeout");
        return result;
      }
      try {
        const points = [[160,100,"front",.2,0,"fan"],[0,0,"top",0,0,"strip"],[319,199,"bottom",1,0,"strip"]];
        await send({ type: "start", points, params: { ...effect.params, ...values } });
        const frame = await send({ type: "tick", time: 1.25, dt: 1/30, audio: null, backdrop: true });
        frame.backdrop?.close();
        return { colors: Array.from(frame.colors), ms: frame.ms };
      } finally { worker.terminate(); }
    }
    const results = [];
    for (const item of entries) {
      const a = await render(item.a), b = await render(item.b), mixes = [];
      for (const blend of [0, 25, 50, 75, 100]) mixes.push(await render(item.blend, { blend }));
      results.push({ name: item.name, a, b, mixes });
    }
    return results;
  }, { prefix: runtime + "\n" + layers + "\n", entries });
  for (const result of results) {
    for (const [i, amount] of [0, .25, .5, .75, 1].entries())
      assert.deepEqual(result.mixes[i].colors, result.a.colors.map((a, index) => Math.round(a * (1 - amount) + result.b.colors[index] * amount)), `${result.name}: ${amount}`);
    console.log(`${result.name}: exact endpoints and 25/50/75% mix; max ${Math.max(...result.mixes.map(f => f.ms)).toFixed(2)} ms including backdrop`);
  }
  assert.equal(results[2].a.colors[0], 128, "transparent canvas is composited over black before crossfading");
  assert.ok(results[3].a.colors[1] >= 127 && results[3].a.colors[1] <= 128, "WebGL half alpha is preserved through canvas readback");
} finally { await browser.close(); }
