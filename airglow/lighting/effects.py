"""Lighting effects. Each effect declares typed parameters (the UI builds its controls from them) and renders
colours for a batch of LEDs at once with numpy.

render(z, t, params, ctx) gets:
  z.pos    (N,2) LED positions, 0..1 across the whole rig
  z.local  (N,)  0..1 position within the LED's own fan ring or strip (for chases that run around each fan)
  z.fan    (N,)  which fan in the zone the LED belongs to
and returns (N,3) floats 0..1. Brightness and colour order are applied later by the engine.
"""

import math
import re

import numpy as np


# ---------------------------------------------------------------- helpers

def hex_rgb(h):
    h = h.lstrip("#")
    return np.array([int(h[i:i + 2], 16) / 255 for i in (0, 2, 4)])


def hsv(h, s, v):
    """Vectorised HSV -> RGB. h, s, v broadcast; returns (..., 3)."""
    h = np.asarray(h, float) % 1.0
    s, v = np.broadcast_to(s, h.shape), np.broadcast_to(v, h.shape)
    i = np.floor(h * 6).astype(int) % 6
    f = h * 6 - np.floor(h * 6)
    p, q, t = v * (1 - s), v * (1 - f * s), v * (1 - (1 - f) * s)
    r = np.choose(i, [v, q, p, p, t, v])
    g = np.choose(i, [t, v, v, q, p, p])
    b = np.choose(i, [p, p, t, v, v, q])
    return np.stack([r, g, b], axis=-1)


def lerp(a, b, f):
    f = np.asarray(f, float)[..., None]
    return a * (1 - f) + b * f


def along(pos, angle_deg):
    a = math.radians(angle_deg)
    return pos[:, 0] * math.cos(a) + pos[:, 1] * math.sin(a)


def speed_hz(speed, lo=0.03, hi=1.2):
    """Map a 0..100 speed slider onto an exponential frequency range that feels even to the eye."""
    return lo * (hi / lo) ** (speed / 100)


# ---------------------------------------------------------------- parameter schema

def p_color(id_, label, default):
    return {"id": id_, "label": label, "type": "color", "default": default}


def p_colors(id_, label, default):
    return {"id": id_, "label": label, "type": "colors", "default": default, "min": 2, "max": 4}


def p_range(id_, label, default, lo=0, hi=100, step=1, unit=""):
    return {"id": id_, "label": label, "type": "range", "default": default, "min": lo, "max": hi, "step": step, "unit": unit}


def p_choice(id_, label, default, options):
    return {"id": id_, "label": label, "type": "choice", "default": default,
            "options": [{"value": v, "label": l} for v, l in options]}


def p_toggle(id_, label, default):
    return {"id": id_, "label": label, "type": "toggle", "default": default}


SPEED = p_range("speed", "Speed", 40)
DIRECTION = p_range("angle", "Direction", 0, 0, 359, 1, "°")


class Effect:
    id = name = description = ""
    params = []
    needs = ()            # "screen", "audio", "sensors"
    animated = True

    def render(self, z, t, p, ctx):
        raise NotImplementedError


class Solid(Effect):
    id, name, animated = "solid", "Solid", False
    description = "One steady colour everywhere."
    params = [p_color("color", "Colour", "#009bde")]

    def render(self, z, t, p, ctx):
        return np.tile(hex_rgb(p["color"]), (len(z.pos), 1))


class Breathing(Effect):
    id, name = "breathing", "Breathing"
    description = "Fades slowly in and out, like a sleeping laptop."
    params = [p_color("color", "Colour", "#009bde"), p_range("speed", "Speed", 30)]

    def render(self, z, t, p, ctx):
        k = 0.5 - 0.5 * math.cos(2 * math.pi * speed_hz(p["speed"], 0.05, 0.6) * t)
        return np.tile(hex_rgb(p["color"]) * (0.06 + 0.94 * k * k), (len(z.pos), 1))


class ColorCycle(Effect):
    id, name = "cycle", "Colour cycle"
    description = "The whole rig drifts through every colour together."
    params = [SPEED, p_range("saturation", "Saturation", 100)]

    def render(self, z, t, p, ctx):
        c = hsv(np.array([speed_hz(p["speed"], 0.005, 0.25) * t]), p["saturation"] / 100, 1.0)[0]
        return np.tile(c, (len(z.pos), 1))


