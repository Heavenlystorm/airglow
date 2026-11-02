import json
import time
import unittest
from collections import namedtuple
from unittest import mock
from pathlib import Path
from types import SimpleNamespace

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

    def test_quick_picks_preserve_legitimate_fixture_and_system_values(self):
        readings = sensors.build(sensors.flatten(FIXTURE), {
            "cpu_load": 99, "ram_load": 40, "ram_used": 12, "ram_total": 32,
            "net_down": 1024, "net_up": 512, "disk_read": 4096, "disk_write": 2048,
        })
        expected = {
            "cpu_temp": 40.4, "gpu_temp": 43, "gpu_hotspot": None, "liquid_temp": None,
            "cpu_load": 10.8, "gpu_load": 5, "cpu_power": 51.4, "gpu_power": 56.3,
            "total_power": 107.7, "cpu_clock": 4624, "gpu_clock": 2805, "vram_used": 1759,
            "ram_load": 40, "ram_used": 12, "ram_temp": 33.8, "ssd_temp": None,
            "net_down": 1024, "net_up": 512, "disk_read": 4096, "disk_write": 2048,
        }
        for key, value in expected.items():
            with self.subTest(alias=key):
                self.assertEqual(readings["auto/" + key]["value"], value)


class SystemSensorAccuracyTests(unittest.TestCase):
    def setUp(self):
        # Only the OS reads and clock are replaced; exercise the real sampler across polls.
        import psutil
        vm = SimpleNamespace(total=32 * 1024 ** 3, available=20 * 1024 ** 3, percent=37.5,
                             used=12 * 1024 ** 3, free=20 * 1024 ** 3)
        self.patch = lambda name, **kw: self.enterContext(mock.patch.object(psutil, name, **kw))
        self.patch("virtual_memory", return_value=vm)
        self.patch("cpu_percent", return_value=0.0)
        self.patch("net_io_counters", return_value=None)
        self.patch("disk_io_counters", return_value=None)
        self.enterContext(mock.patch.object(sensors.time, "monotonic", side_effect=range(1, 20)))
        self.sampler = sensors.SystemSensors()

    def test_first_cpu_sample_is_a_baseline_and_later_zero_is_valid(self):
        first, second = self.sampler.read(), self.sampler.read()
        self.assertNotIn("cpu_load", first)
        self.assertEqual(second["cpu_load"], 0)
        self.assertEqual(first["ram_used"], 12)

    def test_missing_io_counters_are_not_reported_as_zero_throughput(self):
        self.sampler.read()
        result = self.sampler.read()
        for key in ("net_down", "net_up", "disk_read", "disk_write"):
            with self.subTest(key=key):
                self.assertNotIn(key, result)

    def test_returning_counters_need_a_new_baseline(self):
        snetio = namedtuple("snetio", "bytes_sent bytes_recv packets_sent packets_recv errin errout dropin dropout")
        sdiskio = namedtuple("sdiskio", "read_count write_count read_bytes write_bytes read_time write_time")
        net = snetio(bytes_sent=1000, bytes_recv=2000, packets_sent=0, packets_recv=0,
                     errin=0, errout=0, dropin=0, dropout=0)
        disk = sdiskio(read_count=0, write_count=0, read_bytes=4000, write_bytes=8000,
                      read_time=0, write_time=0)
        self.patch("net_io_counters", side_effect=[net, None, net, net._replace(bytes_recv=2300, bytes_sent=1100)])
        self.patch("disk_io_counters", side_effect=[disk, None, disk, disk._replace(read_bytes=4400, write_bytes=8000)])
        self.sampler.read()
        missing, returning, measured = self.sampler.read(), self.sampler.read(), self.sampler.read()
        for key in ("net_down", "net_up", "disk_read", "disk_write"):
            with self.subTest(key=key):
                self.assertNotIn(key, missing)
                self.assertNotIn(key, returning)
        self.assertEqual({key: measured[key] for key in ("net_down", "net_up", "disk_read", "disk_write")},
                         {"net_down": 300, "net_up": 100, "disk_read": 400, "disk_write": 0})


