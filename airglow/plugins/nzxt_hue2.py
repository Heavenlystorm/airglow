"""NZXT HUE 2 generation: Control Hub, Smart Device V2, HUE 2, HUE 2 Ambient, RGB & Fan Controller and the HUE 2
channels built into NZXT N7 motherboards.

They share one protocol (airglow/devices.py, NzxtControlHub) and differ only in how many lighting channels and fan
ports they have. Product ids, channel counts and accessory sizes follow liquidctl and OpenRGB.
"""

import time

from .. import devices as native
from ..hw import Device, Zone
from ..lighting.effects import p_range
from . import Product

NAME = "NZXT HUE 2 family"
VENDOR = "NZXT"
TRANSPORT = "hid"
ID_PREFIX = "nzxt-hub"
OPENRGB_SKIP = ["NZXT RGB & Fan Controller", "NZXT Hue 2", "NZXT Control Hub", "NZXT Smart Device", "NZXT B650E"]

PARAMETERS = [
    p_range("brightness_limit", "Brightness limit", 100, lo=10, hi=100, step=5, unit="%"),
]

_FANS_ONLY = ("Airglow drives its fans only: this revision changed its lighting protocol. OpenRGB can light it, "
              "and Airglow leaves it to OpenRGB.")


def _nzxt(pid, name, leds, fans, detector, experimental=True, note=None):
    return Product(0x1E71, pid, name, experimental=experimental, note=note, openrgb_detector=detector,
                   facts={"led_channels": leds, "fan_ports": fans})


# The fifth value is OpenRGB's detector for that id (NZXTHue2ControllerDetect.cpp): one name covers several ids, and
# the fans-only revisions share theirs with ones Airglow lights.
PRODUCTS = [
    _nzxt(0x2022, "NZXT Control Hub", 5, 5, "NZXT RGB & Fan Controller 2024", experimental=False),
    _nzxt(0x2006, "NZXT Smart Device V2", 2, 3, "NZXT Smart Device V2"),
    _nzxt(0x200D, "NZXT Smart Device V2", 2, 3, "NZXT Smart Device V2"),
    _nzxt(0x200F, "NZXT Smart Device V2", 2, 3, "NZXT Smart Device V2"),
    _nzxt(0x2001, "NZXT HUE 2", 4, 0, "NZXT Hue 2"),
    _nzxt(0x2002, "NZXT HUE 2 Ambient", 2, 0, "NZXT Hue 2 Ambient"),
    _nzxt(0x2009, "NZXT RGB & Fan Controller", 2, 3, "NZXT RGB & Fan Controller"),
    _nzxt(0x200E, "NZXT RGB & Fan Controller", 2, 3, "NZXT RGB & Fan Controller"),
    _nzxt(0x2010, "NZXT RGB & Fan Controller", 2, 3, "NZXT RGB & Fan Controller"),
    _nzxt(0x2011, "NZXT RGB & Fan Controller", 0, 3, "NZXT RGB & Fan Controller", note=_FANS_ONLY),
    _nzxt(0x2019, "NZXT RGB & Fan Controller", 0, 3, "NZXT RGB & Fan Controller", note=_FANS_ONLY),
    _nzxt(0x201F, "NZXT RGB & Fan Controller", 0, 3, "NZXT RGB & Fan Controller", note=_FANS_ONLY),
    _nzxt(0x2020, "NZXT RGB & Fan Controller", 6, 3, "NZXT RGB & Fan Controller"),
    # NZXT N7 motherboards' built-in HUE 2 channels (OpenRGB drives them with the same protocol). Lighting only:
    # their fan headers belong to the board's Super I/O chip, which LibreHardwareMonitor drives (Fans tab).
    _nzxt(0x2005, "NZXT motherboard lighting", 2, 0, "NZXT Hue 2 Motherboard"),
    _nzxt(0x200B, "NZXT motherboard lighting", 2, 0, "NZXT Hue 2 Motherboard"),
    _nzxt(0x201B, "NZXT N7 B650E lighting", 6, 0, "NZXT B650E Motherboard"),
]

