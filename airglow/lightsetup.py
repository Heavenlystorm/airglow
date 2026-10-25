"""Guided light setup: the product catalog, where things sit in the case, and turning answers into zone layouts.

The catalog lives in airglow/data/components/*.json, one product per line, so adding a fan or strip is a one-line
pull request. A product says what it is (fan, strip, cooler ring, single LED), where it plugs in ("argb" for a
standard 5 V addressable header, "nzxt" for HUE 2 accessories) and, when known, how many LEDs it has. Products
with no LED count ("leds": null) are counted with the LED counter instead.
"""

import json
import logging
import math
import re

from . import paths

log = logging.getLogger(__name__)

KINDS = ("fan", "strip", "ring", "single")
CONNECTORS = ("argb", "nzxt")
MAX_UNITS = 6                    # fans (or strips) of one product chained on one header

# Where a light can sit, on the canvas (1.6 x 1.0, a PC case seen from the side with the front on the left).
# angle: the direction fans line up and strips run; step: how far the next thing in the same spot moves.
SPOTS = {
    "front": {"label": "Front", "x": 0.18, "y": 0.5, "angle": 90, "length": 0.62, "step": (0.2, 0.0)},
    "top": {"label": "Top", "x": 0.8, "y": 0.13, "angle": 0, "length": 0.62, "step": (0.0, 0.17)},
    "rear": {"label": "Rear", "x": 1.42, "y": 0.27, "angle": 90, "length": 0.3, "step": (-0.2, 0.0)},
    "bottom": {"label": "Bottom", "x": 0.8, "y": 0.85, "angle": 0, "length": 0.62, "step": (0.0, -0.17)},
    "side": {"label": "Side panel", "x": 0.6, "y": 0.45, "angle": 0, "length": 0.5, "step": (0.0, 0.2)},
    "cooler": {"label": "CPU cooler", "x": 1.02, "y": 0.3, "angle": 0, "length": 0.16, "step": (0.14, 0.0)},
    "ram": {"label": "Memory", "x": 1.22, "y": 0.38, "angle": 90, "length": 0.14, "step": (0.04, 0.0)},
    "board": {"label": "Motherboard", "x": 1.02, "y": 0.52, "angle": 0, "length": 0.3, "step": (0.0, 0.06)},
    "gpu": {"label": "Graphics card", "x": 0.95, "y": 0.66, "angle": 0, "length": 0.62, "step": (0.0, 0.08)},
    "desk": {"label": "Desk", "x": 0.8, "y": 0.93, "angle": 0, "length": 0.5, "step": (0.0, -0.07)},
}
FAN_SIZE_120 = 0.16             # canvas units for a 120 mm fan
GENERIC_NAMES = {"fan": "fans", "strip": "strip", "ring": "ring", "single": "light"}


# ---------------------------------------------------------------- catalog
def _str(item, key, longest, required=True):
    v = item.get(key, "")
    if not isinstance(v, str) or len(v) > longest or (required and not v.strip()):
        raise ValueError(f"{key} must be text of up to {longest} characters")
    return v.strip()


def validate_item(item):
    if not isinstance(item, dict):
        raise ValueError("each product is an object")
    pid = item.get("id")
    if not isinstance(pid, str) or not re.fullmatch(r"[a-z0-9][a-z0-9\-]{0,59}", pid):
        raise ValueError(f"product id {pid!r} must be lowercase letters, digits and dashes")
    out = {"id": pid, "brand": _str(item, "brand", 40, required=False), "name": _str(item, "name", 60)}
    if item.get("kind") not in KINDS:
        raise ValueError(f"{pid}: kind must be one of {', '.join(KINDS)}")
    out["kind"] = item["kind"]
    connect = item.get("connect")
    if not isinstance(connect, list) or not connect or any(c not in CONNECTORS for c in connect):
        raise ValueError(f"{pid}: connect lists where it plugs in ({', '.join(CONNECTORS)})")
    out["connect"] = list(dict.fromkeys(connect))
    leds = item.get("leds")
    if leds is not None and (not isinstance(leds, int) or isinstance(leds, bool) or not 1 <= leds <= 300):
        raise ValueError(f"{pid}: leds is a whole number from 1 to 300, or null when it has to be counted")
    out["leds"] = leds                       # per fan for fans, per unit otherwise
    fans = item.get("fans", 1)
    if not isinstance(fans, int) or isinstance(fans, bool) or not 1 <= fans <= MAX_UNITS:
        raise ValueError(f"{pid}: fans is how many fans one unit has, 1 to {MAX_UNITS}")
    out["fans"] = fans if out["kind"] == "fan" else 1
    size = item.get("size_mm")
    if size is not None and (not isinstance(size, int) or not 40 <= size <= 420):
        raise ValueError(f"{pid}: size_mm is the fan size in millimetres")
    out["size_mm"] = size
    nid = item.get("nzxt_id")
    if nid is not None and (not isinstance(nid, int) or not 0 <= nid <= 255):
        raise ValueError(f"{pid}: nzxt_id is the accessory id the hub reports")
    out["nzxt_id"] = nid
    out["hint"] = _str(item, "hint", 80, required=False)
    return out


