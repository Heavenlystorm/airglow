"""Unified device layer: native USB drivers (NZXT, ASUS Aura) and OpenRGB devices, all exposed as devices with
zones. The lighting engine only ever sees zones and LED colour lists.

Which USB controllers get a native driver, and NZXT accessory sizes, live in data/devices.json, so supporting
another model of a known family is a data change, not a code change.
"""

import json
import logging
import re
import threading
import time

from . import devices as native
from . import paths
from .openrgb import OpenRGBClient, OpenRGBError, ZONE_MATRIX

log = logging.getLogger(__name__)


def load_registry(path=None):
    """data/devices.json with hex ids turned into ints."""
    raw = json.loads((path or paths.PACKAGE_DIR / "data" / "devices.json").read_text(encoding="utf-8"))
    entries = []
    for e in raw["native"]:
        entries.append({**e, "vid": int(e["vid"], 16), "pid": int(e["pid"], 16),
                        "interface": e.get("interface"), "experimental": bool(e.get("experimental"))})
    accessories = {int(k, 16): v for k, v in raw["nzxt_accessories"].items()}
    return {"native": entries, "openrgb_skip": raw.get("openrgb_skip", {}), "nzxt_accessories": accessories}


REGISTRY = load_registry()
# NZXT accessory id (from the hub's LED info report) -> {"name", "leds", "fans"}. Unknown accessories fall back to
# the channel maximum and the user sets the count (with the identify sweep).
NZXT_ACCESSORIES = REGISTRY["nzxt_accessories"]


class Zone:
    def __init__(self, zid, name, max_leds, default_leds, shape="strip", resizable=True):
        self.id, self.name = zid, name
        self.max_leds, self.default_leds = max_leds, default_leds
        self.shape, self.resizable = shape, resizable
        self.fans = 1
        self.provisional = False          # True while the device hasn't told us its real layout yet

    def info(self):
        return {"id": self.id, "name": self.name, "max_leds": self.max_leds, "default_leds": self.default_leds,
                "shape": self.shape, "fans": self.fans, "resizable": self.resizable, "provisional": self.provisional}


class Device:
    id = name = vendor = kind = ""
    type_name = "Device"
    experimental = False      # driver not yet confirmed on real hardware (shown in the Devices tab)
    note = None

    def __init__(self):
        self.zones = []
        self.error = None

    @property
    def online(self):
        raise NotImplementedError

    def open(self):
        pass

    def close(self):
        pass

    def write_zone(self, zone_id, colors):
        raise NotImplementedError

    def flush(self):
        pass

    def info(self):
        return {"id": self.id, "name": self.name, "vendor": self.vendor, "kind": self.kind, "type": self.type_name,
                "online": self.online, "error": self.error, "experimental": self.experimental, "note": self.note,
                "zones": [z.info() for z in self.zones]}


# ---------------------------------------------------------------- native: NZXT HUE 2 family

def nzxt_channel(acc):
    """One channel's accessory ids -> (LEDs, shape, fans). acc is None before the hub has reported."""
    if acc is None:
        return 40, "strip", 1                          # not reported yet
    if not acc:
        return 0, "strip", 1                           # nothing plugged in
    if not all(a in NZXT_ACCESSORIES for a in acc):
        return 40, "strip", 1                          # unknown accessory: the user sets the count
    parts = [NZXT_ACCESSORIES[a] for a in acc]
    leds = min(sum(p["leds"] for p in parts), native.NzxtControlHub.LEDS_PER_CHANNEL)
    if all(p["fans"] for p in parts):
        return leds, "fans", sum(p["fans"] for p in parts)
    return leds, "strip", 1                            # strips, or fans mixed with strips


