"""Unified device layer: native drivers (NZXT Control Hub, ASUS Aura) and OpenRGB devices, all exposed as
devices with zones. The lighting engine only ever sees zones and LED colour lists.
"""

import logging
import re
import threading
import time

from . import devices as native
from .openrgb import OpenRGBClient, OpenRGBError, ZONE_MATRIX

log = logging.getLogger(__name__)

# NZXT accessory ids (from the hub's LED info report) -> LEDs. Only ids confirmed on real hardware are listed;
# unknown accessories fall back to the channel maximum and the user sets the count (with the identify sweep).
NZXT_ACCESSORY_LEDS = {0x1F: 24, 0x19: 8}


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
                "online": self.online, "error": self.error, "zones": [z.info() for z in self.zones]}


# ---------------------------------------------------------------- native: NZXT Control Hub

class NzxtDevice(Device):
    id, name, vendor, kind, type_name = "nzxt-hub", "NZXT Control Hub", "NZXT", "native", "Fan and lighting hub"

    REPORT_GRACE = 5.0        # seconds to wait for the accessory report before trusting the 40-LED fallback

    def __init__(self, hub=None):
        self._zones, self._zones_for = [], object()
        self._opened_at = None
        super().__init__()
        self.hub = hub or native.NzxtControlHub()

    @property
    def zones(self):
        """Rebuilt whenever the hub reports (or changes) its accessories, even if that report arrives late."""
        acc = self.hub.accessories
        waiting = acc is None and (self._opened_at is None or time.monotonic() - self._opened_at < self.REPORT_GRACE)
        key = (acc if acc is not None else None, waiting)
        if key != self._zones_for:
            self._set_zones(acc if acc else [None] * 5)
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
            if acc is None:
                leds, shape, fans = 40, "strip", 1          # not reported yet
            elif not acc:
                leds, shape, fans = 0, "strip", 1           # nothing plugged in
            elif all(a in NZXT_ACCESSORY_LEDS for a in acc):
                leds, shape, fans = sum(NZXT_ACCESSORY_LEDS[a] for a in acc), "fans", len(acc)
            else:
                leds, shape, fans = 40, "strip", 1          # unknown accessory: user sets the count
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

    def __init__(self, aura=None):
        super().__init__()
        self.aura = aura or native.AsusAura()
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
        if self.aura.model:
            self.name = "ASUS Aura"
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

class DeviceManager:
    RECONNECT = 5.0
    RESET_DEBOUNCE = 5.0     # Windows sends two resume broadcasts, and the engine and controller notice the gap too

    def __init__(self, hid_backend=None, natives=None, bridge=None):
        if natives is None:
            natives = []
            try:
                import hid
                present = {(d["vendor_id"], d["product_id"]) for d in (hid_backend or hid).enumerate()}
            except Exception:
                present = set()
            if (0x1E71, 0x2022) in present:
                natives.append(NzxtDevice())
            if (0x0B05, 0x19AF) in present:
                natives.append(AuraDevice())
        self.natives = {d.id: d for d in natives}
        skip = []
        if "nzxt-hub" in self.natives:
            skip += ["NZXT RGB & Fan Controller", "NZXT Hue 2", "NZXT Control Hub"]
        if "asus-aura" in self.natives:
            skip += ["ASUS TUF", "ASUS ROG", "ASUS PRIME", "ASUS Aura", "AULA"]
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
