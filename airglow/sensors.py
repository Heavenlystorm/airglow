"""Sensor readings.

LibreHardwareMonitor's web server gives every sensor it knows (temperatures, loads, clocks, fans, memory, drives,
network). We keep all of them in a catalog keyed by LHM's own stable SensorId, plus a summary (CPU/GPU temperature
and power) that fan curves and effects use. A few readings come from Windows itself (memory, network, disk) so the
Overview still has something to show when LHM isn't running. "auto/..." ids are portable shortcuts that resolve to the
right sensor on any PC, which is what the default Overview uses.
"""

import json
import re
import time
import urllib.error
import urllib.request

_NUM = re.compile(r"-?\d+(?:[.,]\d+)?")
_IGPU = re.compile(r"Radeon\(TM\) Graphics|Intel.*(UHD|Iris|Arc\(TM\) Graphics)|Intel\(R\) Graphics", re.I)

OFFLINE = {"online": False, "cpu_temp": None, "gpu_temp": None, "gpu_hotspot": None, "liquid_temp": None,
           "cpu_power": None, "gpu_power": None, "total_power": None, "fans": [],
           "cpu_name": None, "gpu_name": None}

# LHM sensor type -> (kind, unit). Throughput is normalised to bytes per second because LHM switches between
# KB/s and MB/s as the value changes, which would break graphs.
TYPES = {
    "temperature": ("temperature", "°C"), "load": ("load", "%"), "control": ("control", "%"), "level": ("level", "%"),
    "power": ("power", "W"), "clock": ("clock", "MHz"), "data": ("data", "GB"), "smalldata": ("data", "MB"),
    "fan": ("fan", "RPM"), "voltage": ("voltage", "V"), "throughput": ("throughput", "B/s"), "timing": ("timing", "ns"),
    "factor": ("factor", ""), "frequency": ("frequency", "Hz"), "flow": ("flow", "L/h"), "current": ("current", "A"),
    "energy": ("energy", "mWh"), "noise": ("noise", "dBA"), "humidity": ("humidity", "%"),
}
_RATE = {"b/s": 1, "kb/s": 1024, "mb/s": 1024 ** 2, "gb/s": 1024 ** 3}

# Sensors that describe a part rather than measure it: hidden from the picker.
_NOISE = re.compile(r"limit|resolution|warning temperature|critical temperature|threshold|capacity|power on|"
                    r"^t[A-Z]|^D3D (?!3D)|optical flow|jpeg|security|timer|fullscreen fps", re.I)


def _num(text):
    m = _NUM.search(text or "")
    return float(m.group().replace(",", ".")) if m else None


def _hw_kind(node):
    hid = (node.get("HardwareId") or "").lower()
    img = (node.get("ImageURL") or "").lower()
    if "cpu" in hid or img.endswith("cpu.png"):
        return "cpu"
    if "gpu" in hid or img.endswith(("nvidia.png", "ati.png", "amd.png")):
        return "igpu" if _IGPU.search(node.get("Text", "")) else "gpu"
    if "motherboard" in hid or "lpc" in hid or img.endswith(("mainboard.png", "chip.png")):
        return "board"
    if hid.startswith(("/ram", "/vram", "/memory")) or img.endswith("ram.png"):
        return "memory"
    if hid.startswith(("/nvme", "/hdd", "/ssd", "/storage")) or img.endswith("hdd.png"):
        return "storage"
    if hid.startswith("/nic") or img.endswith("nic.png"):
        return "network"
    return None


def flatten(node, hw=None, hw_name=None, group=None, out=None):
    """Walk LHM's tree into a flat list of sensors tagged with hardware kind, hardware name and group."""
    out = [] if out is None else out
    children = node.get("Children") or []
    kind = _hw_kind(node) if children else None
    if kind or (children and node.get("HardwareId")):
        hw, hw_name = kind or "other", node.get("Text")
    if children:
        for child in children:
            g = child.get("Text") if child.get("Children") and not child.get("HardwareId") else group
            flatten(child, hw, hw_name, g, out)
    elif node.get("Value") not in (None, ""):
        raw_type = (node.get("Type") or group or "").lower()
        value = _num(node.get("Value"))
        if raw_type == "throughput" and value is not None:
            unit = re.sub(r"^[-\d.,\s]+", "", node.get("Value") or "").strip().lower()
            value *= _RATE.get(unit, 1)
        out.append({"id": node.get("SensorId") or f"{hw_name}/{group}/{node.get('Text')}", "hw": hw,
                    "hw_name": hw_name, "type": raw_type, "name": node.get("Text", ""), "value": value})
    return out


