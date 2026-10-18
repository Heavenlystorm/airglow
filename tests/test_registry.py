import types
import unittest

from airglow import devices, hw, plugins
from airglow.plugins import asus_aura_usb, nzxt_hue2
from tests.fakes import (
    FakeHidBackend,
    FakeHidDevice,
    FakeHub,
    FakeLightDevice,
    FakeManager,
)


def hid_entry(vid, pid, interface=0, path=None, serial=""):
    return {"vendor_id": vid, "product_id": pid, "interface_number": interface, "serial_number": serial,
            "path": path or f"{vid:04x}:{pid:04x}:{interface}:{serial}".encode()}


class PluginTests(unittest.TestCase):
    def test_builtin_plugins_load_and_meet_the_contract(self):
        loaded = plugins.load()
        self.assertEqual([m.ID_PREFIX for m in loaded], ["asus-aura", "nzxt-hub"])
        for m in loaded:
            plugins.check(m)                                    # raises on a broken contract
        self.assertEqual(nzxt_hue2.ACCESSORIES[0x19]["leds"], 8)

    def test_catalog_lists_every_product(self):
        cat = {c["plugin"]: c for c in plugins.catalog()}
        self.assertIn({"vid": "1E71", "pid": "2022", "name": "NZXT Control Hub", "experimental": False},
                      cat["nzxt_hue2"]["products"])

    def test_contract_violations_are_reported(self):
        bad = types.ModuleType("airglow.plugins.bad")
        bad.NAME, bad.VENDOR, bad.TRANSPORT, bad.ID_PREFIX = "Bad", "X", "hid", "bad"
        bad.create = lambda *a: None
        bad.PRODUCTS = []
        with self.assertRaisesRegex(plugins.PluginError, "PRODUCTS"):
            plugins.check(bad)
        bad.PRODUCTS = [plugins.Product(1, 2, "A"), plugins.Product(1, 2, "B")]
        with self.assertRaisesRegex(plugins.PluginError, "twice"):
            plugins.check(bad)
        bad.PRODUCTS, bad.TRANSPORT = [plugins.Product(1, 2, "A")], "serial"
        with self.assertRaisesRegex(plugins.PluginError, "transport"):
            plugins.check(bad)

    def test_a_plugin_that_fails_to_create_does_not_cost_the_others(self):
        broken = types.ModuleType("broken")
        broken.NAME, broken.ID_PREFIX = "Broken", "broken"
        broken.PRODUCTS = [plugins.Product(0x1234, 0x0001, "Broken thing")]
        def create(*a):
            raise RuntimeError("boom")
        broken.create = create
        found = hw.detect_natives([hid_entry(0x1234, 0x0001), hid_entry(0x1E71, 0x2022)],
                                  plugins=[broken, nzxt_hue2], backend=FakeHidBackend())
        self.assertEqual([d.id for d in found], ["nzxt-hub"])

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
        self.assertEqual(sorted(d.id for d in found), ["asus-aura", "asus-aura-2", "nzxt-hub", "nzxt-hub-2"])
        by_id = {d.id: d for d in found}
        self.assertFalse(by_id["nzxt-hub"].experimental)
        self.assertTrue(by_id["nzxt-hub-2"].experimental)
        self.assertEqual(by_id["nzxt-hub-2"].name, "NZXT RGB & Fan Controller")
        self.assertEqual(by_id["nzxt-hub-2"].hub.FAN_PORTS, 3)
        self.assertEqual(by_id["asus-aura"].aura.path, hid_entry(0x0B05, 0x19AF, interface=2)["path"])
        self.assertEqual(by_id["nzxt-hub"].plugin, "nzxt_hue2")

    def test_openrgb_skips_what_we_drive_natively(self):
        manager = hw.DeviceManager(natives=hw.detect_natives([hid_entry(0x1E71, 0x2022)], backend=FakeHidBackend()))
        self.assertIn(("NZXT Control Hub", None), manager.bridge.skip)
        self.assertFalse(any(name == "ASUS" for name, _ in manager.bridge.skip))

    @staticmethod
    def _orgb(name, type_name, vendor="ASUS"):
        return {"vendor": vendor, "name": name, "type_name": type_name}

    def test_an_aura_board_only_hides_asus_motherboard_entries(self):
        # OpenRGB names the board "ASUS <board>"; the same brand's cards, memory and keyboards are "ASUS ROG ..." too
        manager = hw.DeviceManager(natives=hw.detect_natives([hid_entry(0x0B05, 0x19AF, interface=2)],
                                                             backend=FakeHidBackend()))
        skipped = manager.bridge.skipped
        self.assertTrue(skipped(self._orgb("ASUS ROG STRIX B650E-F GAMING WIFI", "Motherboard")))
        self.assertTrue(skipped(self._orgb("ASUS TUF GAMING X670E-PLUS", "Motherboard")))
        self.assertFalse(skipped(self._orgb("ASUS ROG STRIX RTX 4080 OC", "Graphics card")))
        self.assertFalse(skipped(self._orgb("ASUS ROG Strix Scope II", "Keyboard")))
        self.assertFalse(skipped(self._orgb("ASUS TUF Gaming K3", "Keyboard")))
        self.assertFalse(skipped(self._orgb("ASUS Aura DRAM", "Memory")))
        self.assertFalse(skipped(self._orgb("NZXT Hue 2", "LED strip", vendor="NZXT")))

    def test_a_fans_only_nzxt_controller_leaves_its_lighting_to_openrgb(self):
        manager = hw.DeviceManager(natives=hw.detect_natives([hid_entry(0x1E71, 0x2011)], backend=FakeHidBackend()))
        self.assertEqual(list(manager.natives), ["nzxt-hub"])                  # Airglow still drives its fans
        self.assertFalse(manager.natives["nzxt-hub"].lighting)
        self.assertFalse(manager.bridge.skipped(self._orgb("NZXT RGB & Fan Controller", "LED strip", vendor="NZXT")))
        both = hw.DeviceManager(natives=hw.detect_natives([hid_entry(0x1E71, 0x2011), hid_entry(0x1E71, 0x2022)],
                                                          backend=FakeHidBackend()))
        self.assertTrue(both.bridge.skipped(self._orgb("NZXT Control Hub", "LED strip", vendor="NZXT")))


