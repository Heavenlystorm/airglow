// A read-only optical view of the stage's configured LED coordinates and its delivered frames.
import { S, effectName, esc, setHTML } from "./core.js";
import { reduced } from "./motion.js";
import { R } from "./readings.js";
import { closeSheet, openSheet } from "./sheet.js";
import { fanCentres, ledRadius, matrixCorners, portraitBounds } from "./stage.js";

const TAU = Math.PI * 2;
const plural = (n, noun) => `${n} ${noun}${n === 1 ? "" : "s"}`;
const write = (el, text) => { if (el.textContent !== text) el.textContent = text; };

function poly(ctx, points) {
  ctx.beginPath();
  points.forEach(([x, y], index) => index ? ctx.lineTo(x, y) : ctx.moveTo(x, y));
  ctx.closePath();
}
function circle(ctx, x, y, r, fill, stroke) {
  ctx.beginPath(); ctx.arc(x, y, r, 0, TAU);
  if (fill) { ctx.fillStyle = fill; ctx.fill(); }
  if (stroke) { ctx.strokeStyle = stroke; ctx.stroke(); }
}

// Fit the transformed corners, so even long diagonal strips and wide matrices stay on the glass.
function lens(bounds, w, h, oblique) {
  const midX = (bounds.x0 + bounds.x1) / 2, midY = (bounds.y0 + bounds.y1) / 2;
  const width = Math.max(90, bounds.x1 - bounds.x0), height = Math.max(90, bounds.y1 - bounds.y0);
  const rim = Math.max(width, height) * .06 + 8;
  const left = midX - width / 2 - rim, top = midY - height / 2 - rim;
  const right = midX + width / 2 + rim, bottom = midY + height / 2 + rim;
  const a = 1, b = oblique ? -.17 : 0, c = oblique ? .36 : 0, d = oblique ? .82 : 1;
  const dw = right - left, dh = bottom - top;
  const s = Math.min(w * .88 / (a * dw + c * dh), h * .82 / (-b * dw + d * dh));
  const transform = [s * a, s * b, s * c, s * d, w / 2 - s * (a * midX + c * midY), h / 2 - s * (b * midX + d * midY)];
  return { left, top, right, bottom, rim, s, transform };
}

function fan(ctx, cx, cy, r, px, metal, edge) {
  ctx.save(); ctx.translate(cx, cy);
  const disc = ctx.createLinearGradient(-r, -r, r, r);
  disc.addColorStop(0, edge); disc.addColorStop(.36, metal); disc.addColorStop(.68, "#27232f"); disc.addColorStop(1, edge);
  ctx.lineWidth = px;
  circle(ctx, 0, 0, r * 1.08, disc, edge);
  circle(ctx, 0, 0, r * .95, "#100f18", "#ffffff20");
  // Curved vanes are a stationary outline of the configured fan, never an RPM simulation.
  for (let i = 0; i < 7; i++) {
    ctx.save(); ctx.rotate(i / 7 * TAU);
    ctx.beginPath(); ctx.moveTo(r * .19, 0);
    ctx.bezierCurveTo(r * .45, -r * .1, r * .63, -r * .65, r * .83, -r * .31);
    ctx.bezierCurveTo(r * .96, -r * .03, r * .7, r * .33, r * .19, r * .12);
    ctx.closePath(); ctx.fillStyle = disc; ctx.fill(); ctx.strokeStyle = "#e6d4fa48"; ctx.stroke();
    ctx.restore();
  }
  circle(ctx, 0, 0, r * .24, disc, edge);
  circle(ctx, 0, 0, r * .16, "#191722", "#ddd4ef32");
  ctx.restore();
}