# An AIO's or custom loop's coolant: "Liquid" (NZXT Kraken, MSI CoreLiquid), "Liquid Temperature", "Coolant" or
# "Water" (Aquacomputer, a board's "Water In" header). Never the processor, graphics card or a drive, whatever their
# sensors are called, and never a reading no coolant can have: an unplugged probe reads -40 or 127 °C, and fans
# following that would stay slow.
_LIQUID = re.compile(r"liquid|coolant|water", re.I)
LIQUID_RANGE = (5.0, 85.0)


def liquid_temp(sensors):
    lo, hi = LIQUID_RANGE
    for s in sensors:
        if (s["type"] in ("temperature", "temperatures") and s["value"] is not None and _LIQUID.search(s["name"])
                and s["hw"] not in ("cpu", "gpu", "igpu", "storage", "memory", "network") and lo <= s["value"] <= hi):
            return s["value"]
    return None


def _pick(sensors, hw, type_word, prefer):
    s = _pick_sensor(sensors, hw, type_word, prefer)
    return s["value"] if s else None


def _pick_sensor(sensors, hw, type_word, prefer):
    pool = [s for s in sensors if s["hw"] == hw and type_word in s["type"] and s["value"] is not None]
    for word in prefer:
        for s in pool:
            if word in s["name"].lower():
                return s
    return max(pool, key=lambda s: s["value"], default=None)


def summary(sensors):
    cpu_w = _pick(sensors, "cpu", "power", ["package"])
    gpu_w = _pick(sensors, "gpu", "power", ["gpu package", "gpu power", "board power", "total"])
    def fan_name(s):
        if s["hw"] == "gpu":
            return "GPU " + s["name"].replace("GPU ", "").lower()
        if s["hw"] == "board":
            return f"Board {s['name'].lower()}"
        return f"{s['hw_name']} {s['name'].lower()}"            # a cooler or fan hub: "NZXT Kraken Z pump"
    fans = [{"id": s["id"], "name": fan_name(s), "rpm": round(s["value"])}
            for s in sensors if s["type"] in ("fan", "fans") and s["value"] and s["hw"] in ("gpu", "board", "other")]
    names = {s["hw"]: s["hw_name"] for s in sensors if s["hw"] in ("cpu", "gpu")}
    return {
        "online": True,
        "cpu_name": names.get("cpu"),
        "gpu_name": names.get("gpu"),
        "cpu_temp": _pick(sensors, "cpu", "temperature", ["tctl/tdie", "tdie", "package", "core max"]),
        "gpu_temp": _pick(sensors, "gpu", "temperature", ["gpu core", "edge", "core"]),
        "gpu_hotspot": _pick(sensors, "gpu", "temperature", ["hot spot", "hotspot", "junction"]),
        "liquid_temp": liquid_temp(sensors),
        "cpu_power": cpu_w,
        "gpu_power": gpu_w,
        "total_power": round(cpu_w + gpu_w, 1) if cpu_w is not None and gpu_w is not None else None,
        "fans": fans,
    }


def parse(tree):
    return summary(flatten(tree))


# ---------------------------------------------------------------- catalog

def _meta(sid, name, group, kind, unit, value, hw=None, short=None):
    return {"id": sid, "name": name, "short": short or name, "group": group, "kind": kind, "unit": unit,
            "value": value, "hw": hw}


def catalog_from(flat):
    """Every LHM sensor worth showing, as picker entries."""
    out = []
    for s in flat:
        if _NOISE.search(s["name"]) or s["hw"] == "igpu" and s["type"] in ("load", "data", "smalldata"):
            continue
        kind, unit = TYPES.get(s["type"], (s["type"] or "other", ""))
        out.append(_meta(s["id"], s["name"], s["hw_name"] or "Other", kind, unit, s["value"], s["hw"]))
    return out


# "auto/..." -> (name, short name, kind, unit, resolver(flat sensors, system readings) -> value)
def _by(hw, type_word, prefer):
    return lambda flat, sysr: _pick(flat, hw, type_word, prefer)


