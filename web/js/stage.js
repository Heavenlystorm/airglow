// The stage: your zones drawn where you placed them, lit with the exact colours the hardware shows.
// LEDs go on a <canvas> (additive glow), interaction (select, arrange, rotate) on an SVG layer above it.

import { $, S, clamp, emit, esc, fail, on, post, refreshZones } from "./core.js";

const U = 1000;                       // canvas units -> viewBox units
const VB_W = 1600, VB_H = 1000;
const VB_M = 48;                      // room around the case: a zone at its very edge still shows whole, glow and all

const st = { selected: null, arranging: false, drag: null, mode: "2d", view: { s: 1, ox: 0, oy: 0 }, frames: {} };
let rig3d = null, rigModule = null, modeRequest = 0;

const canvas = () => $("#stage-canvas");
const svg = () => $("#stage-svg");

// Stay in the established 2D editor until the optional renderer is ready. A late
// import cannot reopen 3D after Arrange, 2D, or setup placement has been requested.
async function setRigMode(mode, focus = false) {
  const request = ++modeRequest;
  const button = $("#stage-view-3d");
  if (mode === "3d") {
    button?.setAttribute("aria-busy", "true");
    try {
      rigModule ||= import("./rig-3d.js").catch(error => { rigModule = null; throw error; });
      const { createRig3D } = await rigModule;
      if (request !== modeRequest) return;
      rig3d ||= createRig3D($("#stage"), {
        onSelect: key => selectZone(key),
        isVisible: () => !$('[data-view="lighting"]').hidden,
        onUnavailable: () => {
          rig3d?.dispose(); rig3d = null; setRigMode("2d");
          fail(new Error("3D drawing became unavailable. Your layout is back in 2D."));
        },
      });
      st.arranging = false; st.drag = null;
      $("#arrange").setAttribute("aria-pressed", "false");
      rig3d.setZones(S.zones?.zones || []); rig3d.setFrames(st.frames); rig3d.select(st.selected);
    } catch (error) {
      if (request !== modeRequest) return;
      mode = "2d";
      fail(new Error(`3D view is unavailable. You can continue in 2D. ${error.message || ""}`));
    } finally { if (request === modeRequest) button?.removeAttribute("aria-busy"); }
  } else button?.removeAttribute("aria-busy");
  st.mode = mode;
  const active = mode === "3d";
  canvas().hidden = active; svg().toggleAttribute("hidden", active);
  $("#stage-hint").hidden = active;
  $("#stage-view-2d")?.setAttribute("aria-pressed", String(!active));
  button?.setAttribute("aria-pressed", String(active));
  $("#stage").classList.toggle("in-3d", active);
  rig3d?.setVisible(active);
  renderOverlay();
  if (active && focus) rig3d.focus();
  if (!active) { computeView(); draw(); }
}

// ------------------------------------------------------------ geometry helpers
function ledRadius(z) {
  const L = z.config.layout;
  if (L.shape === "fans") return clamp((L.size * U / 2) * Math.PI / L.leds_per_fan * 0.42, 3, 11);
  if (L.shape === "matrix") return clamp((L.length * U) / Math.max(L.cols, 1) * 0.32, 2, 8);
  return clamp((L.length * U) / Math.max(L.leds, 1) * 0.36, 3, 9);
}

function fanCentres(z) {
  const L = z.config.layout;
  const a = L.angle * Math.PI / 180, gap = L.size * 1.12;
  return Array.from({ length: L.fans }, (_, f) => {
    const off = (f - (L.fans - 1) / 2) * gap;
    return [(L.x + Math.cos(a) * off) * U, (L.y + Math.sin(a) * off) * U];
  });
}

const framed = () => document.documentElement.dataset.rig !== "minimal";

function matrixCorners(L) {
  const a = L.angle * Math.PI / 180, w = L.length * U / 2, h = w * L.rows / L.cols;
  return [[-w, -h], [w, -h], [w, h], [-w, h]].map(([dx, dy]) => [
    L.x * U + Math.cos(a) * dx - Math.sin(a) * dy, L.y * U + Math.sin(a) * dx + Math.cos(a) * dy,
  ]);
}

