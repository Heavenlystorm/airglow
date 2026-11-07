// Presentation mathematics only. Values, units and warning levels come from readings.js.
export function sensorFamily(meta, id = '') {
  if (meta?.unit === '°C') return 'thermal';
  if (['GB', 'MB'].includes(meta?.unit) || /ram|memory|vram|storage|disk.*load/i.test(`${id} ${meta?.name || ''}`) && meta?.unit === '%') return 'capacity';
  if (meta?.unit === '%') return 'load';
  if (meta?.unit === 'B/s') return 'network';
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
