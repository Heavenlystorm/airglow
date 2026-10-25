import json
import time
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


class FormatTests(unittest.TestCase):
    def test_money_per_hour(self):
        self.assertEqual(sensors.fmt({"unit": "€/h"}, 0.123), "€0.12/h")
        self.assertEqual(sensors.fmt({"unit": "€/h"}, None), "n/a")


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




class LhmPollerTests(unittest.TestCase):
    def poller(self, replies):
        calls = []

        def read(url, headers=None):
            calls.append(time.monotonic())
            return replies.pop(0) if replies else None
        p = sensors.LhmPoller(lambda: "http://127.0.0.1:8085", read=read)
        p.INTERVAL, p.OFFLINE_RETRY = 0.02, 0.5
        return p, calls

    def test_offline_lhm_is_retried_slowly(self):
        p, calls = self.poller([])
        p.start()
        time.sleep(0.3)
        p.stop()
        self.assertEqual(len(calls), 1)            # offline: the next try waits OFFLINE_RETRY, not INTERVAL
        self.assertIsNone(p.tree)

    def test_online_lhm_is_read_every_interval_and_wake_retries_now(self):
        p, calls = self.poller([None, {"Children": []}, {"Children": []}])
        p.start()
        time.sleep(0.05)
        self.assertEqual(len(calls), 1)
        p.wake()                                   # a new address or helper setup: try again right away
        time.sleep(0.2)
        p.stop()
        self.assertGreaterEqual(len(calls), 3)

    def test_a_failing_read_counts_as_offline(self):
        def boom(url, headers=None):
            raise RuntimeError("bug")
        p = sensors.LhmPoller(lambda: "x", read=boom)
        p.tree = {"old": 1}
        with self.assertLogs("airglow.sensors", "ERROR"):
            self.assertIsNone(p.poll())


if __name__ == "__main__":
    unittest.main()
