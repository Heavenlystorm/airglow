// Motion follows the Windows animation setting and Airglow's Reduce preference.

const mq = globalThis.matchMedia?.("(prefers-reduced-motion: reduce)") ?? { matches: false };
export const reduced = () => mq.matches || globalThis.document?.documentElement?.dataset?.motion === "reduced";
const inactive = (el) => globalThis.document?.hidden || el.isConnected === false || el.closest?.("[hidden]");
const active = new Map();

export function stopMotion(el) {
  const animation = active.get(el);
  if (animation) {
    active.delete(el);
    animation.cancel();
  }
}

function stopInactiveMotion() {
  for (const el of active.keys()) if (reduced() || inactive(el)) stopMotion(el);
}

globalThis.document?.addEventListener?.("appearancechange", stopInactiveMotion);
globalThis.document?.addEventListener?.("visibilitychange", stopInactiveMotion);
mq.addEventListener?.("change", stopInactiveMotion);

// DOM rectangles use rendered pixels; CSS transforms use pixels before interface zoom.
export const uiScale = () => Number(document.documentElement?.dataset.scale || 100) / 100;

export const EASE = "cubic-bezier(.2, .7, .2, 1)";   // the --ease token in app.css

export function animateMotion(el, frames, duration) {
  stopMotion(el);
  if (reduced() || inactive(el)) return;
  const animation = el.animate?.(frames, { duration, easing: EASE });
  if (!animation) return;
  active.set(el, animation);
  const forget = () => { if (active.get(el) === animation) active.delete(el); };
  animation.finished.then(forget, forget);
}

// The numeric value of a displayed reading, to tell a rise from a fall.
const amount = (text) => parseFloat(String(text).replace(/[^\d.-]/g, ""));

// Change the reading immediately. Only its opacity animates; with `roll`, each digit that changed slides in from
// the direction the value moved, and digits that stayed do not move at all.
export function countTo(el, text, { roll = false } = {}) {
  if (!el || el.textContent === text) return;
  const previous = el.textContent;
  if (roll && el.replaceChildren && text !== "–" && previous !== "–" && previous && !reduced() && !inactive(el)) {
    stopMotion(el);
    const rising = amount(text) >= amount(previous), offset = previous.length - text.length;
    el.replaceChildren(...[...text].map((character, i) => {
      const digit = document.createElement("span");
      digit.className = "digit";
      digit.textContent = character;
      if (previous[i + offset] !== character) digit.dataset.moved = rising ? "up" : "down";
      return digit;
    }));
    for (const digit of el.children) {
      if (!digit.dataset.moved) continue;
      // opacity and transform only, and brief: the compositor moves the digit without repainting it or its glow, but
      // still presents the window on every refresh while it does
      digit.animate?.([{ opacity: 0, transform: `translateY(${digit.dataset.moved === "up" ? ".34em" : "-.34em"})` },
        { opacity: 1, transform: "none" }], { duration: 160, easing: "cubic-bezier(.16, 1, .3, 1)" });
    }
    return;
  }
  el.textContent = text;
  if (text === "–" || previous === "–" || reduced() || inactive(el)) {
    stopMotion(el);
  } else {
    const opacity = active.has(el) ? Number(getComputedStyle(el).opacity) : .78;
    animateMotion(el, [{ opacity }, { opacity: 1 }], 140);
  }
}

// Capture what is on screen before replacing the previous motion with the new layout.
export function flip(container, mutate, { skip = null } = {}) {
  const items = [...container.children].filter(el => el !== skip && el.id !== "ov-snap-guide");
  const animate = !reduced() && !inactive(container);
  const before = new Map(animate ? items.map(el => [el, el.getBoundingClientRect()]) : []);
  items.forEach(stopMotion);
  mutate();
  if (!animate) return;
  const scale = uiScale();
  const after = [...container.children].filter(el => el !== skip && el.id !== "ov-snap-guide")
    .map(el => [el, el.getBoundingClientRect()]);
  for (const [el, b] of after) {
    const a = before.get(el);
    if (!a) {
      animateMotion(el, [{ opacity: .7, transform: "scale(.98)" }, { opacity: 1, transform: "none" }], 180);
      continue;
    }
    if (!b.width || !b.height) continue;
    const dx = (a.left - b.left) / scale, dy = (a.top - b.top) / scale, sx = a.width / b.width, sy = a.height / b.height;
    if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5 && Math.abs(sx - 1) < 0.01 && Math.abs(sy - 1) < 0.01) continue;
    animateMotion(el, [{ transformOrigin: "top left", transform: `translate(${dx}px, ${dy}px) scale(${sx}, ${sy})` },
                       { transformOrigin: "top left", transform: "none" }], 200);
  }
}

// ------------------------------------------------------------ tab marker
export function moveMarker(marker, target, instant = false) {
  if (!marker || !target) return;
  const nav = marker.parentElement.getBoundingClientRect(), r = target.getBoundingClientRect();
  // An underline sits inside its tab; a pill (--ink-inset: 0) covers the whole tab.
  const inset = parseFloat(globalThis.getComputedStyle?.(marker)?.getPropertyValue?.("--ink-inset"));
  const pad = Number.isFinite(inset) ? inset : 12;
  const x = (r.left - nav.left) / uiScale() + pad, w = r.width / uiScale() - pad * 2;
  const immediate = instant || reduced() || document.hidden;
  if (immediate) marker.style.transition = "none";
  marker.style.transform = `translateX(${x}px)`;
  marker.style.width = `${w}px`;
  marker.style.opacity = "1";
  if (immediate) { marker.offsetWidth; marker.style.transition = ""; }
}

// Fade a freshly shown view in.
export function enter(el) {
  stopInactiveMotion();
  animateMotion(el, [{ opacity: .88, transform: "translateY(4px)" }, { opacity: 1, transform: "none" }], 180);
}