function portraitBounds(zones) {
  let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
  const include = (x, y) => { x0 = Math.min(x0, x); y0 = Math.min(y0, y); x1 = Math.max(x1, x); y1 = Math.max(y1, y); };
  for (const z of zones) {
    for (const [x, y] of z.positions) include(x * U, y * U);
    if (z.config.layout.shape === "fans" && z.positions.length) {
      const r = z.config.layout.size * U / 2;
      for (const [x, y] of fanCentres(z)) { include(x - r, y - r); include(x + r, y + r); }
    } else if (z.config.layout.shape === "matrix" && z.positions.length) {
      for (const [x, y] of matrixCorners(z.config.layout)) include(x, y);
    }
  }
  return Number.isFinite(x0) ? { x0, y0, x1, y1 } : null;
}

// A neutral display frame around the user's layout. Its rings and tracks come from configured
// zone geometry, not guessed case models, fan detection or simulated airflow. LEDs are painted
// afterwards with the unchanged live frame colors; interface accents never tint this layer.
function paintPortrait(ctx, zones, { ox, oy, s, w, h }, bounds = portraitBounds(zones)) {
  if (!framed() || !bounds || !s) return;
  ctx.save();
  ctx.globalCompositeOperation = "source-over";
  const margin = Math.max(15, Math.min(32, 45 * s));
  const x = Math.max(8, ox + bounds.x0 * s - margin), y = Math.max(8, oy + bounds.y0 * s - margin);
  const right = Math.min(w - 8, ox + bounds.x1 * s + margin), bottom = Math.min(h - 8, oy + bounds.y1 * s + margin);
  const plate = ctx.createLinearGradient(x, y, right, bottom);
  plate.addColorStop(0, "#20252b"); plate.addColorStop(.42, "#0c1014"); plate.addColorStop(1, "#15191e");
  ctx.fillStyle = plate;
  ctx.strokeStyle = "#41474e"; ctx.lineWidth = 1;
  ctx.beginPath(); ctx.roundRect(x, y, Math.max(1, right - x), Math.max(1, bottom - y), 12); ctx.fill(); ctx.stroke();
  ctx.strokeStyle = "#7c858e"; ctx.lineWidth = 1;
  const tick = Math.min(17, Math.max(6, (right - x) / 12));
  for (const [cx, cy, dx, dy] of [[x, y, 1, 1], [right, y, -1, 1], [x, bottom, 1, -1], [right, bottom, -1, -1]]) {
    ctx.beginPath(); ctx.moveTo(cx + dx * 5, cy + dy * tick); ctx.lineTo(cx + dx * 5, cy + dy * 5); ctx.lineTo(cx + dx * tick, cy + dy * 5); ctx.stroke();
  }
  for (const z of zones) {
    if (!z.positions.length) continue;
    const L = z.config.layout;
    ctx.strokeStyle = "#373d44"; ctx.lineWidth = 1;
    if (L.shape === "fans") {
      const r = L.size * U * s / 2;
      for (const [cx, cy] of fanCentres(z)) {
        const px = ox + cx * s, py = oy + cy * s;
        const disc = ctx.createRadialGradient(px - r * .2, py - r * .3, r * .1, px, py, r);
        disc.addColorStop(0, "#262d34"); disc.addColorStop(.45, "#14191f"); disc.addColorStop(1, "#080c10");
        ctx.fillStyle = disc;
        ctx.beginPath(); ctx.arc(px, py, r, 0, Math.PI * 2); ctx.fill(); ctx.stroke();
        ctx.beginPath(); ctx.arc(px, py, r * .76, 0, Math.PI * 2); ctx.stroke();
        ctx.fillStyle = "#1c2228";
        ctx.beginPath(); ctx.arc(px, py, r * .29, 0, Math.PI * 2); ctx.fill(); ctx.stroke();
      }
    } else if (L.shape === "matrix") {
      ctx.beginPath();
      for (const [i, [px0, py0]] of matrixCorners(L).entries()) {
        const px = ox + px0 * s, py = oy + py0 * s;
        if (!i) ctx.moveTo(px, py); else ctx.lineTo(px, py);
      }
      ctx.closePath(); ctx.fillStyle = "#11171d"; ctx.fill(); ctx.stroke();
    } else {
      ctx.lineWidth = Math.max(4, ledRadius(z) * s * 2.8); ctx.lineCap = "round"; ctx.strokeStyle = "#242a30";
      // A one-LED GPU zone has a nominal strip length too. Use its actual points so it
      // remains a point; drawing that nominal length invents a long unlit bar around it.
      if (z.positions.length === 1) {
        const [px, py] = z.positions[0];
        ctx.fillStyle = "#242a30"; ctx.beginPath(); ctx.arc(ox + px * U * s, oy + py * U * s, Math.max(3, ledRadius(z) * s * 1.4), 0, Math.PI * 2); ctx.fill();
      } else {
        ctx.beginPath();
        z.positions.forEach(([px, py], i) => {
          if (!i) ctx.moveTo(ox + px * U * s, oy + py * U * s);
          else ctx.lineTo(ox + px * U * s, oy + py * U * s);
        });
        ctx.stroke();
      }
    }
  }
  ctx.restore();
}

