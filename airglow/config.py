"""Settings, zones, scenes (presets), automations and runtime state, stored as JSON in %APPDATA%\\Airglow."""

import copy
import json
import logging
import re
import threading
import time
from pathlib import Path

from . import paths

log = logging.getLogger(__name__)

COLOR_ORDERS = ("RGB", "RBG", "BGR", "BRG", "GBR", "GRB")
TEMP_SOURCES = ("max", "cpu", "gpu", "liquid")    # liquid: an AIO's or custom loop's coolant, when LHM reads one
DESIGNS = ("graphite", "ascii", "paper", "amber", "contrast", "neon")   # interface designs; ids match web/js/design.js
MIN_FAN_DUTY = 20          # below this some fans stall
SAFETY_TEMP = 90           # at or above this the hub fans run at 100% whatever the curve says
PERIPHERAL_TYPES = {"Keyboard", "Mouse", "Mouse mat", "Headset", "Headset stand", "Gamepad", "Microphone", "Keypad"}

DEFAULT_CURVES = {
    "silent":      [[40, 25], [55, 30], [68, 45], [78, 65], [88, 100]],
    "balanced":    [[40, 30], [55, 40], [68, 60], [78, 80], [88, 100]],
    "performance": [[40, 45], [55, 60], [68, 80], [75, 100]],
}

DEFAULT_CONFIG = {
    "port": 8787,
    # not "localhost": Windows tries ::1 and then 127.0.0.1, so a poll with LHM missing waited out two timeouts
    "lhm_url": "http://127.0.0.1:8085",
    "temp_source": "max",
    "temp_unit": "C",
    "design": "graphite",             # how the window looks; the lights keep their real colours either way
    "energy_price": 0.3797,           # €/kWh for the Cost per hour tile: the German household average, H1 2026
    "fan_failsafe_duty": 30,
    "fan_max_step": 5,
    "fancontrol_exe": "",
    "fancontrol_configs_dir": "",
    "openrgb_enabled": True,
    "openrgb_host": "127.0.0.1",
    "openrgb_port": 6742,
    "lights_off_on_sleep": True,
    "lights_off_on_lock": False,
    "dynamic_lighting_enabled": False,
    "online_checks": False,   # opt-in: look for updates (and later community effects) on a schedule
    "update_channel": "stable",
    "fan_curves": DEFAULT_CURVES,
    "zones": {},
    "device_params": {},      # device id -> plugin settings (validated against the plugin's PARAMETERS on save)
    "fan_channels": {},       # LHM control sensor id -> {"mode": "curve"|"steady", "duty"}; missing: left alone
}

DEFAULT_PRESETS = [
    {"id": "silent", "name": "Silent", "effect": "solid", "params": {"color": "#ff7a1a"}, "brightness": 35,
     "fan_curve": "silent", "fan_profile": None},
    {"id": "balanced", "name": "Balanced", "effect": "gradient",
     "params": {"colors": ["#00b4ff", "#2f5bff"], "speed": 20, "angle": 0}, "brightness": 75,
     "fan_curve": "balanced", "fan_profile": None},
    {"id": "gaming", "name": "Gaming", "effect": "rainbow", "params": {"speed": 45, "size": 55, "angle": 0, "saturation": 100},
     "brightness": 100, "fan_curve": "performance", "fan_profile": None},
    {"id": "night", "name": "Night", "effect": "breathing", "params": {"color": "#ff5a1f", "speed": 15}, "brightness": 12,
     "fan_curve": "silent", "fan_profile": None},
]