class RainbowWave(Effect):
    id, name = "rainbow", "Rainbow wave"
    description = "Bands of colour sweep across every device in the direction you choose."
    params = [SPEED, p_range("size", "Band size", 50), DIRECTION, p_range("saturation", "Saturation", 100)]

    def render(self, z, t, p, ctx):
        scale = 0.4 + 2.6 * (1 - p["size"] / 100)
        h = along(z.pos, p["angle"]) * scale - speed_hz(p["speed"], 0.02, 0.8) * t
        return hsv(h, p["saturation"] / 100, 1.0)


class Gradient(Effect):
    id, name = "gradient", "Gradient"
    description = "Your colours blend across the rig and drift slowly."
    params = [p_colors("colors", "Colours", ["#ff2fb4", "#2f5bff"]), p_range("speed", "Drift", 20), DIRECTION]

    def render(self, z, t, p, ctx):
        stops = np.array([hex_rgb(c) for c in p["colors"]])
        s = (along(z.pos, p["angle"]) * 0.8 + speed_hz(p["speed"], 0.005, 0.2) * t) % 1.0
        s = 1 - np.abs(2 * s - 1)                                  # mirrored, so it loops seamlessly
        x = s * (len(stops) - 1)
        i = np.clip(np.floor(x).astype(int), 0, len(stops) - 2)
        return lerp(stops[i], stops[i + 1], x - i)


class Comet(Effect):
    id, name = "comet", "Comet"
    description = "A bright head with a fading tail races around each fan and along each strip."
    params = [p_color("color", "Colour", "#ffffff"), p_color("background", "Background", "#06121c"),
              SPEED, p_range("tail", "Tail length", 45), p_toggle("reverse", "Reverse direction", False)]

    def render(self, z, t, p, ctx):
        head = (speed_hz(p["speed"], 0.08, 2.0) * t) % 1.0
        d = (z.local - head) % 1.0 if p["reverse"] else (head - z.local) % 1.0
        tail = 0.04 + 0.9 * p["tail"] / 100
        k = np.clip(1 - d / tail, 0, 1) ** 2
        return lerp(hex_rgb(p["background"]), hex_rgb(p["color"]), k)


class Temperature(Effect):
    id, name = "temperature", "Temperature"
    description = "Cool colour when the PC is cool, shifting to the hot colour as it heats up."
    needs = ("sensors",)
    params = [p_choice("source", "Follow", "max", [("max", "Whichever is hotter"), ("cpu", "Processor"), ("gpu", "Graphics card")]),
              p_color("cool", "Cool colour", "#00b4ff"), p_color("hot", "Hot colour", "#ff2200"),
              p_range("low", "Cool at", 40, 20, 90, 1, "°C"), p_range("high", "Hot at", 80, 30, 100, 1, "°C")]

    def render(self, z, t, p, ctx):
        temps = {"cpu": ctx.cpu_temp, "gpu": ctx.gpu_temp}
        known = [v for v in temps.values() if v is not None]
        temp = (max(known) if known else None) if p["source"] == "max" else temps.get(p["source"])
        lo, hi = p["low"], max(p["high"], p["low"] + 1)
        f = 0.0 if temp is None else min(1.0, max(0.0, (temp - lo) / (hi - lo)))
        c = lerp(hex_rgb(p["cool"]), hex_rgb(p["hot"]), f)
        if temp is not None and temp >= hi:                       # a gentle pulse once it's past "hot"
            c = c * (0.75 + 0.25 * math.cos(2 * math.pi * 1.2 * t))
        return np.tile(c, (len(z.pos), 1))


class ScreenAmbience(Effect):
    id, name = "screen", "Screen ambience"
    description = "Each LED takes the colour of the part of your screen it sits next to."
    needs = ("screen",)
    params = [p_choice("display", "Display", "1", [("1", "Display 1"), ("2", "Display 2"), ("3", "Display 3"),
                                                   ("0", "All displays")]),
              p_range("saturation", "Colour boost", 40), p_toggle("mirror", "Mirror left and right", False)]

    def render(self, z, t, p, ctx):
        if ctx.screen is None:
            return np.zeros((len(z.pos), 3))
        pos = z.pos.copy()
        if p["mirror"]:
            pos[:, 0] = 1 - pos[:, 0]
        rgb = ctx.screen.sample(pos)
        boost = 1 + p["saturation"] / 50
        grey = rgb.mean(axis=1, keepdims=True)
        return np.clip(grey + (rgb - grey) * boost, 0, 1)