function computeView() {
  if (st.mode === "3d") return;
  const c = canvas(), r = c.getBoundingClientRect();
  const s = Math.min(r.width / (VB_W + VB_M * 2), r.height / (VB_H + VB_M * 2));
  st.view = { s, ox: (r.width - VB_W * s) / 2, oy: (r.height - VB_H * s) / 2, w: r.width, h: r.height };
  const dpr = window.devicePixelRatio || 1;
  c.width = Math.round(r.width * dpr);
  c.height = Math.round(r.height * dpr);
  c.getContext("2d").setTransform(dpr, 0, 0, dpr, 0, 0);
}

function toScreen(x, y) {
  const v = st.view;
  return [v.ox + x * v.s, v.oy + y * v.s];
}

function svgPoint(e) {
  const p = svg().createSVGPoint();
  p.x = e.clientX; p.y = e.clientY;
  return p.matrixTransform(svg().getScreenCTM().inverse());
}

// ------------------------------------------------------------ SVG overlay (zones, labels, handles)
// Zones packed close together (a graphics card's four LEDs) would print their names on top of each other: a label
// that would overlap an earlier one waits until you point at or select its zone. A label that would sit on another
// zone (fans stacked above each other) goes below its own zone when there's room there. Needs the stage on screen
// and the font loaded to measure, so it runs again when either happens.
const overlaps = (b, o) => b.x < o.x + o.width && o.x < b.x + b.width && b.y < o.y + o.height && o.y < b.y + b.height;

function declutter() {
  const groups = [...svg().querySelectorAll("g.zone")];
  const shapes = groups.map((g) => [...g.querySelectorAll(".ring, .hit:not(circle)")].map((e) => {
    const b = e.getBBox();
    return { x: b.x - 14, y: b.y - 14, width: b.width + 28, height: b.height + 28 };   // the LEDs and their strokes
  }));
  const placed = [];
  groups.forEach((g, i) => {
    const t = g.querySelector("text.label");
    t.classList.remove("clash");
    t.setAttribute("y", t.dataset.y);
    let b = t.getBBox();
    if (!b.width) return;
    const onZone = (b) => shapes.some((list, j) => j !== i && list.some((o) => overlaps(b, o)));
    const onLabel = (b) => placed.some((o) => overlaps(b, o));
    if (onZone(b) && t.dataset.below) {
      t.setAttribute("y", t.dataset.below);
      const below = t.getBBox();
      if (below.y + below.height <= VB_H + VB_M && !onZone(below) && !onLabel(below)) { placed.push(below); return; }
      t.setAttribute("y", t.dataset.y);
    }
    if (onLabel(b)) t.classList.add("clash");
    else placed.push(b);
  });
}