# The Overview: a grid of tiles (4 columns). Sensor tiles name readings by id ("auto/..." shortcuts work on any PC).
TILE_TYPES = ("sensor", "lighting", "presets", "fans")
TILE_STYLES = ("spark", "ring", "graph", "bar")
GRID_COLUMNS = 4
DEFAULT_OVERVIEW = {
    "tiles": [
        {"id": "lighting", "type": "lighting", "w": 2, "h": 2},
        {"id": "cpu-temp", "type": "sensor", "sensors": ["auto/cpu_temp"], "style": "spark", "w": 1, "h": 1},
        {"id": "gpu-temp", "type": "sensor", "sensors": ["auto/gpu_temp"], "style": "spark", "w": 1, "h": 1},
        {"id": "cpu-load", "type": "sensor", "sensors": ["auto/cpu_load"], "style": "ring", "w": 1, "h": 1},
        {"id": "gpu-load", "type": "sensor", "sensors": ["auto/gpu_load"], "style": "ring", "w": 1, "h": 1},
        {"id": "presets", "type": "presets", "w": 2, "h": 1},
        {"id": "power", "type": "sensor", "sensors": ["auto/cpu_power", "auto/gpu_power"], "style": "graph", "w": 2, "h": 1,
         "label": "Power draw"},
        {"id": "fans", "type": "fans", "w": 1, "h": 1},
        {"id": "ram", "type": "sensor", "sensors": ["auto/ram_load"], "style": "bar", "w": 1, "h": 1},
        {"id": "ssd-temp", "type": "sensor", "sensors": ["auto/ssd_temp"], "style": "spark", "w": 1, "h": 1},
        {"id": "net-down", "type": "sensor", "sensors": ["auto/net_down"], "style": "spark", "w": 1, "h": 1},
        {"id": "cost", "type": "sensor", "sensors": ["app/energy_cost"], "style": "spark", "w": 1, "h": 1},
    ],
    "pins": ["auto/cpu_temp", "auto/gpu_temp"],
}

DEFAULT_SCENE = {"effect": "solid", "params": {"color": "#009bde"}, "brightness": 100, "on": True}
DEFAULT_STATE = {"scene": DEFAULT_SCENE, "fan_curve": "balanced", "active_preset": None, "onboarded": False,
                 "online_asked": False}

# Bump when the saved format changes, and add a step to Store._upgrade. 0 = files from before versioning
# (single-colour presets and state), 1 = effects with typed parameters.
SCHEMA_VERSION = 1

# What "Reset settings" puts back: the per-PC settings, but not the layout and presets someone built up, and not the
# port (Airglow is listening on it).
RESETTABLE_SETTINGS = ("lhm_url", "temp_source", "temp_unit", "fan_failsafe_duty", "fan_max_step", "fancontrol_exe",
                       "fancontrol_configs_dir", "openrgb_enabled", "openrgb_host", "openrgb_port",
                       "lights_off_on_sleep", "lights_off_on_lock", "dynamic_lighting_enabled", "online_checks",
                       "update_channel", "design", "energy_price")


# ---------------------------------------------------------------- validation

def parse_hex(color):
    m = re.fullmatch(r"#?([0-9a-fA-F]{6})", color or "")
    if not m:
        raise ValueError(f"'{color}' is not a colour like #12abef")
    v = m.group(1)
    return [int(v[i:i + 2], 16) for i in (0, 2, 4)]


def norm_hex(color):
    parse_hex(color)
    return "#" + color.lstrip("#").lower()


def _int_in(value, lo, hi, name):
    try:
        v = int(value)
    except (TypeError, ValueError):
        raise ValueError(f"{name} must be a whole number") from None
    if not lo <= v <= hi:
        raise ValueError(f"{name} must be between {lo} and {hi}")
    return v


def validate_curve(points):
    if not isinstance(points, list) or not 2 <= len(points) <= 8:
        raise ValueError("a fan curve needs between 2 and 8 points")
    clean = []
    for p in points:
        if not isinstance(p, (list, tuple)) or len(p) != 2:
            raise ValueError("each curve point is [temperature, speed]")
        clean.append([_int_in(p[0], 20, 100, "Curve temperature"), _int_in(p[1], MIN_FAN_DUTY, 100, "Curve speed")])
    clean.sort()
    temps = [t for t, _ in clean]
    if len(set(temps)) != len(temps):
        raise ValueError("two curve points can't share the same temperature")
    return clean


