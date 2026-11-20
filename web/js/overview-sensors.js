// Presentation mathematics only. Values, units and warning levels come from readings.js.
export function sensorFamily(meta, id = '') {
  if (meta?.unit === '°C') return 'thermal';
  if (['GB', 'MB'].includes(meta?.unit) || /ram|memory|vram|storage|disk.*load/i.test(`${id} ${meta?.name || ''}`) && meta?.unit === '%') return 'capacity';
  if (meta?.unit === '%') return 'load';
  if (meta?.unit === 'B/s') return /disk/i.test(id) || meta.hw === 'storage' ? 'signal' : 'network';
  if (meta?.unit === 'W' || meta?.unit === '€/h') return 'power';
  return 'signal';
}

export function instrumentModel({ value, lo = 0, hi = 100, samples = [], thresholds = null }) {
  const fraction = v => Math.max(0, Math.min(1, (v - lo) / (hi - lo || 1)));
  const values = samples.filter(Number.isFinite);
  return {
    missing: !Number.isFinite(value), fraction: Number.isFinite(value) ? fraction(value) : 0,
    warning: Number.isFinite(thresholds?.[0]) ? fraction(thresholds[0]) : null,
    critical: Number.isFinite(thresholds?.[1]) ? fraction(thresholds[1]) : null,
    stats: values.length ? { min: Math.min(...values), max: Math.max(...values), mean: values.reduce((a,b) => a+b,0)/values.length, count: values.length } : null,
  };
}

// A scale fitted to what a reading did since `from`, with headroom, so small movements stay visible in a short
// time span. Percentages stay inside 0–100; a flat reading still gets a sensible band around it.
export function liveRange(meta, points, from, fallback = [0, 100]) {
  const values = (points || []).filter(p => p[0] >= from && Number.isFinite(p[1])).map(p => p[1]);
  if (!values.length) return fallback;
  const low = Math.min(...values), high = Math.max(...values), unit = meta?.unit;
  const least = unit === '°C' ? 8 : unit === '%' ? 24 : unit === 'W' ? 30 : unit === 'RPM' ? 200 : Math.max(Math.abs(high) * .25, 1e-6);
  const span = Math.max(high - low, least) * 1.24, middle = (low + high) / 2;
  let lo = middle - span / 2, hi = middle + span / 2;
  const floor = unit === '°C' ? -Infinity : 0;                 // only a temperature can be below zero
  if (lo < floor) { hi += floor - lo; lo = floor; }
  if (unit === '%' && hi > 100) { lo = Math.max(0, lo - (hi - 100)); hi = 100; }
  return [lo, hi];
}

// Which way a reading moved over the last minute: { state: "rising" | "falling" | "steady", perMinute }, or null
// when there is too little recent history to say. Small wobble counts as steady.
export function trendOf(meta, points, now) {
  const recent = (points || []).filter(p => Number.isFinite(p[1]) && p[0] >= now - 62 && p[0] <= now);
  if (recent.length < 4 || recent.at(-1)[0] - recent[0][0] < 20) return null;
  // compare the average of the oldest and newest quarter, so one noisy sample does not flip the arrow
  const quarter = Math.max(1, Math.floor(recent.length / 4));
  const mean = part => part.reduce((sum, p) => sum + p[1], 0) / part.length, time = part => part.reduce((sum, p) => sum + p[0], 0) / part.length;
  const early = recent.slice(0, quarter), late = recent.slice(-quarter);
  const perMinute = (mean(late) - mean(early)) * 60 / (time(late) - time(early));
  const unit = meta?.unit, level = Math.abs(mean(late));
  const quiet = unit === '°C' ? .6 : unit === '%' ? 4 : unit === 'W' ? 6 : unit === 'RPM' ? 40 : Math.max(level * .04, 1e-6);
  return { perMinute, state: Math.abs(perMinute) < quiet ? 'steady' : perMinute > 0 ? 'rising' : 'falling' };
}

export function peakBuckets(points, now, window, count) {
  const start = now - window;
  const peaks = Array(count).fill(null);
  for (const [time, value] of points) {
    if (!Number.isFinite(value) || time < start || time > now) continue;
    const bucket = Math.min(count - 1, Math.floor((time - start) / window * count));
    peaks[bucket] = Math.max(peaks[bucket] ?? value, value);
  }
  return peaks;
}
