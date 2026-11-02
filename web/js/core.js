// Shared helpers: API calls, toasts, DOM, formatting, colour maths, and a tiny event bus.

export const $ = (sel, el = document) => el.querySelector(sel);
export const $$ = (sel, el = document) => [...el.querySelectorAll(sel)];
export const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
export const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
export const cap = (s) => (s ? s[0].toUpperCase() + s.slice(1) : "");

// Replace markup only when it changed, so polling never swaps a control out from under the pointer.
export function setHTML(el, html) {
  if (el._html !== html) { el.innerHTML = html; el._html = html; }
}

// ------------------------------------------------------------ api
// Every call gives up instead of waiting for ever: a wedged server used to leave the window on old readings with no
// way back until Airglow was restarted.
async function request(method, path, body, timeout = 8000) {
  const opts = { method, headers: {} };
  if (method === "POST") {
    opts.headers = { "Content-Type": "application/json", "X-Airglow": "1" };
    opts.body = JSON.stringify(body ?? {});
  }
  const abort = new AbortController();
  opts.signal = abort.signal;
  const timer = setTimeout(() => abort.abort(), timeout);
  try {
    const res = await fetch(path, opts);
    const data = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(data.error || `Request failed (${res.status})`);
    return data;
  } catch (e) {
    if (e.name === "AbortError") throw new Error("Airglow didn't answer in time");
    throw e;
  } finally {
    clearTimeout(timer);
  }
}
export const get = (p, timeout) => request("GET", p, undefined, timeout);

// Serialize writes to the same resource across editors. Reads and unrelated resources remain independent.
const mutationTails = new Map(), mutations = new Map();
let mutationId = 0;
function mutationInfo(path, body) {
  if (path === "/api/scene") return ["lighting", "Lighting", "lighting", true];
  if (/^\/api\/presets\/[^/]+\/apply$/.test(path) || /^\/api\/profiles\/(try|discard|keep)$/.test(path))
    return ["lighting", "Profile application", "lighting", false];
  if (path === "/api/studio/use") return ["lighting", "Effect application", "studio", false];
  if (path === "/api/studio/draft") return ["lighting", "Live preview", "studio", false, true];
  if (/^\/api\/zones\/[^/]+$/.test(path)) return [path, "Zone settings", "devices", true];
  if (/^\/api\/curves\/[^/]+(?:\/reset)?$/.test(path)) return [path.replace(/\/reset$/, ""), "Fan curve", "fans", true];
  if (path === "/api/fan_curve") return ["fan-curve", "Active fan curve", "fans", true];
  if (path === "/api/fans/channel") return [`fan:${body?.id}`, "Fan or pump setting", "fans", true];
  if (path === "/api/settings") return ["settings", "Settings", "settings", true];
  if (path === "/api/overview") return ["overview", "Overview layout", "overview", true];
  if (path === "/api/automations") return ["automations", "Automations", "automations", true];
  if (path === "/api/presets" || path === "/api/presets/from-current")
    return ["profiles", "Profiles", "lighting", path === "/api/presets" || !!body?.id];
  return null;
}

export const mutationStates = () => [...mutations.values()].filter((s) => !s.silent).map(({ body, timeout, ...s }) => ({ ...s }));
const mutationChanged = () => emit("mutations", mutationStates());
export function dismissMutation(key, id) {
  if (mutations.get(key)?.id !== id || mutations.get(key)?.status !== "failed") return;
  mutations.delete(key); mutationChanged();
}
export function retryMutation(key, id) {
  const s = mutations.get(key);
  if (!s || s.id !== id || s.status !== "failed" || !s.retry) return Promise.resolve(false);
  return post(s.path, s.body, s.timeout);
}

export function post(path, body, timeout) {
  const info = mutationInfo(path, body);
  if (!info) return request("POST", path, body, timeout);
  const [key, label, view, retry, silent = false] = info;
  const snapshot = structuredClone(body ?? {}), id = ++mutationId;
  if (!silent) {
    mutations.set(key, { key, id, label, view, retry, silent, path, body: snapshot, timeout, status: "queued", error: "" });
    while (mutations.size > 64) mutations.delete(mutations.keys().next().value);
    mutationChanged();
  } else if (mutations.delete(key)) {
    // A newer preview owns this resource even though it has its own status UI.
    // An older failed scene write must never remain available to interrupt it.
    mutationChanged();
  }
  const update = (status, error = "") => {
    if (mutations.get(key)?.id === id) {
      Object.assign(mutations.get(key), { status, error }); mutationChanged();
    }
  };
  const run = async () => {
    update("saving");
    try {
      const result = await request("POST", path, snapshot, timeout);
      update("saved");
      const applied = /^\/api\/presets\/([^/]+)\/apply$/.exec(path);
      if (applied && result.ok === true) emit("profile:applied", decodeURIComponent(applied[1]));
      return result;
    } catch (error) {
      update("failed", error.message || String(error));
      throw error;
    }
  };
  const previous = mutationTails.get(key), result = previous ? previous.then(run) : run();
  const tail = result.then(() => {}, () => {}).finally(() => {
    if (mutationTails.get(key) === tail) mutationTails.delete(key);
  });
  mutationTails.set(key, tail);
  return result;
}