def validate_settings(patch):
    """Validate a partial settings update; returns the cleaned subset."""
    out = {}
    for key, value in patch.items():
        if key == "temp_source":
            if value not in TEMP_SOURCES:
                raise ValueError("temperature source must be max, cpu, gpu or liquid")
            out[key] = value
        elif key == "temp_unit":
            if value not in ("C", "F"):
                raise ValueError("temperature unit must be C or F")
            out[key] = value
        elif key == "design":
            if value not in DESIGNS:
                raise ValueError(f"unknown interface design '{value}'")
            out[key] = value
        elif key == "energy_price":
            try:
                price = float(value)
            except (TypeError, ValueError):
                raise ValueError("Electricity price must be a number") from None
            if not 0.01 <= price <= 5:
                raise ValueError("Electricity price must be between 0.01 and 5 €/kWh")
            out[key] = round(price, 4)
        elif key == "fan_failsafe_duty":
            out[key] = _int_in(value, 30, 100, "Failsafe speed")
        elif key == "fan_max_step":
            out[key] = _int_in(value, 1, 25, "Ramp speed")
        elif key == "lhm_url":
            if not re.fullmatch(r"https?://[\w.\-]+(:\d{1,5})?/?", str(value)):
                raise ValueError("sensor address must look like http://127.0.0.1:8085")
            out[key] = re.sub(r"^(https?://)localhost(?=[:/]|$)", r"\g<1>127.0.0.1", str(value).rstrip("/"), flags=re.I)
        elif key in ("fancontrol_exe", "fancontrol_configs_dir"):
            out[key] = str(value or "")
        elif key in ("openrgb_enabled", "lights_off_on_sleep", "lights_off_on_lock", "dynamic_lighting_enabled",
                     "online_checks"):
            out[key] = bool(value)
        elif key == "update_channel":
            if value not in ("stable", "beta"):
                raise ValueError("update channel must be stable or beta")
            out[key] = value
        elif key == "openrgb_host":
            if not re.fullmatch(r"[\w.\-]{1,253}", str(value)):
                raise ValueError("OpenRGB host must be a computer name or address")
            out[key] = str(value)
        elif key == "openrgb_port":
            out[key] = _int_in(value, 1, 65535, "OpenRGB port")
        else:
            raise ValueError(f"unknown setting '{key}'")
    return out


def validate_areas(areas):
    """Per-area effects: {spot: {"effect", "params"}}, keyed by where lights sit (lightsetup.SPOTS), so a profile
    lands on the right lights on any rig."""
    from . import lightsetup
    from .lighting import effects
    if not areas:
        return {}
    if not isinstance(areas, dict):
        raise ValueError("areas must be an object")
    out = {}
    for spot, a in areas.items():
        if spot not in lightsetup.SPOTS:
            raise ValueError(f"unknown area '{spot}'")
        if not isinstance(a, dict) or not (a.get("effect") in effects.EFFECTS or effects.is_missing_studio(a.get("effect"))):
            raise ValueError(f"the {lightsetup.SPOTS[spot]['label'].lower()} area needs an effect")
        out[spot] = {"effect": a["effect"], "params": effects.clean_params(a["effect"], a.get("params"))}
    return out


def validate_scene(scene):
    from .lighting import effects
    if not isinstance(scene, dict):
        raise ValueError("a scene must be an object")
    effect = scene.get("effect") or "solid"
    out = {"effect": effect, "params": effects.clean_params(effect, scene.get("params")),
           "brightness": _int_in(scene.get("brightness", 100), 0, 100, "Brightness"),
           "on": bool(scene.get("on", True))}
    areas = validate_areas(scene.get("areas"))
    if areas:
        out["areas"] = areas
    return out


def _migrate_preset(p):
    """Pre-2.0 presets had a single colour; they become Solid scenes."""
    if isinstance(p, dict) and "effect" not in p and "color" in p:
        p = {**p, "effect": "solid", "params": {"color": norm_hex(p["color"])}}
    return p


def validate_preset(p, curve_names):
    from .lighting import effects
    p = _migrate_preset(p)
    if not isinstance(p, dict):
        raise ValueError("a preset must be an object")
    name = str(p.get("name") or "").strip()
    if not name or len(name) > 40:
        raise ValueError("preset names need 1 to 40 characters")
    pid = str(p.get("id") or "").strip()
    if not re.fullmatch(r"[a-z0-9\-]{1,64}", pid):
        raise ValueError("preset id must be lowercase letters, digits and dashes")
    curve = p.get("fan_curve") or None
    if curve is not None and curve not in curve_names:
        raise ValueError(f"unknown fan curve '{curve}'")
    effect = p.get("effect") or "solid"
    description = str(p.get("description") or "").strip()
    if len(description) > 200:
        raise ValueError("descriptions can be up to 200 characters")
    out = {
        "id": pid,
        "name": name,
        "effect": effect,
        "params": effects.clean_params(effect, p.get("params")),
        "brightness": _int_in(p.get("brightness", 100), 0, 100, "Brightness"),
        "fan_curve": curve,
        "fan_profile": _fan_profile(p.get("fan_profile")),
    }
    # profile extras, only stored when used so older presets look the same
    if description:
        out["description"] = description
    areas = validate_areas(p.get("areas"))
    if areas:
        out["areas"] = areas
    if p.get("fan_points"):
        out["fan_points"] = validate_curve(p["fan_points"])        # the profile's own curve, used while it's active
    return out