# Accessory id (from the hub's LED info report) -> what's plugged in. Unknown accessories fall back to the channel
# maximum and the user sets the count (with the identify sweep).
ACCESSORIES = {
    0x01: {"name": "HUE+ LED strip", "leds": 10, "fans": 0},
    0x02: {"name": "Aer RGB fan", "leds": 8, "fans": 1},
    0x04: {"name": "HUE 2 LED strip (10 LEDs)", "leds": 10, "fans": 0},
    0x05: {"name": "HUE 2 LED strip (8 LEDs)", "leds": 8, "fans": 0},
    0x06: {"name": "HUE 2 LED strip (6 LEDs)", "leds": 6, "fans": 0},
    0x08: {"name": "HUE 2 Cable Comb", "leds": 14, "fans": 0},
    0x09: {"name": "HUE 2 Underglow 300 mm", "leds": 15, "fans": 0},
    0x0A: {"name": "HUE 2 Underglow 200 mm", "leds": 10, "fans": 0},
    0x0B: {"name": "Aer RGB 2 fan 120 mm", "leds": 8, "fans": 1},
    0x0C: {"name": "Aer RGB 2 fan 140 mm", "leds": 8, "fans": 1},
    0x10: {"name": "Kraken X3 ring", "leds": 8, "fans": 1},
    0x11: {"name": "Kraken X3 logo", "leds": 1, "fans": 0},
    0x13: {"name": "F120 RGB", "leds": 18, "fans": 1},
    0x14: {"name": "F140 RGB", "leds": 18, "fans": 1},
    0x15: {"name": "F120 RGB Duo", "leds": 20, "fans": 1},
    0x16: {"name": "F140 RGB Duo", "leds": 20, "fans": 1},
    0x17: {"name": "F120 RGB Core", "leds": 8, "fans": 1},
    0x18: {"name": "F140 RGB Core", "leds": 8, "fans": 1},
    0x19: {"name": "F120 RGB Core (case version)", "leds": 8, "fans": 1},
    0x1C: {"name": "F280 RGB Core (case version)", "leds": 16, "fans": 2},
    0x1D: {"name": "F360 RGB Core (case version)", "leds": 24, "fans": 3},
    0x1E: {"name": "Kraken Elite ring", "leds": 24, "fans": 1},
    0x1F: {"name": "F420 RGB", "leds": 24, "fans": 3},
}


def channel_layout(acc):
    """One channel's accessory ids -> (LEDs, shape, fans). acc is None before the hub has reported."""
    if acc is None:
        return 40, "strip", 1                          # not reported yet
    if not acc:
        return 0, "strip", 1                           # nothing plugged in
    if not all(a in ACCESSORIES for a in acc):
        return 40, "strip", 1                          # unknown accessory: the user sets the count
    parts = [ACCESSORIES[a] for a in acc]
    leds = min(sum(p["leds"] for p in parts), native.NzxtControlHub.LEDS_PER_CHANNEL)
    if all(p["fans"] for p in parts):
        return leds, "fans", sum(p["fans"] for p in parts)
    return leds, "strip", 1                            # strips, or fans mixed with strips


class NzxtDevice(Device):
    id, name, vendor, kind, type_name = "nzxt-hub", "NZXT Control Hub", "NZXT", "native", "Fan and lighting hub"
    plugin = __name__.rpartition(".")[2]
    parameters = PARAMETERS

    REPORT_GRACE = 5.0        # seconds to wait for the accessory report before trusting the 40-LED fallback

    def __init__(self, hub=None, dev_id="nzxt-hub", product=None):
        self._zones, self._zones_for = [], object()
        self._opened_at = None
        super().__init__()
        self.hub = hub or native.NzxtControlHub()
        self.id = dev_id
        if product:
            self.name, self.experimental, self.note = product.name, product.experimental, product.note
            leds, fans = product.facts["led_channels"], product.facts["fan_ports"]
            self.type_name = ("Fan and lighting hub" if leds and fans else "Fan controller" if fans
                              else "Lighting controller")
            self.lighting = bool(leds)              # fans-only revisions: OpenRGB can still light them

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
            leds, shape, fans = channel_layout(acc)
            z = Zone(f"ch{ch + 1}", f"Channel {ch + 1}", 40, leds, shape)
            z.fans = fans
            z.connector = "nzxt"
            z.detected = [{"name": ACCESSORIES[a]["name"], "nzxt_id": a} for a in acc or [] if a in ACCESSORIES]
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
        limit = self.params.get("brightness_limit", 100)
        if limit < 100:
            f = limit / 100
            colors = [(int(r * f), int(g * f), int(b * f)) for r, g, b in colors]
        self.hub.set_channel_colors(int(zone_id[2:]) - 1, colors)


def create(info, product, dev_id, backend):
    hub = native.NzxtControlHub(backend, pid=product.pid, name=product.name,
                                led_channels=product.facts["led_channels"], fan_ports=product.facts["fan_ports"],
                                path=info.get("path"))
    return NzxtDevice(hub, dev_id, product)
