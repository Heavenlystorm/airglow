import { interpolateCurve } from "./fans.js";

export const MIN_TEMP = 20;
export const MAX_TEMP = 100;
export const MAX_POINTS = 8;
export const displayTemp = (c, fahrenheit) => fahrenheit ? Math.round((c * 1.8 + 32) * 10) / 10 : c;
const celsius = (n, fahrenheit) => fahrenheit ? (n - 32) / 1.8 : n;

export function pointRows(points, fahrenheit) {
  return points.map(([t, d]) => [String(displayTemp(t, fahrenheit)), String(d)]);
}

export function parseRows(rows, minDuty, fahrenheit) {
  if (rows.length < 2 || rows.length > MAX_POINTS) return { error: "A curve needs two to eight points." };
  const points = [], used = new Set();
  for (const [i, [rawTemp, rawDuty]] of rows.entries()) {
    const input = Number(rawTemp), d = Number(rawDuty), t = Math.round(celsius(input, fahrenheit));
    if (!rawTemp.trim() || !Number.isFinite(input) || !Number.isFinite(t)
        || input < displayTemp(MIN_TEMP, fahrenheit) || input > displayTemp(MAX_TEMP, fahrenheit)) {
      return { error: `Point ${i + 1} needs a temperature from ${displayTemp(MIN_TEMP, fahrenheit)} to ${displayTemp(MAX_TEMP, fahrenheit)} °${fahrenheit ? "F" : "C"}.` };
    }
    if (!fahrenheit && !Number.isInteger(input)) return { error: `Point ${i + 1} needs a whole temperature in °C.` };
    if (!rawDuty.trim() || !Number.isInteger(d) || d < minDuty || d > 100) {
      return { error: `Point ${i + 1} needs a whole duty from ${minDuty} to 100%.` };
    }
    if (used.has(t)) return { error: `Point ${i + 1} shares a temperature with another point. Choose a different whole degree Celsius.` };
    used.add(t);
    points.push([t, d]);
  }
  return { points: points.sort((a, b) => a[0] - b[0]) };
}

// The Python controller uses round() on outgoing commands, which rounds exact halves to the even integer.
function commandPercent(n) {
  const whole = Math.floor(n);
  return n - whole === .5 ? whole + (whole % 2) : Math.round(n);
}

export function demand(points, t, { failsafe, safety, maxStep, previous, hot }) {
  if (!points?.length) return { target: null, next: null };
  const last = Number.isFinite(previous) ? previous : null;
  let target;
  if (hot || (Number.isFinite(t) && t >= safety)) target = 100;
  else if (!Number.isFinite(t)) target = Number.isFinite(failsafe) ? Math.max(failsafe, last ?? 0) : null;
  else target = interpolateCurve(points, t);
  let next = null;
  if (target !== null) {
    if (!Number.isFinite(t) || target >= 100 || last === null) next = commandPercent(target);
    else if (Number.isFinite(maxStep)) next = commandPercent(last + Math.max(-maxStep, Math.min(maxStep, target - last)));
  }
  return { target, next };
}

export function targetLine(points, safety, hot = false) {
  if (!points?.length) return [];
  if (hot || safety <= MIN_TEMP) return [[MIN_TEMP, 100], [MAX_TEMP, 100]];
  const start = [[MIN_TEMP, interpolateCurve(points, MIN_TEMP)]];
  const shape = points.filter(([t]) => t > MIN_TEMP && t < Math.min(safety, MAX_TEMP));
  if (safety > MAX_TEMP) return [...start, ...shape, [MAX_TEMP, interpolateCurve(points, MAX_TEMP)]];
  return [...start, ...shape, [safety, interpolateCurve(points, safety)], [safety, 100], [MAX_TEMP, 100]];
}

export function largestGapPoint(points, temperature) {
  const taken = new Set(points.map(p => p[0]));
  let t = temperature;
  if (!Number.isInteger(t) || t < MIN_TEMP || t > MAX_TEMP || taken.has(t)) {
    const marks = [...new Set([MIN_TEMP, ...taken, MAX_TEMP])].sort((a, b) => a - b);
    const gaps = marks.slice(1).map((v, i) => [marks[i], v]).sort((a,b) => (b[1] - b[0]) - (a[1] - a[0]));
    t = Math.round((gaps[0][0] + gaps[0][1]) / 2);
  }
  return [t, Math.round(interpolateCurve(points, t))];
}
