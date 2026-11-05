"""Sensor readings.

LibreHardwareMonitor's web server gives every sensor it knows (temperatures, loads, clocks, fans, memory, drives,
network). We keep all of them in a catalog keyed by LHM's own stable SensorId, plus a summary (CPU/GPU temperature
and power) that fan curves and effects use. A few readings come from Windows itself (memory, network, disk) so the
Overview still has something to show when LHM isn't running. "auto/..." ids are portable shortcuts that resolve to the
right sensor on any PC, which is what the default Overview uses.
"""

import json
import logging
import re
import threading
import time
import urllib.error
import urllib.request

from . import lhmhttp

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
_NOISE = re.compile(r"limit|resolution|warning.*temperature|critical.*temperature|threshold|capacity|power on|"
                    r"^(?-i:t[A-Z])|^D3D (?!3D)|optical flow|jpeg|security|timer|fullscreen fps", re.I)


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


def flatten(node, hw=None, hw_name=None, group=None, out=None, hw_id=None):
    """Walk LHM's tree into a flat list of sensors tagged with hardware kind, hardware name and group."""
    out = [] if out is None else out
    children = node.get("Children") or []
    kind = _hw_kind(node) if children else None
    if kind or (children and node.get("HardwareId")):
        hw, hw_name = kind or "other", node.get("Text")
        hw_id = node.get("HardwareId")
    if children:
        for child in children:
            g = child.get("Text") if child.get("Children") and not child.get("HardwareId") else group
            flatten(child, hw, hw_name, g, out, hw_id)
    elif node.get("Value") not in (None, ""):
        raw_type = (node.get("Type") or group or "").lower().replace(" ", "")
        if raw_type.endswith("s") and raw_type[:-1] in TYPES:
            raw_type = raw_type[:-1]
        value = _num(node.get("Value"))
        if raw_type == "throughput" and value is not None:
            unit = re.sub(r"^[-\d.,\s]+", "", node.get("Value") or "").strip().lower()
            value *= _RATE.get(unit, 1)
        out.append({"id": node.get("SensorId") or f"{hw_name}/{group}/{node.get('Text')}", "hw": hw,
                    "hw_name": hw_name, "hw_id": hw_id, "type": raw_type, "name": node.get("Text", ""), "value": value})
    return out


# An AIO's or custom loop's coolant: "Liquid" (NZXT Kraken, MSI CoreLiquid), "Liquid Temperature", "Coolant" or
# "Water" (Aquacomputer, a board's "Water In" header). Never the processor, graphics card or a drive, whatever their
# sensors are called, and never a reading no coolant can have: an unplugged probe reads -40 or 127 °C, and fans
# following that would stay slow.
_LIQUID = re.compile(r"liquid|coolant|water", re.I)
LIQUID_RANGE = (5.0, 85.0)


def liquid_temp(sensors):
    s = _liquid_sensor(sensors)
    return s["value"] if s else None


def _liquid_sensor(sensors):
    lo, hi = LIQUID_RANGE
    for s in sensors:
        if (s["type"] in ("temperature", "temperatures") and s["value"] is not None and _LIQUID.search(s["name"])
                and not _NOISE.search(s["name"])
                and s["hw"] not in ("cpu", "gpu", "igpu", "storage", "memory", "network") and lo <= s["value"] <= hi):
            return s
    return None


def _pick(sensors, hw, type_word, prefer, *, strict=False):
    s = _pick_sensor(sensors, hw, type_word, prefer, strict=strict)
    return s["value"] if s else None


def _pick_sensor(sensors, hw, type_word, prefer, *, strict=False):
    """Named metrics require an exact name; generic temperatures retain their hottest-reading fallback."""
    pool = [s for s in sensors if s["hw"] == hw and type_word in s["type"] and s["value"] is not None
            and (strict or not _NOISE.search(s["name"]))]
    for word in prefer:
        for s in pool:
            name = s["name"].strip().lower()
            matches = name == word if strict else word in name
            if matches:
                return s
    return None if strict else max(pool, key=lambda s: s["value"], default=None)


_CPU_POWER = ("cpu package", "package", "cpu package power", "package power")
_GPU_POWER = ("gpu package", "gpu power", "board power", "gpu board power", "gpu total board power",
              "total board power", "gpu total", "total")
