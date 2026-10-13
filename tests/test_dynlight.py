import sys
import types
import unittest
from types import SimpleNamespace as NS
from unittest import mock

from airglow import dynlight


class FakeLampArray:
    def __init__(self, n=6, available=True, kind=1, positions=None):
        self.lamp_count, self.lamp_array_kind = n, kind
        self.hardware_vendor_id, self.hardware_product_id = 0x3434, 0x0A10
        self.device_id = r"\\?\HID#VID_3434&PID_0A10#serial-that-must-not-leak"
        self.is_connected, self.is_available, self.is_enabled = True, available, True
        self._pos = positions or [(i * 0.02, (i % 2) * 0.02) for i in range(n)]
        self.sent = []

    def get_lamp_info(self, i):
        x, y = self._pos[i]
        return NS(position=NS(x=x, y=y, z=0.0))

    def set_colors_for_indices(self, colors, indices):
        self.sent.append((colors, list(indices)))


class FakeColor:
    def __init__(self, a, r, g, b):
        self.argb = (a, r, g, b)


class LampArrayDeviceTests(unittest.TestCase):
    def setUp(self):
        ui = types.ModuleType("winrt.windows.ui")
        ui.Color = FakeColor
        self.modules = mock.patch.dict(sys.modules, {"winrt": types.ModuleType("winrt"),
                                                      "winrt.windows": types.ModuleType("winrt.windows"),
                                                      "winrt.windows.ui": ui})
        self.modules.start()

    def tearDown(self):
        self.modules.stop()

    def test_device_shape_and_private_id(self):
        dev = dynlight.LampArrayDevice(FakeLampArray(), "Keychron K2 HE")
        self.assertEqual(dev.type_name, "Keyboard")
        self.assertTrue(dev.id.startswith("dynlight:3434-0a10:"))
        self.assertNotIn("serial", dev.id)
        self.assertEqual(dev.zones[0].shape, "matrix")                  # laid out from the lamp positions
        self.assertTrue(dev.experimental)

    def test_writes_colours_when_windows_allows_it(self):
        arr = FakeLampArray(n=3)
        dev = dynlight.LampArrayDevice(arr, "Mouse")
        dev.write_zone("lamps", [(255, 0, 0), (0, 255, 0), (0, 0, 255)])
        colors, indices = arr.sent[-1]
        self.assertEqual([c.argb for c in colors], [(255, 255, 0, 0), (255, 0, 255, 0), (255, 0, 0, 255)])
        self.assertEqual(indices, [0, 1, 2])

    def test_says_when_another_app_has_control(self):
        arr = FakeLampArray(available=False)
        dev = dynlight.LampArrayDevice(arr, "Mouse")
        dev.write_zone("lamps", [(1, 2, 3)] * 6)
        self.assertEqual(arr.sent, [])
        self.assertIn("another app", dev.error)


class GridTests(unittest.TestCase):
    def test_grid_from_positions(self):
        pts = [(0.0, 0.0), (0.1, 0.0), (0.2, 0.0), (0.0, 0.05), (0.2, 0.05)]
        g = dynlight.grid_from_positions(pts, cols=3, rows=4)
        self.assertEqual((g["rows"], g["cols"]), (2, 3))
        self.assertEqual(g["map"], [0, 1, 2, 3, -1, 4])
        self.assertIsNone(dynlight.grid_from_positions([(0, 0)]))


class BridgeTests(unittest.TestCase):
    def test_off_means_no_devices_and_no_scan(self):
        b = dynlight.DynamicLightingBridge()
        with mock.patch.object(b, "_scan", side_effect=AssertionError("must not scan")):
            b.refresh(False)
        self.assertEqual((b.devices, b.status), ({}, "Off"))

    def test_missing_winrt_is_reported_not_raised(self):
        b = dynlight.DynamicLightingBridge()
        async def no_winrt():
            raise ImportError("winrt")
        with mock.patch.object(b, "_scan", no_winrt):
            b.refresh(True)
        self.assertIn("winrt", b.status)


if __name__ == "__main__":
    unittest.main()