class NzxtDevice(Device):
    id, name, vendor, kind, type_name = "nzxt-hub", "NZXT Control Hub", "NZXT", "native", "Fan and lighting hub"

    REPORT_GRACE = 5.0        # seconds to wait for the accessory report before trusting the 40-LED fallback

    def __init__(self, hub=None, dev_id="nzxt-hub", spec=None):
        self._zones, self._zones_for = [], object()
        self._opened_at = None
        super().__init__()
        self.hub = hub or native.NzxtControlHub()
        self.id = dev_id
        if spec:
            self.name, self.experimental, self.note = spec["name"], spec["experimental"], spec.get("note")
            leds, fans = spec["led_channels"], spec["fan_ports"]
            self.type_name = ("Fan and lighting hub" if leds and fans else "Fan controller" if fans
                              else "Lighting controller")

    @property
    def zones(self):
        """Rebuilt whenever the hub reports (or changes) its accessories, even if that report arrives late."""
        acc = self.hub.accessories
        waiting = acc is None and (self._opened_at is None or time.monotonic() - self._opened_at < self.REPORT_GRACE)
        key = (acc if acc is not None else None, waiting)
        if key != self._zones_for:
            self._set_zones(acc if acc is not None else [None] * getattr(self.hub, "led_channels", 5))
            for z in self._zones:
                z.provisional = waiting
            self._zones_for = key
        return self._zones

    @zones.setter
    def zones(self, value):
        self._zones = value

    def _set_zones(self, accessories):
        """accessories: per channel a list of accessory ids, or None before the hub has reported."""
        self._zones = []
        for ch, acc in enumerate(accessories):
            leds, shape, fans = nzxt_channel(acc)
            z = Zone(f"ch{ch + 1}", f"Channel {ch + 1}", 40, leds, shape)
            z.fans = fans
            self._zones.append(z)

    @property
    def online(self):
        return self.hub.is_open

    def open(self):
        self.hub.open()
        if self._opened_at is None:
            self._opened_at = time.monotonic()

    def close(self):
        self.hub.close()

    def write_zone(self, zone_id, colors):
        self.hub.set_channel_colors(int(zone_id[2:]) - 1, colors)


# ---------------------------------------------------------------- native: ASUS Aura

class AuraDevice(Device):
    id, name, vendor, kind, type_name = "asus-aura", "ASUS Aura", "ASUS", "native", "Motherboard lighting"

    def __init__(self, aura=None, dev_id="asus-aura", spec=None):
        super().__init__()
        self.aura = aura or native.AsusAura()
        self.id = dev_id
        if spec:
            self.name, self.experimental, self.note = spec["name"], spec["experimental"], spec.get("note")
        self._set_zones()

    def _set_zones(self):
        board = max(self.aura.board_leds, 1)
        self.zones = [Zone("board", "Motherboard", board, board, "strip", resizable=False)]
        # Until the board reports its headers (or if it never answers), offer three, the common count.
        known = getattr(self.aura, "config_known", False)
        for i in range(self.aura.argb_channels if known else max(self.aura.argb_channels, 3)):
            self.zones.append(Zone(f"argb{i + 1}", f"ARGB header {i + 1}", 120, 30, "strip"))

    @property
    def online(self):
        return self.aura.is_open

    def open(self):
        self.aura.open()
        self._set_zones()

    def close(self):
        self.aura.close()

    def write_zone(self, zone_id, colors):
        if zone_id == "board":
            self.aura.set_board_colors(colors)
        else:
            self.aura.set_argb_colors(int(zone_id[4:]) - 1, colors)


# ---------------------------------------------------------------- OpenRGB devices

class OpenRGBDevice(Device):
    kind = "openrgb"

    def __init__(self, bridge, ctrl):
        super().__init__()
        self.bridge = bridge
        self.update(ctrl)

    def update(self, ctrl):
        self.index = ctrl["index"]
        self.ctrl = ctrl
        self.name, self.vendor, self.type_name = ctrl["name"], ctrl["vendor"], ctrl["type_name"]
        def slug(text):
            return re.sub(r"-+", "-", "".join(ch if ch.isalnum() else "-" for ch in str(text).lower())).strip("-")
        name = ctrl["name"]
        if ctrl["vendor"] and not name.lower().startswith(ctrl["vendor"].lower()):
            name = f"{ctrl['vendor']} {name}"
        # serial when the device has one, else its bus location (e.g. "I2C: NVIDIA GPU ...") made key-safe
        self.id = f"openrgb:{slug(name)}:{slug(ctrl.get('serial') or ctrl.get('location') or self.index)[:48]}"
        self.zones = []
        for zi, z in enumerate(ctrl["zones"]):
            resizable = z["leds_min"] != z["leds_max"]
            count = z["leds_count"] if z["leds_count"] or not resizable else min(30, z["leds_max"])
            shape = "strip" if z["type"] != ZONE_MATRIX else "strip"
            self.zones.append(Zone(f"z{zi}", z["name"], z["leds_max"] or count, count, shape, resizable))
        self._custom_mode_set = False

    @property
    def online(self):
        return self.bridge.client.connected

    def zone_index(self, zone_id):
        return int(zone_id[1:])

    def ensure_size(self, zone_id, count):
        zi = self.zone_index(zone_id)
        z = self.ctrl["zones"][zi]
        if z["leds_min"] != z["leds_max"] and z["leds_count"] != count:
            self.bridge.client.resize_zone(self.index, zi, count)
            z["leds_count"] = count

    def write_zone(self, zone_id, colors):
        if not self._custom_mode_set:
            self.bridge.client.set_custom_mode(self.index)
            self._custom_mode_set = True
        self.ensure_size(zone_id, len(colors))
        self.bridge.client.update_zone(self.index, self.zone_index(zone_id), colors)