def load_catalog(folder=None):
    """Every product from the catalog files, in file order. A broken file is logged and left out."""
    folder = folder or paths.PACKAGE_DIR / "data" / "components"
    items, seen = [], set()
    for f in sorted(folder.glob("*.json")):
        try:
            raw = json.loads(f.read_text(encoding="utf-8"))
            products = [validate_item(i) for i in raw.get("items", [])]
        except (OSError, ValueError, AttributeError) as e:
            log.error("component catalog %s not loaded: %s", f.name, e)
            continue
        for p in products:
            if p["id"] in seen:
                log.error("component catalog %s: %s is listed twice", f.name, p["id"])
                continue
            seen.add(p["id"])
            items.append(p)
    return items


# ---------------------------------------------------------------- layouts
def _anchor(spot, slot):
    s = SPOTS[spot]
    dx, dy = s["step"]
    return round(s["x"] + dx * slot, 3), round(s["y"] + dy * slot, 3), s["angle"]


def layout_for(kind, spot, slot=0, *, leds, fans=1, size_mm=None):
    """Layout for something plugged into a header. leds: per fan for fans and rings, in total otherwise."""
    if spot not in SPOTS:
        raise ValueError("pick where it sits")
    x, y, angle = _anchor(spot, slot)
    if kind == "fan":
        size = min(max(FAN_SIZE_120 * (size_mm or 120) / 120, 0.08), 0.4)
        return {"shape": "fans", "fans": fans, "leds_per_fan": leds, "size": round(size, 3), "x": x, "y": y,
                "angle": angle, "reverse": False}
    if kind == "ring":
        return {"shape": "fans", "fans": 1, "leds_per_fan": leds, "size": 0.1, "x": x, "y": y, "angle": 0,
                "reverse": False}
    length = 0.05 if kind == "single" or leds == 1 else SPOTS[spot]["length"]
    return {"shape": "strip", "leds": leds, "length": length, "x": x, "y": y, "angle": angle, "reverse": False}


def move_layout(layout, spot, slot=0, index=0, of=1):
    """A fixed device's zone, moved to a spot. Zones of one device spread out along the spot, in order."""
    x, y, angle = _anchor(spot, slot)
    off = (index - (of - 1) / 2) * SPOTS[spot]["length"] / max(of, 1)
    a = math.radians(angle)
    out = dict(layout, x=round(x + math.cos(a) * off, 3), y=round(y + math.sin(a) * off, 3))
    if layout["shape"] != "matrix":                # keyboards stay level
        out["angle"] = angle
    return out


# ---------------------------------------------------------------- what needs setting up
def needs_setup(cfg, zone):
    """A zone Airglow placed by itself (never saved by you) that has lights, once its device has reported."""
    return bool(cfg and cfg.get("auto") and not zone.get("provisional") and zone.get("default_leds", 0) > 0)


def pending(store, devices):
    """Keys of zones still to set up. devices: Device.info() dicts."""
    zones = store.zones()
    return [f"{d['id']}/{z['id']}" for d in devices if d.get("online") for z in d["zones"]
            if needs_setup(zones.get(f"{d['id']}/{z['id']}"), z)]


def pending_count(store, devices):
    """How many lights the guide will ask about: each header, and each device's built-in lights once."""
    keys = set(pending(store, devices))
    n = 0
    for d in devices:
        mine = [z for z in d["zones"] if f"{d['id']}/{z['id']}" in keys]
        n += sum(1 for z in mine if z.get("resizable")) + (1 if any(not z.get("resizable") for z in mine) else 0)
    return n


def _slot(store, spot, exclude):
    return sum(1 for k, c in store.zones().items()
               if k not in exclude and (c.get("component") or {}).get("spot") == spot)


def _int(v, lo, hi, what):
    try:
        v = int(v)
    except (TypeError, ValueError):
        raise ValueError(f"{what} must be a number") from None
    if not lo <= v <= hi:
        raise ValueError(f"{what} must be between {lo} and {hi}")
    return v