def _fan_profile(name):
    """A FanControl profile is a file name inside its Configurations folder, never a path."""
    if not name:
        return None
    name = str(name)
    if not re.fullmatch(r'[^\\/:*?"<>|]{1,120}\.json', name, re.I) or name.startswith("."):
        raise ValueError("pick a FanControl profile from the list")
    return name


def validate_zone(cfg, max_leds):
    from . import lightsetup
    from .lighting import effects, geometry
    if not isinstance(cfg, dict):
        raise ValueError("zone settings must be an object")
    name = str(cfg.get("name") or "").strip()
    if not name or len(name) > 40:
        raise ValueError("zone names need 1 to 40 characters")
    order = cfg.get("order", "RGB")
    if order not in COLOR_ORDERS:
        raise ValueError(f"colour order must be one of {', '.join(COLOR_ORDERS)}")
    override = cfg.get("override")
    if override and not isinstance(override, dict):
        raise ValueError("a zone's own effect must be an object")
    if override:
        override = {"effect": override.get("effect"), "params": effects.clean_params(override.get("effect"), override.get("params"))}
    out = {"name": name, "enabled": bool(cfg.get("enabled", True)), "order": order, "override": override or None,
           "layout": geometry.validate_layout(cfg.get("layout"), max_leds)}
    component = lightsetup.validate_component(cfg.get("component"))
    if component:
        out["component"] = component            # what the setup guide was told is plugged in, and where it sits
    return out


def _sensor_id(v):
    v = str(v or "")
    if not 1 <= len(v) <= 200 or any(ord(c) < 32 for c in v):
        raise ValueError("unknown sensor")
    return v


def validate_overview(o):
    if not isinstance(o, dict):
        raise ValueError("the overview must be an object")
    tiles, seen = [], set()
    raw = o.get("tiles")
    if not isinstance(raw, list) or len(raw) > 32:
        raise ValueError("the overview holds up to 32 tiles")
    for t in raw:
        if not isinstance(t, dict):
            raise ValueError("a tile must be an object")
        tid = str(t.get("id") or "")
        if not re.fullmatch(r"[a-z0-9\-]{1,32}", tid) or tid in seen:
            raise ValueError("tile ids must be unique short names")
        seen.add(tid)
        kind = t.get("type")
        if kind not in TILE_TYPES:
            raise ValueError(f"unknown tile type '{kind}'")
        tile = {"id": tid, "type": kind, "w": _int_in(t.get("w", 1), 1, GRID_COLUMNS, "Tile width"),
                "h": _int_in(t.get("h", 1), 1, 3, "Tile height")}
        if kind == "sensor":
            ids = t.get("sensors")
            if not isinstance(ids, list) or not 1 <= len(ids) <= 2:
                raise ValueError("a sensor tile shows one or two sensors")
            style = t.get("style", "spark")
            if style not in TILE_STYLES:
                raise ValueError(f"unknown tile style '{style}'")
            tile.update(sensors=[_sensor_id(i) for i in ids], style=style)
            label = str(t.get("label") or "").strip()
            if len(label) > 40:
                raise ValueError("tile names can be up to 40 characters")
            if label:
                tile["label"] = label
        tiles.append(tile)
    pins = o.get("pins", [])
    if not isinstance(pins, list) or len(pins) > 4:
        raise ValueError("up to 4 readings can be pinned")
    pins = list(dict.fromkeys(_sensor_id(p) for p in pins))
    return {"tiles": tiles, "pins": pins}


# ---------------------------------------------------------------- default zone placement

def _short_name(d, z, zones_on_device):
    """'Gigabyte GeForce RTX 5080 Gaming OC' + 'Right fan' -> 'RTX 5080 Gaming OC: Right fan'."""
    dev = d["name"]
    for prefix in (d.get("vendor") or "", "NVIDIA", "GeForce", "AMD", "Radeon", "Intel"):
        if prefix and dev.lower().startswith(prefix.lower() + " "):
            dev = dev[len(prefix) + 1:]
    if zones_on_device == 1:
        return dev
    return f"{dev}: {z['name']}"