class DeviceParameterTests(unittest.TestCase):
    def test_brightness_limit_scales_what_the_hub_is_sent(self):
        class Hub:
            accessories, led_channels, colors = None, 5, []
            def set_channel_colors(self, ch, colors):
                self.colors.append((ch, colors))
        nz = nzxt_hue2.NzxtDevice(Hub())
        nz.write_zone("ch1", [(200, 100, 50)])
        nz.set_params({"brightness_limit": 50})
        nz.write_zone("ch1", [(200, 100, 50)])
        self.assertEqual(nz.hub.colors, [(0, [(200, 100, 50)]), (0, [(100, 50, 25)])])

    def test_board_leds_off_keeps_argb_headers_lit(self):
        aura = devices.AsusAura(backend=FakeHidBackend(device=FakeHidDevice()))
        dev = asus_aura_usb.AuraDevice(aura)
        sent = []
        aura.set_board_colors = lambda c: sent.append(("board", c))
        aura.set_argb_colors = lambda ch, c: sent.append((ch, c))
        dev.set_params({"board_leds": False})
        dev.write_zone("board", [(255, 0, 0)] * 2)
        dev.write_zone("argb1", [(255, 0, 0)])
        self.assertEqual(sent, [("board", [(0, 0, 0)] * 2), (0, [(255, 0, 0)])])

    def test_saved_values_are_validated_against_the_schema(self):
        import tempfile

        from airglow import config
        with tempfile.TemporaryDirectory() as tmp:
            store = config.Store(tmp)
            clean = store.set_device_params("nzxt-hub", nzxt_hue2.PARAMETERS, {"brightness_limit": 500, "x": 1})
            self.assertEqual(clean, {"brightness_limit": 100})       # clamped, unknown key dropped
            self.assertEqual(config.Store(tmp).device_params()["nzxt-hub"], clean)
            with self.assertRaises(ValueError):
                store.set_device_params("nzxt-hub", nzxt_hue2.PARAMETERS, {"brightness_limit": "bright"})


class NzxtAccessoryTests(unittest.TestCase):
    def test_channel_layouts(self):
        self.assertEqual(nzxt_hue2.channel_layout(None), (40, "strip", 1))
        self.assertEqual(nzxt_hue2.channel_layout([]), (0, "strip", 1))
        self.assertEqual(nzxt_hue2.channel_layout([0x13, 0x13, 0x13]), (40, "fans", 3))       # 54 LEDs, capped at 40
        self.assertEqual(nzxt_hue2.channel_layout([0x17, 0x17]), (16, "fans", 2))
        self.assertEqual(nzxt_hue2.channel_layout([0x1D]), (24, "fans", 3))                    # one 360 mm triple
        self.assertEqual(nzxt_hue2.channel_layout([0x04, 0x04]), (20, "strip", 1))
        self.assertEqual(nzxt_hue2.channel_layout([0x17, 0x04]), (18, "strip", 1))             # fan + strip: a strip
        self.assertEqual(nzxt_hue2.channel_layout([0x7E]), (40, "strip", 1))                   # unknown accessory

    def test_fans_only_model_has_no_light_zones_and_never_asks_for_led_info(self):
        dev = FakeHidDevice()
        hub = devices.NzxtControlHub(FakeHidBackend(device=dev), pid=0x2011, led_channels=0, fan_ports=3)
        nz = nzxt_hue2.NzxtDevice(hub)
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
