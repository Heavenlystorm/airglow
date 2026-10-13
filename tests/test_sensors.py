import json
import unittest
from pathlib import Path

from airglow import sensors

FIXTURE = json.loads((Path(__file__).parent / "fixtures" / "lhm_data.json").read_text(encoding="utf-8"))


class ParseRealDataTests(unittest.TestCase):
    """Uses a capture of LibreHardwareMonitor's data.json from a real PC (host name replaced)."""

    def setUp(self):
        self.s = sensors.parse(FIXTURE)

    def test_cpu(self):
        self.assertIn("9800X3D", self.s["cpu_name"])
        self.assertTrue(20 < self.s["cpu_temp"] < 100)
        self.assertTrue(0 < self.s["cpu_power"] < 300)

    def test_discrete_gpu_preferred_over_integrated(self):
        self.assertIn("RTX 5080", self.s["gpu_name"])
        self.assertTrue(15 < self.s["gpu_temp"] < 100)

    def test_total_power(self):
        self.assertAlmostEqual(self.s["total_power"], self.s["cpu_power"] + self.s["gpu_power"], places=0)

    def test_only_spinning_fans_listed(self):
        # In this capture the RTX 5080 is idle with its fans stopped (0 RPM), so only the board fan shows.
        self.assertEqual([f["name"] for f in self.s["fans"]], ["Board fan #2"])
        self.assertTrue(all(f["rpm"] > 0 for f in self.s["fans"]))


class ParseEdgeCaseTests(unittest.TestCase):
    def test_decimal_comma(self):
        self.assertEqual(sensors._num("41,9 °C"), 41.9)
        self.assertEqual(sensors._num("1200 RPM"), 1200)

    def test_empty_tree(self):
        s = sensors.parse({"Text": "Sensor", "Children": []})
        self.assertIsNone(s["cpu_temp"])
        self.assertIsNone(s["total_power"])

    def test_unreachable_server_is_offline(self):
        s = sensors.read("http://127.0.0.1:1", timeout=0.3)
        self.assertFalse(s["online"])
        self.assertEqual(s["fans"], [])


if __name__ == "__main__":
    unittest.main()