export function renderOverlay() {
  const s = svg();
  s.setAttribute("viewBox", `${-VB_M} ${-VB_M} ${VB_W + VB_M * 2} ${VB_H + VB_M * 2}`);
  const zones = S.zones?.zones || [];
  rig3d?.setZones(zones); rig3d?.select(st.selected);
  const visible = zones.filter((z) => z.device.online && (z.config.enabled || st.arranging));
  const identity = $("#stage-identity");
  if (identity) {
    const selected = visible.find((z) => z.key === st.selected);
    identity.innerHTML = `<b>Living Rig</b><span>${selected ? esc(selected.config.name) : `${visible.length} zone${visible.length === 1 ? "" : "s"}`}<span class="rig-stage-note"> · ${selected ? `${selected.positions.length} LEDs` : "Your layout"}</span></span>`;
  }
  $("#stage-empty").hidden = zones.some((z) => z.device.online && z.config.enabled) || st.arranging;
  let html = "", handles = "";
  for (const z of visible) {
    const L = z.config.layout, on = z.config.enabled && z.positions.length;
    const sel = st.selected === z.key;
    let hit = "", deco = "", lx, ly, below;
    if (L.shape === "fans") {
      const r = L.size * U / 2;
      for (const [cx, cy] of fanCentres(z)) {
        hit += `<circle class="hit" cx="${cx}" cy="${cy}" r="${r + 14}" stroke-width="2"/>`;
        deco += `<circle class="ring" cx="${cx}" cy="${cy}" r="${r}" stroke-width="${Math.max(6, ledRadius(z) * 2.2)}"/>`;
        deco += `<circle class="hub-dot" cx="${cx}" cy="${cy}" r="${r * 0.32}"/>`;
      }
      const c = fanCentres(z);
      lx = c[0][0] - r; ly = Math.min(...c.map((p) => p[1])) - r - 22; below = Math.max(...c.map((p) => p[1])) + r + 40;
    } else if (L.shape === "matrix") {
      const a = L.angle * Math.PI / 180, w = L.length * U / 2, h = w * L.rows / L.cols, ca = Math.cos(a), sa = Math.sin(a);
      const pts = [[-w, -h], [w, -h], [w, h], [-w, h]].map(([ox, oy]) => [L.x * U + ca * ox - sa * oy, L.y * U + sa * ox + ca * oy]);
      hit = `<polygon class="hit" points="${pts.map((p) => p.join(",")).join(" ")}" stroke-width="2" style="fill:${sel ? "rgba(236,235,230,.06)" : "transparent"}"/>`;
      lx = Math.min(...pts.map((p) => p[0])); ly = Math.min(...pts.map((p) => p[1])) - 18; below = Math.max(...pts.map((p) => p[1])) + 36;
    } else {
      const a = L.angle * Math.PI / 180, h = L.length * U / 2;
      const x1 = L.x * U - Math.cos(a) * h, y1 = L.y * U - Math.sin(a) * h, x2 = L.x * U + Math.cos(a) * h, y2 = L.y * U + Math.sin(a) * h;
      hit = `<line class="hit" x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke-width="34" stroke-linecap="round" style="stroke:${sel ? "rgba(236,235,230,.12)" : "transparent"}"/>`;
      lx = Math.min(x1, x2); ly = Math.min(y1, y2) - 24; below = Math.max(y1, y2) + 42;
    }
    let handle = "";
    if (sel && st.arranging) {
      const [cx, cy] = [L.x * U, L.y * U];
      const a = (L.angle - 90) * Math.PI / 180, d = (L.shape === "fans" ? L.size * U * 0.5 * Math.max(1, L.fans * 0.6) : L.length * U * 0.5) + 60;
      const hx = cx + Math.cos(a) * d, hy = cy + Math.sin(a) * d;
      handle = `<line class="rot-line" x1="${cx}" y1="${cy}" x2="${hx}" y2="${hy}"/><circle class="rot" data-rot="${esc(z.key)}" cx="${hx}" cy="${hy}" r="11"/>`;
    }
    const placingThis = !!st.placing?.includes(z.key);
    const aria = `${z.config.name}${on ? "" : " (off)"}${z.device?.name ? `, ${z.device.name}` : ""}`;
    html += `<g class="zone${on ? "" : " off"}${sel || placingThis ? " selected" : ""}${placingThis ? " placing" : ""}" data-key="${esc(z.key)}" tabindex="0" role="button" aria-label="${esc(aria)}" aria-pressed="${sel}">${deco}${hit}
      <text class="label" x="${lx}" y="${ly}" data-y="${ly}" data-below="${below}">${esc(z.config.name)}${on ? "" : " (off)"}</text></g>`;
    handles += handle;
  }
  s.innerHTML = html + handles;             // handles must stay above every zone's transparent hit area
  declutter();
  $("#stage").classList.toggle("arranging", st.arranging);
  $("#stage-hint").textContent = st.arranging
    ? "Drag zones to where they sit in your case. Use the handle to rotate."
    : (visible.length ? "Select a zone to give it its own effect. Arrange shows switched-off zones too." : "");
}