let toastTimer;
export function toast(msg, isErrorOrOptions = false) {
  const opts = typeof isErrorOrOptions === "boolean" ? { isError: isErrorOrOptions } : (isErrorOrOptions || {});
  const t = $("#toast");
  t.inert = false;
  t.replaceChildren();
  const text = document.createElement("span");
  text.textContent = msg;
  t.append(text);
  let actionButton = null;
  if (opts.action) {
    actionButton = document.createElement("button");
    actionButton.type = "button";
    actionButton.className = "toast-action";
    actionButton.textContent = opts.action.label;
    actionButton.addEventListener("click", () => {
      clearTimeout(toastTimer);
      t.classList.remove("show", "has-action");
      t.inert = true;
      opts.action.onClick?.();
    });
    t.append(actionButton);
  }
  t.classList.toggle("error", !!opts.isError);
  t.classList.toggle("has-action", !!opts.action);
  t.classList.add("show");
  if (actionButton) actionButton.focus({ preventScroll: true });
  clearTimeout(toastTimer);
  const duration = opts.duration ?? (opts.action ? 9000 : opts.isError ? 4500 : 2200);
  toastTimer = setTimeout(() => {
    t.classList.remove("show", "has-action");
    t.inert = true;
  }, duration);
}
export const fail = (err) => toast(err.message || String(err), true);

// ------------------------------------------------------------ shared state + events
export const S = { state: null, effects: [], presets: [], curves: null, zones: null, settings: null, rules: [], history: [] };
let zonesRequest = null;
export function refreshZones() {
  const pending = get("/api/zones").then((zones) => {
    // Older readers wait for the newest request instead of reinstalling a stale whole-device snapshot.
    if (zonesRequest !== pending) return zonesRequest;
    S.zones = zones;
    return zones;
  });
  zonesRequest = pending;
  return pending;
}
const listeners = {};
export const on = (ev, fn) => (listeners[ev] ||= []).push(fn);
export const emit = (ev, data) => (listeners[ev] || []).forEach((fn) => { try { fn(data); } catch (e) { console.error(e); } });

// ------------------------------------------------------------ formatting
export const unitF = () => S.state?.temp_unit === "F";
export const toUnit = (c) => (unitF() ? c * 9 / 5 + 32 : c);
export const fmtTemp = (c) => (c == null ? "–" : `${Math.round(toUnit(c))}°${unitF() ? "F" : "C"}`);
export const tempLevel = (c, warm, hot) => (c == null ? "" : c >= hot ? "hot" : c >= warm ? "warm" : "");
export const curveLabel = (c) => ({ silent: "Silent", balanced: "Balanced", performance: "Performance", trial: "Trial" }[c]
  || (c?.startsWith("profile:") ? S.presets.find((p) => p.id === c.slice(8))?.name || "Profile" : cap(c)));
export const effectName = (id) => S.effects.find((e) => e.id === id)?.name || cap(id);

// ------------------------------------------------------------ colour
export const hexToRgb = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
export const rgbToHex = (rgb) => "#" + rgb.map((v) => clamp(Math.round(v), 0, 255).toString(16).padStart(2, "0")).join("");
export const QUICK = ["#ffffff", "#ff1040", "#ff5a1f", "#ffb21a", "#c8ff2a", "#22e06b", "#00e0c8", "#00b4ff", "#2f5bff", "#8a3dff", "#ff2fb4"];

// A representative colour for a scene or preset (for preset dots and the like).
export function sceneColor(s) {
  const p = s?.params || {};
  if (p.color) return p.color;
  if (p.colors) return p.colors[0];
  if (p.cool) return p.cool;
  return { rainbow: "#ff5a1f", cycle: "#22e06b", spectrum: "#2f5bff", screen: "#00b4ff", off: "#3a3b40" }[s?.effect] || "#e9e5da";
}

// CSS background that previews a scene: solid colour, its gradient, or a colour wheel for rainbow-style effects.
export function sceneSwatch(s) {
  const p = s?.params || {};
  if (["rainbow", "cycle", "spectrum"].includes(s?.effect)) return "conic-gradient(#f33, #fb3, #3f6, #3ef, #36f, #c3f, #f33)";
  if (p.colors) return `linear-gradient(135deg, ${p.colors.join(", ")})`;
  if (s?.effect === "temperature") return `linear-gradient(135deg, ${p.cool}, ${p.hot})`;
  if (s?.effect === "comet") return `radial-gradient(circle at 30% 30%, ${p.color} 0 35%, ${p.background} 70%)`;
  return sceneColor(s);
}

export function rangeFill(input) {
  const v = (input.value - input.min) / (input.max - input.min) * 100;
  input.style.setProperty("--fill", `${v}%`);
}

export function debounce(fn, ms) {
  let t;
  return (...a) => { clearTimeout(t); t = setTimeout(() => fn(...a), ms); };
}

// Coalesce rapid writes (sliders): first call goes out within `ms`, later calls merge into the next send.
export function coalesce(send, ms = 70) {
  let pending = null, timer = null, sending = false;
  const schedule = () => {
    if (timer || sending || !pending) return;
    timer = setTimeout(async () => {
      const body = pending; pending = null; timer = null;
      sending = true;
      try { await send(body); } catch (e) { fail(e); }
      finally { sending = false; schedule(); }
    }, ms);
  };
  return (patch) => {
    pending = { ...(pending || {}), ...patch };
    schedule();
  };
}
