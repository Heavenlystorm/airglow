import csv
import io
import json
import tempfile
import threading
import unittest
from types import SimpleNamespace
from unittest.mock import patch

from airglow.api import ApiError
from airglow.cooling import Recorder
from airglow.operationsapi import route


class SessionNotesTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.folder, self.now = tmp.name, 1000.0
        self.rec = Recorder(tmp.name, monotonic=lambda: self.now, epoch=lambda: self.now + 500)
        self.addCleanup(self.rec.wait_for_save, 3)
        self.srv = SimpleNamespace(ctrl=SimpleNamespace(cooling=self.rec))
        self.sid = self.rec.start("Original workload")["active"]["id"]

    def sample(self):
        self.rec.sample({"cpu": 53, "curve": {"name": "balanced", "source": "cpu", "failsafe": 50,
                                              "points": [[30, 30], [80, 100]]}})

    def test_live_and_placed_events_have_distinct_origins_and_survive_restart(self):
        self.sample()
        self.now += 1.345
        marked = self.rec.mark(self.sid, "a" * 32, "Opened game")
        self.assertEqual(marked["markers"][0]["elapsed"], 1.345)
        self.assertEqual(marked["markers"][0]["created_at"], self.now + 500)
        self.now += 2
        self.sample()
        self.rec.stop()
        # Simulates an acknowledged-late retry after Stop. The original event stays at its original time.
        self.assertEqual(self.rec.mark(self.sid, "a" * 32, "Opened game"), marked)
        self.rec.mark(self.sid, "b" * 32, "=event", elapsed=0)
        original_csv = self.rec.export(self.sid, "csv")["content"]
        self.rec.annotate(self.sid, "Run B", "+notes,\nnext line", {"name": "", "notes": ""})
        reloaded = Recorder(self.folder)
        session = reloaded.get(self.sid)
        self.assertEqual([m["kind"] for m in session["markers"]], ["placed", "live"])
        self.assertEqual((session["name"], session["notes"], session["annotation"]), ("Run B", "+notes,\nnext line", "Original workload"))
        self.assertEqual(len(session["samples"]), 2)
        self.assertEqual(reloaded.export(self.sid, "csv")["content"], original_csv)
        payload = json.loads(reloaded.export(self.sid, "json")["content"])
        self.assertEqual(payload["markers"], session["markers"])
        timeline = list(csv.DictReader(io.StringIO(reloaded.export(self.sid, "timeline-csv")["content"])))
        self.assertEqual([r["row_type"] for r in timeline], ["recording", "marker", "marker"])
        self.assertEqual(timeline[0]["notes"], "'+notes,\nnext line")
        self.assertEqual(timeline[1]["label"], "'=event")
        self.assertEqual(timeline[2]["elapsed_s"], "1.345")

    def test_annotation_write_failure_leaves_previous_file_and_revision_intact(self):
        self.now += 4
        self.rec.stop()
        original = self.rec.path.read_bytes()
        with patch("airglow.cooling.atomic_write_text", side_effect=OSError("disk full")):
            with self.assertRaisesRegex(ValueError, "disk full"):
                self.rec.mark(self.sid, "c"*32, "After closing", elapsed=2)
            with self.assertRaisesRegex(ValueError, "disk full"):
                self.rec.annotate(self.sid, "New", "Draft", {"name": "", "notes": ""})
        self.assertEqual(self.rec.path.read_bytes(), original)
        self.assertNotIn("name", self.rec.get(self.sid))
        self.assertEqual(self.rec.status()["sessions"][0]["metadata_revision"], 0)
        self.rec.mark(self.sid, "c"*32, "After closing", elapsed=2)
        self.rec.annotate(self.sid, "New", "Draft", {"name": "", "notes": ""})
        self.assertEqual(Recorder(self.folder).get(self.sid)["metadata_revision"], 2)

    def test_two_editors_cannot_overwrite_notes_and_marker_retries_do_not_duplicate(self):
        self.now += 5
        self.rec.stop()
        body = {"id": self.sid, "name": "A", "notes": "Morning run", "expected": {"name": "", "notes": ""}}
        first = route(self.srv, "POST", "/api/cooling/annotate", body)
        self.assertEqual(route(self.srv, "POST", "/api/cooling/annotate", body), first)
        with self.assertRaises(ApiError) as error:
            route(self.srv, "POST", "/api/cooling/annotate", {**body, "name": "Other"})
        self.assertEqual(error.exception.status, 409)
        args = {"id": self.sid, "marker_id": "d"*32, "label": "Repeated request", "elapsed": 1.5}
        marked = route(self.srv, "POST", "/api/cooling/mark", args)
        self.assertEqual(route(self.srv, "POST", "/api/cooling/mark", args), marked)
        with self.assertRaises(ApiError):
            route(self.srv, "POST", "/api/cooling/mark", {**args, "label": "Different event"})
        other_id = self.rec.start("Other run")["active"]["id"]
        self.rec.unmark(other_id, args["marker_id"])
        self.assertEqual(len(self.rec.get(self.sid)["markers"]), 1)
        removed = route(self.srv, "POST", "/api/cooling/unmark", args)
        self.assertEqual(removed["markers"], [])
        self.assertEqual(route(self.srv, "POST", "/api/cooling/unmark", args), removed)

    def test_bounds_protect_existing_records_and_legacy_headers(self):
        self.now += 3
        self.rec.stop()
        with self.assertRaises(ValueError):
            self.rec.mark(self.sid, "d"*32, "Past end", elapsed=3.1)
        for n in range(64):
            self.rec.mark(self.sid, f"{n:032x}", f"Marker {n}", elapsed=1)
        with self.assertRaisesRegex(ValueError, "64"):
            self.rec.mark(self.sid, "f"*32, "Overflow", elapsed=1)
        with self.assertRaises(ValueError):
            self.rec.annotate(self.sid, "x"*81, "", {"name":"", "notes":""})
        with self.assertRaises(ValueError):
            self.rec.annotate(self.sid, "Name", "x"*4001, {"name":"", "notes":""})
        self.assertEqual(self.rec.status()["sessions"][0]["marker_count"], 64)
        data = json.loads(self.rec.path.read_text(encoding="utf-8"))
        record = data["sessions"][0]
        record["markers"][0]["elapsed"] = -1
        invalid = json.dumps(data)
        self.rec.path.write_text(invalid, encoding="utf-8")
        self.assertIn("read", Recorder(self.folder).load_error)
        self.assertEqual(self.rec.path.read_text(encoding="utf-8"), invalid)
        record.pop("markers"); record.pop("metadata_revision"); record.pop("duration_seconds", None)
        self.rec.path.write_text(json.dumps(data), encoding="utf-8")
        legacy = Recorder(self.folder)
        self.assertEqual(legacy.status()["sessions"][0]["marker_count"], 0)
        legacy.annotate(self.sid, "Legacy", "No samples", {"name":"", "notes":""})
        self.assertEqual(legacy.get(self.sid)["name"], "Legacy")

    def test_in_flight_durable_marker_does_not_block_samples_or_resurrect_active_run(self):
        from airglow.cooling import atomic_write_text
        self.rec.MAX_SAMPLES = 2
        self.sample(); self.now += 2
        entered, release, done = threading.Event(), threading.Event(), threading.Event()
        errors = []
        def slow_write(path, data):
            entered.set()
            if not release.wait(2): raise TimeoutError("test gate remained closed")
            atomic_write_text(path, data)
        def mark():
            try: self.rec.mark(self.sid, "e"*32, "At cap")
            except Exception as error: errors.append(error)
            finally: done.set()
        with patch("airglow.cooling.atomic_write_text", side_effect=slow_write):
            thread = threading.Thread(target=mark); thread.start()
            try:
                self.assertTrue(entered.wait(1))
                self.assertEqual(self.rec.status()["active"]["marker_count"], 0)
                self.sample()
                self.assertIsNone(self.rec.status()["active"])
            finally:
                release.set(); thread.join(2)
            self.assertTrue(done.is_set())
            self.assertTrue(self.rec.wait_for_save(2))
        self.assertFalse(errors)
        saved = Recorder(self.folder).get(self.sid)
        self.assertEqual(saved["ended_reason"], "sample_limit")
        self.assertEqual(saved["markers"][0]["elapsed"], 2)
        self.assertEqual(len(saved["samples"]), 2)

    def test_event_before_first_measurement_survives_interruption_at_its_actual_time(self):
        self.now += 1.5
        self.rec.mark(self.sid, "a"*32, "Before sensor poll")
        recovered = Recorder(self.folder)
        trace = recovered.trace(self.sid)
        self.assertEqual(trace["ended_reason"], "interrupted")
        self.assertIn("1 event marker", trace["recovery_note"])
        self.assertEqual(trace["duration_seconds"], 1.5)
        self.assertEqual(trace["samples"], [])