// ------------------------------------------------------------ canvas (LEDs)
// A glow is a radial gradient of one colour at one radius, and those repeat: most scenes show a handful of colours
// across every LED. Built at (0, 0) they're position-independent, so they can be reused from LED to LED and frame to
// frame instead of allocating one per LED (150+ of them, 15 times a second, on the Overview alone).
const glows = new WeakMap();             // canvas context -> Map(key -> CanvasGradient)
const GLOW_MAX = 400;                    // a rainbow at high zoom can fill this: then start again
function glowFor(ctx, hex, r, peak = 0.55, spread = 4.2) {
  let cache = glows.get(ctx);
  if (!cache) glows.set(ctx, (cache = new Map()));
  const key = `${hex}|${Math.round(r * 2)}|${peak}|${spread}`;
  let g = cache.get(key);
  if (g) return g;
  const R = parseInt(hex.slice(0, 2), 16), G = parseInt(hex.slice(2, 4), 16), B = parseInt(hex.slice(4, 6), 16);
  g = ctx.createRadialGradient(0, 0, 0, 0, 0, r * spread);
  g.addColorStop(0, `rgba(${R},${G},${B},${peak})`);   // the LED's own brightness is the globalAlpha
  g.addColorStop(0.35, `rgba(${R},${G},${B},${peak * 0.29})`);
  g.addColorStop(1, `rgba(${R},${G},${B},0)`);
  if (cache.size >= GLOW_MAX) {
    // Drop the oldest quarter, not everything: a rainbow refills the map within two frames, and rebuilding a
    // gradient per LED is exactly what this cache exists to avoid.
    for (const k of cache.keys()) {
      cache.delete(k);
      if (cache.size < GLOW_MAX * 0.75) break;
    }
  }
  cache.set(key, g);
  return g;
}

function draw() {
  if (st.mode === "3d" || document.hidden || $('[data-view="lighting"]').hidden) return;
  const c = canvas();
  const ctx = c.getContext("2d");
  const v = st.view;
  ctx.globalCompositeOperation = "source-over";
  ctx.clearRect(0, 0, v.w, v.h);
  const zones = (S.zones?.zones || []).filter((z) => z.device.online && (z.config.enabled || st.arranging));
  const drag = st.drag;
  // During a move the original frame stays stationary, keeping the layout's reference point clear.
  paintPortrait(ctx, zones, v);
  // dark "unlit LED" dots first, then additive light
  for (const pass of [0, 1]) {
    ctx.globalCompositeOperation = pass ? "lighter" : "source-over";
    for (const z of zones) {
      const hex = st.frames[z.key];
      const r = ledRadius(z) * v.s;
      let dx = 0, dy = 0, rot = null;
      if (drag && (drag.keys || [drag.key]).includes(z.key)) {
        if (drag.mode === "move") { dx = drag.dx; dy = drag.dy; }
        else rot = drag;
      }
      for (let i = 0; i < z.positions.length; i++) {
        let [px, py] = z.positions[i];
        px *= U; py *= U;
        if (rot) {
          const cx = z.config.layout.x * U, cy = z.config.layout.y * U, a = rot.delta * Math.PI / 180;
          const ox = px - cx, oy = py - cy;
          px = cx + ox * Math.cos(a) - oy * Math.sin(a);
          py = cy + ox * Math.sin(a) + oy * Math.cos(a);
        }
        const [x, y] = toScreen(px + dx, py + dy);
        if (!pass) {
          ctx.fillStyle = z.config.enabled ? "#1e1e20" : "#141415";
          ctx.beginPath(); ctx.arc(x, y, r, 0, Math.PI * 2); ctx.fill();
          continue;
        }
        if (!hex || !z.config.enabled) continue;
        const col = hex.substr(i * 6, 6);
        const R = parseInt(col.slice(0, 2), 16), G = parseInt(col.slice(2, 4), 16), B = parseInt(col.slice(4, 6), 16);
        const k = Math.max(R, G, B) / 255;
        if (!(k >= 0.02)) continue;                     // NaN (a frame from before a zone changed size) is skipped too
        ctx.save();
        ctx.translate(x, y);
        ctx.globalAlpha = k;
        ctx.fillStyle = glowFor(ctx, col, r);
        ctx.beginPath(); ctx.arc(0, 0, r * 4.2, 0, Math.PI * 2); ctx.fill();
        ctx.restore();
        // a hotter core: the LED itself, lifted toward white so bright LEDs read as light, not paint
        const lift = 0.35 * k;
        ctx.fillStyle = `rgb(${R + (255 - R) * lift},${G + (255 - G) * lift},${B + (255 - B) * lift})`;
        ctx.beginPath(); ctx.arc(x, y, r * 0.9, 0, Math.PI * 2); ctx.fill();
      }
    }
  }
}

