// Run community effects for 10 seconds each in headless Chromium, with the same runtime Airglow uses
// (web/runtime/worker.js and layers.js), on a made-up rig of 400 LEDs, with the test beat playing.
// Fails if an effect throws, never produces a frame, or averages more than 20 ms a frame.
//
//   npx playwright@1.56.1 install chromium
//   node tools/effects-render.mjs [effects/...html ...]      (no files: every effect)
//
// Writes a Markdown table to $GITHUB_STEP_SUMMARY when it's set.

import { readFileSync, readdirSync, appendFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = process.env.AIRGLOW_ROOT || fileURLToPath(new URL("..", import.meta.url));
const FRAMES = 300, BUDGET_MS = 20;

function allEffects() {
  const dir = join(ROOT, "effects");
  return readdirSync(dir).filter((d) => statSync(join(dir, d)).isDirectory())
    .flatMap((d) => readdirSync(join(dir, d)).filter((f) => f.endsWith(".html")).map((f) => join(dir, d, f)));
}

// the same split the app's DOMParser does, without a DOM: layers JSON, or the script and its controls' defaults
function parse(source) {
  const layers = /<script type="application\/json" id="layers">([\s\S]*?)<\/script>/i.exec(source);
  const code = /<script>([\s\S]*?)<\/script>/i.exec(source);
  const params = {};
  for (const m of source.matchAll(/<meta property="([^"]+)"[^>]*?type="([^"]+)"[^>]*?default="([^"]*)"/g)) {
    params[m[1]] = m[2] === "number" ? +m[3] : m[2] === "boolean" ? m[3] === "true" : m[3];
  }
  return layers ? { kind: "layers", layers: JSON.parse(layers[1].replace(/<\\\//g, "</")), params } : { kind: "code", code: code?.[1] || "", params };
}

const { chromium } = await import("playwright");
const runtime = readFileSync(join(ROOT, "web/runtime/worker.js"), "utf8");
const layersLib = readFileSync(join(ROOT, "web/runtime/layers.js"), "utf8");
const layerInfo = JSON.parse(readFileSync(join(ROOT, "web/runtime/layers.json"), "utf8"));
const defaults = Object.fromEntries(Object.entries(layerInfo.types).map(([id, t]) => [id, Object.fromEntries(t.params.map((p) => [p.id, p.default]))]));
const AREAS = ["front", "top", "rear", "bottom", "gpu", "cooler", "ram", "board"];
const points = Array.from({ length: 400 }, (_, i) => [((i * 37) % 320) + 0.5, ((i * 53) % 200) + 0.5, AREAS[i % AREAS.length]]);

// AIRGLOW_BROWSER=msedge uses an installed Edge instead of Playwright's own Chromium (handy on Windows)
const browser = await chromium.launch(process.env.AIRGLOW_BROWSER ? { channel: process.env.AIRGLOW_BROWSER } : {});
const page = await browser.newPage();
await page.setContent("<!doctype html><title>effects</title>");
const files = process.argv.slice(2).length ? process.argv.slice(2).map((f) => join(ROOT, f)) : allEffects();
const rows = [];
let failed = 0;
for (const file of files) {
  const eff = parse(readFileSync(file, "utf8"));
  const body = eff.kind === "layers" ? `airglow.effect(AirglowLayers.effect(${JSON.stringify(eff.layers)}, ${JSON.stringify(defaults)}));` : eff.code;
  const r = await page.evaluate(async ({ prefix, body, params, points, FRAMES }) => {
    const url = URL.createObjectURL(new Blob([prefix, body], { type: "text/javascript" }));
    const w = new Worker(url);
    const out = { frames: 0, total: 0, worst: 0, error: null, lit: 0 };
    const answer = () => new Promise((res) => {
      const t = setTimeout(() => res({ type: "timeout" }), 3000);
      w.onmessage = (e) => { clearTimeout(t); res(e.data); };
      w.onerror = (e) => { clearTimeout(t); e.preventDefault(); res({ type: "error", message: e.message }); };
    });
    w.postMessage({ type: "start", params, points });
    let m = await answer();
    if (m.type !== "ready") { out.error = m.message || m.type; w.terminate(); return out; }
    for (let i = 0; i < FRAMES; i++) {
      const time = i / 30, beat = 0.5;
      const kick = Math.exp(-(time % beat) * 9);
      const bands = Array.from({ length: 64 }, (_, k) => Math.max(0, kick * (1 - k / 30)) + 0.5 * Math.exp(-((k / 63 - (time * 0.35) % 1) ** 2) / 0.004));
      w.postMessage({ type: "tick", time, dt: 1 / 30, audio: { level: 0.5 * kick, bass: kick, beat: time % beat < 1 / 30, bands } });
      m = await answer();
      if (m.type !== "frame") { out.error = m.message || (m.type === "timeout" ? "stopped answering for 3 seconds" : m.type); break; }
      out.frames++; out.total += m.ms; out.worst = Math.max(out.worst, m.ms);
      if (i === FRAMES - 1) for (let k = 0; k < m.colors.length; k += 3) if (m.colors[k] + m.colors[k + 1] + m.colors[k + 2] > 30) out.lit++;
    }
    w.terminate();
    return out;
  }, { prefix: `${runtime}\n${layersLib}\n`, body, params: eff.params, points, FRAMES });
  const avg = r.frames ? r.total / r.frames : 0;
  const ok = !r.error && r.frames === FRAMES && avg <= BUDGET_MS;
  if (!ok) failed++;
  if (!r.error && avg > BUDGET_MS) r.error = `averages ${avg.toFixed(1)} ms a frame; Airglow pauses effects over ${BUDGET_MS} ms`;
  rows.push(`| \`${relative(ROOT, file).replaceAll("\\", "/")}\` | ${ok ? "ok" : "**failed**"} | ${avg.toFixed(2)} ms | ${r.worst.toFixed(1)} ms | ${r.lit}/400 | ${r.error ? r.error.replaceAll("|", "/") : ""} |`);
}
await browser.close();
const table = ["| Effect | Result | Average frame | Slowest | LEDs lit at the end | Problem |", "|---|---|---|---|---|---|", ...rows].join("\n");
console.log(table);
if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, `## Effects running for 10 seconds\n\n${table}\n`);
process.exit(failed ? 1 : 0);