def _first(hw, type_word, name_word):
    def get(flat, sysr):
        for s in flat:
            if s["hw"] == hw and s["type"] == type_word and name_word in s["name"].lower() and s["value"] is not None:
                return s["value"]
        return None
    return get


def _either(lhm, sys_key):
    def get(flat, sysr):
        v = lhm(flat, sysr)
        return v if v is not None else sysr.get(sys_key)
    return get


def _total_power(flat, sysr):
    c, g = _by("cpu", "power", ["package"])(flat, sysr), _by("gpu", "power", ["gpu package", "gpu power", "board power", "total"])(flat, sysr)
    return round(c + g, 1) if c is not None and g is not None else None


AUTO = {
    "auto/cpu_temp":    ("Processor temperature", "CPU", "temperature", "°C", _by("cpu", "temperature", ["tctl/tdie", "tdie", "package", "core max"])),
    "auto/gpu_temp":    ("Graphics temperature", "GPU", "temperature", "°C", _by("gpu", "temperature", ["gpu core", "edge", "core"])),
    "auto/gpu_hotspot": ("Graphics hot spot", "GPU hot spot", "temperature", "°C", _by("gpu", "temperature", ["hot spot", "hotspot", "junction"])),
    "auto/liquid_temp": ("Coolant temperature", "Coolant", "temperature", "°C", lambda flat, sysr: liquid_temp(flat)),
    "auto/cpu_load":    ("Processor load", "CPU load", "load", "%", _either(_first("cpu", "load", "total"), "cpu_load")),
    "auto/gpu_load":    ("Graphics load", "GPU load", "load", "%", _by("gpu", "load", ["gpu core", "d3d 3d"])),
    "auto/cpu_power":   ("Processor power", "CPU power", "power", "W", _by("cpu", "power", ["package"])),
    "auto/gpu_power":   ("Graphics power", "GPU power", "power", "W", _by("gpu", "power", ["gpu package", "gpu power", "board power", "total"])),
    "auto/total_power": ("Power draw", "Power", "power", "W", _total_power),
    "auto/cpu_clock":   ("Processor clock", "CPU clock", "clock", "MHz", _first("cpu", "clock", "cores (average)")),
    "auto/gpu_clock":   ("Graphics clock", "GPU clock", "clock", "MHz", _first("gpu", "clock", "core")),
    "auto/vram_used":   ("Graphics memory used", "VRAM", "data", "MB", _first("gpu", "smalldata", "memory used")),
    "auto/ram_load":    ("Memory in use", "RAM", "load", "%", lambda flat, sysr: sysr.get("ram_load")),
    "auto/ram_used":    ("Memory used", "RAM", "data", "GB", lambda flat, sysr: sysr.get("ram_used")),
    "auto/ram_temp":    ("Memory temperature", "RAM", "temperature", "°C", _by("memory", "temperature", ["dimm"])),
    "auto/ssd_temp":    ("Drive temperature", "SSD", "temperature", "°C", _by("storage", "temperature", ["composite", "temperature"])),
    "auto/net_down":    ("Download", "Down", "throughput", "B/s", lambda flat, sysr: sysr.get("net_down")),
    "auto/net_up":      ("Upload", "Up", "throughput", "B/s", lambda flat, sysr: sysr.get("net_up")),
    "auto/disk_read":   ("Disk read", "Read", "throughput", "B/s", lambda flat, sysr: sysr.get("disk_read")),
    "auto/disk_write":  ("Disk write", "Write", "throughput", "B/s", lambda flat, sysr: sysr.get("disk_write")),
}


class SystemSensors:
    """Memory, network, disk and processor load straight from Windows (psutil), so they work without LHM."""

    def __init__(self):
        self._last = None

    def read(self):
        try:
            import psutil
        except ImportError:
            return {}
        out = {}
        try:
            vm = psutil.virtual_memory()
            out["ram_load"] = round(vm.percent, 1)
            out["ram_used"] = round((vm.total - vm.available) / 1024 ** 3, 1)
            out["ram_total"] = round(vm.total / 1024 ** 3, 1)
            out["cpu_load"] = round(psutil.cpu_percent(interval=None), 1)
            now = time.monotonic()
            net, disk = psutil.net_io_counters(), psutil.disk_io_counters()
            counters = (now, net.bytes_recv if net else 0, net.bytes_sent if net else 0,
                        disk.read_bytes if disk else 0, disk.write_bytes if disk else 0)
            if self._last:
                dt = max(now - self._last[0], 0.1)
                rates = [max(0.0, (a - b) / dt) for a, b in zip(counters[1:], self._last[1:])]
                out.update(net_down=rates[0], net_up=rates[1], disk_read=rates[2], disk_write=rates[3])
            self._last = counters
        except (OSError, RuntimeError, AttributeError):
            pass
        return out