// The Overview's lighting tile: every lit zone, zoomed to fit, same glow as the stage.
// frames: zone key -> hex colours; the live ones by default, or a profile's preview from the gallery
export function paintMini(c, frames = st.frames) {
  paintRig(c, frames, { fit: "zones", pad: framed() ? 42 : 22, maxScale: Math.max(0.6, Math.min(2, c.clientHeight / 600)) });
}

export const litZones = () => (S.zones?.zones || []).filter((z) => z.device.online && z.config.enabled && z.positions.length);

// Draw the rig into a canvas. fit "zones": zoom to the lit zones (tiles, gallery cards); fit "stage": the whole
// 1.6 x 1.0 stage, so a picture of a Studio effect's 320 x 200 canvas (backdrop) lines up behind the LEDs.
export function paintRig(c, frames, { fit = "zones", backdrop = null, pad = 22, maxScale = 0.6, zones: given = null, overlay = null } = {}) {
  const r = c.getBoundingClientRect();
  if (!r.width || !r.height) return;
  const dpr = window.devicePixelRatio || 1;
  if (c.width !== Math.round(r.width * dpr) || c.height !== Math.round(r.height * dpr)) {
    c.width = Math.round(r.width * dpr); c.height = Math.round(r.height * dpr);
  }
  const ctx = c.getContext("2d");
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.globalCompositeOperation = "source-over";
  ctx.clearRect(0, 0, r.width, r.height);
  const zones = given || litZones();
  const bounds = portraitBounds(zones);
  let x0 = 0, y0 = 0, x1 = 1.6, y1 = 1.0;
  if (fit === "zones") {
    if (!bounds) return;
    x0 = y0 = Infinity; x1 = y1 = -Infinity;
    for (const z of zones) for (const [x, y] of z.positions) { x0 = Math.min(x0, x); y0 = Math.min(y0, y); x1 = Math.max(x1, x); y1 = Math.max(y1, y); }
    if (framed()) { x0 = bounds.x0 / U; y0 = bounds.y0 / U; x1 = bounds.x1 / U; y1 = bounds.y1 / U; }
  }
  const inset = Math.min(pad, r.width * .18, r.height * .18);
  const s = Math.min((r.width - inset * 2) / Math.max((x1 - x0) * U, 1), (r.height - inset * 2) / Math.max((y1 - y0) * U, 1), maxScale);
  const ox = (r.width - (x1 - x0) * U * s) / 2 - x0 * U * s, oy = (r.height - (y1 - y0) * U * s) / 2 - y0 * U * s;
  paintPortrait(ctx, zones, { ox, oy, s, w: r.width, h: r.height }, bounds);
  if (backdrop) {                                    // the effect's canvas, faint and soft, behind the LEDs
    ctx.save();
    ctx.globalAlpha = 0.32;
    ctx.filter = "blur(10px)";
    ctx.drawImage(backdrop, ox, oy, 1.6 * U * s, 1.0 * U * s);
    ctx.restore();
  }
  for (const pass of [0, 1]) {
    ctx.globalCompositeOperation = pass ? "lighter" : "source-over";
    for (const z of zones) {
      const hex = frames[z.key];
      const rad = Math.max(1.4, ledRadius(z) * s);
      for (let i = 0; i < z.positions.length; i++) {
        const x = ox + z.positions[i][0] * U * s, y = oy + z.positions[i][1] * U * s;
        if (!pass) { ctx.fillStyle = "#1e1e20"; ctx.beginPath(); ctx.arc(x, y, rad, 0, Math.PI * 2); ctx.fill(); continue; }
        if (!hex) continue;
        const col = hex.substr(i * 6, 6);
        const R = parseInt(col.slice(0, 2), 16), G = parseInt(col.slice(2, 4), 16), B = parseInt(col.slice(4, 6), 16);
        const k = Math.max(R, G, B) / 255;
        if (!(k >= 0.02)) continue;
        ctx.save();
        ctx.translate(x, y);
        ctx.globalAlpha = k;
        ctx.fillStyle = glowFor(ctx, col, rad, 0.5, 4.5);
        ctx.beginPath(); ctx.arc(0, 0, rad * 4.5, 0, Math.PI * 2); ctx.fill();
        ctx.restore();
        const lift = 0.35 * k;
        ctx.fillStyle = `rgb(${R + (255 - R) * lift},${G + (255 - G) * lift},${B + (255 - B) * lift})`;
        ctx.beginPath(); ctx.arc(x, y, rad * 0.9, 0, Math.PI * 2); ctx.fill();
      }
    }
  }
  // The Studio can draw the effect's own coordinates over the rig (LED numbers, the x/y grid).
  if (overlay) {
    ctx.save();
    ctx.globalCompositeOperation = "source-over";
    ctx.globalAlpha = 1;
    overlay(ctx, { ox, oy, s, u: U, zones });
    ctx.restore();
  }
}

