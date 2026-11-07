// Static, display-only instruments. Saved artwork never evaluates a Studio script or reads live inputs.
const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const bounded = (v, lo, hi) => Number.isFinite(v) && v >= lo && v <= hi;
const param = (v, fallback, lo, hi) => bounded(v, lo, hi) ? v : fallback;
const hex = (v) => typeof v === "string" && /^#[\da-f]{6}$/i.test(v);
const rgb = (c) => [1, 3, 5].map((i) => parseInt(c.slice(i, i + 2), 16));
const color = (v) => `#${v.map((n) => Math.round(n).toString(16).padStart(2, "0")).join("")}`;
const mix = (a, b, f) => color(rgb(a).map((v, i) => v + (rgb(b)[i] - v) * f));
const coord = (v) => Math.round(v * 100) / 100;

function palette(params) {
  return Object.values(params || {}).flatMap((v) => Array.isArray(v) ? v.slice(0, 16) : [v]).filter(hex).slice(0, 16);
}

function hue(h, saturation = 1) {
  const k = (n) => (n + h * 12) % 12;
  return color([0, 8, 4].map((n) => 255 * (1 - saturation * Math.max(0, Math.min(k(n) - 3, 9 - k(n), 1)))));
}

/** Saved pattern/palette artwork, suitable inside an existing profile button. No SVG IDs or side effects. */
export function profileArtwork(preset) {
  const p = preset?.params || {}, effect = preset?.effect;
  const areas = preset?.areas && typeof preset.areas === "object" ? Object.values(preset.areas).slice(0, 16) : [];
  const colors = [...new Set([...palette(p), ...areas.flatMap((a) => palette(a?.params))])].slice(0, 16);
  const stops = Array.isArray(p.colors) ? p.colors.filter(hex).slice(0, 16) : [];
  const simple = ["solid", "breathing", "pulse", "comet"].includes(effect) && hex(p.color);
  const patterned = !areas.length && (simple || (effect === "gradient" && stops.length > 1) || effect === "rainbow");
  const off = effect === "off" || preset?.on === false || preset?.brightness === 0;
  const label = off ? "Lights off" : areas.length ? "Saved area palette" : patterned ? "Saved pattern"
    : effect === "screen" ? "Screen reactive" : colors.length || ["cycle", "spectrum"].includes(effect) ? "Saved palette" : "Preview unavailable";
  const brightness = bounded(preset?.brightness, 0, 100) ? `${preset.brightness}%` : "";
  const angle = param(p.angle, 0, 0, 359) * Math.PI / 180;
  const along = (x, y) => x * Math.cos(angle) + y * Math.sin(angle);
  const sample = (x, y) => {
    if (off) return "#000000";
    if (patterned) {
      if (effect === "rainbow") {
        const h = ((along(x, y) * (0.4 + 2.6 * (1 - param(p.size, 50, 0, 100) / 100))) % 1 + 1) % 1;
        return hue(h, param(p.saturation, 100, 0, 100) / 100);
      }
      if (effect === "gradient") {
        const position = ((along(x, y) * .8) % 1 + 1) % 1;
        const n = (1 - Math.abs(2 * position - 1)) * (stops.length - 1);
        const i = Math.min(stops.length - 2, Math.floor(n));
        return mix(stops[i], stops[i + 1], n - i);
      }
      if (effect === "comet") {
        const distance = ((p.reverse ? x - .8 : .8 - x) + 1) % 1;
        const strength = Math.max(0, 1 - distance / (.04 + .9 * param(p.tail, 45, 0, 100) / 100)) ** 2;
        return mix(hex(p.background) ? p.background : "#000000", p.color, strength);
      }
      return p.color;
    }
    if (colors.length) return colors[Math.min(colors.length - 1, Math.floor(x * colors.length))];
    if (effect === "cycle") return hue(x, param(p.saturation, 100, 0, 100) / 100);
    if (effect === "spectrum") return hue((param(p.hue, 0, 0, 359) / 360 + .66 - x * .66 + 1) % 1);
    return "#16191d";
  };
  let cells = "";
  for (let y = 0; y < 4; y++) for (let x = 0; x < 24; x++) {
    cells += `<rect x="${x * 10}" y="${y * 18}" width="10.1" height="18.1" fill="${sample(x / 23, y / 3)}"/>`;
  }
  const rhythm = effect === "breathing" ? '<path d="M12 36 C35 6 55 6 78 36 S122 66 145 36 S190 6 228 36"/>'
    : effect === "pulse" ? '<path d="M12 44 H63 L81 18 L100 57 L119 29 L137 44 H228"/>' : "";
  const detail = colors.length ? colors.join(", ") : effect === "screen" ? "Colors follow the screen" : "";
  const title = esc(`${preset?.name || "Profile"}: ${label.toLowerCase()}${detail ? `; ${detail}` : ""}${brightness ? `; ${brightness} saved brightness` : ""}`);
  const swatches = colors.map((c) => `<i style="background:${c}" aria-hidden="true"></i>`).join("");
  return `<span class="profile-artwork"><svg viewBox="0 0 240 72" preserveAspectRatio="none" role="img" aria-label="${title}">
    <rect width="240" height="72" fill="#080a0c"/><g opacity="${off ? 0 : param(preset?.brightness, 100, 0, 100) / 100}">${cells}</g>
    ${rhythm && !off ? `<g class="profile-rhythm" aria-hidden="true">${rhythm}</g>` : ""}</svg>
    <span class="profile-art-caption"><span>${label}</span><span>${brightness}</span></span>
    ${swatches ? `<span class="profile-palette" aria-hidden="true">${swatches}</span>` : ""}</span>`;
}