class OpenRGBBridge:
    """Keeps a connection to OpenRGB's SDK server and mirrors its controller list."""

    RETRY = 10.0

    def __init__(self, host="127.0.0.1", port=6742, skip=()):
        self.client = OpenRGBClient(host, port)
        self.skip = skip            # name fragments of devices we drive natively (avoid double control)
        self.devices = {}
        self.status = "OpenRGB isn't running"
        self._last_try = 0.0

    def refresh(self, enabled):
        if not enabled:
            if self.client.connected:
                self.client.close()
            self.devices, self.status = {}, "OpenRGB bridge is off"
            return
        if not self.client.connected:
            if time.monotonic() - self._last_try < self.RETRY:
                return
            self._last_try = time.monotonic()
            try:
                self.client.connect()
            except OSError:
                self.devices, self.status = {}, "OpenRGB isn't running (start it with its SDK server on)"
                return
            self._load()
        else:
            self.client.poll_notifications()
            if self.client.devices_changed:
                self._load()

    def _load(self):
        try:
            ctrls = self.client.controllers()
        except (OpenRGBError, OSError) as e:
            self.status = f"OpenRGB connection lost ({e})"
            self.client.close()
            self.devices = {}
            return
        found = {}
        for c in ctrls:
            if any(s.lower() in f"{c['vendor']} {c['name']}".lower() for s in self.skip):
                continue
            d = OpenRGBDevice(self, c)
            found[d.id] = d
        self.devices = found
        self.status = f"Connected, {len(found)} device{'s' if len(found) != 1 else ''}"


# ---------------------------------------------------------------- manager

DRIVER_IDS = {"nzxt": "nzxt-hub", "aura": "asus-aura"}


def detect_natives(hid_list, registry=None, backend=None):
    """Native devices for every registry controller in an hid.enumerate() list. One device per physical controller
    (the first matching interface, or the one the registry names); ids stay "nzxt-hub" / "asus-aura" for the first
    of a kind, so saved zone settings keep working, then "-2", "-3"."""
    registry = registry or REGISTRY
    found, seen, counts = [], set(), {}
    for spec in registry["native"]:
        for info in hid_list:
            if (info.get("vendor_id"), info.get("product_id")) != (spec["vid"], spec["pid"]):
                continue
            if spec["interface"] is not None and info.get("interface_number") != spec["interface"]:
                continue
            key = (spec["vid"], spec["pid"], info.get("serial_number") or "")
            if key in seen and spec["interface"] is None:
                continue                                 # another interface of a controller we already have
            seen.add(key)
            drv = spec["driver"]
            counts[drv] = counts.get(drv, 0) + 1
            dev_id = DRIVER_IDS[drv] + ("" if counts[drv] == 1 else f"-{counts[drv]}")
            path = info.get("path")
            if drv == "nzxt":
                hub = native.NzxtControlHub(backend, pid=spec["pid"], name=spec["name"],
                                            led_channels=spec["led_channels"], fan_ports=spec["fan_ports"], path=path)
                found.append(NzxtDevice(hub, dev_id, spec))
            elif drv == "aura":
                aura = native.AsusAura(backend=backend, pid=spec["pid"], interface=spec["interface"], path=path)
                found.append(AuraDevice(aura, dev_id, spec))
            log.info("found %s (%04x:%04x)%s", spec["name"], spec["vid"], spec["pid"],
                     ", experimental support" if spec["experimental"] else "")
    return found


