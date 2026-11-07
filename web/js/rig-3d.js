// A real perspective scene, rasterized locally on Canvas. Geometry comes from the
// configured layout; extrusion and optional depth are presentation, not a case scan.
// Imported only when 3D is requested. No network, hardware writes, or animation loop.
const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
const finite = (v, fallback = 0) => Number.isFinite(v) ? v : fallback;
const DEPTH_KEY = "airglow.rig3d.depth.v1";
const homeCamera = () => ({ yaw: -.42, pitch: .22, distance: 3.1, panX: 0, panY: 0 });

export function parseDepths(raw) {
  const result = Object.create(null);
  try {
    const values = JSON.parse(raw);
    if (!values || typeof values !== "object" || Array.isArray(values)) return result;
    for (const [key, value] of Object.entries(values).slice(0, 2048)) {
      if (typeof value === "number" && Number.isFinite(value)) result[key] = clamp(value, -.6, .6);
    }
  } catch { /* unavailable or corrupt storage starts with a flat layout */ }
  return result;
}

export function frameColor(frames, key, index) {
  const value = frames?.[key];
  if (typeof value !== "string") return null;
  const hex = value.slice(index * 6, index * 6 + 6);
  return /^[\da-f]{6}$/i.test(hex) ? `#${hex}` : null;
}

export function buildScene(zones, depths = {}) {
  const surfaces = [], leds = [], labels = [];
  const front = "#27313b", edge = "#141c24", back = "#10171e";
  const prism = (points, z, thickness, key) => {
    const face = points.map(([x, y]) => [x, -y, z]);
    const rear = points.map(([x, y]) => [x, -y, z - thickness]);
    surfaces.push({ points: face, color: front, key }, { points: rear, color: back, key });
    for (let i = 0; i < face.length; i++) {
      const next = (i + 1) % face.length;
      surfaces.push({ points: [face[i], face[next], rear[next], rear[i]], color: edge, key });
    }
  };
  const circle = (x, y, radius, count = 24) => Array.from({ length: count }, (_, i) => {
    const a = i / count * Math.PI * 2;
    return [x + Math.cos(a) * radius, y + Math.sin(a) * radius];
  });
  for (const zone of zones || []) {
    if (!zone.device?.online || !zone.config?.enabled || !zone.positions?.length) continue;
    const L = zone.config.layout, key = zone.key;
    const points = zone.positions.map((p, index) => ({ p, index })).filter(({ p }) => Number.isFinite(p[0]) && Number.isFinite(p[1]));
    if (!points.length) continue;
    const z = clamp(finite(depths[key]), -.6, .6), angle = finite(L.angle) * Math.PI / 180;
    const ca = Math.cos(angle), sa = Math.sin(angle);
    let radius = .007;
    if (L.shape === "fans") {
      const size = Math.max(.01, finite(L.size, .2)), count = clamp(Math.round(finite(L.fans, 1)), 1, 64);
      radius = clamp(size * Math.PI / Math.max(1, finite(L.leds_per_fan, 12)) * .21, .003, .011);
      for (let f = 0; f < count; f++) {
        const off = (f - (count - 1) / 2) * size * 1.12;
        const cx = finite(L.x) + ca * off, cy = finite(L.y) + sa * off;
        const outer = circle(cx, cy, size * .5, 32), inner = circle(cx, cy, size * .37, 32);
        for (let i = 0; i < 32; i++) {
          const next = (i + 1) % 32;
          prism([outer[i], outer[next], inner[next], inner[i]], z, size * .15, key);
        }
        prism(circle(cx, cy, size * .13), z - size * .015, size * .12, key);
      }
    } else if (L.shape === "matrix") {
      const w = Math.max(.01, finite(L.length, .2)) / 2;
      const h = w * Math.max(1, finite(L.rows, 1)) / Math.max(1, finite(L.cols, 1));
      const corners = [[-w, -h], [w, -h], [w, h], [-w, h]].map(([x, y]) => [finite(L.x) + ca * x - sa * y, finite(L.y) + sa * x + ca * y]);
      prism(corners, z, .018, key);
      radius = clamp(w * 2 / Math.max(1, finite(L.cols, 1)) * .22, .002, .008);
    } else {
      radius = clamp(finite(L.length, .2) / Math.max(points.length, 1) * .25, .003, .009);
      if (points.length === 1) prism(circle(...points[0].p, .014, 16), z, .018, key);
      else for (let i = 1; i < points.length; i++) {
        const [ax, ay] = points[i - 1].p, [bx, by] = points[i].p;
        const len = Math.hypot(bx - ax, by - ay);
        if (!len) continue;
        const nx = (by - ay) / len * radius * 1.5, ny = (ax - bx) / len * radius * 1.5;
        prism([[ax + nx, ay + ny], [bx + nx, by + ny], [bx - nx, by - ny], [ax - nx, ay - ny]], z, .014, key);
      }
    }
    for (const { p, index } of points) leds.push({ point: [p[0], -p[1], z + .004], key, index, radius });
    labels.push({ key, name: zone.config.name || key, point: [finite(L.x, points[0].p[0]), -finite(L.y, points[0].p[1]), z + .008] });
  }
  const lo = [Infinity, Infinity, Infinity], hi = [-Infinity, -Infinity, -Infinity];
  const include = point => point.forEach((v, i) => { lo[i] = Math.min(lo[i], v); hi[i] = Math.max(hi[i], v); });
  for (const surface of surfaces) surface.points.forEach(include);
  for (const led of leds) include(led.point);
  if (!leds.length) return { surfaces, leds, labels, center: [0, 0, 0], radius: 1, bounds: null };
  const center = lo.map((v, i) => (v + hi[i]) / 2);
  const radius = Math.max(.16, Math.hypot(...hi.map((v, i) => (v - lo[i]) / 2)));
  return { surfaces, leds, labels, center, radius, bounds: { lo, hi } };
}