SYSTEM = [
    ("sys/ram_load", "Memory in use", "load", "%"), ("sys/ram_used", "Memory used", "data", "GB"),
    ("sys/cpu_load", "Processor load", "load", "%"), ("sys/net_down", "Download", "throughput", "B/s"),
    ("sys/net_up", "Upload", "throughput", "B/s"), ("sys/disk_read", "Disk read", "throughput", "B/s"),
    ("sys/disk_write", "Disk write", "throughput", "B/s"),
]


def _source_hw(sid):
    if "vram" in sid or "gpu" in sid:
        return "gpu"
    if "cpu" in sid:
        return "cpu"
    if "ssd" in sid:
        return "storage"
    if "ram_temp" in sid:
        return "memory"
    return None


def build(flat, sysr, extra=()):
    """All readings as {id: meta-with-value}: shortcuts first, then Windows, then everything from LHM."""
    out = {}
    names = {}
    for f in flat:
        if f["hw"] != "memory" or f["type"] == "temperature":      # name memory after the sticks, not "Virtual Memory"
            names.setdefault(f["hw"], f["hw_name"])
    vram_total = _first("gpu", "smalldata", "memory total")(flat, sysr)
    for sid, (name, short, kind, unit, get) in AUTO.items():
        try:
            v = get(flat, sysr)
        except (TypeError, ValueError):
            v = None
        m = out[sid] = _meta(sid, name, "Quick picks", kind, unit, v, short=short)
        m["source"] = names.get(_source_hw(sid)) or ("Windows" if sid[5:] in sysr or "net" in sid or "disk" in sid else None)
    if vram_total:
        out["auto/vram_used"]["max"] = vram_total
    if sysr.get("ram_total"):
        out["auto/ram_used"]["max"] = sysr["ram_total"]
    for sid, name, kind, unit in SYSTEM:
        out[sid] = _meta(sid, name, "Windows", kind, unit, sysr.get(sid[4:]))
        if sid == "sys/ram_used" and sysr.get("ram_total"):
            out[sid]["max"] = sysr["ram_total"]
    for m in extra:
        out[m["id"]] = m
    for m in catalog_from(flat):
        out.setdefault(m["id"], m)
    return out


def fmt(meta, value, fahrenheit=False):
    """A reading as short text for the tray: 45°C, 63%, 212 W, 4.6 GHz, 12.3 MB/s."""
    if value is None:
        return "n/a"
    unit = meta.get("unit", "")
    if unit == "°C":
        return f"{value * 9 / 5 + 32:.0f}°F" if fahrenheit else f"{value:.0f}°C"
    if unit == "B/s":
        for div, u in ((1024 ** 3, "GB/s"), (1024 ** 2, "MB/s"), (1024, "KB/s")):
            if value >= div:
                return f"{value / div:.1f} {u}"
        return f"{value:.0f} B/s"
    if unit == "MHz" and value >= 1000:
        return f"{value / 1000:.2f} GHz"
    if unit == "%":
        return f"{value:.0f}%"
    if unit == "GB":
        return f"{value:.1f} GB"
    if unit == "V":
        return f"{value:.2f} V"
    return f"{value:.0f} {unit}".strip()


def read_tree(base_url, timeout=1.5, headers=None):
    """LHM's sensor tree, or None when it isn't reachable."""
    req = urllib.request.Request(base_url.rstrip("/") + "/data.json", headers=headers or {})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return json.loads(resp.read().decode("utf-8"))
    except (urllib.error.URLError, OSError, ValueError):
        return None


def read(base_url, timeout=1.5):
    tree = read_tree(base_url, timeout)
    if tree is None:
        return dict(OFFLINE)
    try:
        return parse(tree)
    except (KeyError, TypeError, AttributeError):
        return dict(OFFLINE)
