// Guided light setup: for each header, which lights blink, what's plugged in, how many LEDs (counted by eye when the
// product doesn't say) and where it sits in the case. Built-in lights (graphics card, board, RAM, keyboard) only
// need the last step. Answers become ordinary zone settings, so Arrange and the Devices tab keep working as before.

import { $, $$, S, emit, esc, fail, get, post } from "./core.js";
import { closeSheet, openSheet } from "./sheet.js";
import { placing } from "./stage.js";

const G = { data: null, tasks: [], queue: [], task: null, step: null, back: [], a: {}, probe: null, timer: null, done: 0 };
const PROBE_RENEW = 5000;          // the server drops a probe after 15 s without news, so the lights never stay white

// ------------------------------------------------------------ drawn icons
function ringDots(cx, cy, r, n, size = 1.5) {
  return Array.from({ length: n }, (_, i) => {
    const a = (i / n) * 2 * Math.PI - Math.PI / 2;
    return `<circle cx="${(cx + r * Math.cos(a)).toFixed(1)}" cy="${(cy + r * Math.sin(a)).toFixed(1)}" r="${size}"/>`;
  }).join("");
}
function rowDots(x0, x1, y, n, size = 1.5) {
  return Array.from({ length: n }, (_, i) => `<circle cx="${(x0 + ((x1 - x0) * i) / Math.max(n - 1, 1)).toFixed(1)}" cy="${y}" r="${size}"/>`).join("");
}
export function icon(kind) {
  const body = {
    fan: `<rect x="4" y="4" width="32" height="32" rx="4" class="i-line"/><circle cx="20" cy="20" r="12" class="i-line"/><circle cx="20" cy="20" r="4" class="i-line"/><g class="i-led">${ringDots(20, 20, 12, 12)}</g>`,
    ring: `<circle cx="20" cy="20" r="14" class="i-line"/><circle cx="20" cy="20" r="7" class="i-fill"/><g class="i-led">${ringDots(20, 20, 10.5, 10)}</g>`,
    strip: `<rect x="3" y="15" width="34" height="10" rx="2" class="i-line"/><g class="i-led">${rowDots(7, 33, 20, 7)}</g>`,
    single: `<rect x="12" y="12" width="16" height="16" rx="3" class="i-line"/><g class="i-led"><circle cx="20" cy="20" r="3"/></g>`,
    gpu: `<rect x="2" y="11" width="36" height="18" rx="2" class="i-line"/><circle cx="11" cy="20" r="5.5" class="i-line"/><circle cx="29" cy="20" r="5.5" class="i-line"/><g class="i-led">${rowDots(6, 34, 8, 6, 1.2)}</g>`,
    keyboard: `<rect x="2" y="11" width="36" height="18" rx="2" class="i-line"/><g class="i-led">${[14.5, 19.5, 24.5].map((y) => rowDots(6, 34, y, 8, 1.1)).join("")}</g>`,
    board: `<rect x="6" y="5" width="28" height="30" rx="2" class="i-line"/><rect x="12" y="10" width="10" height="10" class="i-line"/><g class="i-led">${rowDots(10, 30, 30, 5, 1.2)}</g>`,
  }[kind] || "";
  return `<svg class="su-ico" viewBox="0 0 40 40" aria-hidden="true">${body}</svg>`;
}

function deviceIcon(dev, zones) {
  if (dev.type === "Graphics card") return icon("gpu");
  if (dev.type === "Keyboard" || zones.some((z) => z.zone.matrix)) return icon("keyboard");
  if (dev.type === "Motherboard lighting" || /board/i.test(dev.type)) return icon("board");
  return icon(zones.length > 1 ? "strip" : "single");
}