// LED frames arrive on the live stream (live.js) while a view with the preview is showing.
function onLiveFrames(f) {
  st.frames = f.zones;
  rig3d?.setFrames(st.frames);
  if (document.hidden) return;
  if (!$('[data-view="lighting"]').hidden) draw();
  if (!$('[data-view="overview"]').hidden) emit("frames");
}

// ------------------------------------------------------------ interaction
function zoneByKey(key) {
  return (S.zones?.zones || []).find((z) => z.key === key);
}

async function saveLayout(z, layout) {
  return saveLayouts([{ z, layout }]);
}

// One gesture, one reload: a device's zones move together, and N concurrent GET /api/zones used to race, so
// whichever answered last decided the state the page kept.
async function saveLayouts(items) {
  try {
    for (const { z, layout } of items) {
      const r = await post(`/api/zones/${encodeURIComponent(z.key)}`, { ...z.config, layout });
      z.config = r.config;
    }
    await refreshZones();
    renderOverlay();
    draw();
  } catch (e) { fail(e); renderOverlay(); draw(); }
}

function zoneEl(key) {
  return key ? svg().querySelector(`.zone[data-key="${CSS.escape(key)}"]`) : null;
}

function selectZone(key, focus = false) {
  if (st.selected !== key) {
    st.selected = key;
    renderOverlay();
    emit("select-zone", key);
  }
  if (focus) zoneEl(key)?.focus({ preventScroll: true });
}

function onKey(e) {
  const g = e.target.closest?.(".zone");
  if (!g) return;
  if (e.key === "Enter" || e.key === " ") {
    e.preventDefault();
    selectZone(g.dataset.key, true);
  }
}

function onPointerDown(e) {
  const rotEl = e.target.closest("[data-rot]");
  const g = e.target.closest(".zone");
  if (rotEl && st.arranging) {
    const z = zoneByKey(rotEl.dataset.rot);
    const p = svgPoint(e);
    const cx = z.config.layout.x * U, cy = z.config.layout.y * U;
    st.drag = { key: z.key, mode: "rotate", start: Math.atan2(p.y - cy, p.x - cx), delta: 0, moved: false };
    svg().setPointerCapture(e.pointerId);
    return;
  }
  if (!g) {
    if (st.selected) selectZone(null);
    return;
  }
  const key = g.dataset.key;
  if (st.arranging) {
    const p = svgPoint(e);
    const keys = st.placing?.includes(key) ? st.placing : [key];      // a device being placed moves as one
    st.drag = { key, keys, mode: "move", x0: p.x, y0: p.y, dx: 0, dy: 0, moved: false };
    svg().setPointerCapture(e.pointerId);
  }
  selectZone(key);
}

