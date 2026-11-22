import csv
import io
import json
import tempfile
import unittest
import time
from pathlib import Path
from types import SimpleNamespace

from airglow.cooling import Recorder
from airglow.operationsapi import route


class SessionTelemetryTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.directory = temp.name
        self.clock = 1000.0
        self.recorder = Recorder(self.directory, monotonic=lambda: self.clock, epoch=lambda: self.clock)
        self.addCleanup(self.recorder.wait_for_save, 3)
        self.catalog = {"sys/ram": {"id": "sys/ram", "name": "Memory used", "group": "Windows",
                                    "unit": "GB", "value": 0}}
        self.server = SimpleNamespace(ctrl=SimpleNamespace(cooling=self.recorder, readings=self.catalog))

    def start(self):
        return route(self.server, "POST", "/api/cooling/start", {"annotation": "Run A", "sensor_ids": ["sys/ram"]})["active"]["id"]

    def sample(self):
        self.recorder.sample({"cpu": 40, "readings": self.catalog, "curve": {
            "name": "balanced", "source": "cpu", "failsafe": 60, "points": [[30, 30], [80, 100]]}})

    def test_catalog_selection_tracks_current_values_units_and_gaps(self):
        sid = self.start()
        self.sample()
        self.clock += 2
        self.catalog["sys/ram"]["unit"] = "MB"
        self.sample()
        self.clock += 2
        self.catalog.clear()
        self.sample()
        self.clock += 3
        self.recorder.stop()
        reloaded = Recorder(self.directory)
        saved = reloaded.get(sid)
        self.assertEqual(saved["sensors"][0]["unit"], "GB")
        self.assertEqual([s["readings"] for s in saved["samples"]], [[0], [None], [None]])
        self.assertEqual(saved["duration_seconds"], 7)
        trace = route(self.server, "GET", "/api/cooling/trace", {}, {"id": [sid], "after": ["1"]})
        self.assertEqual((trace["offset"], trace["sample_count"]), (1, 3))
        self.assertEqual([s["elapsed"] for s in trace["samples"]], [2, 4])
        self.assertNotIn("curve", trace["samples"][0])
        self.assertEqual(json.loads(reloaded.export(sid)["content"])["samples"][0]["readings"], [0])
        rows = list(csv.reader(io.StringIO(reloaded.export(sid, "csv")["content"])))
        self.assertIn("sys/ram", rows[0][-1])
        self.assertIn("GiB", rows[0][-1])
        self.assertEqual([r[-1] for r in rows[1:]], ["0.0", "", ""])

    def test_only_known_unique_catalog_entries_can_be_requested(self):
        from airglow.api import ApiError
        for ids in (["missing"], ["sys/ram"] * 2, ["sys/ram"] * 7, ["sys/ram", {}], "sys/ram", None):
            with self.subTest(ids=ids), self.assertRaises(ApiError):
                route(self.server, "POST", "/api/cooling/start", {"sensor_ids": ids})
            self.assertIsNone(self.recorder.status()["active"])

    def test_legacy_files_read_and_corrupt_extensions_are_protected(self):
        sid = self.recorder.start()["active"]["id"]
        self.sample()
        self.recorder.stop()
        path = Path(self.directory) / "cooling-sessions.json"
        data = json.loads(path.read_text(encoding="utf-8"))
        session = data["sessions"][0]
        session.pop("sensors", None)
        session.pop("duration_seconds", None)
        session["samples"][0].pop("readings", None)
        path.write_text(json.dumps(data), encoding="utf-8")
        self.assertEqual(Recorder(self.directory).trace(sid)["samples"][0]["cpu"], 40)
        session["sensors"] = [{"id": "sys/ram", "name": "RAM", "unit": "GB", "group": ""}]
        session["samples"][0]["readings"] = ["bad"]
        original = json.dumps(data)
        path.write_text(original, encoding="utf-8")
        broken = Recorder(self.directory)
        self.assertIn("read", broken.load_error)
        with self.assertRaises(ValueError):
            broken.start()
        self.assertEqual(path.read_text(encoding="utf-8"), original)

    def test_background_checkpoint_recovers_measurements_after_interruption(self):
        sid = self.start()
        self.sample()
        self.clock += 61
        self.sample()
        self.assertTrue(self.recorder.wait_for_save(3))
        recovered = Recorder(self.directory).get(sid)
        self.assertEqual(recovered["ended_reason"], "interrupted")
        self.assertEqual([s["readings"] for s in recovered["samples"]], [[0], [0]])

    def test_outdated_stop_request_leaves_new_recording_running(self):
        from airglow.api import ApiError
        first = self.start()
        self.recorder.stop()
        second = self.start()
        with self.assertRaisesRegex(ApiError, "changed"):
            route(self.server, "POST", "/api/cooling/stop", {"id": first})
        self.assertEqual(self.recorder.status()["active"]["id"], second)

    def test_stale_controller_cycle_records_missing_cooling_and_sensor_values(self):
        from airglow.config import Store
        from airglow.controller import Controller
        from tests.fakes import FakeManager, FakeNzxt, signals
        dev = FakeNzxt()
        manager = FakeManager(dev)
        manager.maintain(True)
        ctrl = Controller(Store(self.directory), manager=manager, read_sensors=lambda: {"online": True, "cpu_temp": 53},
                          signals=signals, async_writes=False)
        ctrl.tick()
        sid = route(SimpleNamespace(ctrl=ctrl), "POST", "/api/cooling/start", {"sensor_ids": ["auto/cpu_temp"]})["active"]["id"]
        ctrl._record(time.monotonic() - ctrl.SAMPLE_STALE_AFTER - 1)
        record = ctrl.cooling.trace(sid)["samples"][0]
        self.assertEqual(record["readings"], [None])
        self.assertTrue(all(record[k] is None for k in ("cpu", "gpu", "liquid", "duty", "rpm")))
        ctrl.cooling.stop()