def plan_header(store, key, zone, body, catalog):
    """The zone settings for what's plugged into a header (a resizable zone), from the setup guide's answers.
    body: {"component": product id, "count": units, "leds": counted LEDs (for products without a count),
    "spot": spot id} or {"off": true} when nothing is connected."""
    cfg = dict(store.zones().get(key) or {})
    if body.get("off"):
        return {**cfg, "name": cfg.get("name") or zone.name, "enabled": False,
                "component": {"id": None, "count": 0, "spot": None}}
    item = next((p for p in catalog if p["id"] == body.get("component")), None)
    if not item:
        raise ValueError("pick what's plugged in")
    if zone.connector not in item["connect"]:
        raise ValueError(f"{item['name']} doesn't plug into this kind of header")
    spot = body.get("spot")
    if spot not in SPOTS:
        raise ValueError("pick where it sits")
    count = _int(body.get("count", 1), 1, MAX_UNITS, "How many")
    fans = item["fans"] * count if item["kind"] == "fan" else 1
    if fans > 12:                                 # the layout validator takes at most 12 fans on one header
        raise ValueError(f"that would be {fans} fans on one header, and a header takes at most 12")
    if item["leds"]:
        per = item["leds"]
        total = per * fans if item["kind"] in ("fan", "ring") else per * count
    else:
        total = _int(body.get("leds"), 1, zone.max_leds, "The LED count")
        per = max(total // fans, 1) if item["kind"] == "fan" else total
    if total > zone.max_leds:
        raise ValueError(f"that's {total} LEDs, and this header drives at most {zone.max_leds}")
    layout = layout_for(item["kind"], spot, _slot(store, spot, {key}), leds=per if item["kind"] in ("fan", "ring") else total,
                        fans=fans, size_mm=item["size_mm"])
    # "Front F360 RGB Core" for a named product, "Front fans" or "Top strip" for a generic one
    what = item["name"] if item["brand"] else GENERIC_NAMES[item["kind"]]
    name = f"{SPOTS[spot]['label']} {what}"
    if count > 1 and item["kind"] != "fan":
        name += f" ×{count}"
    return {**cfg, "name": name[:40], "enabled": True, "order": cfg.get("order", "RGB"),
            "override": cfg.get("override"), "layout": layout,
            "component": {"id": item["id"], "count": count, "spot": spot}}


def plan_fixed(store, device, spot):
    """Settings for every built-in zone of a device (graphics card, board, RAM, keyboard) moved to one spot."""
    zones = store.zones()
    fixed = [z for z in device.zones if not z.resizable and f"{device.id}/{z.id}" in zones]
    keys = {f"{device.id}/{z.id}" for z in fixed}
    slot = len({k.rpartition("/")[0] for k, c in zones.items()
                if k not in keys and (c.get("component") or {}).get("spot") == spot})
    out = {}
    for i, z in enumerate(fixed):
        key = f"{device.id}/{z.id}"
        cfg = dict(zones[key])
        out[key] = {**cfg, "enabled": True, "layout": move_layout(cfg["layout"], spot, slot, i, len(fixed)),
                    "component": {"id": None, "count": 1, "spot": spot}}
    return out


def zone_spot(cfg):
    """Where a zone sits: the spot the setup guide recorded, else the nearest spot to its position."""
    spot = (cfg.get("component") or {}).get("spot")
    if spot in SPOTS:
        return spot
    L = cfg.get("layout") or {}
    x, y = L.get("x", 0.8), L.get("y", 0.5)
    return min(SPOTS, key=lambda k: (SPOTS[k]["x"] - x) ** 2 + (SPOTS[k]["y"] - y) ** 2)


def validate_component(c):
    """The setup guide's answers kept with a zone, so it can show them again later."""
    if c is None:
        return None
    if not isinstance(c, dict):
        raise ValueError("component must be an object")
    pid = c.get("id")
    if pid is not None and (not isinstance(pid, str) or not re.fullmatch(r"[a-z0-9][a-z0-9\-]{0,59}", pid)):
        raise ValueError("unknown product")
    spot = c.get("spot")
    if spot is not None and spot not in SPOTS:
        raise ValueError("unknown spot")
    return {"id": pid, "count": _int(c.get("count", 1), 0, MAX_UNITS, "How many"), "spot": spot}


def spots():
    return [{"id": k, "label": v["label"], "x": v["x"], "y": v["y"]} for k, v in SPOTS.items()]
