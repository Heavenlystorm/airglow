"""Test doubles for the HID backend, devices, the device manager and Windows signals."""

import time

from airglow.devices import DeviceError
from airglow.hw import Device, Zone


class FakeHidDevice:
    def __init__(self, reads=None, fail_writes=False):
        self.writes, self.reads = [], list(reads or [])
        self.fail_writes, self.closed = fail_writes, False

    def open_path(self, path):
        pass

    def set_nonblocking(self, flag):
        pass

    def write(self, data):
        if self.fail_writes:
            return -1
        self.writes.append(list(data))
        return len(data)

    def read(self, size, timeout_ms=0):
        return self.reads.pop(0) if self.reads else []

    def close(self):
        self.closed = True


class FakeHidBackend:
    def __init__(self, present=True, device=None):
        self.present = present
        self.dev = device or FakeHidDevice()

    def enumerate(self, vid=0, pid=0):
        return [{"path": b"fake", "interface_number": 2, "vendor_id": vid, "product_id": pid}] if self.present else []

    def device(self):
        return self.dev


class FakeHub:
    """Stands in for devices.NzxtControlHub (fans part)."""

    FAN_PORTS = 5

    def __init__(self):
        self.fans = [{"rpm": 900, "duty": 30}, {"rpm": 900, "duty": 30}, {"rpm": 0, "duty": 0},
                     {"rpm": 0, "duty": 0}, {"rpm": 1000, "duty": 30}]
        self.duties = []
        self.fail_writes = False

    def poll(self):
        pass

    def connected_fans(self):
        return [i for i, f in enumerate(self.fans) if f["rpm"] > 0]

    def set_fan_duty(self, duties):
        if self.fail_writes:
            raise DeviceError("NZXT Control Hub: write failed")
        self.duties.append(dict(duties))


class FakeLightDevice(Device):
    """A lighting device with configurable zones that records what it was sent."""

    def __init__(self, dev_id="fake", name="Fake", zones=(("z1", 8), ("z2", 4)), kind="native", type_name="LED strip",
                 fail_open=False):
        super().__init__()
        self.id, self.name, self.vendor, self.kind, self.type_name = dev_id, name, "Test", kind, type_name
        self.zones = [Zone(zid, zid.upper(), max(n, 1) * 2, n) for zid, n in zones]
        self.fail_open = fail_open
        self.is_open = False
        self.opens = 0
        self.writes = []            # (zone_id, colors)
        self.flushes = 0
        self.fail_writes = False

    @property
    def online(self):
        return self.is_open

    def open(self):
        if self.fail_open:
            raise DeviceError(f"{self.name} isn't connected")
        self.is_open = True
        self.opens += 1

    def close(self):
        self.is_open = False

    def write_zone(self, zone_id, colors):
        if self.fail_writes:
            raise DeviceError(f"{self.name}: write failed")
        self.writes.append((zone_id, list(colors)))

    def flush(self):
        self.flushes += 1

    def last(self, zone_id):
        return next((c for z, c in reversed(self.writes) if z == zone_id), None)


class FakeNzxt(FakeLightDevice):
    def __init__(self):
        super().__init__("nzxt-hub", "NZXT Control Hub", zones=(("ch1", 24), ("ch2", 8)))
        self.hub = FakeHub()


class _FakeClient:
    connected = False

    def close(self):
        pass


class _FakeBridge:
    def __init__(self):
        self.devices = {}
        self.status = "OpenRGB bridge is off"
        self.client = _FakeClient()
        self._last_try = 0.0

    def refresh(self, enabled):
        pass


class FakeManager:
    def __init__(self, *devices):
        self.natives = {d.id: d for d in devices}
        self.bridge = _FakeBridge()
        self.resets = 0

    def all(self):
        return {**self.natives, **self.bridge.devices}

    def get(self, device_id):
        return self.all().get(device_id)

    def native(self, device_id):
        return self.natives.get(device_id)

    def maintain(self, openrgb_enabled, force=False):
        for d in self.natives.values():
            if not d.online:
                try:
                    d.open()
                    d.error = None
                except DeviceError as e:
                    d.error = str(e)

    def start(self, openrgb_enabled):
        pass                                    # tests drive maintain() themselves

    def request_reset(self):
        self.reset(True)

    def reset(self, openrgb_enabled):
        self.resets += 1
        for d in self.natives.values():
            d.close()
        self.maintain(openrgb_enabled, force=True)

    def close(self):
        for d in self.natives.values():
            d.close()


def signals(**over):
    base = {"idle": 0.0, "foreground": "explorer.exe", "running": frozenset({"explorer.exe"}), "locked": False,
            "now": time.localtime()}
    base.update(over)
    return base
