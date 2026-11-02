"""Controller/profile transactions share coherent input snapshots, never hardware-I/O locks."""

import tempfile
import threading
import unittest
from unittest import mock

from airglow.config import Store
from airglow.controller import Controller
from tests.fakes import FakeManager, FakeNzxt, signals


class ControllerSnapshotTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.store = Store(self.tmp.name)
        self.dev = FakeNzxt()
        self.manager = FakeManager(self.dev)
        self.manager.maintain(True)
        self.ctrl = Controller(self.store, manager=self.manager, read_sensors=lambda: {"online": True, "cpu_temp": 50},
                               signals=signals, async_writes=False)
        self.ctrl._system.read = lambda: {}

    def run_thread(self, fn):
        done, result = threading.Event(), {}
        def run():
            try:
                result["value"] = fn()
            except BaseException as error:
                result["error"] = error
            finally:
                done.set()
        thread = threading.Thread(target=run, daemon=True)
        thread.start()
        self.addCleanup(thread.join, 3)
        return done, result

    def test_fan_curve_identity_and_points_are_resolved_before_another_selection(self):
        self.store.set_curve("balanced", [[20, 30], [80, 30]])
        self.store.set_curve("performance", [[20, 70], [80, 70]])
        entered, release = threading.Event(), threading.Event()
        original = self.ctrl.curve_points
        def gated_points(name):
            if threading.current_thread().name != threading.main_thread().name and name == "balanced":
                entered.set()
                if not release.wait(3):
                    raise TimeoutError("test curve gate did not release")
            return original(name)
        with mock.patch.object(self.ctrl, "curve_points", side_effect=gated_points):
            fan_done, fan = self.run_thread(self.ctrl._run_fans)
            try:
                self.assertTrue(entered.wait(1))
                changed, change = self.run_thread(lambda: self.ctrl.set_fan_curve("performance"))
                self.assertFalse(changed.wait(0.15), "a selection cannot change between curve identity and points")
            finally:
                release.set()
                self.assertTrue(fan_done.wait(2))
                self.assertTrue(changed.wait(2))
        self.assertNotIn("error", fan)
        self.assertNotIn("error", change)
        self.assertEqual(set(self.dev.hub.duties[0].values()), {30})
        self.assertEqual(self.ctrl.effective_curve, "performance")

    def test_automation_decision_finishes_before_a_profile_mutation_guard_observes_it(self):
        entered, release = threading.Event(), threading.Event()
        def evaluate():
            entered.set()
            if not release.wait(3):
                raise TimeoutError("test automation gate did not release")
            self.ctrl.automations.fan_rule = {"id": "activated", "name": "Rule", "then": {"type": "fan_curve", "curve": "balanced"}}
        def guarded_profile_read():
            with self.ctrl._lock:
                return self.ctrl.automations.fan_rule
        with mock.patch.object(self.ctrl.automations, "evaluate", side_effect=evaluate):
            tick_done, tick = self.run_thread(self.ctrl.tick)
            try:
                self.assertTrue(entered.wait(1))
                guard_done, guard = self.run_thread(guarded_profile_read)
                self.assertFalse(guard_done.wait(0.15), "the profile guard must not read an automation decision in progress")
            finally:
                release.set()
                self.assertTrue(tick_done.wait(2))
                self.assertTrue(guard_done.wait(2))
        self.assertNotIn("error", tick)
        self.assertEqual(guard["value"]["id"], "activated")

    def test_hardware_wait_does_not_hold_the_controller_snapshot_lock(self):
        entered, release = threading.Event(), threading.Event()
        def poll():
            entered.set()
            if not release.wait(3):
                raise TimeoutError("test hardware gate did not release")
        with mock.patch.object(self.dev.hub, "poll", side_effect=poll):
            tick_done, tick = self.run_thread(self.ctrl._run_fans)
            try:
                self.assertTrue(entered.wait(1))
                changed, change = self.run_thread(lambda: self.ctrl.set_scene({"brightness": 60}))
                self.assertTrue(changed.wait(0.5), "controller lock must be released before hardware I/O")
            finally:
                release.set()
                self.assertTrue(tick_done.wait(2))
        self.assertNotIn("error", tick)
        self.assertNotIn("error", change)

    def test_recording_curve_metadata_is_captured_before_profile_curve_replacement(self):
        self.ctrl.cooling.start("Metadata snapshot")
        entered, release = threading.Event(), threading.Event()
        original = self.ctrl.curve_points
        def gated_points(name):
            if name == "balanced":
                entered.set()
                if not release.wait(3):
                    raise TimeoutError("test recording gate did not release")
            return original(name)
        with mock.patch.object(self.ctrl, "curve_points", side_effect=gated_points):
            recorded, result = self.run_thread(self.ctrl._record_cooling)
            try:
                self.assertTrue(entered.wait(1))
                changed, change = self.run_thread(lambda: self.ctrl.set_fan_curve("performance"))
                self.assertFalse(changed.wait(0.15), "recording metadata must resolve its named curve before replacement")
            finally:
                release.set()
                self.assertTrue(recorded.wait(2))
                self.assertTrue(changed.wait(2))
        self.assertNotIn("error", result)
        self.assertNotIn("error", change)
        saved = self.ctrl.cooling.stop()
        self.assertEqual(self.ctrl.cooling.get(saved["session"]["id"])["samples"][0]["curve"]["name"], "balanced")
