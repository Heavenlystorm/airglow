import unittest

from airglow import devices, hw
from airglow.plugins.nzxt_hue2 import NzxtDevice
from tests.fakes import FakeHidBackend, FakeHidDevice
from tests.test_devices import NZXT_FAN_REPORT

# An LED info report captured from a real hub: channel 1 and 2 have accessory 0x1F, channel 5 has 0x19.
NZXT_LED_INFO = bytes.fromhex(
    "21 03 c5 86 d0 11 77 e7 f8 50 9e 4f ff f5 05 1f 00 00 00 00 00 1f 00 00 00 00 00 00 00 00 00 00 00 "
    "00 00 00 00 00 00 19 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00")


class NzxtZoneTests(unittest.TestCase):
    def test_parse_accessories_matches_openrgb(self):
        acc = devices.nzxt_parse_accessories(NZXT_LED_INFO, 5)
        self.assertEqual(acc, [[0x1F], [0x1F], [], [], [0x19]])

    def test_zones_from_real_report(self):
        dev = FakeHidDevice(reads=[list(NZXT_LED_INFO), list(NZXT_FAN_REPORT)])
        nz = NzxtDevice(devices.NzxtControlHub(backend=FakeHidBackend(device=dev)))
        nz.open()
        leds = [z.default_leds for z in nz.zones]
        self.assertEqual(leds, [24, 24, 0, 0, 8])                  # exactly what OpenRGB reported
        self.assertEqual([z.shape for z in nz.zones][:2], ["fans", "fans"])

    def test_late_accessory_report_still_updates_zones(self):
        dev = FakeHidDevice(reads=[])                               # nothing arrives during open()
        nz = NzxtDevice(devices.NzxtControlHub(backend=FakeHidBackend(device=dev)))
        nz.open()
        self.assertEqual([z.default_leds for z in nz.zones], [40] * 5)
        self.assertTrue(all(z.provisional for z in nz.zones))      # not trusted yet, so never saved
        dev.reads.append(list(NZXT_LED_INFO))                       # the report shows up later
        nz.hub.poll()
        self.assertEqual([z.default_leds for z in nz.zones], [24, 24, 0, 0, 8])
        self.assertFalse(any(z.provisional for z in nz.zones))

    def test_fallback_becomes_final_after_grace_period(self):
        nz = NzxtDevice(devices.NzxtControlHub(backend=FakeHidBackend(device=FakeHidDevice())))
        nz.open()
        nz._opened_at -= nz.REPORT_GRACE + 1
        self.assertFalse(any(z.provisional for z in nz.zones))

    def test_stream_packets_count_matches_leds(self):
        packets = devices.nzxt_stream_packets([(255, 0, 0)] * 24, 1)
        self.assertEqual(len(packets), 3)                          # 20 + 4 LEDs, then commit
        self.assertEqual(packets[1][:2], [0x22, 0x11])
        self.assertEqual(len(packets[1]), 4 + 4 * 3)
        self.assertEqual(packets[0][4:7], [0, 255, 0])             # GRB


class ManagerTests(unittest.TestCase):
    def test_only_present_native_devices_are_created(self):
        class Backend:
            def enumerate(self):
                return [{"vendor_id": 0x1E71, "product_id": 0x2022}]
        m = hw.DeviceManager(hid_backend=Backend())
        self.assertEqual(list(m.natives), ["nzxt-hub"])
        self.assertIn(("NZXT RGB & Fan Controller", None), m.bridge.skip)
        self.assertFalse(any(name == "ASUS" for name, _ in m.bridge.skip))

    def test_openrgb_duplicates_of_native_devices_are_skipped(self):
        bridge = hw.OpenRGBBridge(skip=("NZXT RGB & Fan Controller",))

        class Client:
            connected = True

            def controllers(self):
                base = {"type": 4, "description": "", "version": "", "serial": "", "location": "", "modes": [],
                        "active_mode": 0, "led_names": [], "colors": [], "type_name": "LED strip",
                        "zones": [{"name": "Z", "type": 1, "leds_min": 0, "leds_max": 40, "leds_count": 0}]}
                return [{**base, "index": 0, "name": "NZXT RGB & Fan Controller 2024", "vendor": "NZXT"},
                        {**base, "index": 1, "name": "RTX 5080 Gaming OC", "vendor": "Gigabyte", "type_name": "Graphics card"}]
        bridge.client = Client()
        bridge._load()
        self.assertEqual([d.name for d in bridge.devices.values()], ["RTX 5080 Gaming OC"])
        dev = next(iter(bridge.devices.values()))
        self.assertEqual(dev.zones[0].default_leds, 30)            # empty ARGB zone gets a sensible start size
        self.assertTrue(dev.zones[0].resizable)


class ResetTests(unittest.TestCase):
    def setUp(self):
        from tests.fakes import FakeLightDevice, _FakeBridge
        self.dev = FakeLightDevice("strip", "Strip")
        self.manager = hw.DeviceManager(natives=[self.dev], bridge=_FakeBridge())

    def test_resume_storm_resets_once(self):
        # two resume broadcasts plus the engine and controller noticing the gap
        results = [self.manager.reset(True) for _ in range(4)]
        self.assertEqual(results, [True, False, False, False])
        self.assertEqual(self.dev.opens, 1)
        self.manager._last_reset -= self.manager.RESET_DEBOUNCE + 1
        self.assertTrue(self.manager.reset(True))

    def test_request_reset_runs_on_the_device_thread(self):
        self.manager.request_reset()
        self.assertTrue(self.manager._reset_wanted.is_set())
        self.assertEqual(self.dev.opens, 0)                         # nothing happens on the caller's thread

    def test_read_on_closed_hid_device_is_a_device_error(self):
        hub = devices.NzxtControlHub(backend=FakeHidBackend())
        with self.assertRaises(devices.DeviceError):
            hub.poll()


if __name__ == "__main__":
    unittest.main()