function drawGlass(ctx, zones, view, style) {
  const { left, right, top, bottom, rim, s } = view, cut = rim * .64, px = 1 / s;
  const outline = [[left + cut, top], [right - cut, top], [right, top + cut], [right, bottom - cut],
    [right - cut, bottom], [left + cut, bottom], [left, bottom - cut], [left, top + cut]];
  ctx.lineWidth = px;
  const metal = style.getPropertyValue("--theatre-metal").trim(), edge = style.getPropertyValue("--theatre-edge").trim();
  const glass = ctx.createLinearGradient(left, top, right, bottom);
  glass.addColorStop(0, metal); glass.addColorStop(.34, "#17131edc"); glass.addColorStop(.75, "#11141ee8"); glass.addColorStop(1, metal);
  // Bevelled glass has a visible thickness; the colored edges belong to the presentation plane.
  ctx.save(); ctx.translate(0, 9 * px); poly(ctx, outline); ctx.fillStyle = "#101019"; ctx.fill(); ctx.strokeStyle = edge; ctx.stroke(); ctx.restore();
  poly(ctx, outline); ctx.fillStyle = glass; ctx.fill(); ctx.strokeStyle = edge; ctx.stroke();
  ctx.save(); ctx.clip(); ctx.strokeStyle = "#e0cfff0c";
  const step = Math.max(right - left, bottom - top) / 16;
  ctx.beginPath();
  for (let x = left + step; x < right; x += step) { ctx.moveTo(x, top); ctx.lineTo(x, bottom); }
  for (let y = top + step; y < bottom; y += step) { ctx.moveTo(left, y); ctx.lineTo(right, y); }
  ctx.stroke();
  const span = right - left, sweep = ctx.createLinearGradient(left, top, right, bottom);
  sweep.addColorStop(0, "#ffe5db0b"); sweep.addColorStop(.52, "#d7c1ff0a"); sweep.addColorStop(1, "#bceee900");
  poly(ctx, [[left - span * .15, top], [left + span * .32, top], [right - span * .12, bottom], [right - span * .5, bottom]]);
  ctx.fillStyle = sweep; ctx.fill();
  ctx.restore();
  const rail = ctx.createLinearGradient(left, top, right, bottom);
  rail.addColorStop(0, style.getPropertyValue("--theatre-rim").trim()); rail.addColorStop(.4, "#e0b9f12b");
  rail.addColorStop(.7, "#748a9a44"); rail.addColorStop(1, style.getPropertyValue("--theatre-readout").trim());
  ctx.strokeStyle = rail; ctx.lineWidth = 1.5 * px; ctx.beginPath();
  ctx.moveTo(left, top + cut * 2); ctx.lineTo(left, top + cut); ctx.lineTo(left + cut, top); ctx.lineTo(right - cut, top);
  ctx.moveTo(right, bottom - cut * 2); ctx.lineTo(right, bottom - cut); ctx.lineTo(right - cut, bottom); ctx.lineTo(left + cut, bottom); ctx.stroke();
  for (const zone of zones) {
    const L = zone.config.layout, positions = zone.positions;
    ctx.save(); ctx.globalAlpha = zone.device.online && zone.config.enabled ? 1 : .46;
    if (L.shape === "fans") {
      for (const [cx, cy] of fanCentres(zone)) fan(ctx, cx, cy, L.size * 500, px, metal, edge);
    } else if (L.shape === "matrix") {
      poly(ctx, matrixCorners(L)); ctx.fillStyle = "#11101c"; ctx.fill(); ctx.strokeStyle = edge; ctx.stroke();
    } else if (positions.length > 1) {
      const [first, last] = [positions[0], positions.at(-1)];
      ctx.beginPath(); ctx.moveTo(first[0] * 1000, first[1] * 1000); ctx.lineTo(last[0] * 1000, last[1] * 1000);
      ctx.lineCap = "round"; ctx.lineWidth = ledRadius(zone) * 2.8; ctx.strokeStyle = edge; ctx.stroke();
      ctx.lineWidth *= .8; ctx.strokeStyle = "#14121d"; ctx.stroke();
    }
    const radius = Math.max(px * 1.5, ledRadius(zone));
    for (const [x, y] of positions) circle(ctx, x * 1000, y * 1000, radius, "#35303f", null);
    ctx.restore();
  }
}

