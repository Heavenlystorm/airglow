"""Windows Dynamic Lighting (HID LampArray) devices, through the Windows.Devices.Lights API.

Many newer keyboards, mice and accessories implement the open HID LampArray standard, and Windows 11 controls them
without vendor software. Airglow can drive them like any other device, with one Windows rule: an unpackaged app (like
this one) may only set colours while it's the app in front. Background control needs a packaged (MSIX) app that
declares the lighting extension, and the user ranking it in Settings, Personalization, Dynamic Lighting. So this is
off by default, and devices say when Windows is holding control back.

Needs the winrt-* packages (pywinrt); without them the bridge just reports that it isn't available.
"""

import asyncio
import hashlib
import logging
import time

from .hw import Device, Zone

log = logging.getLogger(__name__)

# Windows.Devices.Lights.LampArrayKind -> the device types the rest of Airglow uses (peripherals start switched off)
KINDS = {1: "Keyboard", 2: "Mouse", 3: "Gamepad", 4: "Accessory", 5: "Light", 6: "Light", 7: "Case", 8: "Accessory",
         9: "Light", 10: "Light", 11: "Headset", 12: "Microphone", 13: "Speaker"}
GRID_COLS, GRID_ROWS = 48, 16


def device_key(lamp_array):
    """A stable, non-identifying id: vendor and product, plus a hash of the Windows device path."""
    digest = hashlib.sha256(lamp_array.device_id.encode()).hexdigest()[:10]
    return f"dynlight:{lamp_array.hardware_vendor_id:04x}-{lamp_array.hardware_product_id:04x}:{digest}"


def grid_from_positions(points, cols=GRID_COLS, rows=GRID_ROWS):
    """Lamp positions (x, y in metres, from Windows) -> an Airglow grid {"rows", "cols", "map"} so effects sweep
    across the device the way it's built. Two lamps in one cell: the second is left off the map."""
    if len(points) < 2:
        return None
    xs, ys = [p[0] for p in points], [p[1] for p in points]
    w, h = max(xs) - min(xs), max(ys) - min(ys)
    if w <= 0 and h <= 0:
        return None
    # as many columns/rows as there are distinct positions (to the millimetre), up to the grid size
    c = max(1, min(cols, len({round(x, 3) for x in xs})))
    r = max(1, min(rows, len({round(y, 3) for y in ys})))
    cells = [-1] * (r * c)
    for i, (x, y) in enumerate(points):
        ci = round((x - min(xs)) / w * (c - 1)) if w > 0 else 0
        ri = round((y - min(ys)) / h * (r - 1)) if h > 0 else 0
        if cells[ri * c + ci] == -1:
            cells[ri * c + ci] = i
    return {"rows": r, "cols": c, "map": cells}


class LampArrayDevice(Device):
    kind = "dynamic"

    def __init__(self, lamp_array, name):
        super().__init__()
        self.lamps = lamp_array
        self.name = name or "Dynamic Lighting device"
        self.vendor = f"{lamp_array.hardware_vendor_id:04X}"
        self.type_name = KINDS.get(int(lamp_array.lamp_array_kind), "Light")
        self.id = device_key(lamp_array)
        n = lamp_array.lamp_count
        zone = Zone("lamps", "Lights", n, n, "strip", resizable=False)
        try:
            points = [(lamp_array.get_lamp_info(i).position.x, lamp_array.get_lamp_info(i).position.y)
                      for i in range(n)]
            grid = grid_from_positions(points)
        except Exception as e:  # a layout is a nicety
            log.debug("%s: no lamp positions (%s)", self.name, e)
            grid = None
        if grid and n > 4:
            zone.shape, zone.matrix = "matrix", grid
        self.zones = [zone]
        self._indices = list(range(n))
        self.experimental = True
        self.note = ("Windows only lets Airglow light this while Airglow's window is in front, until a packaged "
                     "version of Airglow exists.")

    @property
    def online(self):
        try:
            return bool(self.lamps.is_connected)
        except Exception:
            return False

    @property
    def available(self):
        """False while Windows gives control to another app (Airglow not in front, or Dynamic Lighting off)."""
        try:
            return bool(self.lamps.is_available) and bool(self.lamps.is_enabled)
        except Exception:
            return False

    def write_zone(self, zone_id, colors):
        from winrt.windows.ui import Color
        if not self.available:
            self.error = "Windows is giving this device to another app right now"
            return
        self.error = None
        n = min(len(colors), len(self._indices))
        try:
            self.lamps.set_colors_for_indices([Color(255, r, g, b) for r, g, b in colors[:n]], self._indices[:n])
        except Exception as e:
            # A WinRT/COM failure here is not a DeviceError, so it would otherwise reach the writer's catch-all and
            # log a traceback thirty times a second. Report it on the device and stop drawing it.
            self.error = f"Windows wouldn't take the colours for this device ({e})"
            log.warning("Dynamic Lighting %s: %s", self.name, self.error)


class DynamicLightingBridge:
    """Finds LampArray devices (on the device housekeeping thread, every few seconds while switched on)."""

    RESCAN = 10.0

    def __init__(self):
        self.devices = {}
        self.status = "Off"
        self._last = 0.0

    def refresh(self, enabled):
        if not enabled:
            self.devices, self.status = {}, "Off"
            return
        if time.monotonic() - self._last < self.RESCAN:
            return
        self._last = time.monotonic()
        try:
            found = asyncio.run(self._scan())
        except ImportError:
            self.devices, self.status = {}, "Not available in this build (the winrt packages are missing)"
            return
        except Exception as e:
            log.warning("Dynamic Lighting scan failed: %s", e)
            self.status = f"Couldn't list Dynamic Lighting devices ({e})"
            return
        for dev_id, (arr, name) in found.items():
            old = self.devices.get(dev_id)
            if old is None or not old.online:
                # a device that was re-plugged (or went to sleep) is a new WinRT object: the old one stays
                # disconnected forever, so it has to be replaced, not kept
                self.devices[dev_id] = LampArrayDevice(arr, name)
                log.info("Dynamic Lighting device: %s (%d lamps)", name, arr.lamp_count)
        for gone in set(self.devices) - set(found):
            del self.devices[gone]
        self.status = f"{len(self.devices)} device{'s' if len(self.devices) != 1 else ''} found"

    @staticmethod
    async def _scan():
        from winrt.windows.devices.enumeration import DeviceInformation
        from winrt.windows.devices.lights import LampArray
        infos = await DeviceInformation.find_all_async_aqs_filter(LampArray.get_device_selector())
        out = {}
        for info in infos:
            arr = await LampArray.from_id_async(info.id)
            if arr is not None and arr.lamp_count:
                out[device_key(arr)] = (arr, info.name)
        return out
