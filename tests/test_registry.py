import unittest

from airglow import devices, hw
from tests.fakes import FakeHidBackend, FakeHidDevice, FakeHub, FakeLightDevice, FakeManager


def hid_entry(vid, pid, interface=0, path=None, serial=""):
    return {"vendor_id": vid, "product_id": pid, "interface_number": interface, "serial_number": serial,
            "path": path or f"{vid:04x}:{pid:04x}:{interface}:{serial}".encode()}


class RegistryTests(unittest.TestCase):
    def test_registry_loads_and_every_entry_has_a_driver(self):
        reg = hw.load_registry()
        self.assertTrue(reg["native"])
        for e in reg["native"]:
            self.assertIn(e["driver"], hw.DRIVER_IDS, e)
        self.assertEqual(hw.NZXT_ACCESSORIES[0x19]["leds"], 8)

    def test_detects_known_controllers_once_each(self):
        found = hw.detect_natives([
            hid_entry(0x1E71, 0x2022),
            hid_entry(0x1E71, 0x2009),                               # an RGB & Fan Controller as well
            hid_entry(0x0B05, 0x19AF, interface=0),                  # wrong interface for this board
            hid_entry(0x0B05, 0x19AF, interface=2),
            hid_entry(0x0B05, 0x1939, interface=0, serial="A"),      # second board with two interfaces
            hid_entry(0x0B05, 0x1939, interface=1, serial="A"),
            hid_entry(0x046D, 0xC52B),                               # a mouse receiver: not ours
        ], backend=FakeHidBackend())
        self.assertEqual([d.id for d in found], ["nzxt-hub", "nzxt-hub-2", "asus-aura", "asus-aura-2"])
        self.assertFalse(found[0].experimental)
        self.assertTrue(found[1].experimental)
        self.assertEqual(found[1].name, "NZXT RGB & Fan Controller")
        self.assertEqual(found[1].hub.FAN_PORTS, 3)
        self.assertEqual(found[2].aura.path, hid_entry(0x0B05, 0x19AF, interface=2)["path"])

    def test_openrgb_skips_what_we_drive_natively(self):
        manager = hw.DeviceManager(natives=hw.detect_natives([hid_entry(0x1E71, 0x2022)], backend=FakeHidBackend()))
        self.assertIn("NZXT Control Hub", manager.bridge.skip)
        self.assertNotIn("ASUS ROG", manager.bridge.skip)


class NzxtAccessoryTests(unittest.TestCase):
    def test_channel_layouts(self):
        self.assertEqual(hw.nzxt_channel(None), (40, "strip", 1))
        self.assertEqual(hw.nzxt_channel([]), (0, "strip", 1))
        self.assertEqual(hw.nzxt_channel([0x13, 0x13, 0x13]), (40, "fans", 3))       # 54 LEDs, capped at 40
        self.assertEqual(hw.nzxt_channel([0x17, 0x17]), (16, "fans", 2))
        self.assertEqual(hw.nzxt_channel([0x1D]), (24, "fans", 3))                    # one 360 mm triple
        self.assertEqual(hw.nzxt_channel([0x04, 0x04]), (20, "strip", 1))
        self.assertEqual(hw.nzxt_channel([0x17, 0x04]), (18, "strip", 1))             # fan + strip: a strip
        self.assertEqual(hw.nzxt_channel([0x7E]), (40, "strip", 1))                   # unknown accessory

    def test_fans_only_model_has_no_light_zones_and_never_asks_for_led_info(self):
        dev = FakeHidDevice()
        hub = devices.NzxtControlHub(FakeHidBackend(device=dev), pid=0x2011, led_channels=0, fan_ports=3)
        nz = hw.NzxtDevice(hub)
        nz.open()
        self.assertEqual(nz.zones, [])
        self.assertNotIn([0x20, 0x03], [w[:2] for w in dev.writes])

    def test_lighting_only_model_sends_no_fan_commands(self):
        dev = FakeHidDevice()
        hub = devices.NzxtControlHub(FakeHidBackend(device=dev), pid=0x2001, led_channels=4, fan_ports=0)
        hub.open()
        hub.set_fan_duty({0: 50})
        self.assertFalse(any(w[0] in (0x60, 0x62) for w in dev.writes))


class MultiHubFanTests(unittest.TestCase):
    def test_fans_run_on_every_fan_controller(self):
        import tempfile
        from airglow import config
        from airglow.controller import Controller
        from tests.fakes import signals

        class Hub(FakeLightDevice):
            def __init__(self, dev_id):
                super().__init__(dev_id, dev_id, zones=())
                self.hub = FakeHub()

        a, b = Hub("nzxt-hub"), Hub("nzxt-hub-2")
        with tempfile.TemporaryDirectory() as tmp:
            store = config.Store(tmp)
            ctrl = Controller(store, manager=FakeManager(a, b), read_sensors=lambda: {"online": True, "cpu_temp": 50.0},
                              signals=signals, async_writes=False)
            ctrl.manager.maintain(True)
            ctrl.tick()
            self.assertTrue(a.hub.duties and b.hub.duties)
            ports = ctrl.snapshot()["fans"]["ports"]
            self.assertEqual({p["device"] for p in ports}, {"nzxt-hub", "nzxt-hub-2"})
            ctrl.failsafe()
            self.assertEqual(set(b.hub.duties[-1].values()), {store.settings()["fan_failsafe_duty"]})


if __name__ == "__main__":
    unittest.main()