def _role(d, z):
    if not z["default_leds"]:
        return "parked"                     # nothing plugged in: keep it out of the way of real zones
    if d.get("type") in PERIPHERAL_TYPES:
        return "peripheral"
    if z["shape"] == "fans":
        return "fans"
    if d.get("type") == "Graphics card":
        return "gpu"
    if d["kind"] == "native":
        return "strip"
    return "other"


def _fan_split(z, leds):
    """(fans, LEDs per fan) for a fan zone. Fans are drawn alike, so a chain whose LEDs don't divide evenly
    (24 + 8 + 8 on one channel) gets the whole count spread over the fans, rounded down."""
    fans = max(1, z.get("fans", 1))
    return fans, max(leds // fans, 1)


def _matrix_layout(z, leds, geometry):
    m = z["matrix"]
    layout = geometry.default_layout("strip", max(leds, 1))
    layout.update(shape="matrix", leds=max(leds, 1), rows=m["rows"], cols=m["cols"], map=list(m["map"]), length=0.6)
    return layout


def _expected_leds(z):
    """How many LEDs a zone we place ourselves ends up with (what ensure_zones compares saved zones against)."""
    leds = z["default_leds"]
    if leds and z["shape"] == "fans":
        fans, per = _fan_split(z, leds)
        return fans * per
    return max(leds, 1)


FAN_ROWS = 4                # fans stacked in one column of the default layout, like a front intake


def _fan_bits(col, row, fans):
    return ((1 << min(fans, FAN_ROWS)) - 1) << (col * FAN_ROWS + row)


def _fan_slot(used, fans):
    """(column, first row) for a fan zone: the first gap of enough rows in the columns placed so far (used is a
    bit per column row). A zone never splits over two columns, so a chain of three fans stays together instead of
    overlapping the next zone, and a single fan fills the gap under a kit."""
    col = 0
    while True:
        for row in range(FAN_ROWS - min(fans, FAN_ROWS) + 1):
            if not used & _fan_bits(col, row, fans):
                return col, row
        col += 1


def _slots_after(role, n, z, leds):
    """A role's slot state once this zone is placed at n: a count, or for fans the rows now taken."""
    if role != "fans":
        return n + 1
    fans = _fan_split(z, leds)[0]
    return n | _fan_bits(*_fan_slot(n, fans), fans)


def _place(role, n, z, leds, geometry):
    """Lay zones out like a PC seen from the side: front fans in a column on the left, the graphics card low in
    the middle, board and strips around it, peripherals parked below the case."""
    if role == "fans":
        fans, per = _fan_split(z, leds)
        size = 0.18
        col, row = _fan_slot(n, fans)
        layout = geometry.default_layout("fans", per)
        layout.update(fans=fans, leds_per_fan=per, size=size, angle=90, x=round(0.22 + 0.24 * col, 3),
                      y=round(0.14 + 0.24 * (row + (min(fans, FAN_ROWS) - 1) / 2), 3))
        return layout
    if z.get("shape") == "matrix" and z.get("matrix") and leds:
        layout = _matrix_layout(z, leds, geometry)
    else:
        layout = geometry.default_layout("strip", max(leds, 1))
    if role == "gpu":
        layout.update(x=1.0, y=round(0.66 + 0.06 * n, 3), length=0.62)
    elif role == "strip":
        layout.update(x=1.0, y=round(0.12 + 0.07 * n, 3), length=0.7 if leds > 1 else 0.05)
    elif role == "other":
        layout.update(x=1.0, y=round(0.4 + 0.06 * n, 3), length=0.62)
    elif role == "parked":
        layout.update(x=round(1.45 - 0.0 * n, 3), y=round(0.06 + 0.06 * n, 3), length=0.05)
    else:
        layout.update(x=round(0.5 + 0.45 * (n % 2), 3), y=round(0.92 + 0.0 * n, 3), length=0.4)
    return layout


# ---------------------------------------------------------------- storage

class Store:
    """Thread-safe JSON files with defaults. Writes are atomic (temp file + rename)."""

    def __init__(self, directory=None):
        self.dir = Path(directory) if directory else paths.data_dir()
        self.dir.mkdir(parents=True, exist_ok=True)
        self._lock = threading.RLock()
        self.config = self._load_config()
        self.presets = self._load_presets()
        self.state = self._load_state()
        self.zones_rev = 0                   # bumps whenever zone settings change (the UI reloads zones on change)
        self.rules = self._load_rules()
        try:
            self.overview = validate_overview(self._read("overview.json", DEFAULT_OVERVIEW))
        except ValueError as e:
            log.warning("ignoring saved overview: %s", e)
            self.overview = copy.deepcopy(DEFAULT_OVERVIEW)
        self._upgrade()

    # -- low level
    def _read(self, name, default):
        try:
            return json.loads((self.dir / name).read_text(encoding="utf-8"))
        except FileNotFoundError:
            return copy.deepcopy(default)
        except (OSError, json.JSONDecodeError) as e:
            log.warning("could not read %s (%s); using defaults", name, e)
            return copy.deepcopy(default)

    def _write(self, name, data):
        with self._lock:
            tmp = self.dir / (name + ".tmp")
            tmp.write_text(json.dumps(data, indent=2, ensure_ascii=False), encoding="utf-8")
            for wait in (0.05, 0.1, 0.2, 0.4, None):
                try:
                    tmp.replace(self.dir / name)
                    return
                except PermissionError:             # antivirus or the search indexer has the file open for a moment
                    if wait is None:
                        raise
                    time.sleep(wait)

    def _upgrade(self):
        """Files are converted while loading (see _migrate_preset and _load_state); once that happened, save them
        in the current format so the conversions only ever run once."""
        if self.schema > SCHEMA_VERSION:
            log.warning("settings were saved by a newer Airglow (format %d); they won't be rewritten", self.schema)
            return
        if self.schema == SCHEMA_VERSION:
            return
        log.info("updating saved settings from format %d to %d", self.schema, SCHEMA_VERSION)
        with self._lock:
            self.schema = self.config["schema"] = SCHEMA_VERSION
            self._write("config.json", self.config)
            self._write("presets.json", self.presets)
            self._write("state.json", self.state)
            self._write("automations.json", self.rules)

    def _load_config(self):
        raw = self._read("config.json", {})
        cfg = copy.deepcopy(DEFAULT_CONFIG)
        self.schema = raw.get("schema", 0) if isinstance(raw.get("schema", 0), int) else 0
        cfg["schema"] = self.schema
        for key, value in raw.items():
            if key == "fan_curves" and isinstance(value, dict):
                for name, pts in value.items():
                    try:
                        cfg["fan_curves"][name] = validate_curve(pts)
                    except ValueError as e:
                        log.warning("ignoring invalid curve %s: %s", name, e)
            elif key == "zones" and isinstance(value, dict):
                cfg["zones"] = {k: v for k, v in value.items() if isinstance(v, dict) and "layout" in v}
            elif key == "device_params" and isinstance(value, dict):
                cfg["device_params"] = {k: v for k, v in value.items() if isinstance(k, str) and isinstance(v, dict)}
            elif key == "fan_channels" and isinstance(value, dict):
                from . import lhmfans
                for sid, ch in list(value.items())[:lhmfans.MAX_CHANNELS]:
                    try:
                        cfg["fan_channels"][lhmfans.validate_id(sid)] = lhmfans.validate(ch)
                    except ValueError as e:
                        log.warning("ignoring fan setting for %s: %s", sid, e)
            elif key == "port":
                cfg["port"] = value if isinstance(value, int) and 1024 <= value <= 65535 else cfg["port"]
            elif key in DEFAULT_CONFIG:
                try:
                    cfg.update(validate_settings({key: value}))
                except ValueError as e:
                    log.warning("ignoring invalid setting %s: %s", key, e)
        return cfg

    def _load_presets(self):
        raw = self._read("presets.json", DEFAULT_PRESETS)
        out = []
        for p in raw if isinstance(raw, list) else []:
            try:
                out.append(validate_preset(p, self.config["fan_curves"]))
            except ValueError as e:
                log.warning("ignoring invalid preset %r: %s", p.get("name") if isinstance(p, dict) else p, e)
        return out

    def _load_rules(self):
        from .automation import validate_rule
        raw = self._read("automations.json", [])
        out = []
        for r in raw if isinstance(raw, list) else []:
            try:
                out.append(validate_rule(r))
            except (ValueError, TypeError, AttributeError) as e:
                log.warning("ignoring invalid automation %r: %s", r.get("id") if isinstance(r, dict) else r, e)
        return out

    def _load_state(self):
        raw = self._read("state.json", {})
        state = copy.deepcopy(DEFAULT_STATE)
        if "scene" not in raw and "color" in raw:                     # pre-2.0 state
            raw["scene"] = {"effect": "solid", "params": {"color": raw["color"]},
                            "brightness": raw.get("brightness", 100), "on": raw.get("lights_on", True)}
        try:
            state["scene"] = validate_scene(raw.get("scene", DEFAULT_SCENE))
        except ValueError as e:
            log.warning("ignoring saved scene: %s", e)
        if raw.get("fan_curve") in self.config["fan_curves"] or str(raw.get("fan_curve")).startswith("profile:"):
            state["fan_curve"] = raw["fan_curve"]             # a profile's own curve; the controller checks it exists
        state["active_preset"] = raw.get("active_preset")
        state["onboarded"] = bool(raw.get("onboarded", False))
        state["online_asked"] = bool(raw.get("online_asked", False))
        return state

    # -- settings / curves
    def settings(self):
        with self._lock:
            return {k: v for k, v in self.config.items() if k not in ("fan_curves", "zones", "schema", "fan_channels")}

    def update_settings(self, patch):
        clean = validate_settings(patch)
        with self._lock:
            self.config.update(clean)
            self._write("config.json", self.config)
        return clean

    def reset_settings(self):
        """Put the settings back to their defaults. Everything someone built up stays: zones, presets, fan curves,
        the overview and the automations. The port is left alone too (it would need a restart to change)."""
        with self._lock:
            for key in RESETTABLE_SETTINGS:
                self.config[key] = copy.deepcopy(DEFAULT_CONFIG[key])
            self._write("config.json", self.config)
        return self.settings()

    def openrgb_enabled(self):
        return self.config["openrgb_enabled"]

    def has_openrgb_zones(self):
        """True when lights from OpenRGB are set up, so an OpenRGB that isn't running is worth saying out loud."""
        with self._lock:
            return any(key.startswith("openrgb:") for key in self.config["zones"])

    def curves(self):
        with self._lock:
            return copy.deepcopy(self.config["fan_curves"])

    def set_curve(self, name, points):
        if name not in DEFAULT_CURVES:
            raise ValueError(f"unknown fan curve '{name}'")
        clean = validate_curve(points)
        with self._lock:
            self.config["fan_curves"][name] = clean
            self._write("config.json", self.config)
        return clean

    def reset_curve(self, name):
        return self.set_curve(name, DEFAULT_CURVES[name])

    # -- zones
    def zones(self):
        """Live dict of zone settings keyed 'device/zone' (treat as read-only; use set_zone to change)."""
        return self.config["zones"]

    def set_zone(self, key, cfg, max_leds):
        clean = validate_zone(cfg, max_leds)
        with self._lock:
            self.config["zones"] = {**self.config["zones"], key: clean}
            self._write("config.json", self.config)
            self.zones_rev += 1
        return clean

    # -- fans and pumps driven through LibreHardwareMonitor
    def fan_channels(self):
        """sensor id -> {"mode", "duty"} for every channel Airglow drives (read-only; use set_fan_channel)."""
        return self.config["fan_channels"]

    def set_fan_channel(self, sensor_id, cfg):
        """cfg None: leave the channel alone again (its BIOS or cooler curve)."""
        from . import lhmfans
        sid = lhmfans.validate_id(sensor_id)
        clean = lhmfans.validate(cfg) if cfg is not None else None
        with self._lock:
            channels = {k: v for k, v in self.config["fan_channels"].items() if k != sid}
            if clean is not None:
                if len(channels) >= lhmfans.MAX_CHANNELS:
                    raise ValueError(f"Airglow drives up to {lhmfans.MAX_CHANNELS} fans this way")
                channels[sid] = clean
            self.config["fan_channels"] = channels
            self._write("config.json", self.config)
        return clean

    # -- device plugin settings
    def device_params(self):
        """device id -> saved plugin settings (read-only; use set_device_params to change)."""
        return self.config["device_params"]

    def set_device_params(self, device_id, schema, values):
        from .lighting.effects import clean_values
        clean = clean_values(schema, {**self.config["device_params"].get(device_id, {}), **(values or {})})
        with self._lock:
            self.config["device_params"] = {**self.config["device_params"], device_id: clean}
            self._write("config.json", self.config)
        return clean

    def ensure_zones(self, devices):
        """Create default settings for zones seen for the first time, and correct zones we created ourselves
        ("auto") when the hardware reports better information, e.g. detected NZXT accessories.
        Zones the user has saved lose the auto mark and are never touched. devices: Device.info() dicts."""
        with self._lock:                                  # an API zone save can't slip in between read and write
            return self._ensure_zones(devices)

    def _ensure_zones(self, devices):
        from .lighting import geometry
        existing = self.config["zones"]
        new = {}
        all_zones = [(d, z) for d in devices for z in d["zones"]]
        slots = {"fans": 0, "strip": 0, "gpu": 0, "other": 0, "peripheral": 0, "parked": 0}
        stale = set()
        all_zones = [(d, z) for d, z in all_zones if not z.get("provisional")]   # wait until the device has reported
        for d, z in all_zones:                              # count what's already placed so new zones don't overlap
            key = f"{d['id']}/{z['id']}"
            cfg = existing.get(key)
            if not cfg:
                continue
            # a fallback layout (the device never reported) only places new zones, never rewrites saved ones
            if cfg.get("auto") and not z.get("guessed") and (geometry.total_leds(cfg["layout"]) != _expected_leds(z)
                                                             or cfg["layout"]["shape"] != z["shape"]):
                stale.add(key)
            else:
                role = _role(d, z)
                slots[role] = _slots_after(role, slots[role], z, z["default_leds"])
        for d, z in all_zones:
            key = f"{d['id']}/{z['id']}"
            if key in existing and key not in stale:
                continue
            leds = z["default_leds"]
            role = _role(d, z)
            layout = _place(role, slots[role], z, leds, geometry)
            slots[role] = _slots_after(role, slots[role], z, leds)
            order = "RGB"
            enabled = leds > 0 and d.get("type") not in PERIPHERAL_TYPES
            name = z["name"] if d["kind"] == "native" else _short_name(d, z, len([1 for dd, _ in all_zones if dd is d]))
            if key in stale:                                 # keep what the user may have set elsewhere
                old = existing[key]
                name, order = old.get("name", name), old.get("order", order)
                log.info("zone %s updated from detected hardware (%d LEDs)", key, leds)
            new[key] = {"name": name[:40], "enabled": enabled, "order": order, "override": None, "layout": layout,
                        "auto": True}
        if new:
            self.config["zones"] = {**existing, **new}
            self._write("config.json", self.config)
            self.zones_rev += 1
        return bool(new)

    # -- presets
    def get_presets(self):
        with self._lock:
            return copy.deepcopy(self.presets)

    def set_presets(self, presets):
        if not isinstance(presets, list) or len(presets) > 24:
            raise ValueError("expected a list of up to 24 presets")
        clean = [validate_preset(p, self.config["fan_curves"]) for p in presets]
        if len({p["id"] for p in clean}) != len(clean):
            raise ValueError("preset ids must be unique")
        with self._lock:
            self.presets = clean
            self._write("presets.json", clean)
        return clean

    # -- automations
    def get_rules(self):
        with self._lock:
            return copy.deepcopy(self.rules)

    def set_rules(self, rules, validator):
        if not isinstance(rules, list) or len(rules) > 50:
            raise ValueError("expected a list of up to 50 automations")
        clean = [validator(r) for r in rules]
        if len({r["id"] for r in clean}) != len(clean):
            raise ValueError("automation ids must be unique")
        with self._lock:
            self.rules = clean
            self._write("automations.json", clean)
        return clean

    # -- overview
    def get_overview(self):
        with self._lock:
            return copy.deepcopy(self.overview)

    def set_overview(self, overview):
        clean = validate_overview(overview)
        with self._lock:
            self.overview = clean
            self._write("overview.json", clean)
        return clean

    def reset_overview(self):
        return self.set_overview(copy.deepcopy(DEFAULT_OVERVIEW))

    def tracked_sensors(self):
        """Sensor ids the Overview and the pins show (sent with every state update)."""
        o = self.overview
        return list(dict.fromkeys([i for t in o["tiles"] for i in t.get("sensors", [])] + o["pins"]))

    # -- state
    def save_state(self, **changes):
        with self._lock:
            self.state.update(changes)
            self._write("state.json", self.state)
