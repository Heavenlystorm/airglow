export const DEFAULT_FACE = Object.freeze({ layout: "clock", scenery: "glasshouse", format: "system", seconds: false });
export const DESK_FACE_KEY = "airglow.desk.face.v1";
export const DESK_TIMER_KEY = "airglow.desk.timer.v1";

export function facePreferences(source) {
  const pick = (key, choices) => choices.includes(source?.[key]) ? source[key] : DEFAULT_FACE[key];
  return { layout: pick("layout", ["clock", "instruments", "quiet"]), scenery: pick("scenery", ["glasshouse", "dusk", "plain", "daily", ...Array.from({length:6},(_,i)=>"plate-"+i)]),
    format: pick("format", ["system", "12", "24"]), seconds: source?.seconds === true };
}

export function plateForDay(scenery, date) {
  if (scenery === "daily") {
    const day = Math.floor(Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) / 86400000);
    return ((day % 6) + 6) % 6;
  }
  return /^plate-[0-5]$/.test(scenery) ? Number(scenery.slice(-1)) : null;
}

export function readTimer(timer) {
  if (!Number.isInteger(timer?.minutes) || timer.minutes < 1 || timer.minutes > 240) return null;
  if (timer.mode === "running" && Number.isSafeInteger(timer.deadline) && timer.deadline > 0)
    return { minutes: timer.minutes, mode: "running", deadline: timer.deadline };
  if (timer.mode === "paused" && Number.isSafeInteger(timer.left) && timer.left > 0 && timer.left <= timer.minutes * 60000)
    return { minutes: timer.minutes, mode: "paused", left: timer.left };
  return null;
}

export function remaining(timer, now = Date.now()) {
  if (!timer) return 0;
  return timer.mode === "paused" ? timer.left : Math.max(0, Math.min(timer.minutes * 60000, timer.deadline - now));
}

export function durationText(milliseconds) {
  const seconds = Math.ceil(Math.max(0, milliseconds) / 1000);
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2,"0")}`;
}

// Split at missing samples, backwards time, and long gaps. Only real timestamps can extend a trace.
export function minuteTrace(times, samples, end) {
  const points = [];
  let previous = null, min = Infinity, max = -Infinity;
  for (let i = 0; i < times.length; i++) {
    const t = times[i], v = samples[i];
    if (!Number.isFinite(t) || !Number.isFinite(v) || t < end - 60 || t > end + 1) { previous = null; continue; }
    points.push({ t, v, move: previous === null || t - previous > 10 || t <= previous });
    previous = t; min = Math.min(min,v); max = Math.max(max,v);
  }
  if (!points.length) return { path: "", min: null, max: null };
  const pad = Math.max((max - min) * .14, Math.abs(max) * .002, .01);
  const path = points.map(({ t, v, move }) => {
    const x = Math.min(180, ((t - end + 60) / 60) * 180);
    const y = 30 - 28 * (v - min + pad) / (max - min + 2 * pad);
    return `${move ? "M" : "L"}${x.toFixed(2)} ${y.toFixed(2)}${move ? "l0.01 0" : ""}`;
  }).join(" ");
  return { path, min, max };
}