// ------------------------------------------------------------ probing (the lights you're looking at)
function probe(keys, mode, n = 0) {
  stopProbe(keys);
  G.probe = { keys, mode, n };
  const send = () => G.probe && Promise.all(G.probe.keys.map((zone) => post("/api/setup/probe", { zone, mode: G.probe.mode, n: G.probe.n }))).catch(() => {});
  send();
  G.timer = setInterval(send, PROBE_RENEW);
}
let countTimer = null;
function probeCount(n) {                      // slider moves: send at most every 60 ms
  if (!G.probe) return;
  G.probe.n = n;
  clearTimeout(countTimer);
  countTimer = setTimeout(() => post("/api/setup/probe", { zone: G.probe.keys[0], mode: "count", n }).catch(() => {}), 60);
}
function stopProbe(except = []) {
  clearInterval(G.timer);
  G.timer = null;
  const p = G.probe;
  G.probe = null;
  if (p) for (const zone of p.keys) if (!except.includes(zone)) post("/api/setup/probe", { zone, mode: "off" }).catch(() => {});
}

// ------------------------------------------------------------ what there is to set up
function buildTasks() {
  const byDev = new Map();
  for (const z of S.zones?.zones || []) {
    if (!z.device.online) continue;
    if (!byDev.has(z.device.id)) byDev.set(z.device.id, { dev: z.device, zones: [] });
    byDev.get(z.device.id).zones.push(z);
  }
  const pend = new Set(G.data.pending);
  const tasks = [];
  for (const { dev, zones } of byDev.values()) {
    for (const z of zones.filter((x) => x.zone.resizable)) tasks.push({ type: "header", dev, zones: [z], keys: [z.key], pending: pend.has(z.key) });
    const fixed = zones.filter((x) => !x.zone.resizable);
    if (fixed.length) tasks.push({ type: "fixed", dev, zones: fixed, keys: fixed.map((x) => x.key), pending: fixed.some((x) => pend.has(x.key)) });
  }
  return tasks;
}

const product = (id) => G.data.catalog.find((p) => p.id === id);
const spotLabel = (id) => G.data.spots.find((s) => s.id === id)?.label || "";
const total = (L) => (L.shape === "fans" ? L.fans * L.leds_per_fan : L.leds);

function taskTitle(t) {
  return t.type === "header" ? t.zones[0].zone.name : t.zones.length === 1 && t.zones[0].zone.name !== t.dev.name ? t.zones[0].zone.name : "Built-in lighting";
}

const leds = (n) => `${n} LED${n === 1 ? "" : "s"}`;

// "set": answered in the guide or saved by hand; "off": nothing there; "new": still to do
function taskState(t) {
  const c = t.zones[0].config;
  if (t.pending) return "new";
  if (t.type === "header" && !c.enabled && !c.component?.id) return "off";
  return "set";
}

function taskStatus(t) {
  const z = t.zones[0], c = z.config;
  if (t.type === "fixed") {
    const spot = c.component?.spot;
    return spot ? spotLabel(spot) : t.pending ? "Where does it sit?" : "Placed by hand";
  }
  if (c.component?.id) {
    const p = product(c.component.id);
    return `${p ? p.name : "Set up"}${c.component.count > 1 ? ` ×${c.component.count}` : ""}, ${leds(total(c.layout))} · ${spotLabel(c.component.spot)}`;
  }
  if (!c.enabled) return c.component || z.zone.default_leds === 0 ? "Nothing plugged in" : "Switched off";
  return t.pending ? `${leds(total(c.layout))} guessed, not set up yet` : `${leds(total(c.layout))}, set by hand`;
}

// ------------------------------------------------------------ frame
function topHTML() {
  const t = G.task;
  const steps = t.type === "header" ? ["blink", "what", "count", "leds", "spot"].filter((s) => s !== "count" || G.a.needsCount).filter((s) => s !== "leds" || G.a.needsLeds) : ["spot"];
  const at = steps.indexOf(G.step);
  const pos = G.queue.length + 1;
  const left = G.total > 1 ? `<span>Light ${G.total - pos + 1} of ${G.total}</span>` : "";
  return `<div class="su-top"><span class="su-where">${deviceIcon(t.dev, t.zones)}<span><b>${esc(t.dev.name)}</b>${t.type === "header" ? ` › ${esc(t.zones[0].zone.name)}` : ""}</span></span>
      ${left}<ol class="su-steps" aria-label="Steps">${steps.map((s, i) => `<li class="${i < at ? "done" : i === at ? "now" : ""}"></li>`).join("")}</ol>
      <button class="icon-btn" data-close aria-label="Close"><svg aria-hidden="true"><use href="#i-close"/></svg></button></div>`;
}

