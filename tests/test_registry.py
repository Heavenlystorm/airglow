import types
import tempfile
import unittest
from unittest import mock

from airglow import config, devices, hw, plugins
from airglow.controller import Controller
from airglow.plugins import asus_aura_usb, nzxt_hue2
from tests.fakes import (
    FakeHidBackend,
    FakeHidDevice,
    FakeHub,
    FakeLightDevice,
    FakeManager,
    signals,
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
        self.assertTrue(both.bridge.skipped(self._orgb("NZXT RGB & Fan Controller 2024", "LED strip", vendor="NZXT")))
        self.assertFalse(both.bridge.skipped(self._orgb("NZXT RGB & Fan Controller", "LED strip", vendor="NZXT")))

    def test_same_name_nzxt_models_are_distinguished_by_serial(self):
        for lit_pid in (0x2009, 0x200E, 0x2010, 0x2020):
            for fan_pid in (0x2011, 0x2019, 0x201F):
                with self.subTest(lit_pid=lit_pid, fan_pid=fan_pid):
                    manager = hw.DeviceManager(natives=hw.detect_natives([
                        hid_entry(0x1E71, lit_pid, serial="LIGHT"), hid_entry(0x1E71, fan_pid, serial="FANS")],
                        backend=FakeHidBackend()))
                    ctrl = self._orgb("NZXT RGB & Fan Controller", "LED strip", vendor="NZXT")
                    self.assertTrue(manager.bridge.skipped({**ctrl, "serial": "LIGHT"}))
                    self.assertFalse(manager.bridge.skipped({**ctrl, "serial": "FANS"}))


class HotPlugTests(unittest.TestCase):
    class Backend(FakeHidBackend):
        def __init__(self, *entries):
            super().__init__()
            self.entries = list(entries)

        def enumerate(self, vid=0, pid=0):
            return list(self.entries)

    def test_a_hub_plugged_in_later_is_found_and_hidden_from_openrgb(self):
        usb = self.Backend(hid_entry(0x0B05, 0x19AF, interface=2))
        manager = hw.DeviceManager(hid_backend=usb)
        self.assertEqual(list(manager.natives), ["asus-aura"])
        hub = {"vendor": "NZXT", "name": "NZXT RGB & Fan Controller 2024", "type_name": "LED strip"}
        self.assertFalse(manager.bridge.skipped(hub))
        self.assertEqual(manager.rescan(), [])                              # nothing new: nothing changes
        aura = manager.natives["asus-aura"]
        usb.entries.append(hid_entry(0x1E71, 0x2022))
        with self.assertLogs("airglow.hw", "INFO"):
            self.assertEqual([d.id for d in manager.rescan()], ["nzxt-hub"])
        self.assertIs(manager.natives["asus-aura"], aura)                  # what was there is kept as it is
        self.assertTrue(manager.bridge.skipped(hub))
        self.assertEqual(manager.rescan(), [])

    def test_a_second_identical_hub_gets_the_next_id_and_an_unplugged_one_stays(self):
        usb = self.Backend(hid_entry(0x1E71, 0x2022, serial="A"))
        manager = hw.DeviceManager(hid_backend=usb)
        usb.entries = [hid_entry(0x1E71, 0x2022, serial="B")]                # A unplugged, B plugged in
        self.assertEqual([d.id for d in manager.rescan()], ["nzxt-hub-2"])
        self.assertEqual(sorted(manager.natives), ["nzxt-hub", "nzxt-hub-2"])

    def test_housekeeping_rescans_on_a_slow_timer(self):
        usb = self.Backend()
        manager = hw.DeviceManager(hid_backend=usb)
        usb.entries.append(hid_entry(0x1E71, 0x2022))
        manager.maintain(False)
        self.assertEqual(manager.natives, {})                               # not yet: RESCAN hasn't passed
        manager._last_scan -= hw.DeviceManager.RESCAN
        manager.maintain(False)
        self.assertEqual(list(manager.natives), ["nzxt-hub"])
        fixed = hw.DeviceManager(natives=[], bridge=manager.bridge)          # devices passed in are never rescanned
        fixed._last_scan -= hw.DeviceManager.RESCAN
        fixed.maintain(False)
        self.assertEqual(fixed.natives, {})

    def test_saved_ids_survive_reversed_enumeration_and_absent_controllers(self):
        usb = self.Backend(hid_entry(0x1E71, 0x2022, serial="A"), hid_entry(0x1E71, 0x2022, serial="B"))
        with tempfile.TemporaryDirectory() as tmp, mock.patch("hid.enumerate", side_effect=usb.enumerate):
            def restart():
                ctrl = Controller(config.Store(tmp), signals=signals, read_sensors=lambda: {}, async_writes=False)
                return {d.hid_key[2]: d.id for d in ctrl.manager.natives.values()}
            self.assertEqual(restart(), {"A": "nzxt-hub", "B": "nzxt-hub-2"})
            usb.entries.reverse()
            self.assertEqual(restart(), {"A": "nzxt-hub", "B": "nzxt-hub-2"})
            usb.entries = [hid_entry(0x1E71, 0x2022, serial="B"), hid_entry(0x1E71, 0x2022, serial="C")]
            self.assertEqual(restart(), {"B": "nzxt-hub-2", "C": "nzxt-hub-3"})
            usb.entries.append(hid_entry(0x1E71, 0x2022, serial="A"))
            self.assertEqual(restart(), {"A": "nzxt-hub", "B": "nzxt-hub-2", "C": "nzxt-hub-3"})

    def test_rescan_reopens_the_same_serial_at_its_new_usb_path(self):
        usb = self.Backend(hid_entry(0x1E71, 0x2011, serial="A", path=b"old"))
        manager = hw.DeviceManager(hid_backend=usb)
        dev = manager.natives["nzxt-hub"]
        manager.maintain(False, force=True)
        self.assertTrue(dev.online)
        usb.entries = [hid_entry(0x1E71, 0x2011, serial="A", path=b"new")]
        self.assertEqual(manager.rescan(), [])
        self.assertIs(manager.natives["nzxt-hub"], dev)
        self.assertEqual(dev.hub.path, b"new")
        self.assertFalse(dev.online)
        manager.maintain(False, force=True)
        self.assertTrue(dev.online)
        manager.close()

    def test_serialless_controllers_at_distinct_paths_keep_distinct_ids(self):
        usb = self.Backend(hid_entry(0x0B05, 0x19AF, interface=2, path=b"first"),
                           hid_entry(0x0B05, 0x19AF, interface=2, path=b"second"))
        with tempfile.TemporaryDirectory() as tmp:
            first = hw.DeviceManager(hid_backend=usb, store=config.Store(tmp))
            self.assertEqual({d.aura.path: d.id for d in first.natives.values()},
                             {b"first": "asus-aura", b"second": "asus-aura-2"})
            usb.entries.reverse()
            again = hw.DeviceManager(hid_backend=usb, store=config.Store(tmp))
            self.assertEqual({d.aura.path: d.id for d in again.natives.values()},
                             {b"first": "asus-aura", b"second": "asus-aura-2"})

    def test_failed_initial_fan_setup_closes_the_handle_for_retry(self):
        usb = self.Backend(hid_entry(0x1E71, 0x2011, serial="A"))
        usb.dev.fail_writes = True
        manager = hw.DeviceManager(hid_backend=usb)
        manager.maintain(False, force=True)
        dev = manager.natives["nzxt-hub"]
        self.assertFalse(dev.online)
        self.assertTrue(usb.dev.closed)
        usb.dev.fail_writes = False
        manager.maintain(False, force=True)
        self.assertTrue(dev.online)
        self.assertIsNone(dev.error)
        manager.close()

    def test_identity_write_failure_keeps_fan_controllers_and_retries_on_rescan(self):
        usb = self.Backend(hid_entry(0x1E71, 0x2011, serial="A"), hid_entry(0x1E71, 0x2011, serial="B"))
        with tempfile.TemporaryDirectory() as tmp:
            store = config.Store(tmp)
            with mock.patch.object(store, "set_device_ids", side_effect=OSError("disk full")):
                manager = hw.DeviceManager(hid_backend=usb, store=store)
            self.assertEqual(list(manager.natives), ["nzxt-hub", "nzxt-hub-2"])
            self.assertEqual(store.device_ids(), {})
            usb.entries.reverse()
            self.assertEqual(manager.rescan(), [])
            again = hw.DeviceManager(hid_backend=usb, store=config.Store(tmp))
            self.assertEqual({d.hid_key[2]: d.id for d in again.natives.values()},
                             {"A": "nzxt-hub", "B": "nzxt-hub-2"})

    def test_hotplug_fan_only_model_is_exposed_by_serial_when_names_match(self):
        usb = self.Backend(hid_entry(0x1E71, 0x2009, serial="LIGHT"))
        manager = hw.DeviceManager(hid_backend=usb)
        usb.entries.append(hid_entry(0x1E71, 0x2011, serial="FANS"))
        manager.rescan()
        ctrl = {"vendor": "NZXT", "name": "NZXT RGB & Fan Controller", "type_name": "LED strip"}
        self.assertTrue(manager.bridge.skipped({**ctrl, "serial": "LIGHT"}))
        self.assertFalse(manager.bridge.skipped({**ctrl, "serial": "FANS"}))


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
            previous_duty = dict(b.hub.duties[-1])
            ctrl.failsafe()
            self.assertEqual(b.hub.duties[-1], previous_duty)


if __name__ == "__main__":
    unittest.main()
