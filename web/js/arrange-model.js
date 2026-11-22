const round = n => Math.round(n * 10000) / 10000;
const bounded = (n, a, b) => Math.max(a, Math.min(b, n));
export const positionOf = ({ x, y, angle }) => ({ x, y, angle });
export const samePosition = (a, b) => a.x === b.x && a.y === b.y && a.angle === b.angle;
export const turnAngle = (angle, delta) => round(((angle + delta + 540) % 360 + 360) % 360 - 180);

// Position a group as one object, including at the case edge. Legacy layouts may already extend outside it.
export function groupOffset(poses, dx, dy, snap = false) {
  if (!poses.length) return { dx: 0, dy: 0 };
  if (snap) {
    dx = Math.round((poses[0].x + dx) / .05) * .05 - poses[0].x;
    dy = Math.round((poses[0].y + dy) / .05) * .05 - poses[0].y;
  }
  const xs = poses.map(p => p.x), ys = poses.map(p => p.y);
  return { dx: round(bounded(dx, Math.min(0, -Math.min(...xs)), Math.max(0, 1.6 - Math.max(...xs)))),
    dy: round(bounded(dy, Math.min(0, -Math.min(...ys)), Math.max(0, 1 - Math.max(...ys)))) };
}

export function shiftPosition(p, dx, dy) { return { ...p, x: round(p.x + dx), y: round(p.y + dy) }; }

// Use returned LED coordinates, including matrix maps and reverse order. On fan banks an angle changes
// the line of fan centres. The individual fans keep LED zero at twelve o'clock, as in geometry.positions.
export function positionedZone(z, pose) {
  const layout = z.config.layout;
  if (!pose || samePosition(layout, pose)) return z;
  const delta = (pose.angle - layout.angle) * Math.PI / 180, c = Math.cos(delta), s = Math.sin(delta);
  const unit = Math.PI / 180;
  const positions = z.positions.map(([x, y], i) => {
    if (layout.shape === "fans") {
      const fan = z.fan_index?.[i] ?? Math.floor((layout.reverse ? z.positions.length - 1 - i : i) / layout.leds_per_fan);
      const offset = (fan - (layout.fans - 1) / 2) * layout.size * 1.12;
      return [round(x + pose.x - layout.x + offset * (Math.cos(pose.angle * unit) - Math.cos(layout.angle * unit))),
        round(y + pose.y - layout.y + offset * (Math.sin(pose.angle * unit) - Math.sin(layout.angle * unit)))];
    }
    const dx = x - layout.x, dy = y - layout.y;
    return [round(pose.x + dx * c - dy * s), round(pose.y + dx * s + dy * c)];
  });
  return { ...z, positions, config: { ...z.config, layout: { ...layout, ...pose } } };
}

export function createArrangement() {
  const changed = new Map(), past = [], future = [];
  const selected = new Set();
  const pose = z => changed.get(z.key)?.position || positionOf(z.config.layout);
  function apply(steps, side) {
    for (const step of steps) {
      const p = step[side];
      if (samePosition(step.expected, p)) changed.delete(step.key);
      else changed.set(step.key, { key: step.key, expected: step.expected, position: p });
    }
  }
  function edit(zones, fn) {
    const poses = zones.map(pose);
    const steps = zones.flatMap((z, i) => {
      const before = poses[i], after = fn(before, i, poses);
      return samePosition(before, after) ? [] : [{ key: z.key, expected: changed.get(z.key)?.expected || positionOf(z.config.layout), before, after }];
    });
    if (!steps.length) return false;
    apply(steps, "after"); past.push(steps); if (past.length > 60) past.shift();
    future.length = 0;
    return true;
  }
  function history(redo) {
    const from = redo ? future : past, to = redo ? past : future, steps = from.pop();
    if (!steps) return false;
    apply(steps, redo ? "after" : "before"); to.push(steps); return true;
  }
  function align(zones, axis, distribute) {
    if (zones.length < (distribute ? 3 : 2)) return false;
    const sorted = [...zones].sort((a, b) => pose(a)[axis] - pose(b)[axis]);
    const values = sorted.map(z => pose(z)[axis]);
    const mid = values.reduce((a, b) => a + b, 0) / values.length;
    return edit(sorted, (p, i) => ({ ...p, [axis]: round(distribute ?
      values[0] + (values.at(-1) - values[0]) * i / (values.length - 1) : mid) }));
  }
  return { selected, changed, past, future, pose, edit, history, align, snap: false, busy: false };
}