function frame({ title, lede = "", body = "", back = true, primary = "", secondary = "" }) {
  openSheet(`<div class="body wide setup">
    ${topHTML()}
    <div><h2>${title}</h2>${lede ? `<p class="lede" style="margin-top:6px">${lede}</p>` : ""}</div>
    ${body}
    <div class="actions">${back ? `<button class="btn quiet" id="su-back">Back</button>` : ""}<span class="spacer"></span>${secondary}${primary}</div>
  </div>`);
  $("#su-back")?.addEventListener("click", goBack);
  ($("#sheet [data-autofocus]") || $("#sheet .btn.primary"))?.focus();
}

function go(step, push = true) {
  if (push && G.step) G.back.push(G.step);
  G.step = step;
  ({ blink: stepBlink, what: stepWhat, count: stepCount, leds: stepLeds, spot: stepSpot })[step]();
}
function goBack() {
  const prev = G.back.pop();
  if (prev) go(prev, false);
  else home();
}

// ------------------------------------------------------------ home: everything, set up or not
function home() {
  if (G.docked) undock();
  stopProbe();
  G.task = null; G.step = null; G.back = [];
  const tasks = (G.tasks = buildTasks());
  const groups = new Map();
  for (const t of tasks) {
    if (!groups.has(t.dev.id)) groups.set(t.dev.id, []);
    groups.get(t.dev.id).push(t);
  }
  const rows = [...groups.values()].map((ts) => `<section class="su-group"><h3>${esc(ts[0].dev.name)}</h3>
    ${ts.map((t) => `<div class="su-row ${taskState(t)}">
      ${t.type === "header" ? icon(product(t.zones[0].config.component?.id)?.kind || (t.zones[0].config.layout.shape === "fans" ? "fan" : "strip")) : deviceIcon(t.dev, t.zones)}
      <div class="su-row-text"><strong>${esc(taskTitle(t))}</strong><span>${esc(taskStatus(t))}</span></div>
      ${{ new: `<span class="tag new">New</span>`, set: `<svg class="su-ok" aria-label="Set up"><use href="#i-check"/></svg>`, off: "" }[taskState(t)]}
      <button class="btn" data-task="${tasks.indexOf(t)}">${{ new: "Set up", set: "Change", off: "Set up" }[taskState(t)]}</button></div>`).join("")}</section>`).join("");
  const pending = tasks.filter((t) => t.pending);
  openSheet(`<div class="body wide setup">
    <div class="su-top"><span class="su-where"><b>Set up your lights</b></span><button class="icon-btn" data-close aria-label="Close"><svg aria-hidden="true"><use href="#i-close"/></svg></button></div>
    <p class="lede" style="margin:0">Tell Airglow what's plugged in where, and effects sweep across your case the right way round. Each light blinks while you set it up, so you always know which one you're looking at.</p>
    <div class="su-list">${rows || `<p class="hint">No lighting is connected right now.</p>`}</div>
    ${coolerNote()}
    <div class="actions"><button class="btn quiet" data-close>${G.done || !pending.length ? "Done" : "Not now"}</button><span class="spacer"></span>
      <button class="btn primary" id="su-run"${pending.length ? "" : " disabled"}>${pending.length ? `Set up ${pending.length} new light${pending.length === 1 ? "" : "s"}` : "Everything is set up"}</button></div>
  </div>`);
  $$("#sheet [data-task]").forEach((b) => b.addEventListener("click", () => run([tasks[+b.dataset.task]])));
  $("#su-run")?.addEventListener("click", () => run(pending));
  ($("#su-run:not(:disabled)") || $("#sheet [data-task]"))?.focus();
}

