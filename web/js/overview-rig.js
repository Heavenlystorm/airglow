// A live portrait of the configured lighting layout. This component only reads
// zones/frames; its two callbacks leave navigation and persistence to Overview.
import { paintRig } from "./stage.js";

let instance = 0;
const activeZones = zones => zones.filter(z => z.device?.online && z.config?.enabled && z.positions?.length);

// Coordinates are the painter's displayed pixels, not clientWidth: both canvas
// drawing and pointer positions therefore follow browser/CSS zoom identically.
export function pickRigZone(zones, transform, x, y) {
  if (!transform?.s) return null;
  const { ox, oy, s, u } = transform, scale = s * u;
  let nearest = null, distance = 18;
  for (const z of zones) for (const [px, py] of z.positions) {
    const d = Math.hypot(ox + px * scale - x, oy + py * scale - y);
    if (d < distance) { distance = d; nearest = z.key; }
  }
  if (nearest) return nearest;
  const px = (x - ox) / scale, py = (y - oy) / scale;
  for (const z of zones) {
    const L = z.config.layout, a = L.angle * Math.PI / 180;
    const dx = px - L.x, dy = py - L.y;
    const localX = dx * Math.cos(a) + dy * Math.sin(a), localY = -dx * Math.sin(a) + dy * Math.cos(a);
    if (L.shape === "fans") {
      for (let f = 0; f < L.fans; f++) {
        const offset = (f - (L.fans - 1) / 2) * L.size * 1.12;
        if (Math.hypot(localX - offset, localY) <= L.size / 2) return z.key;
      }
    } else if (L.shape === "matrix" && Math.abs(localX) <= L.length / 2 && Math.abs(localY) <= L.length / 2 * L.rows / L.cols) return z.key;
    else if (L.shape !== "fans" && L.shape !== "matrix") {
      // Strips follow their actual points, including one-LED device zones.
      for (let i = 1; i < z.positions.length; i++) {
        const [ax, ay] = z.positions[i - 1], [bx, by] = z.positions[i];
        const length2 = (bx - ax) ** 2 + (by - ay) ** 2;
        const t = length2 ? Math.max(0, Math.min(1, ((px - ax) * (bx - ax) + (py - ay) * (by - ay)) / length2)) : 0;
        if (Math.hypot(px - ax - t * (bx - ax), py - ay - t * (by - ay)) * scale <= 14) return z.key;
      }
    }
  }
  return null;
}

