"""ASUS Aura USB motherboard controllers: the board's own LEDs plus its addressable (ARGB) headers.

Protocol in airglow/devices.py (AsusAura). Product ids follow OpenRGB's AsusAuraUSBController.
"""

from .. import devices as native
from ..hw import Device, Zone
from ..lighting.effects import p_toggle
from . import Product

NAME = "ASUS Aura USB"
VENDOR = "ASUS"
TRANSPORT = "hid"
ID_PREFIX = "asus-aura"
# OpenRGB names this controller "ASUS <board name>". Only its motherboard entries are ours: ASUS graphics cards,
# memory, keyboards, mice and monitors are named "ASUS ROG ..." and "ASUS TUF ..." too, and stay with OpenRGB.
OPENRGB_SKIP = ["ASUS"]
OPENRGB_SKIP_TYPES = ["Motherboard"]

PARAMETERS = [
    p_toggle("board_leds", "Light the board's own LEDs", True),
]

_DETECTOR = "ASUS Aura Motherboard"
PRODUCTS = [
    Product(0x0B05, 0x19AF, "ASUS Aura", interface=2, openrgb_detector=_DETECTOR),
    Product(0x0B05, 0x18F3, "ASUS Aura", experimental=True, openrgb_detector=_DETECTOR),
    Product(0x0B05, 0x1939, "ASUS Aura", experimental=True, openrgb_detector=_DETECTOR),
    Product(0x0B05, 0x1AA6, "ASUS Aura", experimental=True, openrgb_detector=_DETECTOR),
    Product(0x0B05, 0x1BED, "ASUS Aura", experimental=True, openrgb_detector=_DETECTOR),
]


class AuraDevice(Device):
    id, name, vendor, kind, type_name = "asus-aura", "ASUS Aura", "ASUS", "native", "Motherboard lighting"
    plugin = __name__.rpartition(".")[2]
    parameters = PARAMETERS

    def __init__(self, aura=None, dev_id="asus-aura", product=None):
        super().__init__()
        self.aura = aura or native.AsusAura()
        self.id = dev_id
        if product:
            self.name, self.experimental, self.note = product.name, product.experimental, product.note
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
            if not self.params.get("board_leds", True):
                colors = [(0, 0, 0)] * len(colors)
            self.aura.set_board_colors(colors)
        else:
            self.aura.set_argb_colors(int(zone_id[4:]) - 1, colors)


def create(info, product, dev_id, backend):
    aura = native.AsusAura(backend=backend, pid=product.pid, interface=product.interface, path=info.get("path"))
    return AuraDevice(aura, dev_id, product)
