// Motion: counting numbers, FLIP layout animation, and the sliding tab marker. Everything checks the Windows
// "Animation effects" setting (prefers-reduced-motion) and jumps straight to the end state when it's off.

const mq = matchMedia("(prefers-reduced-motion: reduce)");
export const reduced = () => mq.matches;

// Spring-like settle: quick start, soft landing with the slightest overshoot.
export const SPRING = "cubic-bezier(.22, 1.15, .36, 1)";
export const EASE = "cubic-bezier(.2, .7, .2, 1)";

// ------------------------------------------------------------ numbers
// New readings replace the old ones straight away (no counting animation).
export function countTo(el, text) {
  if (!el || el.textContent === text) return;
  el.textContent = text;
}

// ------------------------------------------------------------ FLIP
// Measure children, change the layout, then animate each child from where it was to where it is now.
export function flip(container, mutate, { skip = null, duration = 380 } = {}) {
  const items = [...container.children];
  const before = new Map(items.map((el) => [el, el.getBoundingClientRect()]));
  mutate();
  if (reduced()) return;
  for (const el of container.children) {
    if (el === skip) continue;
    const a = before.get(el);
    if (!a) {                                         // new element: rise in
      el.animate([{ opacity: 0, transform: "scale(.96)" }, { opacity: 1, transform: "none" }], { duration: 260, easing: EASE });
      continue;
    }
    const b = el.getBoundingClientRect();
    const dx = a.left - b.left, dy = a.top - b.top, sx = a.width / b.width, sy = a.height / b.height;
    if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5 && Math.abs(sx - 1) < 0.01 && Math.abs(sy - 1) < 0.01) continue;
    el.animate([{ transformOrigin: "top left", transform: `translate(${dx}px, ${dy}px) scale(${sx}, ${sy})` },
                { transformOrigin: "top left", transform: "none" }], { duration, easing: SPRING });
  }
}

// ------------------------------------------------------------ tab marker
export function moveMarker(marker, target, instant = false) {
  if (!marker || !target) return;
  const nav = marker.parentElement.getBoundingClientRect(), r = target.getBoundingClientRect();
  const x = r.left - nav.left + 12, w = r.width - 24;
  if (instant || reduced()) marker.style.transition = "none";
  marker.style.transform = `translateX(${x}px)`;
  marker.style.width = `${w}px`;
  marker.style.opacity = "1";
  if (instant || reduced()) { marker.offsetWidth; marker.style.transition = ""; }
}

// Fade a freshly shown view in.
export function enter(el) {
  if (reduced()) return;
  el.animate([{ opacity: 0, transform: "translateY(6px)" }, { opacity: 1, transform: "none" }], { duration: 220, easing: EASE });
}
