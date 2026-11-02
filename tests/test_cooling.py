import importlib.util
import json
from pathlib import Path
import tempfile
import threading
import unittest
from unittest import mock


class CoolingTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.mono, self.wall = 10.0, 1000.0

    def recorder(self):
        self.assertIsNotNone(importlib.util.find_spec("airglow.cooling"), "cooling recorder module is required")
        from airglow.cooling import Recorder
        recorder = Recorder(self.tmp.name, monotonic=lambda: self.mono, epoch=lambda: self.wall)
        self.addCleanup(recorder.wait_for_save, 3)
        return recorder

    def sample(self, rec, temp=50, rpm=None):
        return rec.sample({"cpu": temp, "gpu": None, "liquid": None, "temp": temp, "duty": 45,
                           "rpm": rpm, "channels": [], "curve": {"name": "balanced", "points": [[20, 30], [80, 100]],
                           "source": "cpu", "failsafe": 60}})

    def test_opt_in_cadence_missing_data_and_sample_limit(self):
        rec = self.recorder()
        self.sample(rec)
        self.assertIsNone(rec.status()["active"])
        rec.start("Game benchmark")
        self.sample(rec)
        self.sample(rec)
        self.assertEqual(rec.status()["active"]["sample_count"], 1)
        for _ in range(1800):
            self.mono += 2
            self.wall += 2
            self.sample(rec)
        state = rec.status()
        self.assertIsNone(state["active"])
        session = rec.get(state["sessions"][0]["id"])
        self.assertEqual(len(session["samples"]), 1800)
        self.assertIsNone(session["samples"][0]["rpm"])
        self.assertEqual(session["ended_reason"], "sample_limit")

    def test_storage_requires_explicit_deletion_and_recovers_interrupted_recording(self):
        rec = self.recorder()
        rec.start("Interrupted workload")
        recovered = self.recorder()
        session = recovered.get(recovered.status()["sessions"][0]["id"])
        self.assertEqual(session["ended_reason"], "interrupted")
        for i in range(11):
            recovered.start(f"Run {i}")
            recovered.stop()
        with self.assertRaisesRegex(ValueError, "12"):
            recovered.start("Run 13")
        recovered.delete(session["id"])
        recovered.start("Run after deletion")
        self.assertIsNotNone(recovered.status()["active"])

    def test_failed_save_keeps_completed_recording_for_explicit_retry(self):
        rec = self.recorder()
        rec.start("=SUM(A1:A2)")
        self.sample(rec)
        with mock.patch("airglow.cooling.atomic_write_text", side_effect=OSError("disk full")):
            result = rec.stop()
        self.assertFalse(result["saved"])
        self.assertIn("disk full", rec.status()["save_error"])
        session_id = result["session"]["id"]
        self.assertEqual(len(rec.get(session_id)["samples"]), 1)
        self.assertTrue(rec.retry_save()["saved"])
        reloaded = self.recorder()
        self.assertEqual(reloaded.get(session_id)["annotation"], "=SUM(A1:A2)")

    def test_comparison_replay_and_exports_preserve_missing_values(self):
        rec = self.recorder()
        rec.start("Cool room")
        self.sample(rec, 50, 900)
        self.mono += 2
        self.wall += 2
        self.sample(rec, None, None)
        a = rec.stop()["session"]["id"]
        rec.start("Warm room")
        self.sample(rec, 60, 1200)
        b = rec.stop()["session"]["id"]
        comparison = rec.compare(a, b)
        self.assertEqual(comparison["delta"]["cpu_mean"], 10)
        replay = rec.replay(a, [[20, 30], [80, 90]], "cpu")
        self.assertEqual([s["draft_target"] for s in replay["samples"]], [60, None])
        self.assertFalse(replay["applied"])
        payload = rec.export(a, "json")
        self.assertIsNone(json.loads(payload["content"])["samples"][1]["cpu"])
        csv = rec.export(a, "csv")["content"]
        self.assertIn("cpu_c", csv)
        self.assertNotIn("None", csv)

    def test_unread_recording_file_is_protected_from_overwrite(self):
        self.recorder()
        path = Path(self.tmp.name) / "cooling-sessions.json"
        path.write_text("broken", encoding="utf-8")
        rec = self.recorder()
        with self.assertRaisesRegex(ValueError, "read"):
            rec.start("must not erase damaged history")
        self.assertEqual(path.read_text(encoding="utf-8"), "broken")

    def controller(self):
        from airglow.config import Store
        from airglow.controller import Controller
        from tests.fakes import FakeManager, FakeNzxt, signals
        store = Store(self.tmp.name)
        dev = FakeNzxt()
        manager = FakeManager(dev)
        manager.maintain(True)
        ctrl = Controller(store, manager=manager, read_sensors=lambda: {"online": True, "cpu_temp": 50},
                          signals=signals, async_writes=False)
        ctrl._system.read = lambda: {}
        self.addCleanup(ctrl.cooling.wait_for_save, 3)
        return ctrl, dev

    def run_thread(self, fn):
        done, result = threading.Event(), {}
        def run():
            try:
                result["value"] = fn()
            except BaseException as e:
                result["error"] = e
            finally:
                done.set()
        thread = threading.Thread(target=run, daemon=True)
        thread.start()
        self.addCleanup(thread.join, 3)
        return done, result

    def test_start_and_stop_disk_waits_do_not_block_fan_cycles_or_status(self):
        from airglow.cooling import atomic_write_text
        for action in ("start", "stop"):
            with self.subTest(action=action):
                ctrl, dev = self.controller()
                rec = ctrl.cooling
                if action == "stop":
                    rec.start("Baseline")
                    ctrl.tick()
                entered, release = threading.Event(), threading.Event()
                def gated_write(path, text):
                    entered.set()
                    if not release.wait(3):
                        raise TimeoutError("test write gate did not release")
                    return atomic_write_text(path, text)
                with mock.patch("airglow.cooling.atomic_write_text", side_effect=gated_write):
                    save_done, saved = self.run_thread(lambda: rec.start("Baseline") if action == "start" else rec.stop())
                    try:
                        self.assertTrue(entered.wait(1))
                        before = len(dev.hub.duties)
                        tick_done, ticks = self.run_thread(lambda: [ctrl.tick() for _ in range(4)])
                        status_done, status = self.run_thread(lambda: (rec.status(), ctrl.snapshot()))
                        self.assertTrue(tick_done.wait(0.5), "disk I/O must not hold the lock needed by fan cycles")
                        self.assertTrue(status_done.wait(0.5), "status must remain available during persistence")
                        self.assertNotIn("error", ticks)
                        self.assertNotIn("error", status)
                        self.assertTrue(status["value"][0]["saving"])
                        self.assertEqual(len(dev.hub.duties), before + 2)
                        self.assertFalse(save_done.is_set(), "explicit API save waits for its own persistence result")
                    finally:
                        release.set()
                        self.assertTrue(save_done.wait(2))
                self.assertNotIn("error", saved)
                if action == "start":
                    rec.stop()

    def test_sample_limit_queues_persistence_without_waiting(self):
        from airglow.cooling import atomic_write_text
        rec = self.recorder()
        rec.start("Limit run")
        for _ in range(1799):
            self.sample(rec)
            self.mono += 2
        entered, release, written = threading.Event(), threading.Event(), threading.Event()
        def gated_write(path, text):
            entered.set()
            if not release.wait(3):
                raise TimeoutError("test write gate did not release")
            atomic_write_text(path, text)
            written.set()
        with mock.patch("airglow.cooling.atomic_write_text", side_effect=gated_write):
            sample_done, sampled = self.run_thread(lambda: self.sample(rec))
            try:
                self.assertTrue(entered.wait(1))
                self.assertTrue(sample_done.wait(0.5), "the 1,800th sample must not persist on the caller's thread")
                status_done, status = self.run_thread(rec.status)
                self.assertTrue(status_done.wait(0.5))
                self.assertIsNone(status["value"]["active"])
                self.assertEqual(status["value"]["sessions"][0]["sample_count"], 1800)
            finally:
                release.set()
                self.assertTrue(written.wait(2))
        self.assertNotIn("error", sampled)

    def test_delete_and_automatic_finish_persist_in_order_without_resurrecting_history(self):
        from airglow.cooling import atomic_write_text
        rec = self.recorder()
        rec.MAX_SAMPLES = 2
        rec.start("Delete this")
        removed = rec.stop()["session"]["id"]
        rec.start("Keep this")
        kept = rec.status()["active"]["id"]
        self.sample(rec)
        entered, release = threading.Event(), threading.Event()
        writes = []
        def gated_write(path, text):
            writes.append(json.loads(text))
            if len(writes) == 1:
                entered.set()
                if not release.wait(3):
                    raise TimeoutError("test write gate did not release")
            atomic_write_text(path, text)
        with mock.patch("airglow.cooling.atomic_write_text", side_effect=gated_write):
            deleted, result = self.run_thread(lambda: rec.delete(removed))
            try:
                self.assertTrue(entered.wait(1))
                self.mono += 2
                finished, sampled = self.run_thread(lambda: self.sample(rec))
                self.assertTrue(finished.wait(0.5))
                self.assertNotIn("error", sampled)
            finally:
                release.set()
                self.assertTrue(deleted.wait(2))
                self.assertTrue(rec.wait_for_save(2))
        self.assertNotIn("error", result)
        self.assertEqual(len(writes), 2)
        reloaded = self.recorder()
        self.assertIsNone(reloaded.status()["active"])
        self.assertEqual([s["id"] for s in reloaded.status()["sessions"]], [kept])
        self.assertEqual(reloaded.get(kept)["ended_reason"], "sample_limit")

    def test_automatic_save_failure_stays_in_memory_until_explicit_retry(self):
        rec = self.recorder()
        rec.MAX_SAMPLES = 2
        rec.start("Cap failure")
        self.sample(rec)
        self.mono += 2
        with mock.patch("airglow.cooling.atomic_write_text", side_effect=OSError("disk full")):
            self.sample(rec)
            self.assertTrue(rec.wait_for_save(2))
        state = rec.status()
        self.assertFalse(state["saving"])
        self.assertIn("disk full", state["save_error"])
        sid = state["sessions"][0]["id"]
        self.assertEqual(len(rec.get(sid)["samples"]), 2)
        self.assertTrue(rec.retry_save()["saved"])
        self.assertEqual(len(self.recorder().get(sid)["samples"]), 2)

    def test_shutdown_releases_hardware_before_its_bounded_save_drain(self):
        from airglow.cooling import atomic_write_text
        ctrl, dev = self.controller()
        ctrl.cooling.start("Shutdown")
        entered, release = threading.Event(), threading.Event()
        def gated_write(path, text):
            entered.set()
            if not release.wait(3):
                raise TimeoutError("test write gate did not release")
            atomic_write_text(path, text)
        with mock.patch("airglow.cooling.atomic_write_text", side_effect=gated_write):
            stopped, result = self.run_thread(ctrl.shutdown)
            try:
                self.assertTrue(entered.wait(1))
                self.assertTrue(stopped.wait(1.5), "shutdown may drain for one second after safety work, never indefinitely")
                self.assertFalse(dev.online)
                self.assertEqual(set(dev.hub.duties[-1].values()), {ctrl.store.settings()["fan_failsafe_duty"]})
            finally:
                release.set()
                self.assertTrue(ctrl.cooling.wait_for_save(2))
        self.assertNotIn("error", result)

    def test_incomplete_or_invalid_history_is_protected_and_never_breaks_views(self):
        sample = {"t": 1000, "elapsed": 0, "cpu": 50, "gpu": None, "liquid": None, "temp": 50, "duty": 45,
                  "rpm": 900, "channels": [{"id": "fan", "name": "Fan", "mode": "curve", "rpm": 900,
                                            "target": 45, "duty": 45}],
                  "curve": {"name": "balanced", "points": [[20, 30], [80, 100]], "source": "cpu", "failsafe": 60}}
        session = {"id": "one", "annotation": "Baseline", "started_at": 1000, "ended_at": 1002,
                   "ended_reason": "stopped", "samples": [sample]}
        cases = [
            {"id": "one", "annotation": "Bad metadata", "samples": []},
            {**session, "started_at": "not a timestamp"},
            {**session, "ended_at": float("nan")},
            {**session, "samples": [{**sample, "channels": [7]}]},
            {**session, "samples": [{**sample, "curve": []}]},
        ]
        for invalid in cases:
            with self.subTest(invalid=invalid):
                path = Path(self.tmp.name) / "cooling-sessions.json"
                original = json.dumps({"version": 1, "sessions": [invalid], "active": None})
                path.write_text(original, encoding="utf-8")
                rec = self.recorder()
                self.assertIsNotNone(rec.load_error)
                self.assertEqual(rec.status()["sessions"], [])
                with self.assertRaisesRegex(ValueError, "read"):
                    rec.start("Must not overwrite")
                self.assertEqual(path.read_text(encoding="utf-8"), original)
        path.write_text(json.dumps({"version": 1, "sessions": [session, session], "active": None}), encoding="utf-8")
        self.assertIsNotNone(self.recorder().status()["load_error"])

    def test_steady_only_rig_records_channel_target_without_unused_curve_duty(self):
        ctrl, _ = self.controller()
        ctrl.manager.natives.clear()
        ctrl.fan_duty = 37
        ctrl.fan_channels = [{"id": "/fan/steady", "name": "Pump", "mode": "steady", "target": 80, "duty": 80, "rpm": 1000}]
        ctrl.cooling.start("Steady pump")
        ctrl._record_cooling()
        sid = ctrl.cooling.stop()["session"]["id"]
        sample = ctrl.cooling.get(sid)["samples"][0]
        self.assertIsNone(sample["duty"], "37% was computed but never commanded to any output")
        self.assertEqual(sample["channels"][0]["target"], 80)