// Reuse glow masks. Exact frame RGB is painted over the soft optical spill at each LED centre.
function emitterFor(color, atlas) {
  const channels = [1, 3, 5].map(i => Math.round(parseInt(color.slice(i, i + 2), 16) / 51) * 51);
  const tint = "#" + channels.map(c => c.toString(16).padStart(2, "0")).join("");
  if (atlas.has(tint)) return atlas.get(tint);
  const image = document.createElement("canvas"); image.width = image.height = 64;
  const ctx = image.getContext("2d");
  const bloom = ctx.createRadialGradient(32, 32, 0, 32, 32, 32);
  bloom.addColorStop(0, tint); bloom.addColorStop(.14, tint);
  bloom.addColorStop(.24, tint + "8a"); bloom.addColorStop(.53, tint + "24"); bloom.addColorStop(1, tint + "00");
  ctx.fillStyle = bloom; ctx.fillRect(0, 0, 64, 64);
  atlas.set(tint, image);
  return image;
}

function clearLayer(canvas) {
  const ctx = canvas.getContext("2d");
  ctx.setTransform(1, 0, 0, 1, 0, 0); ctx.clearRect(0, 0, canvas.width, canvas.height);
  return ctx;
}

// Each section of a fan diffuser takes the colour of its own physical emitter. Use the positions in frame order
// so reverse wiring and chains of multiple fans retain their real clockwise/counter-clockwise distribution.
function fanOptics(zone) {
  const L = zone.config.layout, count = L.leds_per_fan;
  if (L.shape !== "fans" || count < 3) return [];
  const paths = [];
  for (let start = 0; start + count <= zone.positions.length; start += count) {
    let cx = 0, cy = 0;
    const ring = zone.positions.slice(start, start + count);
    for (const [x, y] of ring) { cx += x * 1000 / count; cy += y * 1000 / count; }
    for (const [x, y] of ring) {
      const angle = Math.atan2(y * 1000 - cy, x * 1000 - cx);
      const path = new Path2D();
      path.arc(cx, cy, L.size * 500, angle - Math.PI / count, angle + Math.PI / count);
      paths.push(path);
    }
  }
  return paths;
}