// USB-connected AIO coolers are named; one whose pump ring hangs off an ARGB header can't be seen by software.
function coolerNote() {
  const found = G.data.coolers || [];
  if (found.length) {
    const fans = found.some((c) => c.lhm) ? " Its pump and fans can follow Airglow: pick them on the Fans tab." : "";
    return `<div class="su-cooler">${icon("ring")}<p><b>${found.map((c) => esc(`${c.brand} ${c.name}`)).join(", ")}</b> found over USB.
      Its lights run through its own USB connection, which Airglow doesn't drive yet; OpenRGB may.${fans}</p></div>`;
  }
  return `<div class="su-cooler">${icon("ring")}<p><b>No USB cooler found.</b> If your CPU cooler or AIO pump has lights, they're on one of the headers above:
    pick <b>Cooler or pump ring</b> there and count its LEDs.</p></div>`;
}

function run(list) {
  G.queue = [...list];
  G.total = list.length;
  next();
}

function next() {
  stopProbe();
  G.task = G.queue.shift();
  if (!G.task) { finished(); return; }
  G.back = []; G.step = null;
  const c = G.task.zones[0].config;
  G.a = { component: c.component?.id || null, count: c.component?.count || 1, leds: total(c.layout), spot: c.component?.spot || null };
  if (G.task.type === "fixed") { probe(G.task.keys, "blink"); go("spot"); }
  else go("blink");
}

function finished() {
  stopProbe();
  G.done += G.total;
  openSheet(`<div class="body wide setup su-done">
    <div class="su-burst" aria-hidden="true">${icon("fan")}</div>
    <h2>Your lights are set up</h2>
    <p class="lede" style="margin:0">Effects now run across your case as it really is. You can fine-tune any position with <b>Arrange</b> on the Lighting tab, and come back here from Devices whenever you add something.</p>
    <div class="actions"><button class="btn quiet" id="su-list">Back to the list</button><span class="spacer"></span><button class="btn primary" id="su-show">Show me</button></div>
  </div>`);
  $("#su-list").addEventListener("click", home);
  $("#su-show").addEventListener("click", () => { closeSheet(); location.hash = "#lighting"; });
  $("#su-show").focus();
}

// ------------------------------------------------------------ steps
function stepBlink() {
  probe(G.task.keys, "blink");
  frame({
    title: "Which lights are blinking?",
    lede: `Everything plugged into <b>${esc(G.task.zones[0].zone.name)}</b> is flashing white right now. Have a look inside your case.`,
    body: `<div class="su-blink" aria-hidden="true">${"<i></i>".repeat(9)}</div>
      <div class="su-nothing" hidden><p class="hint" style="margin:0">If nothing lights up, either nothing is plugged into this header, or its cable isn't seated.</p>
        <div class="btn-row"><button class="btn" id="su-off">Nothing is plugged in here</button><button class="btn quiet" id="su-skip">Skip it for now</button></div></div>`,
    back: false,
    secondary: `<button class="btn quiet" id="su-none">Nothing's blinking</button>`,
    primary: `<button class="btn primary" id="su-seen">I see them</button>`,
  });
  $("#su-seen").addEventListener("click", () => go("what"));
  $("#su-none").addEventListener("click", () => { $("#sheet .su-nothing").hidden = false; $("#su-off").focus(); });
  $("#su-off").addEventListener("click", () => save({ off: true }));
  $("#su-skip").addEventListener("click", next);
}