function curvePoints(raw) {
  if (!Array.isArray(raw) || raw.length < 2 || raw.length > 64 || raw.some((p) => !Array.isArray(p) || p.length !== 2
    || !bounded(p[0], 0, 150) || !bounded(p[1], 0, 100))) return null;
  const points = raw.map((p) => [...p]).sort((a, b) => a[0] - b[0]);
  return points.some((p, i) => i && p[0] === points[i - 1][0]) ? null : points;
}

/** Controller demand is requested duty, not a measured physical fan speed. RPM stays in the parent readout. */
export function fanInstrument(fans, curves, { stale = false, tempUnit = "C" } = {}) {
  const points = curvePoints(curves?.curves?.[fans?.curve]);
  const temp = !stale && bounded(fans?.temp, -50, 200) ? fans.temp : null;
  const duty = !stale && bounded(fans?.duty, 0, 100) ? fans.duty : null;
  const tMin = Math.min(20, temp == null ? 20 : Math.floor(temp / 10) * 10, points?.[0][0] ?? 20);
  const tMax = Math.max(100, temp == null ? 100 : Math.ceil(temp / 10) * 10, points?.at(-1)[0] ?? 100);
  const x = (t) => coord(32 + (t - tMin) / (tMax - tMin) * 272);
  const y = (d) => coord(96 - d * .78);
  const temperature = (t) => `${Math.round(tempUnit === "F" ? t * 1.8 + 32 : t)}°${tempUnit === "F" ? "F" : "C"}`;
  const status = stale ? "Waiting for current readings" : temp == null ? "Temperature unavailable" : `${temperature(temp)} input`;
  const demand = stale ? "" : duty == null ? "Request unavailable" : `${Math.round(duty)}% requested`;
  const curveName = typeof fans?.curve === "string" ? fans.curve : "";
  const description = `${points ? `Active curve: ${curveName}` : "Curve unavailable"}. ${status}${demand ? `, ${demand}` : ""}`;
  let graph = '<g class="fan-plot-grid"><path d="M32 18H304M32 57H304M32 96H304"/></g>';
  if (points) {
    const line = [[tMin, points[0][1]], ...points, [tMax, points.at(-1)[1]]].map(([t, d]) => `${x(t)},${y(d)}`).join(" ");
    graph += `<polygon class="fan-curve-area" points="32,96 ${line} 304,96"/><polyline class="fan-curve-line" points="${line}"/>`;
    if (bounded(curves?.safety_temp, tMin, tMax)) graph += `<path class="fan-safety-line" d="M${x(curves.safety_temp)} 18V96"><title>${temperature(curves.safety_temp)}: full-speed safety override</title></path>`;
  }
  if (temp != null && duty != null) {
    graph += `<path class="fan-current-guide" d="M${x(temp)} ${y(duty)}V96"/>`;
  }
  return `<div class="fan-plot${stale ? " is-stale" : ""}"><div class="fan-graph-box"><svg viewBox="0 0 320 124" preserveAspectRatio="none" role="img" aria-label="${esc(description)}">${graph}</svg>
    ${temp != null && duty != null ? `<i class="fan-current" aria-hidden="true" style="left:${x(temp)/320*100}%;top:${y(duty)/124*100}%"></i>` : ""}
    <span class="fan-plot-axis" aria-hidden="true"><span class="fan-axis-high">100%</span><span class="fan-axis-low">0%</span>
      <span class="fan-axis-start">${temperature(tMin)}</span><span class="fan-axis-middle">${temperature((tMin + tMax) / 2)}</span><span class="fan-axis-end">${temperature(tMax)}</span></span></div>
    ${!points ? '<span class="fan-plot-unavailable">Curve unavailable</span>' : ""}
    <div class="fan-plot-caption"><span>${esc(status)}</span><span>${esc(demand)}</span></div></div>`;
}
