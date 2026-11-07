// Optional per-position layer masks. No network, persistence, or hardware access lives here.
export const PAINT_LIMIT = 2048;
const key = pair => pair.join(",");
export function cleanPaint(value) {
  if (!value || value.v !== 1 || !Array.isArray(value.points) || value.points.length > PAINT_LIMIT
      || value.points.some(p => !Array.isArray(p) || p.length !== 2 || p.some(n => !Number.isInteger(n) || Math.abs(n) > 100000)))
    throw new Error("A painted mask needs up to 2,048 valid LED positions.");
  if (new Set(value.points.map(key)).size !== value.points.length) throw new Error("A painted mask repeats an LED position.");
  return { v: 1, points: value.points.map(p => [...p]) };
}

export function paintPoints(zones, mask = "all") {
  let index = 0;
  return zones.flatMap(z => z.positions.map(([x, y], local) => ({ x, y, index: index++,
    pair: [Math.round(x / 1.6 * 10000), Math.round(y * 10000)],
    eligible: mask === "all" || !mask || z.spot === mask,
    label: `${z.zone?.name || z.name || z.key || "Zone"}, LED ${local + 1}` })));
}

const selection = (paint, points, initialAll = false) => new Map((paint ? cleanPaint(paint).points
  : initialAll ? points.filter(p => p.eligible).map(p => p.pair) : []).map(p => [key(p), p]));
const result = selected => cleanPaint({ v: 1, points: [...selected.values()] });

export function invertMask(paint, points) {
  const previous = selection(paint, points, true);
  return result(new Map(points.filter(p => p.eligible && !previous.has(key(p.pair))).map(p => [key(p.pair), p.pair])));
}

export function setLedMask(paint, points, index, erase = false) {
  const point = points.find(p => p.index === index && p.eligible);
  if (!point) throw new Error("Choose an LED in this layer's area.");
  const selected = selection(paint, points, erase);
  if (erase) selected.delete(key(point.pair)); else selected.set(key(point.pair), point.pair);
  return result(selected);
}

export function strokeMask(paint, points, from, to, radius, erase, map) {
  const selected = selection(paint, points, erase), dx = to.x - from.x, dy = to.y - from.y;
  const length = dx * dx + dy * dy, scale = map.s * map.u;
  let hit = false;
  for (const point of points) {
    if (!point.eligible) continue;
    const x = map.ox + point.x * scale, y = map.oy + point.y * scale;
    const t = length ? Math.max(0, Math.min(1, ((x - from.x) * dx + (y - from.y) * dy) / length)) : 0;
    if (Math.hypot(x - from.x - dx * t, y - from.y - dy * t) > radius) continue;
    hit = true;
    if (erase) selected.delete(key(point.pair)); else selected.set(key(point.pair), point.pair);
  }
  return hit ? result(selected) : paint;
}

export function bindPaintCanvas(canvas, { getLayer, getPoints, getMap, getSettings, onBegin = () => {}, onChange = () => {},
  onCommit = () => {}, onCancel = () => {}, onError = () => {}, onHover = () => {} }) {
  let stroke = null;
  const position = event => { const r = canvas.getBoundingClientRect(); return { x: event.clientX - r.left, y: event.clientY - r.top }; };
  function end(cancel = false) {
    if (!stroke) return;
    const saved = stroke; stroke = null;
    if (canvas.hasPointerCapture?.(saved.id)) canvas.releasePointerCapture(saved.id);
    if (cancel) {
      if (saved.before === undefined) delete saved.layer.paint; else saved.layer.paint = saved.before;
      onCancel();
    } else if (saved.changed) onCommit();
    else onCancel();
  }
  function move(event) {
    const next = position(event); onHover(next);
    if (!stroke || event.pointerId !== stroke.id) return;
    event.preventDefault();
    try {
      const paint = strokeMask(stroke.layer.paint, stroke.points, stroke.last, next, stroke.radius, stroke.erase, getMap());
      if (JSON.stringify(paint) !== JSON.stringify(stroke.layer.paint)) {
        stroke.layer.paint = paint; stroke.changed = true; onChange();
      }
      stroke.last = next;
    } catch (error) { end(true); onError(error); }
  }
  function down(event) {
    const settings = getSettings(), layer = getLayer(), map = getMap();
    if (!settings.enabled || !layer || !map || event.button !== 0 || stroke) return;
    event.preventDefault();
    stroke = { id: event.pointerId, layer, before: layer.paint === undefined ? undefined : structuredClone(layer.paint),
      points: getPoints(), radius: settings.radius, erase: settings.erase, last: position(event), changed: false };
    onBegin(); canvas.focus?.({ preventScroll: true }); canvas.setPointerCapture?.(event.pointerId); move(event);
  }
  const up = event => { if (event.pointerId === stroke?.id) { move(event); end(); } };
  const cancel = event => { if (event.pointerId === stroke?.id) end(true); };
  const leave = () => { if (!stroke) onHover(null); };
  const blur = () => { end(true); onHover(null); };
  const handlers = { pointerdown: down, pointermove: move, pointerup: up, pointercancel: cancel, lostpointercapture: cancel, pointerleave: leave };
  for (const [name, handler] of Object.entries(handlers)) canvas.addEventListener(name, handler);
  globalThis.window?.addEventListener("blur", blur);
  return { finish: () => end(), cancel: () => end(true), destroy() {
    end(true);
    for (const [name, handler] of Object.entries(handlers)) canvas.removeEventListener(name, handler);
    globalThis.window?.removeEventListener("blur", blur);
  } };
}

export function drawPaintOverlay(ctx, map, points, paint, { radius, cursor, selected = 0, erase = false }) {
  let enabled;
  try { enabled = paint ? new Set(cleanPaint(paint).points.map(key)) : null; } catch { enabled = new Set(); }
  ctx.lineWidth = 1.5;
  for (const point of points) {
    if (!point.eligible) continue;
    const x = map.ox + point.x * map.u * map.s, y = map.oy + point.y * map.u * map.s;
    const active = !enabled || enabled.has(key(point.pair));
    ctx.strokeStyle = active ? "#a5edcb" : "rgba(230,230,230,.35)";
    ctx.beginPath(); ctx.arc(x, y, point.index === selected ? 8 : 4, 0, Math.PI * 2); ctx.stroke();
    if (point.index === selected) { ctx.fillStyle = "#ffffff"; ctx.font = "12px sans-serif"; ctx.fillText(String(point.index + 1), x + 10, y - 6); }
  }
  if (cursor) { ctx.strokeStyle = erase ? "#efac9f" : "#a5edcb"; ctx.setLineDash([4, 3]); ctx.beginPath(); ctx.arc(cursor.x, cursor.y, radius, 0, Math.PI * 2); ctx.stroke(); ctx.setLineDash([]); }
}