function stepWhat() {
  probe(G.task.keys, "blink");
  const z = G.task.zones[0].zone;
  const items = G.data.catalog.filter((p) => p.connect.includes(z.connector));
  const detected = new Set((z.detected || []).map((d) => d.nzxt_id));
  const found = items.filter((p) => detected.has(p.nzxt_id));
  const counted = items.filter((p) => p.leds == null);
  const listed = items.filter((p) => p.leds != null && !found.includes(p));
  if (!G.a.component && found.length) G.a.component = found[0].id;          // the hub already said what it is
  const facts = (p) => (p.leds == null ? "You count the LEDs next" : p.kind === "fan" ? `${p.fans > 1 ? `${p.fans} fans, ` : ""}${p.leds} LEDs per fan` : `${p.leds} LED${p.leds === 1 ? "" : "s"}`);
  const btn = (p) => `<button class="su-item" data-pid="${esc(p.id)}" aria-pressed="${G.a.component === p.id}" data-q="${esc(`${p.brand} ${p.name} ${p.hint}`.toLowerCase())}">
    ${icon(p.kind)}<span><strong>${esc(p.name)}</strong><em>${esc(p.hint || [p.brand, facts(p)].filter(Boolean).join(" · "))}</em></span></button>`;
  frame({
    title: "What is it?",
    lede: found.length ? "The hub reported what's connected. Pick it to confirm, or choose something else." : "Pick the product. If it isn't listed, choose the closest type and count its LEDs on the next screen.",
    body: `${listed.length > 6 ? `<input class="text" id="su-q" type="search" placeholder="Search products" aria-label="Search products" data-autofocus>` : ""}
      <div class="su-items">
        ${found.length ? `<h4>Detected</h4>${found.map(btn).join("")}` : ""}
        ${counted.length ? `<h4>${found.length || listed.length ? "Not listed? Count it" : "Choose a type"}</h4>${counted.map(btn).join("")}` : ""}
        ${listed.length ? `<h4>${esc(listed[0].brand || "Products")}</h4>${listed.map(btn).join("")}` : ""}
      </div>`,
    secondary: `<button class="btn quiet" id="su-off">Nothing is plugged in</button>`,
    primary: `<button class="btn primary" id="su-next"${G.a.component ? "" : " disabled"}>Next</button>`,
  });
  const choose = (id) => {
    const p = product(id);
    G.a.component = p.id;
    if (detected.has(p.nzxt_id)) G.a.count = (z.detected || []).filter((d) => d.nzxt_id === p.nzxt_id).length || 1;
    G.a.needsCount = !detected.has(p.nzxt_id) && (p.kind === "fan" || (p.kind === "strip" && p.leds != null));
    G.a.needsLeds = p.leds == null;
    go(G.a.needsCount ? "count" : G.a.needsLeds ? "leds" : "spot");
  };
  $$("#sheet [data-pid]").forEach((b) => b.addEventListener("click", () => choose(b.dataset.pid)));
  $("#su-next").addEventListener("click", () => choose(G.a.component));
  $("#su-off").addEventListener("click", () => save({ off: true }));
  $("#su-q")?.addEventListener("input", (e) => {
    const q = e.target.value.trim().toLowerCase();
    $$("#sheet .su-item").forEach((b) => (b.hidden = q && !b.dataset.q.includes(q)));
  });
}

function stepCount() {
  probe(G.task.keys, "blink");
  const p = product(G.a.component), max = G.task.zones[0].zone.max_leds;
  const unit = p.kind === "fan" ? (p.fans > 1 ? "kits" : "fans") : "strips";
  const fits = (n) => p.leds == null || p.leds * p.fans * n <= max;
  if (!fits(G.a.count)) G.a.count = 1;
  frame({
    title: `How many ${unit}?`,
    lede: `Count the ${esc(p.name)} ${unit} chained together on this header${p.leds == null ? ". You'll count their LEDs next" : ""}.`,
    body: `<div class="seg su-count" role="radiogroup" aria-label="How many">${[1, 2, 3, 4, 5, 6].map((n) => `<button role="radio" aria-checked="${G.a.count === n}" data-n="${n}"${fits(n) ? "" : " disabled"}>${n}</button>`).join("")}</div>`,
    primary: `<button class="btn primary" id="su-next">Next</button>`,
  });
  $$("#sheet [data-n]").forEach((b) => b.addEventListener("click", () => {
    G.a.count = +b.dataset.n;
    $$("#sheet [data-n]").forEach((x) => x.setAttribute("aria-checked", String(x === b)));
  }));
  $("#su-next").addEventListener("click", () => go(G.a.needsLeds ? "leds" : "spot"));
}

