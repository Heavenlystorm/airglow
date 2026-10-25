// Sensor readings for the Overview, the top bar and the picker: catalog, ten-minute history, formatting, ranges.

import { S, get, unitF } from "./core.js";

export const WINDOW = 600;                 // seconds of history shown in graphs
export const R = { meta: {}, series: {}, loaded: false };

export async function loadCatalog() {
  const r = await get("/api/sensors");
  R.meta = Object.fromEntries(r.sensors.map((m) => [m.id, m]));
  R.lhm = r.lhm_online;
  R.loaded = true;
  return r.sensors;
}

// Fetch history for sensors we don't have yet (a tile was just added, or on start).
export async function ensureHistory(ids) {
  const missing = ids.filter((i) => !R.series[i]);
  if (!missing.length) return;
  const h = await get(`/api/sensors/history?ids=${encodeURIComponent(missing.join(","))}`);
  for (const id of missing) {
    const vals = h.series[id] || [];
    R.series[id] = h.t.map((t, k) => [t, vals[k] ?? null]).filter((p) => p[1] != null);
  }
}

export function pushReadings(readings) {
  const t = Date.now() / 1000, cutoff = t - WINDOW;
  for (const [id, v] of Object.entries(readings || {})) {
    const s = (R.series[id] ||= []);
    if (v != null) s.push([t, v]);
    while (s.length && s[0][0] < cutoff) s.shift();
    if (R.meta[id]) R.meta[id].value = v;
  }
}

export const value = (id) => (S.state?.readings && id in S.state.readings ? S.state.readings[id] : R.meta[id]?.value ?? null);

// ------------------------------------------------------------ formatting
// Returns { num, unit } so the number and unit can be set in different sizes.
export function fmt(meta, v, compact = false) {
  if (v == null || !meta) return { num: "–", unit: meta ? displayUnit(meta) : "" };
  const u = meta.unit;
  if (u === "°C") return { num: String(Math.round(unitF() ? v * 9 / 5 + 32 : v)), unit: unitF() ? "°F" : "°C" };
  if (u === "%") return { num: String(Math.round(v)), unit: "%" };
  if (u === "W") return { num: String(Math.round(v)), unit: "W" };
  if (u === "MHz") return v >= 1000 ? { num: (v / 1000).toFixed(2), unit: "GHz" } : { num: String(Math.round(v)), unit: "MHz" };
  if (u === "GB") return { num: v >= 100 ? String(Math.round(v)) : v.toFixed(1), unit: "GB" };
  if (u === "MB") return v >= 1024 ? { num: (v / 1024).toFixed(1), unit: "GB" } : { num: String(Math.round(v)), unit: "MB" };
  if (u === "B/s") {
    for (const [div, unit] of [[1024 ** 3, "GB/s"], [1024 ** 2, "MB/s"], [1024, "KB/s"]]) {
      if (v >= div) return { num: (v / div).toFixed(v / div >= 100 ? 0 : 1), unit };
    }
    return { num: String(Math.round(v)), unit: "B/s" };
  }
  if (u === "RPM") return { num: Math.round(v).toLocaleString(), unit: compact ? "rpm" : "rpm" };
  if (u === "€/h") return { num: `€${v.toFixed(2)}`, unit: "/h" };
  if (u === "V") return { num: v.toFixed(3), unit: "V" };
  if (u === "L/h" || u === "A") return { num: v.toFixed(1), unit: u };   // GB is handled above
  return { num: Math.abs(v) >= 100 ? String(Math.round(v)) : String(+v.toFixed(1)), unit: u || "" };
}
export const fmtText = (meta, v) => { const f = fmt(meta, v); return f.num === "–" ? "–" : `${f.num}${f.unit.startsWith("°") || f.unit === "%" || f.unit.startsWith("/") ? "" : " "}${f.unit}`; };
const displayUnit = (meta) => (meta.unit === "°C" ? (unitF() ? "°F" : "°C") : meta.unit === "MB" ? "MB" : meta.unit);

// Temperatures in the user's unit for graphs; everything else as-is.
export const toDisplay = (meta, v) => (meta?.unit === "°C" && unitF() ? v * 9 / 5 + 32 : v);

// ------------------------------------------------------------ ranges and warning levels
const LEVELS = {
  "auto/cpu_temp": [80, 90], "auto/gpu_temp": [75, 85], "auto/gpu_hotspot": [90, 100], "auto/ssd_temp": [70, 80],
  "auto/ram_temp": [55, 70], "auto/ram_load": [85, 95], "sys/ram_load": [85, 95],
};
export function level(id, meta, v) {
  if (v == null || !meta) return "";
  const lv = LEVELS[id] || (meta.unit === "°C" ? [80, 95] : null);
  if (!lv) return "";
  return v >= lv[1] ? "hot" : v >= lv[0] ? "warm" : "";
}

const niceCeil = (v) => {
  if (v <= 0) return 1;
  const p = 10 ** Math.floor(Math.log10(v)), n = v / p;
  return (n <= 1 ? 1 : n <= 2 ? 2 : n <= 2.5 ? 2.5 : n <= 5 ? 5 : 10) * p;
};
const POWER_FLOOR = { "auto/cpu_power": 120, "auto/gpu_power": 250, "auto/total_power": 400 };

// [min, max] for gauges, bars and graph scales, in the sensor's own unit.
export function range(id, meta, series = R.series[id] || []) {
  if (!meta) return [0, 100];
  if (meta.max) return [0, meta.max];
  const peak = Math.max(0, ...series.map((p) => p[1]), meta.value ?? 0);
  switch (meta.unit) {
    case "%": return [0, 100];
    case "°C": return [20, Math.max(100, niceCeil(peak * 1.1))];
    case "W": return [0, niceCeil(Math.max(POWER_FLOOR[id] || 50, peak * 1.15))];
    case "RPM": return [0, niceCeil(Math.max(1500, peak * 1.1))];
    case "B/s": return [0, niceCeil(Math.max(1024 * 1024, peak * 1.2))];
    case "€/h": return [0, niceCeil(Math.max(0.05, peak * 1.2))];
    default: return [Math.min(0, ...series.map((p) => p[1])), niceCeil(Math.max(1, peak * 1.15))];
  }
}

// Sample a series at N evenly spaced moments across the window, so a path always has the same number of points and
// the browser can morph it smoothly from one update to the next.
export function resample(series, n = 61, now = Date.now() / 1000) {
  if (!series || !series.length) return [];
  const start = now - WINDOW, step = WINDOW / (n - 1), out = [];
  let j = 0;
  for (let k = 0; k < n; k++) {
    const t = start + k * step;
    if (t < series[0][0] - step) { out.push(null); continue; }        // before the first sample: nothing yet
    if (t > series[series.length - 1][0] + step) { out.push(null); continue; }   // after the last: not a flat line
    while (j < series.length - 1 && series[j + 1][0] <= t) j++;
    const a = series[j], b = series[j + 1];
    out.push(b && t > a[0] ? a[1] + (b[1] - a[1]) * Math.min(1, (t - a[0]) / (b[0] - a[0])) : a[1]);
  }
  return out;
}

// Default look for a newly added sensor tile.
export function defaultStyle(meta) {
  if (!meta) return "spark";
  if (meta.unit === "%" || meta.unit === "RPM") return "ring";
  if (meta.max || meta.unit === "GB" || meta.unit === "MB") return "bar";
  return "spark";
}

export function sensorName(id) {
  return R.meta[id]?.name || (id.startsWith("auto/") ? id.slice(5).replace(/_/g, " ") : "Sensor");
}
