"""Unified device layer: native USB controllers (one plugin per family, in airglow/plugins/), OpenRGB devices and
Windows Dynamic Lighting, all exposed as devices with zones. The lighting engine only ever sees zones and LED colour
lists.

Supporting another controller family is one new plugin module; another model of a known family is one more Product
line in that plugin.
"""

import logging
import re
import threading
import time

from . import devices as native
from .openrgb import OpenRGBClient, OpenRGBError, ZONE_MATRIX

log = logging.getLogger(__name__)


class Zone:
    def __init__(self, zid, name, max_leds, default_leds, shape="strip", resizable=True):
        self.id, self.name = zid, name
        self.max_leds, self.default_leds = max_leds, default_leds
        self.shape, self.resizable = shape, resizable
        self.fans = 1
        self.matrix = None                # {"rows", "cols", "map"} for keyboards and other LED grids
        self.provisional = False          # True while the device hasn't told us its real layout yet

    def info(self):
        return {"id": self.id, "name": self.name, "max_leds": self.max_leds, "default_leds": self.default_leds,
                "shape": self.shape, "fans": self.fans, "matrix": self.matrix, "resizable": self.resizable,
                "provisional": self.provisional}


class Device:
    id = name = vendor = kind = ""
    type_name = "Device"
    experimental = False      # driver not yet confirmed on real hardware (shown in the Devices tab)
    note = None
    plugin = None             # the plugin module that drives it (native devices only)
    parameters = ()           # per-device settings schema, drawn by the Devices tab

    def __init__(self):
        self.zones = []
        self.error = None
        self.params = {p["id"]: p["default"] for p in self.parameters}

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

    def set_params(self, values):
        """Apply saved per-device settings (already validated against self.parameters); missing ones keep their
        defaults. Cheap when nothing changed, so it can run every tick."""
        params = {p["id"]: values.get(p["id"], p["default"]) for p in self.parameters}
        if params != self.params:
            self.params = params
            self.on_params_changed()

    def on_params_changed(self):
        """Override when a setting needs more than being read at the next write."""

    def info(self):
        return {"id": self.id, "name": self.name, "vendor": self.vendor, "kind": self.kind, "type": self.type_name,
                "online": self.online, "error": self.error, "experimental": self.experimental, "note": self.note,
                "zones": [z.info() for z in self.zones],
                "plugin": self.plugin, "parameters": list(self.parameters), "params": dict(self.params)}


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
            zone = Zone(f"z{zi}", z["name"], z["leds_max"] or count, count, "strip", resizable)
            m = z.get("matrix")
            if z["type"] == ZONE_MATRIX and m and count and any(0 <= c < count for c in m["map"]):
                zone.shape, zone.matrix = "matrix", m
            segs = z.get("segments") or []
            # an ARGB header with fans on it, as OpenRGB 0.9+ describes it: "Fan 1", "Fan 2", ... of equal size
            if (len(segs) > 1 and all("fan" in s["name"].lower() for s in segs)
                    and len({s["count"] for s in segs}) == 1 and sum(s["count"] for s in segs) == count):
                zone.shape, zone.fans = "fans", len(segs)
            self.zones.append(zone)
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

def detect_natives(hid_list, plugins=None, backend=None):
    """Native devices for every controller in an hid.enumerate() list that a plugin claims. Ids are the plugin's
    ID_PREFIX for the first of a family ("nzxt-hub", "asus-aura"), so saved zone settings keep working, then "-2",
    "-3"."""
    from . import plugins as plugin_mod
    plugins = plugin_mod.load() if plugins is None else plugins
    found, counts = [], {}
    for plugin, product, info in plugin_mod.match(plugins, hid_list):
        counts[plugin.ID_PREFIX] = counts.get(plugin.ID_PREFIX, 0) + 1
        n = counts[plugin.ID_PREFIX]
        dev_id = plugin.ID_PREFIX + ("" if n == 1 else f"-{n}")
        try:
            dev = plugin.create(info, product, dev_id, backend)
        except Exception as e:  # one broken plugin must not cost the user every other device
            log.error("%s: could not set up %s: %s", plugin.NAME, product.name, e)
            counts[plugin.ID_PREFIX] -= 1
            continue
        dev.plugin_module = plugin
        found.append(dev)
        log.info("found %s (%04x:%04x) via %s%s", product.name, product.vid, product.pid, plugin.NAME,
                 ", experimental support" if product.experimental else "")
    return found


class DeviceManager:
    RECONNECT = 5.0
    RESET_DEBOUNCE = 5.0     # Windows sends two resume broadcasts, and the engine and controller notice the gap too

    def __init__(self, hid_backend=None, natives=None, bridge=None, plugins=None):
        if natives is None:
            try:
                import hid
                natives = detect_natives((hid_backend or hid).enumerate(), plugins, hid_backend)
            except Exception as e:  # no USB access must never stop OpenRGB devices from working
                log.warning("USB device scan failed: %s", e)
                natives = []
        self.natives = {d.id: d for d in natives}
        # leave out of OpenRGB whatever a plugin already drives, but only for families that are actually here
        present = {id(m): m for m in (getattr(d, "plugin_module", None) for d in natives) if m is not None}
        skip = [name for m in sorted(present.values(), key=lambda m: m.ID_PREFIX)
                for name in getattr(m, "OPENRGB_SKIP", [])]
        self.bridge = bridge or OpenRGBBridge(skip=tuple(skip))
        from .dynlight import DynamicLightingBridge
        self.dynamic = DynamicLightingBridge()
        self.dynamic_enabled = lambda: False      # the controller connects this to the setting
        self._last_reconnect = 0.0
        self._last_reset = None
        self._lock = threading.RLock()
        # Separate from _lock: maintain() can wait seconds on an OpenRGB connect, and the lighting thread takes
        # _lock (all()) every frame.
        self._maintain_lock = threading.RLock()
        self._reset_wanted = threading.Event()

    def all(self):
        with self._lock:
            return {**self.natives, **self.bridge.devices, **self.dynamic.devices}

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
        try:
            self.dynamic.refresh(self.dynamic_enabled())
        except Exception as e:
            log.warning("Dynamic Lighting error: %s", e)

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