class FormatTests(unittest.TestCase):
    def test_money_per_hour(self):
        self.assertEqual(sensors.fmt({"unit": "€/h"}, 0.123), "€0.12/h")
        self.assertEqual(sensors.fmt({"unit": "€/h"}, None), "n/a")


    def test_binary_transfer_rates_use_iec_units(self):
        for value, expected in ((512, "512 B/s"), (1024, "1.0 KiB/s"),
                                (3 * 1024 ** 2, "3.0 MiB/s"), (2 * 1024 ** 3, "2.0 GiB/s")):
            with self.subTest(value=value):
                self.assertEqual(sensors.fmt({"unit": "B/s"}, value), expected)

    def test_binary_memory_values_use_iec_units(self):
        self.assertEqual(sensors.fmt({"unit": "MB"}, 2048), "2048 MiB")
        self.assertEqual(sensors.fmt({"unit": "GB"}, 12.3), "12.3 GiB")


class ParseEdgeCaseTests(unittest.TestCase):
    def test_catalog_keeps_temperature_and_total_readings_but_hides_memory_timings(self):
        names = ["Temperature", "Tctl/Tdie", "Total Power", "temperature", "tCL", "tRCD", "Critical Temperature"]
        flat = [{"id": f"sensor/{i}", "hw": "cpu", "hw_name": "CPU", "type": "temperature",
                 "name": name, "value": 50.0} for i, name in enumerate(names)]
        self.assertEqual([s["name"] for s in sensors.catalog_from(flat)], names[:4])

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
    def test_success_timestamp_only_advances_after_successful_read(self):
        from unittest import mock
        p = sensors.LhmPoller(lambda: "unused", read=lambda *a, **kw: {"Children": []})
        self.assertTrue(hasattr(p, "last_success"))
        self.assertIsNone(p.last_success)
        with mock.patch("airglow.sensors.time.monotonic", return_value=15.0), \
             mock.patch("airglow.sensors.time.time", return_value=1000.0):
            p.poll()
        self.assertEqual(p.last_success, 15.0)
        self.assertEqual(p.last_success_at, 1000.0)
        p._read = lambda *a, **kw: None
        p.poll()
        self.assertEqual(p.last_success, 15.0)
        self.assertEqual(p.last_success_at, 1000.0)

    def poller(self, replies):
        calls = []

        def read(url, headers=None):
            calls.append(time.monotonic())
            return replies.pop(0) if replies else None
        p = sensors.LhmPoller(lambda: "http://127.0.0.1:8085", read=read)
        p.INTERVAL, p.OFFLINE_RETRY = 0.02, 0.5
        p.STARTUP_WINDOW = 0                         # these tests cover steady-state retry
        return p, calls

    def test_startup_retries_back_off_and_stop_at_the_normal_limit(self):
        p = sensors.LhmPoller(lambda: "fixture", read=lambda *a, **kw: None)
        delays, clock = [], [0.0]
        def wait(delay):
            delays.append(delay)
            clock[0] += delay
            if len(delays) == 10:
                p._stop.set()
        p._wake.wait = wait
        with mock.patch.object(sensors.time, "monotonic", side_effect=lambda: clock[0]):
            p._run()
        self.assertEqual(delays[:5], [0.25, 0.5, 1, 2, 4])
        self.assertEqual(delays[-1], p.OFFLINE_RETRY)
        self.assertLessEqual(max(delays), p.OFFLINE_RETRY)

    def test_availability_changes_notify_without_notifying_every_value(self):
        callback = mock.Mock()
        replies = [None, {"Children": []}, {"Children": []}, None]
        p = sensors.LhmPoller(lambda: "fixture", read=lambda *a, **kw: replies.pop(0), on_change=callback)
        for _ in range(4):
            p.poll()
        self.assertEqual(callback.call_count, 2)

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
