"""Zone shapes -> LED positions on the shared canvas.

The canvas is 1.6 units wide and 1.0 tall (a PC case seen from the side). Effects receive positions normalised
to 0..1 on both axes; the UI draws the same positions, so the preview matches the hardware exactly.
"""

import math

import numpy as np

CANVAS_W, CANVAS_H = 1.6, 1.0
SHAPES = ("fans", "strip")


def total_leds(layout):
    if layout["shape"] == "fans":
        return layout["fans"] * layout["leds_per_fan"]
    return layout["leds"]


def positions(layout):
    """Return (pos[N,2] in canvas units, local[N] 0..1 position within each fan/strip, fan_index[N])."""
    x, y = layout["x"], layout["y"]
    a = math.radians(layout.get("angle", 0))
    dx, dy = math.cos(a), math.sin(a)
    if layout["shape"] == "fans":
        n, k, size = layout["fans"], layout["leds_per_fan"], layout.get("size", 0.16)
        r = size / 2
        gap = size * 1.12
        pts, local, fan = [], [], []
        for f in range(n):
            off = (f - (n - 1) / 2) * gap
            cx, cy = x + dx * off, y + dy * off
            for i in range(k):
                t = i / k
                th = -math.pi / 2 + t * 2 * math.pi            # start at 12 o'clock, clockwise
                pts.append((cx + r * math.cos(th), cy + r * math.sin(th)))
                local.append(t)
                fan.append(f)
        pos, local, fan = np.array(pts, float), np.array(local, float), np.array(fan, int)
    else:
        n, length = layout["leds"], layout.get("length", 0.5)
        t = np.linspace(0, 1, n) if n > 1 else np.array([0.5])
        off = (t - 0.5) * length
        pos = np.stack([x + dx * off, y + dy * off], axis=1)
        local, fan = t, np.zeros(n, int)
    if layout.get("reverse"):
        pos, local, fan = pos[::-1].copy(), local[::-1].copy(), fan[::-1].copy()
    return pos, local, fan


def normalised(pos):
    """Canvas units -> 0..1 on each axis (what effects see), clamped to the canvas."""
    return np.clip(pos / np.array([CANVAS_W, CANVAS_H]), 0.0, 1.0)


def default_layout(shape, count, slot=0, slots=1):
    """A sensible starting position: zones stacked down the canvas so nothing overlaps."""
    y = 0.18 + (0.64 * (slot + 0.5) / max(slots, 1))
    if shape == "fans":
        leds_per = 8 if count <= 8 else count
        return {"shape": "fans", "fans": 1, "leds_per_fan": leds_per, "x": 0.8, "y": round(y, 3),
                "angle": 0, "size": 0.16, "reverse": False}
    return {"shape": "strip", "leds": count, "x": 0.8, "y": round(y, 3), "angle": 0, "length": 0.9,
            "reverse": False}


def validate_layout(layout, max_leds):
    """Clean a layout from the UI. Raises ValueError with a readable message."""
    if not isinstance(layout, dict) or layout.get("shape") not in SHAPES:
        raise ValueError("shape must be fans or strip")

    def num(key, lo, hi, default=None, integer=False):
        v = layout.get(key, default)
        try:
            v = int(v) if integer else float(v)
        except (TypeError, ValueError):
            raise ValueError(f"{key} must be a number") from None
        if not lo <= v <= hi:
            raise ValueError(f"{key} must be between {lo} and {hi}")
        return v

    out = {"shape": layout["shape"], "x": num("x", -0.2, CANVAS_W + 0.2, 0.8), "y": num("y", -0.2, CANVAS_H + 0.2, 0.5),
           "angle": num("angle", -360, 360, 0), "reverse": bool(layout.get("reverse", False))}
    if out["shape"] == "fans":
        out["fans"] = num("fans", 1, 12, 1, integer=True)
        out["leds_per_fan"] = num("leds_per_fan", 1, 64, 8, integer=True)
        out["size"] = num("size", 0.05, 0.5, 0.16)
    else:
        out["leds"] = num("leds", 1, 300, 10, integer=True)
        out["length"] = num("length", 0.02, 2.0, 0.5)
    if total_leds(out) > max_leds:
        raise ValueError(f"this zone takes at most {max_leds} LEDs (you set {total_leds(out)})")
    return out