function stepLeds() {
  const p = product(G.a.component), max = G.task.zones[0].zone.max_leds;
  const fans = p.kind === "fan" ? p.fans * G.a.count : 1;
  G.a.leds = Math.min(Math.max(G.a.leds || 1, 1), max);
  const what = p.kind === "fan" ? (fans > 1 ? "every fan is lit and the amber LED sits on the last LED of the last fan" : "the whole fan ring is lit and the amber LED sits on its last LED")
    : "the whole strip is lit and the amber LED sits on its very last LED";
  frame({
    title: "Count the LEDs",
    lede: `Move the slider until ${what}. One more and the amber dot disappears off the end.`,
    body: `<div class="su-counter">
        <div class="readout"><span class="num" id="su-num">${G.a.leds}</span><span class="unit">LEDs</span><span class="of" id="su-per"></span></div>
        <div class="su-ctl"><button class="icon-btn" data-d="-1" aria-label="One less">−</button>
          <input type="range" id="su-n" min="1" max="${max}" value="${G.a.leds}" aria-label="LEDs lit" data-autofocus>
          <button class="icon-btn" data-d="1" aria-label="One more">+</button></div>
        <p class="hint">Arrow keys move one LED at a time. This header drives up to ${max}.</p></div>`,
    primary: `<button class="btn primary" id="su-next">That's the last one</button>`,
  });
  const input = $("#su-n");
  const show = () => {
    const n = +input.value;
    G.a.leds = n;
    $("#su-num").textContent = n;
    input.style.setProperty("--fill", `${((n - 1) / Math.max(max - 1, 1)) * 100}%`);
    $("#su-per").textContent = fans > 1 ? (n % fans ? `doesn't split evenly over ${fans} fans yet` : `${n / fans} per fan`) : "";
    probeCount(n);
  };
  probe(G.task.keys, "count", G.a.leds);
  input.addEventListener("input", show);
  $$("#sheet [data-d]").forEach((b) => b.addEventListener("click", () => { input.value = +input.value + +b.dataset.d; show(); }));
  show();
  $("#su-next").addEventListener("click", () => go("spot"));
}

// The last step happens on the real Lighting stage: the light glows there (and blinks in the case), you drag it to
// where it sits, and the guide waits in a panel beside it. Spot buttons snap it close first. Every other light on
// the stage can be dragged too, as in Arrange.
const DEFAULT_SPOT = { fan: "front", ring: "cooler", strip: "bottom", single: "board" };
const DEVICE_SPOT = { "Graphics card": "gpu", Keyboard: "desk", Mouse: "desk", "Mouse mat": "desk", Headset: "desk", "Headset stand": "desk", DRAM: "ram" };

async function snap(spot) {
  const t = G.task;
  G.a.spot = spot;
  if (t.type === "fixed") await post("/api/setup/place", { device: t.dev.id, spot });
  else await post("/api/setup/apply", { zone: t.keys[0], component: G.a.component, count: G.a.count, leds: G.a.leds, spot });
  S.zones = await get("/api/zones");
  emit("zones");
  probe(t.keys, "blink");                  // saving ends the probe; keep the real light blinking while you place it
}

async function stepSpot() {
  const t = G.task, fixed = t.type === "fixed";
  G.a.spot ||= fixed ? DEVICE_SPOT[t.dev.type] || (/board/i.test(t.dev.type) ? "board" : "side") : DEFAULT_SPOT[product(G.a.component).kind];
  try {
    await snap(G.a.spot);
  } catch (e) { fail(e); if (!fixed) goBack(); return; }
  G.docked = true;
  closeSheet();
  if (location.hash !== "#lighting") location.hash = "#lighting";
  placing(t.keys);
  renderDock();
}