export function createOverviewRig(host, { onSelect = () => {}, onViewChange = () => {}, isVisible = () => true } = {}) {
  const prefix = `overview-rig-${++instance}`;
  const make = (tag, className, text) => {
    const el = document.createElement(tag); el.className = className;
    if (text != null) el.textContent = text;
    return el;
  };
  const root = make("div", "overview-rig"), toolbar = make("div", "overview-rig-toolbar");
  const switcher = make("div", "overview-rig-switch"), selector = make("select", "overview-rig-zones");
  switcher.setAttribute("role", "group"); switcher.setAttribute("aria-label", "Lighting preview view");
  const buttons = ["2d", "3d"].map(view => {
    const button = make("button", "overview-rig-view", view.toUpperCase()); button.type = "button";
    button.setAttribute("aria-label", `${view.toUpperCase()} lighting view`); switcher.append(button); return button;
  });
  selector.setAttribute("aria-label", "Open lighting zone");
  toolbar.append(selector, switcher);
  const surface = make("div", "overview-rig-surface"), canvas = make("canvas", "overview-rig-canvas");
  canvas.setAttribute("role", "img"); canvas.setAttribute("aria-label", "Live lighting layout. Select a zone from the list to edit its lighting.");
  const empty = make("div", "overview-rig-empty", "No active lighting zones"); empty.hidden = true;
  const status = make("span", "overview-rig-status"); status.setAttribute("role", "status"); status.hidden = true;
  surface.append(canvas, empty, status); root.append(toolbar, surface); host.append(root);
  let zones = [], shownZones = [], frames = {}, view = "2d", selected = null, transform = null;
  let rig3d = null, modulePromise = null, request = 0, loading = false, failed3d = false, pending = null, disposed = false;
  let mounted = host.isConnected;
  const listeners = new AbortController();
  const listen = (el, event, fn) => el.addEventListener(event, fn, { signal: listeners.signal });
  const shown = () => !disposed && host.isConnected && !document.hidden && isVisible();
  const stop = () => {
    if (pending !== null) cancelAnimationFrame(pending);
    pending = null; request++; loading = false;
    buttons[1].removeAttribute("aria-busy"); rig3d?.setVisible(false);
  };
  function paint() {
    pending = null;
    if (!shown() || view === "3d" && rig3d && !failed3d) return;
    transform = null;
    const r = canvas.getBoundingClientRect();
    paintRig(canvas, frames, { zones: shownZones, pad: 38, maxScale: Math.max(.6, Math.min(r.width, r.height) / 220),
      overlay: (_ctx, value) => { transform = value; } });
  }
  function requestPaint() { if (pending === null && shown()) pending = requestAnimationFrame(paint); }
  function chooseZone(key) {
    if (!shownZones.some(z => z.key === key)) return;
    selected = key; selector.value = key; rig3d?.select(key); onSelect(key);
  }
  function controls() {
    const active3d = view === "3d" && !!rig3d && !failed3d;
    canvas.hidden = active3d;
    empty.hidden = shownZones.length > 0;
    buttons[0].setAttribute("aria-pressed", String(view !== "3d" || failed3d));
    buttons[1].setAttribute("aria-pressed", String(view === "3d" && !failed3d));
    root.dataset.view = active3d ? "3d" : "2d";
  }
  function unavailable() {
    failed3d = true; rig3d?.dispose(); rig3d = null;
    status.textContent = "3D is unavailable. Your layout is shown in 2D."; status.hidden = false;
    controls(); requestPaint();
  }
  async function load3d() {
    if (loading || rig3d || failed3d || !shown()) return;
    loading = true; const token = ++request; buttons[1].setAttribute("aria-busy", "true");
    try {
      modulePromise ||= import("./rig-3d.js").catch(error => { modulePromise = null; throw error; });
      const { createRig3D } = await modulePromise;
      if (token !== request || !shown() || view !== "3d") return;
      rig3d = createRig3D(surface, { compact: true, idPrefix: prefix, onSelect: chooseZone, onUnavailable: unavailable,
        isVisible: () => shown() && view === "3d" });
      rig3d.setZones(zones); rig3d.setFrames(frames); rig3d.select(selected); rig3d.setVisible(true);
      controls();
    } catch {
      if (token === request && shown() && view === "3d") unavailable();
    } finally {
      if (token === request) { loading = false; buttons[1].removeAttribute("aria-busy"); }
    }
  }
  function refresh() {
    if (!shown()) { stop(); return; }
    controls();
    if (view === "3d" && !failed3d) {
      if (rig3d) rig3d.setVisible(true); else load3d();
    } else rig3d?.setVisible(false);
    requestPaint();
  }
  buttons.forEach((button, index) => listen(button, "click", () => {
    const value = index ? "3d" : "2d";
    if (value !== view || failed3d) {
      stop(); view = value; failed3d = false; status.hidden = true; refresh(); onViewChange(value);
    }
  }));
  listen(selector, "change", () => chooseZone(selector.value));
  listen(canvas, "click", event => {
    if (!shown() || canvas.hidden) return;
    const r = canvas.getBoundingClientRect();
    const key = pickRigZone(shownZones, transform, event.clientX - r.left, event.clientY - r.top);
    if (key) chooseZone(key);
  });
  listen(document, "visibilitychange", refresh); listen(document, "appearancechange", refresh);
  const resize = new ResizeObserver(refresh); resize.observe(surface);
  const viewObserver = new MutationObserver(refresh), parentView = host.closest("[data-view]");
  if (parentView) viewObserver.observe(parentView, { attributes: true, attributeFilter: ["hidden"] });
  viewObserver.observe(document.documentElement, { attributes: true, attributeFilter: ["data-rig", "data-theme"] });
  // Removal can happen while an optional module is loading, before a parent has
  // another live frame to reconcile. Cancel immediately, without polling.
  const removal = new MutationObserver(() => {
    if (!host.isConnected && mounted) dispose();
    else if (host.isConnected && !mounted) { mounted = true; refresh(); }
  });
  removal.observe(document.documentElement, { childList: true, subtree: true });
  function dispose() {
    if (disposed) return;
    disposed = true; stop(); listeners.abort(); resize.disconnect(); viewObserver.disconnect(); removal.disconnect();
    rig3d?.dispose(); rig3d = null; transform = null; canvas.width = canvas.height = 0; root.remove();
  }
  return {
    update(next) {
      if (disposed) return;
      const nextView = next.view === "3d" ? "3d" : "2d";
      if (nextView !== view) { stop(); view = nextView; failed3d = false; status.hidden = true; }
      if (zones !== next.zones) {
        zones = next.zones || []; shownZones = activeZones(zones); transform = null;
        const options = [make("option", "", "Choose a zone")]; options[0].value = "";
        for (const z of shownZones) { const option = make("option", "", z.config.name || z.key); option.value = z.key; options.push(option); }
        selector.replaceChildren(...options); selector.disabled = !shownZones.length;
        if (!shownZones.some(z => z.key === selected)) selected = null;
        selector.value = selected || ""; rig3d?.setZones(zones); rig3d?.select(selected);
      }
      frames = next.frames || {}; rig3d?.setFrames(frames); refresh();
    },
    dispose,
  };
}