export function projectPoint(point, scene, camera, width, height) {
  const [x, y, z] = point.map((v, i) => (v - scene.center[i]) / scene.radius);
  const cy = Math.cos(camera.yaw), sy = Math.sin(camera.yaw), cp = Math.cos(camera.pitch), sp = Math.sin(camera.pitch);
  const rx = x * cy + z * sy, rz = -x * sy + z * cy;
  const ry = y * cp - rz * sp, depth = camera.distance - (y * sp + rz * cp);
  const scale = Math.min(width, height) * 1.16 / Math.max(.05, depth);
  return { x: width / 2 + rx * scale + camera.panX * width,
    y: height / 2 - ry * scale + camera.panY * height, depth, scale: scale / scene.radius };
}

export function createRenderQueue({ isVisible, paint, request = requestAnimationFrame, cancel = cancelAnimationFrame }) {
  let pending = null;
  return {
    request() {
      if (pending !== null || !isVisible()) return;
      pending = request(() => { pending = null; if (isVisible()) paint(); });
    },
    stop() { if (pending !== null) cancel(pending); pending = null; },
  };
}

export function createRig3D(host, { onSelect, onUnavailable, isVisible, compact = false, idPrefix = "rig-3d" }) {
  // Overview can render several previews alongside the full Lighting stage.
  // Prefix IDs and their ARIA relationships; selectors stay local to this pane.
  const id = String(idPrefix).replace(/[^a-zA-Z0-9_-]/g, "") || "rig-3d";
  const pane = document.createElement("div");
  pane.id = id; pane.className = `rig-3d${compact ? " rig-3d-compact" : ""}`; pane.hidden = true;
  pane.innerHTML = `<canvas id="${id}-canvas" tabindex="0" role="img" aria-label="Interactive 3D lighting layout" aria-describedby="${id}-help"></canvas>
    <div class="rig-3d-top"><span>Spatial layout <small>Presentation depth only</small></span><button class="chip-btn" id="${id}-reset" type="button">Reset view</button></div>
    <div class="rig-3d-bottom"><div class="rig-3d-zone"><label for="${id}-zone">Zone</label><select id="${id}-zone" class="select" aria-label="Select a zone in 3D"></select>
      <label for="${id}-depth">Depth</label><input id="${id}-depth" type="range" min="-60" max="60" step="1" value="0" aria-label="Zone presentation depth" aria-describedby="${id}-depth-note"><output id="${id}-depth-value" for="${id}-depth">0</output></div>
      <span id="${id}-depth-note" class="rig-3d-depth-note">Saved on this device. Lighting positions stay unchanged.</span>
      <details class="rig-3d-help"><summary>View controls</summary><p id="${id}-help">Drag to orbit · Shift-drag or right-drag to pan · Scroll to zoom. Keyboard: arrows orbit, Shift + arrows pan, + / − zoom, Home resets. Select a zone from the list or click its lights.</p></details></div>`;
  host.append(pane);
  const canvas = pane.querySelector("canvas");
  const ctx = canvas.getContext("2d", { alpha: compact });
  if (!ctx) { pane.remove(); throw new Error("3D drawing is unavailable on this device."); }
  const select = pane.querySelector(`#${id}-zone`), depth = pane.querySelector(`#${id}-depth`), depthValue = pane.querySelector("output");
  let zones = [], frames = {}, selected = null, scene = buildScene([]), camera = homeCamera(), visible = false, disposed = false, drag = null, hitPoints = [];
  let depths;
  try { depths = parseDepths(localStorage.getItem(DEPTH_KEY)); } catch { depths = parseDepths(null); }
  const gradients = new Map();
  const listeners = new AbortController();
  const listen = (el, event, fn, options = {}) => el.addEventListener(event, fn, { ...options, signal: listeners.signal });
  const shown = () => !disposed && host.isConnected && visible && !document.hidden && isVisible();
  const queue = createRenderQueue({ isVisible: shown, paint });
  const project = (p, w, h) => projectPoint(p, scene, camera, w, h);
  function path(points) {
    ctx.beginPath(); points.forEach((p, i) => i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y)); ctx.closePath();
  }
  function glow(color, radius) {
    const r = Math.round(radius * 2) / 2, key = `${color}|${r}`;
    if (gradients.has(key)) return gradients.get(key);
    const value = ctx.createRadialGradient(0, 0, 0, 0, 0, r * 4);
    value.addColorStop(0, `${color}80`); value.addColorStop(.3, `${color}28`); value.addColorStop(1, `${color}00`);
    if (gradients.size >= 300) gradients.delete(gradients.keys().next().value);
    gradients.set(key, value); return value;
  }
  function paint() {
    const { width: w, height: h } = canvas.getBoundingClientRect();
    if (!w || !h) return;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const pixelW = Math.round(w * dpr), pixelH = Math.round(h * dpr);
    if (canvas.width !== pixelW || canvas.height !== pixelH) { canvas.width = pixelW; canvas.height = pixelH; gradients.clear(); }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    if (compact) ctx.clearRect(0, 0, w, h);
    else { ctx.fillStyle = "#080c11"; ctx.fillRect(0, 0, w, h); }
    hitPoints = [];
    if (!scene.bounds) return;
    // A reference lattice behind the layout, not a guessed enclosure or component.
    const { lo, hi } = scene.bounds, margin = scene.radius * .14, plane = lo[2] - scene.radius * .12;
    ctx.strokeStyle = "#26313e"; ctx.lineWidth = .6;
    for (let i = 0; i <= 10; i++) {
      const x = lo[0] - margin + (hi[0] - lo[0] + margin * 2) * i / 10;
      const y = lo[1] - margin + (hi[1] - lo[1] + margin * 2) * i / 10;
      for (const ends of [[[x, lo[1] - margin, plane], [x, hi[1] + margin, plane]], [[lo[0] - margin, y, plane], [hi[0] + margin, y, plane]]]) {
        const [a, b] = ends.map(p => project(p, w, h)); ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke();
      }
    }
    const items = scene.surfaces.map(surface => {
      const points = surface.points.map(p => project(p, w, h));
      return { ...surface, points, depth: points.reduce((n, p) => n + p.depth, 0) / points.length };
    });
    for (const led of scene.leds) { const p = project(led.point, w, h); items.push({ led, ...p }); }
    items.sort((a, b) => b.depth - a.depth);
    for (const item of items) {
      if (!item.led) {
        path(item.points); ctx.fillStyle = item.color; ctx.fill();
        ctx.strokeStyle = item.key === selected ? "#9aafbf66" : "#55657533"; ctx.lineWidth = .65; ctx.stroke();
        continue;
      }
      const { led, x, y, scale } = item, radius = clamp(led.radius * scale, 1.2, 13);
      const color = frameColor(frames, led.key, led.index);
      ctx.save(); ctx.translate(x, y);
      if (color && color !== "#000000") {
        ctx.globalCompositeOperation = "lighter"; ctx.fillStyle = glow(color, radius);
        ctx.beginPath(); ctx.arc(0, 0, radius * 4, 0, Math.PI * 2); ctx.fill();
      }
      ctx.globalCompositeOperation = "source-over"; ctx.fillStyle = color || "#303b45";
      ctx.beginPath(); ctx.arc(0, 0, radius, 0, Math.PI * 2); ctx.fill(); ctx.restore();
      hitPoints.push({ x, y, key: led.key, radius: Math.max(12, radius * 2), depth: item.depth });
    }
    const label = scene.labels.find(z => z.key === selected);
    if (label) {
      const p = project(label.point, w, h);
      ctx.font = "12px system-ui, sans-serif"; ctx.textAlign = "center";
      const text = label.name.length > 42 ? `${label.name.slice(0, 40)}…` : label.name;
      const width = ctx.measureText(text).width + 20, x = clamp(p.x, width / 2 + 6, w - width / 2 - 6);
      ctx.fillStyle = "#0b1119e6"; ctx.fillRect(x - width / 2, p.y + 20, width, 25);
      ctx.fillStyle = "#edf3fa"; ctx.fillText(text, x, p.y + 37);
    }
  }
  function updateDepth() {
    depth.disabled = !selected;
    depth.value = String(Math.round((depths[selected] || 0) * 100));
    depthValue.value = `${Number(depth.value) > 0 ? "+" : ""}${depth.value}`;
    depth.setAttribute("aria-valuetext", `${depth.value} presentation units`);
  }
  function selectKey(key) {
    selected = scene.labels.some(z => z.key === key) ? key : null;
    select.value = selected || ""; updateDepth(); queue.request();
  }
  const zoom = amount => { camera.distance = clamp(camera.distance * amount, 1.65, 8); queue.request(); };
  const reset = () => { camera = homeCamera(); queue.request(); };
  listen(pane.querySelector(`#${id}-reset`), "click", reset);
  listen(select, "change", () => { selectKey(select.value || null); onSelect(selected); });
  listen(depth, "input", () => {
    if (!selected) return;
    depths[selected] = clamp(Number(depth.value) / 100, -.6, .6);
    try { localStorage.setItem(DEPTH_KEY, JSON.stringify(depths)); } catch { /* presentation remains usable without storage */ }
    scene = buildScene(zones, depths); updateDepth(); queue.request();
  });
  listen(canvas, "contextmenu", e => e.preventDefault());
  listen(canvas, "pointerdown", e => {
    if (e.button !== 0 && e.button !== 2 || drag) return;
    canvas.focus({ preventScroll: true }); canvas.setPointerCapture(e.pointerId);
    drag = { id: e.pointerId, x: e.clientX, y: e.clientY, startX: e.clientX, startY: e.clientY, pan: e.shiftKey || e.button === 2, moved: false };
    canvas.classList.add("dragging");
  });
  listen(canvas, "pointermove", e => {
    if (!drag || e.pointerId !== drag.id) return;
    const dx = e.clientX - drag.x, dy = e.clientY - drag.y;
    drag.moved ||= Math.hypot(e.clientX - drag.startX, e.clientY - drag.startY) > 4;
    if (drag.pan) { const r = canvas.getBoundingClientRect(); camera.panX = clamp(camera.panX + dx / r.width, -.8, .8); camera.panY = clamp(camera.panY + dy / r.height, -.8, .8); }
    else { camera.yaw += dx * .008; camera.pitch = clamp(camera.pitch + dy * .008, -1.4, 1.4); }
    drag.x = e.clientX; drag.y = e.clientY; queue.request();
  });
  const endDrag = e => {
    if (!drag || e.pointerId !== drag.id) return;
    if (!drag.moved && !drag.pan && e.type === "pointerup") {
      const r = canvas.getBoundingClientRect(), x = e.clientX - r.left, y = e.clientY - r.top;
      const hit = hitPoints.filter(p => Math.hypot(p.x - x, p.y - y) <= p.radius).sort((a, b) => a.depth - b.depth)[0];
      selectKey(hit?.key || null); onSelect(selected);
    }
    if (canvas.hasPointerCapture(e.pointerId)) canvas.releasePointerCapture(e.pointerId);
    drag = null; canvas.classList.remove("dragging");
  };
  listen(canvas, "pointerup", endDrag); listen(canvas, "pointercancel", endDrag); listen(canvas, "lostpointercapture", endDrag);
  listen(canvas, "wheel", e => { e.preventDefault(); zoom(Math.exp(clamp(e.deltaY * (e.deltaMode === 1 ? 16 : 1), -200, 200) * .001)); }, { passive: false });
  listen(canvas, "keydown", e => {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const arrows = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };
    if (arrows[e.key]) {
      const [x, y] = arrows[e.key];
      if (e.shiftKey) { camera.panX = clamp(camera.panX + x * .035, -.8, .8); camera.panY = clamp(camera.panY + y * .035, -.8, .8); }
      else { camera.yaw += x * .12; camera.pitch = clamp(camera.pitch + y * .12, -1.4, 1.4); }
    } else if (e.key === "+" || e.key === "=") zoom(.9);
    else if (e.key === "-" || e.key === "_") zoom(1.1);
    else if (e.key === "Home" || e.key.toLowerCase() === "r") reset();
    else return;
    e.preventDefault(); queue.request();
  });
  listen(canvas, "contextlost", e => { e.preventDefault(); queue.stop(); onUnavailable(); });
  const visibility = () => { if (shown()) queue.request(); else { queue.stop(); drag = null; canvas.classList.remove("dragging"); } };
  listen(document, "visibilitychange", visibility);
  const viewObserver = new MutationObserver(visibility);
  const view = host.closest("[data-view]");
  if (view) viewObserver.observe(view, { attributes: true, attributeFilter: ["hidden"] });
  const resize = new ResizeObserver(() => queue.request()); resize.observe(pane);
  return {
    setVisible(value) { visible = value; pane.hidden = !value; visibility(); },
    setFrames(value) { frames = value || {}; queue.request(); },
    setZones(value) {
      if (zones === value) return;
      zones = value || []; scene = buildScene(zones, depths);
      select.replaceChildren(new Option("All zones", ""), ...scene.labels.map(z => new Option(z.name, z.key)));
      selectKey(selected);
    },
    select: selectKey,
    focus() { canvas.focus({ preventScroll: true }); },
    dispose() {
      disposed = true; queue.stop(); listeners.abort(); resize.disconnect(); viewObserver.disconnect();
      gradients.clear(); canvas.width = canvas.height = 0; pane.remove();
    },
  };
}