_GPU_HOTSPOT = ("gpu hot spot", "gpu hotspot", "hot spot", "hotspot", "gpu junction", "junction")


def summary(sensors):
    cpu_w = _pick(sensors, "cpu", "power", _CPU_POWER, strict=True)
    gpu_w = _pick(sensors, "gpu", "power", _GPU_POWER, strict=True)
    cpu_t = _pick_sensor(sensors, "cpu", "temperature", ["tctl/tdie", "tdie", "package", "core max"])
    gpu_t = _pick_sensor(sensors, "gpu", "temperature", ["gpu core", "edge", "core"])
    def fan_name(s):
        if s["hw"] == "gpu":
            return "GPU " + s["name"].replace("GPU ", "").lower()
        if s["hw"] == "board":
            return f"Board {s['name'].lower()}"
        return f"{s['hw_name']} {s['name'].lower()}"            # a cooler or fan hub: "NZXT Kraken Z pump"
    fans = [{"id": s["id"], "name": fan_name(s), "rpm": round(s["value"])}
            for s in sensors if s["type"] in ("fan", "fans") and s["value"] and s["hw"] in ("gpu", "board", "other")]
    names = {}
    for s in sensors:
        names.setdefault(s["hw"], s["hw_name"])
    return {
        "online": True,
        "cpu_name": cpu_t["hw_name"] if cpu_t else names.get("cpu"),
        "gpu_name": gpu_t["hw_name"] if gpu_t else names.get("gpu"),
        "cpu_temp": cpu_t["value"] if cpu_t else None,
        "gpu_temp": gpu_t["value"] if gpu_t else None,
        "gpu_hotspot": _pick(sensors, "gpu", "temperature", _GPU_HOTSPOT, strict=True),
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


# Resolvers keep the selected reading and its identity together; derived values retain all input ids.
def _by(hw, type_word, prefer, *, strict=False):
    return lambda flat, sysr: _pick_sensor(flat, hw, type_word, prefer, strict=strict)


def _system(key):
    def get(flat, sysr):
        value = sysr.get(key)
        return {"id": "sys/" + key, "value": value, "hw_name": "Windows"} if value is not None else None
    return get


def _either(lhm, sys_key):
    def get(flat, sysr):
        reading = lhm(flat, sysr)
        return reading if reading is not None else _system(sys_key)(flat, sysr)
    return get


def _total_power(flat, sysr):
    c = _pick_sensor(flat, "cpu", "power", _CPU_POWER, strict=True)
    g = _pick_sensor(flat, "gpu", "power", _GPU_POWER, strict=True)
    if c is None or g is None:
        return None
    return {"value": round(c["value"] + g["value"], 1), "source_ids": [c["id"], g["id"]],
            "hw_name": f"{c['hw_name'] or 'CPU'} + {g['hw_name'] or 'GPU'}"}


def _device_id(s):
    return s.get("hw_id") or s["id"].rsplit("/", 2)[0]


def _cpu_clock(flat, sysr):
    average = _pick_sensor(flat, "cpu", "clock", ["cores (average)", "cpu cores (average)"], strict=True)
    if average is not None:
        return average
    cores = [s for s in flat if s["hw"] == "cpu" and "clock" in s["type"]
             and re.fullmatch(r"(?:CPU )?Core #\d+", s["name"], re.I)]
    if not cores:
        return None
    cores = [s for s in cores if _device_id(s) == _device_id(cores[0])]
    if any(s["value"] is None for s in cores):
        return None
    return {"value": round(sum(s["value"] for s in cores) / len(cores), 1),
            "hw_name": cores[0]["hw_name"], "source_ids": [s["id"] for s in cores]}


AUTO = {
    "auto/cpu_temp":    ("Processor temperature", "CPU", "temperature", "°C", _by("cpu", "temperature", ["tctl/tdie", "tdie", "package", "core max"])),
    "auto/gpu_temp":    ("Graphics temperature", "GPU", "temperature", "°C", _by("gpu", "temperature", ["gpu core", "edge", "core"])),
    "auto/gpu_hotspot": ("Graphics hot spot", "GPU hot spot", "temperature", "°C", _by("gpu", "temperature", _GPU_HOTSPOT, strict=True)),
    "auto/liquid_temp": ("Coolant temperature", "Coolant", "temperature", "°C", lambda flat, sysr: _liquid_sensor(flat)),
    "auto/cpu_load":    ("Processor load", "CPU load", "load", "%", _either(_by("cpu", "load", ["cpu total", "total"], strict=True), "cpu_load")),
    "auto/gpu_load":    ("Graphics load", "GPU load", "load", "%", _by("gpu", "load", ["gpu core", "d3d 3d"], strict=True)),
    "auto/cpu_power":   ("Processor power", "CPU power", "power", "W", _by("cpu", "power", _CPU_POWER, strict=True)),
    "auto/gpu_power":   ("Graphics power", "GPU power", "power", "W", _by("gpu", "power", _GPU_POWER, strict=True)),
    "auto/total_power": ("CPU + GPU power", "CPU + GPU", "power", "W", _total_power),
    "auto/cpu_clock":   ("Processor clock", "CPU clock", "clock", "MHz", _cpu_clock),
    "auto/gpu_clock":   ("Graphics clock", "GPU clock", "clock", "MHz", _by("gpu", "clock", ["gpu core", "core"], strict=True)),
    "auto/vram_used":   ("Graphics memory used", "VRAM", "data", "MB", _by("gpu", "smalldata", ["gpu memory used", "memory used", "d3d dedicated memory used"], strict=True)),
    "auto/ram_load":    ("Memory in use", "RAM", "load", "%", _system("ram_load")),
    "auto/ram_used":    ("Memory used", "RAM", "data", "GB", _system("ram_used")),
    "auto/ram_temp":    ("Memory temperature", "RAM", "temperature", "°C", _by("memory", "temperature", ["dimm"])),
    "auto/ssd_temp":    ("Drive temperature", "SSD", "temperature", "°C", _by("storage", "temperature", ["composite", "temperature"])),
    "auto/net_down":    ("Download", "Down", "throughput", "B/s", _system("net_down")),
    "auto/net_up":      ("Upload", "Up", "throughput", "B/s", _system("net_up")),
    "auto/disk_read":   ("Disk read", "Read", "throughput", "B/s", _system("disk_read")),
    "auto/disk_write":  ("Disk write", "Write", "throughput", "B/s", _system("disk_write")),
}


class SystemSensors:
    """Memory, network, disk and processor load straight from Windows (psutil), so they work without LHM."""

    def __init__(self):
        self._last = None
        self._cpu_primed = False

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
            cpu_load = psutil.cpu_percent(interval=None)
            if self._cpu_primed:
                out["cpu_load"] = round(cpu_load, 1)
            self._cpu_primed = True
            now = time.monotonic()
            net, disk = psutil.net_io_counters(), psutil.disk_io_counters()
            counters = (now, net.bytes_recv if net else None, net.bytes_sent if net else None,
                        disk.read_bytes if disk else None, disk.write_bytes if disk else None)
            if self._last:
                dt = max(now - self._last[0], 0.1)
                for key, a, b in zip(("net_down", "net_up", "disk_read", "disk_write"), counters[1:], self._last[1:]):
                    if a is not None and b is not None:
                        out[key] = max(0.0, (a - b) / dt)
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


def build(flat, sysr, extra=()):
    """All readings as {id: meta-with-value}: shortcuts first, then Windows, then everything from LHM."""
    out = {}
    for sid, (name, short, kind, unit, get) in AUTO.items():
        try:
            reading = get(flat, sysr)
        except (TypeError, ValueError):
            reading = None
        v = reading["value"] if reading else None
        m = out[sid] = _meta(sid, name, "Quick picks", kind, unit, v, short=short)
        m["source"] = reading.get("hw_name") if reading else None
        if reading:
            if "id" in reading:
                m["source_id"] = reading["id"]
            if "name" in reading:
                m["sensor_name"] = reading["name"]
            if "source_ids" in reading:
                m["source_ids"] = reading["source_ids"]
            if sid == "auto/vram_used":
                same_gpu = [s for s in flat if _device_id(s) == _device_id(reading)]
                total = _pick_sensor(same_gpu, "gpu", "smalldata",
                                     ["gpu memory total", "memory total", "d3d dedicated memory total"], strict=True)
                if total and total["value"] > 0:
                    m["max"] = total["value"]
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
    """A reading as short text for the tray: 45°C, 63%, 212 W, 4.6 GHz, 12.3 MiB/s."""
    if value is None:
        return "n/a"
    unit = meta.get("unit", "")
    if unit == "°C":
        return f"{value * 9 / 5 + 32:.0f}°F" if fahrenheit else f"{value:.0f}°C"
    if unit == "B/s":
        for div, u in ((1024 ** 3, "GiB/s"), (1024 ** 2, "MiB/s"), (1024, "KiB/s")):
            if value >= div:
                return f"{value / div:.1f} {u}"
        return f"{value:.0f} B/s"
    if unit == "MHz" and value >= 1000:
        return f"{value / 1000:.2f} GHz"
    if unit == "%":
        return f"{value:.0f}%"
    if unit == "GB":
        return f"{value:.1f} GiB"
    if unit == "MB":
        return f"{value:.0f} MiB"
    if unit == "V":
        return f"{value:.2f} V"
    if unit == "€/h":
        return f"€{value:.2f}/h"
    return f"{value:.0f} {unit}".strip()


def read_tree(base_url, timeout=1.5, headers=None):
    """LHM's sensor tree, or None when it isn't reachable."""
    try:
        return lhmhttp.read_json(base_url.rstrip("/") + "/data.json", timeout=timeout, headers=headers)
    except (urllib.error.URLError, OSError, ValueError):
        return None


class LhmPoller:
    """Reads LHM's sensor tree on its own thread, so a slow or missing LHM never holds up automations and fans: a
    connect to a closed port takes up to the whole timeout. While LHM is offline it is tried only every
    OFFLINE_RETRY seconds; wake() tries again now (a new address, or helper setup just started it)."""

    INTERVAL = 2.0
    OFFLINE_RETRY = 12.0
    STARTUP_RETRY = 0.25
    STARTUP_WINDOW = 45.0
    STALE_AFTER = 6.0

    def __init__(self, url, headers=lambda: None, read=None, on_change=lambda: None):
        self._url, self._headers, self._read = url, headers, read or read_tree
        self._on_change = on_change
        self.tree = None                     # the latest tree, or None while LHM isn't answering
        self.last_success = self.last_success_at = None
        self.last_duration = None
        self._wake = threading.Event()
        self._stop = threading.Event()
        self._thread = None

    @property
    def fresh_tree(self):
        if self.last_success is None or time.monotonic() - self.last_success > self.STALE_AFTER:
            return None
        return self.tree

    def poll(self):
        was_online = self.tree is not None
        started = time.monotonic()
        try:
            self.tree = self._read(self._url(), headers=self._headers())
            if self.tree is not None:
                self.last_success, self.last_success_at = time.monotonic(), time.time()
                self.last_duration = self.last_success - started
        except Exception:                    # a bad address or a bug here must not end the thread
            logging.getLogger(__name__).exception("reading LibreHardwareMonitor failed")
            self.tree = None
        if was_online != (self.tree is not None):
            try:
                self._on_change()
            except Exception:
                logging.getLogger(__name__).exception("reporting sensor availability failed")
        return self.tree

    def wake(self):
        self._wake.set()

    def _run(self):
        started, retry, seen_online = time.monotonic(), self.STARTUP_RETRY, False
        while not self._stop.is_set():
            tree = self.poll()
            if tree is not None:
                seen_online = True
                delay = self.INTERVAL
            elif not seen_online and time.monotonic() - started < self.STARTUP_WINDOW:
                delay = min(retry, self.OFFLINE_RETRY)
                retry = min(retry * 2, self.OFFLINE_RETRY)
            else:
                delay = self.OFFLINE_RETRY
            self._wake.wait(delay)
            self._wake.clear()

    def start(self):
        self._thread = threading.Thread(target=self._run, name="sensors", daemon=True)
        self._thread.start()

    def stop(self):
        self._stop.set()
        self._wake.set()


def read(base_url, timeout=1.5):
    tree = read_tree(base_url, timeout)
    if tree is None:
        return dict(OFFLINE)
    try:
        return parse(tree)
    except (KeyError, TypeError, AttributeError):
        return dict(OFFLINE)