function renderDock() {
  const t = G.task, fixed = t.type === "fixed";
  let d = $("#guide-dock");
  if (!d) {
    d = document.createElement("aside");
    d.id = "guide-dock";
    d.className = "guide-dock setup";
    d.setAttribute("aria-label", "Setup guide");
    document.body.append(d);
  }
  const own = fixed && G.tasks.some((x) => x.dev.id === t.dev.id && x.type === "header");
  d.innerHTML = `${topHTML()}
    <div><h2>${fixed ? (own ? "Where are its own lights?" : `Where is the ${esc(t.dev.name)}?`) : "Drag it to where it sits"}</h2>
      <p class="lede">It's glowing on the stage and blinking in your case. Drag it into place, and turn it with the round handle. Or snap it to a spot first:</p></div>
    <div class="su-snaps" role="radiogroup" aria-label="Snap to a spot">${G.data.spots.map((s) => `<button role="radio" data-spot="${s.id}" aria-checked="${G.a.spot === s.id}">${esc(s.label)}</button>`).join("")}</div>
    <p class="hint">Everything else on the stage can be dragged too.</p>
    <div class="actions">${fixed ? "" : `<button class="btn quiet" id="su-back">Back</button>`}<span class="spacer"></span>${fixed ? `<button class="btn quiet" id="su-skip">Skip</button>` : ""}<button class="btn primary" id="su-save">${G.queue.length ? "Save and next" : "Save"}</button></div>`;
  d.hidden = false;
  d.querySelector("[data-close]").addEventListener("click", () => { undock(); stopProbe(); G.task = null; });
  d.querySelectorAll("[data-spot]").forEach((b) => b.addEventListener("click", async () => {
    d.querySelectorAll("[data-spot]").forEach((x) => x.setAttribute("aria-checked", String(x === b)));
    try { await snap(b.dataset.spot); placing(t.keys); } catch (e) { fail(e); }
  }));
  d.querySelector("#su-back")?.addEventListener("click", () => { undock(); goBack(); });
  d.querySelector("#su-skip")?.addEventListener("click", () => { undock(); next(); });
  d.querySelector("#su-save").addEventListener("click", () => {
    undock();
    G.data.pending = G.data.pending.filter((k) => !t.keys.includes(k));
    emit("refresh");
    next();
  });
  d.querySelector("#su-save").focus();
}

function undock() {
  G.docked = false;
  placing(null);
  const d = $("#guide-dock");
  if (d) d.hidden = true;
}

async function save(body) {
  const t = G.task;
  $$("#sheet .actions .btn").forEach((b) => (b.disabled = true));
  try {
    if (t.type === "fixed") await post("/api/setup/place", body);
    else await post("/api/setup/apply", { zone: t.keys[0], ...body });
    G.data.pending = G.data.pending.filter((k) => !t.keys.includes(k));
    S.zones = await get("/api/zones");
    emit("zones");
    emit("refresh");
    next();
  } catch (e) {
    fail(e);
    $$("#sheet .actions .btn").forEach((b) => (b.disabled = false));
  }
}

// ------------------------------------------------------------ entry points
let listening = false;
export async function openSetup({ start = false, only = null } = {}) {
  try {
    [G.data, S.zones] = await Promise.all([get("/api/setup"), get("/api/zones")]);
  } catch (e) { fail(e); return; }
  G.done = 0;
  if (!listening) {
    listening = true;
    $("#sheet").addEventListener("close", () => { if (!G.docked) { stopProbe(); G.task = null; } });
    // leaving the Lighting tab while placing: keep what's saved so far and end the guide
    window.addEventListener("hashchange", () => { if (G.docked && location.hash !== "#lighting") { undock(); stopProbe(); G.task = null; } });
  }
  home();
  if (only) {
    const t = G.tasks.find((x) => x.keys.includes(only));
    if (t) run([t]);
  } else if (start) {
    const pending = G.tasks.filter((t) => t.pending);
    if (pending.length) run(pending);
  }
}