function onPointerMove(e) {
  const d = st.drag;
  if (!d) return;
  const p = svgPoint(e);
  if (d.mode === "move") {
    d.dx = p.x - d.x0; d.dy = p.y - d.y0;
    d.moved ||= Math.hypot(d.dx, d.dy) > 3;
    for (const k of d.keys) svg().querySelector(`.zone[data-key="${CSS.escape(k)}"]`)?.setAttribute("transform", `translate(${d.dx} ${d.dy})`);
  } else {
    const z = zoneByKey(d.key);
    const cx = z.config.layout.x * U, cy = z.config.layout.y * U;
    let deg = (Math.atan2(p.y - cy, p.x - cx) - d.start) * 180 / Math.PI;
    if (!e.shiftKey) deg = Math.round(deg / 15) * 15;          // snap to 15° unless Shift is held
    d.delta = deg; d.moved = true;
  }
  draw();
}

function onPointerUp() {
  const d = st.drag;
  st.drag = null;
  if (!d || !d.moved) { draw(); return; }
  if (d.mode === "move") {
    const moved = [];
    for (const k of d.keys) {
      const z = zoneByKey(k);
      const L = { ...z.config.layout };
      L.x = Math.round(clamp(L.x + d.dx / U, 0, VB_W / U) * 100) / 100;
      L.y = Math.round(clamp(L.y + d.dy / U, 0, VB_H / U) * 100) / 100;
      moved.push({ z, layout: L });
    }
    saveLayouts(moved);
    return;
  }
  const z = zoneByKey(d.key);
  const L = { ...z.config.layout };
  L.angle = Math.round(((L.angle + d.delta) % 360 + 540) % 360 - 180);
  saveLayout(z, L);
}

// The setup guide's last step: Arrange mode with the new light picked out, so you drag it to where it sits.
// keys: the zones being placed (a graphics card's zones move together), or null when done.
export function placing(keys) {
  setRigMode("2d");
  st.arranging = !!keys;
  st.placing = keys;
  st.selected = keys?.[0] || null;
  $("#arrange").setAttribute("aria-pressed", String(st.arranging));
  renderOverlay();
  draw();
}

export function select(key) {
  st.selected = key;
  renderOverlay();
}

export function initStage() {
  const identity = document.createElement("div");
  identity.id = "stage-identity"; identity.className = "stage-identity";
  $("#stage").append(identity);
  const switcher = document.createElement("div");
  switcher.className = "stage-view-switch"; switcher.setAttribute("role", "group"); switcher.setAttribute("aria-label", "Lighting view");
  switcher.innerHTML = '<button type="button" class="chip-btn" id="stage-view-2d" aria-pressed="true">2D</button><button type="button" class="chip-btn" id="stage-view-3d" aria-pressed="false" title="Explore your lighting layout in 3D">3D</button>';
  $(".stage-tools").append(switcher);
  $("#stage-view-2d").addEventListener("click", () => setRigMode("2d"));
  $("#stage-view-3d").addEventListener("click", () => setRigMode("3d", true));
  computeView();
  new ResizeObserver(() => { computeView(); draw(); declutter(); }).observe($("#stage"));
  document.fonts?.ready.then(declutter);              // labels measured before the font arrives come out too narrow
  svg().addEventListener("pointerdown", onPointerDown);
  svg().addEventListener("pointermove", onPointerMove);
  svg().addEventListener("pointerup", onPointerUp);
  svg().addEventListener("pointercancel", onPointerUp);
  svg().addEventListener("keydown", onKey);
  const setArranging = (on) => {
    if (on) setRigMode("2d");
    st.arranging = on;
    $("#arrange").setAttribute("aria-pressed", String(on));
    renderOverlay();
    draw();
  };
  $("#arrange").addEventListener("click", () => setArranging(!st.arranging));
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && st.arranging && !$("#sheet").open) setArranging(false);
  });
  on("zones", () => { renderOverlay(); draw(); });
  document.addEventListener("appearancechange", () => {
    if (!document.hidden && !$('[data-view="lighting"]').hidden) { computeView(); draw(); declutter(); }
  });
  new MutationObserver(() => draw()).observe(document.documentElement, { attributes: true, attributeFilter: ["data-rig"] });
  renderOverlay();
  on("frames:live", onLiveFrames);
}