class DeviceManager:
    RECONNECT = 5.0
    RESET_DEBOUNCE = 5.0     # Windows sends two resume broadcasts, and the engine and controller notice the gap too

    def __init__(self, hid_backend=None, natives=None, bridge=None, registry=None):
        registry = registry or REGISTRY
        if natives is None:
            try:
                import hid
                natives = detect_natives((hid_backend or hid).enumerate(), registry, hid_backend)
            except Exception as e:  # no USB access must never stop OpenRGB devices from working
                log.warning("USB device scan failed: %s", e)
                natives = []
        self.natives = {d.id: d for d in natives}
        drivers = {"nzxt" if isinstance(d, NzxtDevice) else "aura" for d in natives
                   if isinstance(d, (NzxtDevice, AuraDevice))}
        skip = [name for drv in sorted(drivers) for name in registry["openrgb_skip"].get(drv, [])]
        self.bridge = bridge or OpenRGBBridge(skip=tuple(skip))
        self._last_reconnect = 0.0
        self._last_reset = None
        self._lock = threading.RLock()
        # Separate from _lock: maintain() can wait seconds on an OpenRGB connect, and the lighting thread takes
        # _lock (all()) every frame.
        self._maintain_lock = threading.RLock()
        self._reset_wanted = threading.Event()

    def all(self):
        with self._lock:
            return {**self.natives, **self.bridge.devices}

    def get(self, device_id):
        return self.all().get(device_id)

    def native(self, device_id):
        return self.natives.get(device_id)

    def maintain(self, openrgb_enabled, force=False):
        """Open/reconnect devices. Called from the device housekeeping thread (and once at start)."""
        with self._maintain_lock:
            self._maintain(openrgb_enabled, force)

    def _maintain(self, openrgb_enabled, force):
        if force or time.monotonic() - self._last_reconnect >= self.RECONNECT:
            self._last_reconnect = time.monotonic()
            for d in self.natives.values():
                if not d.online:
                    try:
                        d.open()
                        d.error = None
                        log.info("%s connected", d.name)
                    except native.DeviceError as e:
                        if d.error != str(e):
                            log.warning("%s", e)
                        d.error = str(e)
        try:
            self.bridge.refresh(openrgb_enabled)
        except Exception as e:  # the bridge must never stop native devices from working
            log.warning("OpenRGB bridge error: %s", e)

    def start(self, openrgb_enabled):
        """Run device housekeeping (USB reconnects, OpenRGB connects and notifications) on its own thread.
        A refused TCP connect to a closed local port takes ~2 s on Windows; doing that on the lighting thread
        froze the lights every 10 seconds while OpenRGB wasn't running."""
        self._stop = threading.Event()

        def loop():
            while not self._stop.is_set():
                try:
                    if self._reset_wanted.is_set():
                        self._reset_wanted.clear()
                        self.reset(openrgb_enabled())
                    self.maintain(openrgb_enabled())
                except Exception:
                    log.exception("device housekeeping failed")
                self._stop.wait(1.0)

        threading.Thread(target=loop, name="devices", daemon=True).start()

    def request_reset(self):
        """Ask the housekeeping thread to reopen everything (for callers that mustn't block, like the lights)."""
        self._reset_wanted.set()

    def reset(self, openrgb_enabled):
        """After sleep: reopen everything. Repeat calls within a few seconds are ignored."""
        with self._maintain_lock:
            now = time.monotonic()
            if self._last_reset is not None and now - self._last_reset < self.RESET_DEBOUNCE:
                log.debug("device reset skipped (one ran %.1f s ago)", now - self._last_reset)
                return False
            self._last_reset = now
            for d in self.natives.values():
                d.close()
            self.bridge.client.close()
            self.bridge._last_try = 0.0
            self._maintain(openrgb_enabled, force=True)
            return True

    def close(self):
        if getattr(self, "_stop", None):
            self._stop.set()
        for d in self.natives.values():
            d.close()
        self.bridge.client.close()
