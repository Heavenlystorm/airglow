// A trace uses recorder elapsed time. Different runs never share wall-clock timestamps.
const standard = [
  ["cpu", "CPU temperature", "°C"], ["gpu", "GPU temperature", "°C"], ["liquid", "Coolant temperature", "°C"],
  ["duty", "Commanded curve duty", "%"], ["rpm", "Mean measured fan speed", "RPM"],
].map(([id, name, unit]) => ({ id, name, unit, group: "Cooling", key: id }));

export function signals(run) {
  if (!run) return [];
  const extra = (run.sensors || []).map((field, index) => ({
    ...field, index, key: JSON.stringify([field.id, field.unit]),
  }));
  return [...extra, ...standard];
}

export function traceFor(run, key) {
  const metric = signals(run).find(s => s.key === key);
  const rows = [], segments = [];
  let low = null, high = null, sum = 0, count = 0, last = null, line = null;
  for (const sample of run?.samples || []) {
    const time = sample.elapsed;
    if (!Number.isFinite(time) || time < 0 || (last !== null && time < last)) continue;
    const v = !metric ? null : metric.index === undefined ? sample[metric.id] : sample.readings?.[metric.index];
    const value = Number.isFinite(v) ? v : null;
    const row = { time, value };
    rows.push(row);
    // Nominal cadence is 2 s. Keep longer pauses visible even when both endpoints are finite.
    if (value === null || (last !== null && time - last > 6)) line = null;
    if (value !== null) {
      if (!line) { line = []; segments.push(line); }
      line.push(row);
      if (!low || value < low.value) low = row;
      if (!high || value > high.value) high = row;
      sum += value;
      count++;
    }
    last = time;
  }
  return { metric, rows, segments, low, high, count, mean: count ? sum / count : null,
    duration: Math.max(last || 0, Number.isFinite(run?.duration_seconds) ? run.duration_seconds : 0) };
}

export function sharedTimes(traces) {
  return [...new Set(traces.flatMap(t => t?.metric ? t.rows.map(row => row.time) : []))].sort((a, b) => a - b);
}

export function stepTime(stops, time, direction) {
  if (!stops.length) return 0;
  if (direction < 0) {
    for (let i = stops.length - 1; i >= 0; i--) if (stops[i] < time - .0001) return stops[i];
    return stops[0];
  }
  return stops.find(t => t > time + .0001) ?? stops.at(-1);
}

export function nearestRow(rows, time) {
  if (!Number.isFinite(time) || !rows.length) return null;
  let lo = 0, hi = rows.length;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    if (rows[mid].time < time) lo = mid + 1;
    else hi = mid;
  }
  const before = rows[lo - 1], after = rows[lo];
  const best = !after || (before && time - before.time <= after.time - time) ? before : after;
  return Math.abs(best.time - time) <= 1 ? best : null;
}

export function elapsedText(seconds, precise = false) {
  const ms = Math.round(Math.max(0, Number(seconds) || 0) * (precise ? 1000 : 1)) * (precise ? 1 : 1000);
  const whole = Math.floor(ms / 1000), sub = ms % 1000;
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}${precise && sub ? "." + String(sub).padStart(3, "0").replace(/0+$/, "") : ""}`;
}

export function displayScale(unit, traces, fahrenheit = false) {
  const largest = Math.max(0, ...traces.filter(Boolean).flatMap(t => [Math.abs(t.low?.value || 0), Math.abs(t.high?.value || 0)]));
  if (unit === "°C" && fahrenheit) return { unit: "°F", scale: 1.8, offset: 32 };
  if (unit === "B/s") {
    for (const [factor, label] of [[1024 ** 3, "GiB/s"], [1024 ** 2, "MiB/s"], [1024, "KiB/s"]])
      if (largest >= factor) return { unit: label, scale: 1 / factor, offset: 0 };
  }
  if (unit === "MHz" && largest >= 1000) return { unit: "GHz", scale: .001, offset: 0 };
  return { unit: ({ MB: "MiB", GB: "GiB", RPM: "rpm" })[unit] ?? unit, scale: 1, offset: 0 };
}

export function formatValue(value, transform, delta = false) {
  if (!Number.isFinite(value)) return "—";
  const converted = value * transform.scale + (delta ? 0 : transform.offset);
  const precision = Math.abs(converted) >= 100 ? 1 : Math.abs(converted) >= 1 ? 2 : 3;
  const number = Number(converted.toFixed(precision));
  const unit = delta && transform.unit === "%" ? "pp" : transform.unit;
  return `${delta && number > 0 ? "+" : ""}${number.toLocaleString(undefined, { maximumFractionDigits: precision })}${unit ? (/^[°%]/.test(unit) ? "" : " ") + unit : ""}`;
}

export function graphBounds(traces, transform) {
  const values = traces.filter(t => t?.count).flatMap(t => [t.low.value, t.high.value]).map(v => v * transform.scale + transform.offset);
  if (!values.length) return null;
  const min = Math.min(...values), max = Math.max(...values);
  const pad = (max - min) * .09 || Math.max(Math.abs(min) * .025, .01);
  const rough = (max - min + pad * 2) / 4;
  const order = 10 ** Math.floor(Math.log10(rough));
  const step = [1, 2, 2.5, 5, 10].find(n => n * order >= rough) * order;
  let lo = Math.floor((min - pad) / step) * step, hi = Math.ceil((max + pad) / step) * step;
  if (transform.unit === "%") { lo = Math.min(min, Math.max(0, lo)); hi = Math.max(max, Math.min(100, hi)); }
  return { lo, hi };
}