class AudioSpectrum(Effect):
    id, name = "spectrum", "Music spectrum"
    description = "Bass on the left to treble on the right, each LED glowing with its part of the music."
    needs = ("audio",)
    params = [p_range("sensitivity", "Sensitivity", 60), p_range("hue", "Colour shift", 0, 0, 359, 1, "°"),
              DIRECTION]

    def render(self, z, t, p, ctx):
        if ctx.audio is None:
            return np.zeros((len(z.pos), 3))
        bands = ctx.audio.bands
        x = along(z.pos, p["angle"])
        x = np.where(x >= 1.0, 1.0 - 1e-9, x) % 1.0          # the right edge is treble, not wrapped round to bass
        i = np.clip((x * len(bands)).astype(int), 0, len(bands) - 1)
        level = np.clip(bands[i] * (0.4 + 1.6 * p["sensitivity"] / 100), 0, 1)
        h = (p["hue"] / 360 + 0.66 - x * 0.66) % 1.0                # blue bass, red treble
        return hsv(h, 1.0, level ** 1.4)


class AudioPulse(Effect):
    id, name = "pulse", "Music pulse"
    description = "The whole rig flashes with the beat in your chosen colour."
    needs = ("audio",)
    params = [p_color("color", "Colour", "#8a3dff"), p_range("sensitivity", "Sensitivity", 60),
              p_range("floor", "Glow between beats", 10)]

    def render(self, z, t, p, ctx):
        level = 0.0 if ctx.audio is None else ctx.audio.bass
        k = min(1.0, level * (0.4 + 1.6 * p["sensitivity"] / 100))
        floor = p["floor"] / 100
        return np.tile(hex_rgb(p["color"]) * (floor + (1 - floor) * k), (len(z.pos), 1))


class Off(Effect):
    id, name, animated = "off", "Off", False
    description = "All LEDs dark."
    params = []

    def render(self, z, t, p, ctx):
        return np.zeros((len(z.pos), 3))


EFFECTS = {e.id: e for e in (Solid(), Breathing(), ColorCycle(), RainbowWave(), Gradient(), Comet(),
                             Temperature(), ScreenAmbience(), AudioSpectrum(), AudioPulse(), Off())}


def catalog():
    return [{"id": e.id, "name": e.name, "description": e.description, "params": e.params,
             "needs": list(e.needs), "animated": e.animated} for e in EFFECTS.values()]


_HEX = re.compile(r"#[0-9a-fA-F]{6}")


def clean_params(effect_id, params):
    """Validate effect parameters against the schema; missing values take defaults."""
    if effect_id not in EFFECTS:
        raise ValueError(f"unknown effect '{effect_id}'")
    return clean_values(EFFECTS[effect_id].params, params)


def clean_values(schema, params):
    """Validate values against a parameter schema (effects and device plugins share it); missing values take
    defaults, unknown ones are dropped."""
    params = params if isinstance(params, dict) else {}
    out = {}
    for spec in schema:
        v = params.get(spec["id"], spec["default"])
        kind = spec["type"]
        if kind == "color":
            if not isinstance(v, str) or not _HEX.fullmatch(v):
                raise ValueError(f"{spec['label']} must be a colour like #12abef")
            v = v.lower()
        elif kind == "colors":
            if not isinstance(v, list) or not spec["min"] <= len(v) <= spec["max"] or not all(isinstance(c, str) and _HEX.fullmatch(c) for c in v):
                raise ValueError(f"{spec['label']} needs {spec['min']} to {spec['max']} colours")
            v = [c.lower() for c in v]
        elif kind == "range":
            try:
                v = float(v)
            except (TypeError, ValueError):
                raise ValueError(f"{spec['label']} must be a number") from None
            v = min(spec["max"], max(spec["min"], v))
            v = int(v) if float(spec["step"]).is_integer() else v
        elif kind == "choice":
            if v not in [o["value"] for o in spec["options"]]:
                raise ValueError(f"{spec['label']} has no option '{v}'")
        elif kind == "toggle":
            v = bool(v)
        out[spec["id"]] = v
    return out
