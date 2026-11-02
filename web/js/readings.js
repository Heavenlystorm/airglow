// Sensor readings for the Overview, the top bar and the picker: catalog, ten-minute history, formatting, ranges.

import { S, emit, get, unitF } from "./core.js";

export const WINDOW = 600;                 // seconds of history shown in graphs
export const R = { meta: {}, series: {}, loaded: false, stale: false };
let catalogRequest = null;
const historyLoaded = new Set(), historyRequests = new Map();
const reading = (v) => Number.isFinite(v) ? v : null;
const MAX_GAP = 10;                         // normal sampling is every two seconds

export function loadCatalog() {
  if (catalogRequest) return catalogRequest;
  catalogRequest = get("/api/sensors").then((r) => {
    R.meta = Object.fromEntries(r.sensors.map((m) => [m.id, m]));
    R.lhm = r.lhm_online;
    R.revision = r.revision;
    R.loaded = true;
    emit("catalog");
    return r.sensors;
  }).finally(() => { catalogRequest = null; });
  return catalogRequest;
}

// Fetch history for sensors we don't have yet (a tile was just added, or on start).
export async function ensureHistory(ids) {
  const missing = [...new Set(ids)].filter((id) => !historyLoaded.has(id) && !historyRequests.has(id));
  for (let offset = 0; offset < missing.length; offset += 40) {
    const batch = missing.slice(offset, offset + 40);     // the API caps each history request at forty ids
    const request = get(`/api/sensors/history?ids=${encodeURIComponent(batch.join(","))}`).then((h) => {
      const cutoff = Date.now() / 1000 - WINDOW;
      for (const id of batch) {
        const vals = h.series[id] || [];
        // A delayed response must not overwrite live samples. Keep explicit gaps as well as values.
        const points = new Map([...h.t.map((t, k) => [t, reading(vals[k])]), ...(R.series[id] || [])]);
        R.series[id] = [...points].filter(([t]) => Number.isFinite(t) && t >= cutoff).sort((a, b) => a[0] - b[0]);
        historyLoaded.add(id);
      }
    }).finally(() => batch.forEach((id) => historyRequests.delete(id)));
    batch.forEach((id) => historyRequests.set(id, request));
  }
  await Promise.all([...new Set(ids.map((id) => historyRequests.get(id)).filter(Boolean))]);
}

export function pushReadings(readings) {
  R.stale = false;
  const t = Date.now() / 1000, cutoff = t - WINDOW;
  for (const id of new Set([...Object.keys(R.series), ...Object.keys(readings || {})])) {
    const v = reading(readings?.[id]);
    const s = (R.series[id] ||= []);
    if (s.at(-1)?.[0] === t) s[s.length - 1][1] = v;
    else s.push([t, v]);
    while (s.length && s[0][0] < cutoff) s.shift();
    // /api/state contains tracked readings only; omitted picker entries still have their catalog snapshot.
    if (R.meta[id] && id in (readings || {})) R.meta[id].value = v;
    if (!(id in (readings || {})) && !s.some((p) => p[1] != null)) {
      delete R.series[id]; historyLoaded.delete(id);
    }
  }
}

export function markReadingsStale() {
  pushReadings({});
  R.stale = true;
}

export const value = (id) => R.stale ? null : reading(S.state?.readings && id in S.state.readings ? S.state.readings[id] : R.meta[id]?.value);

// ------------------------------------------------------------ formatting
// Returns { num, unit } so the number and unit can be set in different sizes.
export function fmt(meta, v, compact = false) {
  if (!Number.isFinite(v) || !meta) return { num: "–", unit: meta ? displayUnit(meta) : "" };
  const u = meta.unit;
  if (u === "°C") return { num: String(Math.round(unitF() ? v * 9 / 5 + 32 : v)), unit: unitF() ? "°F" : "°C" };
  if (u === "%") return { num: String(Math.round(v)), unit: "%" };
  if (u === "W") return { num: String(Math.round(v)), unit: "W" };
  if (u === "MHz") return v >= 1000 ? { num: (v / 1000).toFixed(2), unit: "GHz" } : { num: String(Math.round(v)), unit: "MHz" };
  if (u === "GB") return { num: v >= 100 ? String(Math.round(v)) : v.toFixed(1), unit: "GiB" };
  if (u === "MB") return v >= 1024 ? { num: (v / 1024).toFixed(1), unit: "GiB" } : { num: String(Math.round(v)), unit: "MiB" };
  if (u === "B/s") {
    for (const [div, unit] of [[1024 ** 3, "GiB/s"], [1024 ** 2, "MiB/s"], [1024, "KiB/s"]]) {
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
const displayUnit = (meta) => (meta.unit === "°C" ? (unitF() ? "°F" : "°C") : ({ MB: "MiB", GB: "GiB" }[meta.unit] || meta.unit));

// Temperatures in the user's unit for graphs; everything else as-is.
export const toDisplay = (meta, v) => !Number.isFinite(v) ? null : (meta?.unit === "°C" && unitF() ? v * 9 / 5 + 32 : v);

// ------------------------------------------------------------ ranges and warning levels
const LEVELS = {
  "auto/cpu_temp": [80, 90], "auto/gpu_temp": [75, 85], "auto/gpu_hotspot": [90, 100], "auto/ssd_temp": [70, 80],
  "auto/ram_temp": [55, 70], "auto/ram_load": [85, 95], "sys/ram_load": [85, 95],
};
export function level(id, meta, v) {
  if (!Number.isFinite(v) || !meta) return "";
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
  if (Number.isFinite(meta.max) && meta.max > 0) return [0, meta.max];
  const vals = [...series.map((p) => p[1]), meta.value].filter(Number.isFinite);
  const peak = Math.max(0, ...vals);
  switch (meta.unit) {
    case "%": return [0, 100];
    case "°C": return [Math.floor(Math.min(20, ...vals) / 10) * 10, Math.max(100, niceCeil(peak * 1.1))];
    case "W": return [0, niceCeil(Math.max(POWER_FLOOR[id] || 50, peak * 1.15))];
    case "RPM": return [0, niceCeil(Math.max(1500, peak * 1.1))];
    case "B/s": return [0, niceCeil(Math.max(1024 * 1024, peak * 1.2))];
    case "€/h": return [0, niceCeil(Math.max(0.05, peak * 1.2))];
    default: return [Math.min(0, ...vals), niceCeil(Math.max(1, peak * 1.15))];
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
    if (t < series[0][0]) { out.push(null); continue; }
    if (t > series[series.length - 1][0] + MAX_GAP) { out.push(null); continue; }
    let crossedGap = false;
    while (j < series.length - 1 && series[j + 1][0] <= t) {
      if (!Number.isFinite(series[j][1]) || !Number.isFinite(series[j + 1][1]) || series[j + 1][0] - series[j][0] > MAX_GAP) crossedGap = true;
      j++;
    }
    const a = series[j], b = series[j + 1];
    if (!Number.isFinite(a[1]) || (b && t > a[0] && (!Number.isFinite(b[1]) || b[0] - a[0] > MAX_GAP))) {
      out.push(null);
    } else {
      // A short outage can fall entirely between two graph points; retain a break instead of hiding it.
      if (crossedGap && out.length && out.at(-1) != null) out[out.length - 1] = null;
      out.push(b && t > a[0] ? a[1] + (b[1] - a[1]) * Math.min(1, (t - a[0]) / (b[0] - a[0])) : a[1]);
    }
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