export function openTheatre(firstFrame, onClose) {
  const sheet = openSheet(`<section class="light-theatre">
    <header class="theatre-head"><div class="theatre-heading"><svg aria-hidden="true"><use href="#i-effect"/></svg><h2>Light theatre</h2></div>
      <button type="button" class="btn" data-close>Exit theatre</button></header>
    <div class="theatre-window"><div class="theatre-caption"><span class="theatre-state" role="status"></span><h3></h3></div>
      <div class="theatre-screen" role="img" aria-label="Configured lighting layout">
        <canvas data-glass aria-hidden="true"></canvas><canvas data-emitters aria-hidden="true"></canvas>
        <p class="theatre-empty" hidden></p></div>
      <div class="theatre-meter"></div></div>
    <footer class="theatre-foot"><label>Focus<select class="select" data-zone aria-label="Focus a lighting zone"></select></label>
      <div class="theatre-controls"><div role="group" class="seg" aria-label="Theatre camera">
        <button type="button" data-angle="oblique" aria-pressed="true">Oblique</button><button type="button" data-angle="front" aria-pressed="false">Face-on</button></div>
      <button type="button" class="btn" data-freeze>Hold frame</button></div></footer>
  </section>`);
  const root = sheet.querySelector(".light-theatre"), find = selector => root.querySelector(selector);
  const screen = find(".theatre-screen"), glass = find("[data-glass]"), lights = find("[data-emitters]");
  if (!glass.getContext("2d") || !lights.getContext("2d")) { closeSheet(); throw new Error("Canvas drawing is unavailable on this device."); }
  const selector = find("[data-zone]"), stillButton = find("[data-freeze]"), empty = find(".theatre-empty");
  const atlas = new Map(), optics = new Map(), listeners = new AbortController();
  const listen = (el, event, fn) => el.addEventListener(event, fn, { signal: listeners.signal });
  let packet = firstFrame, held = null, heldScene = null, still = reduced(), camera = "oblique", raf = 0, disposed = false;
  let zones = [], visible = [], active = [], view = null, structureDirty = true, paintKey = "", gate = "", selected = "", coverage = 0;
  let zoom = 1;
  function requestPaint(structure = false) {
    structureDirty ||= structure;
    if (!raf && !document.hidden && !disposed) raf = requestAnimationFrame(paint);
  }

  function paint() {
    raf = 0;
    if (!root.isConnected || !sheet.open || document.hidden) return;
    const box = screen.getBoundingClientRect(), w = Math.round(box.width), h = Math.round(box.height);
    if (w < 1 || h < 1) return;
    // Bound the pixel cost of a maximized high-DPI display while retaining native resolution on small windows.
    zoom = Math.min(devicePixelRatio || 1, 2, 2400 / w, 1600 / h);
    const pixelW = Math.max(1, Math.round(w * zoom)), pixelH = Math.max(1, Math.round(h * zoom));
    if (glass.width !== pixelW || glass.height !== pixelH) {
      for (const c of [glass, lights]) { c.width = pixelW; c.height = pixelH; }
      structureDirty = true;
    }
    if (structureDirty) {
      const shapes = active.length ? active : visible;
      const bounds = portraitBounds(shapes);
      view = bounds ? lens(bounds, w, h, camera === "oblique") : null;
      const ctx = clearLayer(glass);
      if (view) {
        ctx.setTransform(...view.transform.map(v => v * zoom));
        drawGlass(ctx, shapes, view, getComputedStyle(root));
      }
      structureDirty = false;
    }
    const ctx = clearLayer(lights);
    if (!view || gate) return;
    ctx.setTransform(...view.transform.map(v => v * zoom)); ctx.globalCompositeOperation = "lighter";
    const frames = (held || packet)?.zones || {};
    for (const zone of visible) {
      const hex = frames[zone.key];
      if (!zone.device.online || !zone.config.enabled || typeof hex !== "string" || hex.length !== zone.positions.length * 6) continue;
      const r = Math.max(1.5 / view.s, ledRadius(zone));
      const reach = r * (zone.config.layout.shape === "fans" ? 7 : 5.5);
      for (let i = 0; i < zone.positions.length; i++) {
        const color = "#" + hex.slice(i * 6, i * 6 + 6);
        if (color === "#000000" || !/^#[\da-f]{6}$/i.test(color)) continue;
        const x = zone.positions[i][0] * 1000, y = zone.positions[i][1] * 1000;
        const arc = optics.get(zone.key)?.[i];
        if (arc) {
          ctx.lineWidth = Math.max(1.2 / view.s, r * .65);
          ctx.strokeStyle = color + "bb"; ctx.stroke(arc);
        }
        ctx.drawImage(emitterFor(color, atlas), x - reach, y - reach, reach * 2, reach * 2);
        ctx.globalCompositeOperation = "source-over";
        circle(ctx, x, y, r * .76, color, null);
        ctx.globalCompositeOperation = "lighter";
      }
    }
  }

  const complete = frame => active.reduce((count, z) => count + Number(typeof frame?.zones?.[z.key] === "string" &&
    frame.zones[z.key].length === z.positions.length * 6), 0);
  function refresh(force = false) {
    if (disposed) return;
    const scene = S.state?.effective || S.state?.scene, status = S.state?.scene_status;
    const online = visible.filter(z => z.device.online);
    const previousGate = gate;
    const zoneEffect = z => z?.config.override?.effect || scene?.areas?.[z?.spot]?.effect || scene?.effect;
    gate = R.stale ? "Connection lost" : !visible.length ? "No configured LEDs" : !online.length ? "Devices offline"
      : !active.length ? "Zones disabled" : !scene ? "Waiting for Airglow" : scene.on === false ? "Lights off" : "";
    if (gate) held = null;
    if (R.stale) packet = null;
    if (still && !held && !gate && complete(packet)) {
      held = packet;
      heldScene = { effect: scene.effect, brightness: scene.brightness, zones: new Map(zones.map(z => [z.key, zoneEffect(z)])) };
    }
    const shown = held || packet;
    coverage = complete(shown);
    const stateText = gate || (!coverage ? "Waiting for frames" : held ? "Frame held" :
      coverage < active.length ? "Partial frame" : "Live output");
    write(find(".theatre-state"), stateText);
    const effect = held ? heldScene.zones.get(selected) || heldScene.effect : selected ? zoneEffect(visible[0]) : scene?.effect;
    const name = effectName(effect) || "Lighting";
    write(find("h3"), !held && !selected && status?.status === "pending" ? `Starting ${name}` : name);
    let detail = "";
    if (gate === "No configured LEDs") detail = selected ? "This zone is no longer configured." : "Set up a lighting zone to see your rig here.";
    else if (gate === "Devices offline") detail = "Reconnect a lighting device to see its output.";
    else if (gate === "Zones disabled") detail = "The selected lighting zones are disabled.";
    empty.hidden = !detail; write(empty, detail);
    const leds = visible.reduce((count, z) => count + z.positions.length, 0);
    const facts = [plural(visible.length, "zone"), plural(leds, "LED")];
    if (visible.length > online.length) facts.push(`${visible.length - online.length} offline`);
    if (online.length > active.length) facts.push(`${online.length - active.length} disabled`);
    const brightness = held ? heldScene.brightness : scene?.brightness;
    if (Number.isFinite(brightness) && !R.stale) facts.push(`${brightness}% brightness`);
    write(find(".theatre-meter"), facts.join("  ·  "));
    screen.setAttribute("aria-label", `${name}, ${stateText}. ${facts.join(", ")}.`);
    stillButton.disabled = !held && (!coverage || !!gate);
    write(stillButton, held ? "Resume live" : "Hold frame");
    stillButton.setAttribute("aria-pressed", String(!!held));
    if (gate !== previousGate || force) requestPaint();
  }

  function syncZones() {
    zones = S.zones?.zones || [];
    const missing = selected && !zones.some(z => z.key === selected);
    setHTML(selector, '<option value="">Whole rig</option>' + zones.map(z =>
      `<option value="${esc(z.key)}">${esc(z.config.name)} · ${esc(z.device.name)}${!z.device.online ? " (offline)" : ""}</option>`).join("") +
      (missing ? `<option value="${esc(selected)}">Unavailable zone</option>` : ""));
    selector.value = selected;
    visible = zones.filter(z => (!selected || z.key === selected) && z.positions.length);
    active = visible.filter(z => z.device.online && z.config.enabled);
    optics.clear();
    for (const zone of visible) optics.set(zone.key, fanOptics(zone));
    structureDirty = true;
    refresh(true);
  }
  listen(selector, "change", () => { selected = selector.value; syncZones(); });
  root.querySelectorAll("[data-angle]").forEach(button => listen(button, "click", () => {
    camera = button.dataset.angle;
    root.querySelectorAll("[data-angle]").forEach(b => b.setAttribute("aria-pressed", String(b === button)));
    requestPaint(true);
  }));
  listen(stillButton, "click", () => { still = !held; held = null; refresh(true); });
  function motionChanged() { if (reduced()) still = true; refresh(true); requestPaint(true); }
  listen(document, "appearancechange", motionChanged);
  listen(document, "designchange", () => requestPaint(true));
  listen(matchMedia("(prefers-reduced-motion: reduce)"), "change", motionChanged);
  listen(document, "visibilitychange", () => { if (!document.hidden) requestPaint(true); });
  listen(window, "hashchange", () => { if (root.isConnected && sheet.open) closeSheet(); });
  const observer = new ResizeObserver(() => requestPaint(true));
  observer.observe(screen);
  // The shared sheet may be replaced by another surface without a native close event.
  const replacement = new MutationObserver(() => { if (!root.isConnected) dispose(); });
  replacement.observe(sheet, { childList: true });
  function dispose() {
    if (disposed) return;
    disposed = true; cancelAnimationFrame(raf);
    observer.disconnect(); replacement.disconnect(); listeners.abort(); atlas.clear();
    root.remove();
    onClose();
  }
  listen(sheet, "close", dispose);
  syncZones();
  find("[data-close]").focus();
  return {
    zones: syncZones, state: refresh,
    frames(next) {
      packet = next;
      if (held || disposed) return;
      // A constant focused strip needn't repaint when other zones animate.
      const key = selected ? next.zones?.[selected] || "" : null;
      const changed = key === null || key !== paintKey;
      paintKey = key;
      if (still || coverage !== complete(next)) refresh(changed && !gate);
      else if (changed && !gate) requestPaint();
    },
  };
}
