"""Scene readiness and ownership use fake devices and a host that never spawns a process."""
import base64
import io
import tempfile
import threading
import unittest
from types import SimpleNamespace
from unittest import mock

import numpy as np

from airglow import api, config, fxformat, fxlibrary, studioapi
from airglow.controller import Controller
from airglow.lighting import studio
from tests.fakes import FakeLightDevice, FakeManager, signals


class SceneTransactionsTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.store = config.Store(self.tmp.name)
        self.dev = FakeLightDevice("d", "Device", zones=(("z1", 6),))
        self.manager = FakeManager(self.dev)
        self.manager.maintain(True)
        self.store.ensure_zones([self.dev.info()])
        self.lib = fxlibrary.Library(self.tmp.name + "/effects")
        self.host = studio.EffectHost(self.lib, spawn=lambda _: self.fail("Real host must not start"))
        patch = mock.patch.object(studio, "HOST", self.host)
        patch.start()
        self.addCleanup(patch.stop)
        studio._looked_up.clear()
        self.ctrl = Controller(self.store, manager=self.manager, read_sensors=lambda: {}, signals=signals,
                               async_writes=False)
        self.engine = self.ctrl.engine
        self.ctrl.set_scene({"effect": "solid", "params": {"color": "#ff0000"}})
        self.engine._transition = None
        self.engine.tick()
        self.effects = [self.lib.save_mine(fxformat.to_html({"name": f"Test {i}", "layers": [{"type": "solid"}]}))["fx_id"]
                        for i in range(5)]

    def supply(self, color=(0, 0, 255)):
        self.host.frames({"frames": {iid: base64.b64encode(bytes(color) * len(inst.points)).decode()
                                      for iid, inst in self.host.instances.items()}})

    def test_scene_patch_can_restore_or_clear_profile_areas(self):
        # Undo after a profile puts the earlier look back exactly, per-area effects included.
        areas = {"front": {"effect": "solid", "params": {"color": "#00ff00"}}}
        self.ctrl.set_scene({"effect": "solid", "params": {"color": "#0000ff"}, "areas": areas})
        self.assertEqual(self.ctrl.base_scene["areas"]["front"]["effect"], "solid")
        self.ctrl.set_scene({"effect": "solid", "params": {"color": "#ff0000"}, "areas": None})
        self.assertNotIn("areas", self.ctrl.base_scene)
        with self.assertRaises(ValueError):
            self.ctrl.set_scene({"effect": "solid", "areas": {"nowhere": {"effect": "solid"}}})
        self.assertNotIn("areas", self.ctrl.base_scene)

    def test_100_rapid_changes_reclaim_slots_without_black(self):
        before = self.engine.frames["d/z1"]
        for n in range(100):
            self.ctrl.set_scene({"effect": self.effects[n % 5]})
            self.engine.tick()
            self.assertEqual(self.engine.frames["d/z1"], before)
            self.assertEqual(len(self.host.instances), 1)
            self.assertEqual(self.ctrl.scene_status()["status"], "pending")
        self.supply()
        self.engine.tick()
        self.assertEqual(self.store.state["scene"]["effect"], "solid")
        self.assertEqual(self.ctrl.scene_status()["status"], "ready")
        self.assertEqual(self.store.state["scene"]["effect"], self.effects[4])

    def test_stale_frame_and_wrong_length_cannot_commit(self):
        self.ctrl.set_scene({"effect": self.effects[0]})
        old = next(iter(self.host.instances))
        self.ctrl.set_scene({"effect": self.effects[1]})
        self.host.frames({"frames": {old: base64.b64encode(b"\xff" * 18).decode()}})
        iid = next(iter(self.host.instances))
        self.host.frames({"frames": {iid: base64.b64encode(b"\xff" * 3).decode()}})
        self.engine.tick()
        self.assertEqual(self.ctrl.scene_status()["status"], "pending")

    def test_stale_parameters_and_revision_rejected(self):
        revision = self.ctrl.scene_status()["desired_revision"]
        self.ctrl.set_scene({"effect": "comet"})
        for patch in ({"params": {}, "expected_effect": "solid"},
                      {"brightness": 10, "expected_revision": revision}):
            with self.assertRaises(ValueError):
                self.ctrl.set_scene(patch)

    def test_scene_api_returns_its_accepted_scene_when_another_command_arrives(self):
        read_reply, competitor_done = threading.Event(), threading.Event()
        response = self.ctrl.scene_response

        def competitor():
            if read_reply.wait(5):
                self.ctrl.set_scene({"effect": self.effects[1]})
                competitor_done.set()

        def overlap():
            read_reply.set()
            competitor_done.wait(.25)
            return response()

        thread = threading.Thread(target=competitor, daemon=True)
        thread.start()
        server = SimpleNamespace(ctrl=self.ctrl, store=self.store, hooks={})
        try:
            with mock.patch.object(self.ctrl, "scene_response", side_effect=overlap):
                reply = api.route(server, "POST", "/api/scene", {"effect": self.effects[0]})
        finally:
            thread.join(5)
        self.assertTrue(competitor_done.is_set())
        self.assertEqual(reply["scene"]["effect"], self.effects[0])
        self.assertEqual(self.ctrl.base_scene["effect"], self.effects[1])

    def test_first_frame_is_held_until_persist_succeeds(self):
        before = self.engine.frames["d/z1"]
        self.ctrl.set_scene({"effect": self.effects[0]})
        self.supply()
        self.engine.tick()
        self.assertEqual(self.engine.frames["d/z1"], before)
        with mock.patch.object(self.store, "save_state", side_effect=OSError("disk full")):
            self.assertEqual(self.ctrl.scene_status()["status"], "failed")
        self.assertEqual(self.ctrl.base_scene["effect"], "solid")
        self.engine.tick()
        self.assertEqual(self.engine.frames["d/z1"], before)

    def test_failure_restores_preceding_scene(self):
        self.ctrl.set_scene({"effect": self.effects[0]})
        iid = next(iter(self.host.instances))
        self.host.frames({"problems": {iid: {"reason": "error", "message": "broken"}}})
        self.engine.tick()
        self.assertEqual(self.ctrl.scene_status()["status"], "failed")
        self.assertEqual(self.ctrl.base_scene["effect"], "solid")
        self.engine.tick()
        self.assertEqual(self.engine.frames["d/z1"], "ff0000" * 6)

    def test_required_instance_failure_between_render_and_readiness_cannot_commit(self):
        previous = self.store.state["scene"]
        self.ctrl.set_scene({"effect": self.effects[0]})
        iid = next(iter(self.host.instances))
        readiness = self.host.readiness

        def fail_before_readiness(*args):
            # The API host-frame handler can run after render's problem check releases the host lock.
            self.host.frames({"problems": {iid: {"reason": "error", "message": "first frame failed"}}})
            return readiness(*args)

        with mock.patch.object(self.host, "readiness", side_effect=fail_before_readiness):
            self.engine.tick()
        status = self.ctrl.scene_status()
        self.assertEqual(status["status"], "failed")
        self.assertIn("off", status["error"])
        self.assertFalse(status["undo_available"])
        self.assertEqual(self.store.state["scene"], previous)
        self.assertEqual(self.ctrl.base_scene, previous)
        self.engine.tick()
        self.assertEqual(self.engine.frames["d/z1"], "ff0000" * 6)

    def test_off_cancels_pending_and_bypasses_device_throttle(self):
        self.dev.max_fps = 1
        self.ctrl.set_scene({"effect": self.effects[0]})
        self.ctrl.set_scene({"on": False})
        self.engine.tick()
        self.assertEqual(self.engine.frames["d/z1"], "000000" * 6)
        self.assertEqual(self.dev.last("z1"), [(0, 0, 0)] * 6)
        self.assertEqual(self.host.instances, {})
        self.assertIsNone(self.ctrl._scene_pending)

    def test_explicit_off_wins_over_stale_guard_after_newer_selection(self):
        prior = self.ctrl.scene_status()
        self.ctrl.set_scene({"effect": self.effects[0]})
        self.ctrl.set_scene({"on": False, "expected_revision": prior["desired_revision"],
                             "expected_session": "previous-process"})
        self.engine.tick()
        self.assertEqual(self.engine.frames["d/z1"], "000000" * 6)
        self.assertIsNone(self.ctrl._scene_pending)
        with self.assertRaises(ValueError):
            self.ctrl.set_scene({"on": False, "brightness": 10, "expected_revision": prior["desired_revision"]})

    def test_off_does_not_persist_a_superseded_unready_effect(self):
        self.ctrl.set_scene({"effect": self.effects[0]})
        self.ctrl.set_scene({"on": False})
        self.assertEqual(self.store.state["scene"]["effect"], "solid")
        self.assertFalse(self.store.state["scene"]["on"])

    def test_snapshot_scene_and_revision_are_consistent_during_change(self):
        previous = self.ctrl.scene_status()
        fan_devices = self.ctrl._fan_devices

        def change_scene():
            self.ctrl.set_scene({"effect": self.effects[0]})
            return fan_devices()

        with mock.patch.object(self.ctrl, "_fan_devices", side_effect=change_scene):
            snapshot = self.ctrl.snapshot()
        self.assertEqual(snapshot["scene_status"]["desired_revision"], previous["desired_revision"])
        self.assertEqual(snapshot["scene"]["effect"], "solid")
        self.assertEqual(self.ctrl.base_scene["effect"], self.effects[0])

    def test_off_cancels_pending_even_when_disk_is_full(self):
        self.ctrl.set_scene({"effect": self.effects[0]})
        with mock.patch.object(self.store, "save_state", side_effect=OSError("disk full")):
            with self.assertRaises(OSError):
                self.ctrl.set_scene({"on": False})
        self.engine.tick()
        self.assertEqual(self.engine.frames["d/z1"], "000000" * 6)
        self.assertFalse(self.ctrl.base_scene["on"])
        self.assertIsNone(self.ctrl._scene_pending)
        self.assertEqual(self.ctrl.scene_status()["status"], "failed")

    def test_return_to_previous_scene_during_disk_failure_does_not_orphan_commit_gate(self):
        previous = self.ctrl.base_scene
        self.ctrl.set_scene({"effect": self.effects[0]})
        with mock.patch.object(self.store, "save_state", side_effect=OSError("disk full")):
            with self.assertRaises(OSError):
                self.ctrl.set_scene(previous)
        self.engine.tick()
        self.assertEqual(self.ctrl.base_scene, previous)
        self.assertIsNone(self.ctrl._scene_pending)
        self.assertTrue(self.engine._transition is None or self.engine._transition["committed"])
        self.assertEqual(self.engine.frames["d/z1"], "ff0000" * 6)

    def test_transition_retargets_displayed_color_and_zero_disables(self):
        self.engine.set_scene({"effect": "solid", "params": {"color": "#0000ff"}, "on": True}, transition_ms=350)
        self.engine.tick()
        self.engine._transition["ready_at"] -= .175
        self.engine.tick()
        midway = self.engine._frames[0]["d/z1"].copy()
        self.assertTrue(100 < midway[0, 0] < 150)
        self.engine.set_scene({"effect": "solid", "params": {"color": "#00ff00"}, "on": True}, transition_ms=350)
        self.engine.tick()
        np.testing.assert_array_equal(midway, self.engine._frames[0]["d/z1"])
        self.engine.set_scene({"effect": "solid", "params": {"color": "#00ff00"}, "on": True}, transition_ms=0)
        self.engine.tick()
        self.assertEqual(self.engine.frames["d/z1"], "00ff00" * 6)

    def test_undo_changes_only_lighting_and_invalidates_after_other_edit(self):
        self.ctrl.set_scene({"effect": "comet"})
        curve = self.ctrl.base_curve
        self.ctrl.undo_scene()
        self.assertEqual(self.ctrl.base_scene["effect"], "solid")
        self.assertEqual(self.ctrl.base_curve, curve)
        self.assertIsNone(self.ctrl.active_preset)
        self.store.update_settings({"fan_failsafe_duty": 85})
        with self.assertRaises(ValueError):
            self.ctrl.undo_scene()

    def test_stale_undo_cannot_undo_a_newer_selection(self):
        self.ctrl.set_scene({"effect": "comet"})
        old = self.ctrl.scene_status()
        self.ctrl.set_scene({"effect": "rainbow"})
        with self.assertRaises(ValueError):
            self.ctrl.undo_scene(old["desired_revision"], old["session"])
        self.assertEqual(self.ctrl.base_scene["effect"], "rainbow")
        with self.assertRaises(ValueError):
            self.ctrl.undo_scene(self.ctrl.scene_status()["desired_revision"], "previous-process")

    def test_geometry_change_uses_fresh_instance_identity(self):
        self.ctrl.set_scene({"effect": self.effects[0]})
        first = next(iter(self.host.instances))
        self.engine.invalidate("d/z1")
        self.engine.tick()
        self.assertNotEqual(first, next(iter(self.host.instances)))

    def test_zone_area_edit_preflight_uses_candidate_geometry_without_rebuilding_live_slices(self):
        devices = [FakeLightDevice(f"d{i}", f"Device {i}", zones=(("z1", 6),)) for i in range(1, 5)]
        self.manager.natives.update((device.id, device) for device in devices)
        self.manager.maintain(True)
        self.store.ensure_zones([d.info() for d in devices])
        for device, effect in zip(devices, self.effects):
            key = f"{device.id}/z1"
            self.store.set_zone(key, {**self.store.zones()[key], "override": {"effect": effect}}, device.zones[0].max_leds)
        key = "d/z1"
        top = self.store.set_zone(key, {**self.store.zones()[key], "component": {"spot": "top"}}, 12)
        self.ctrl.apply_preset({"id": "areas", "effect": self.effects[0], "params": {}, "brightness": 100,
                                "areas": {"front": {"effect": self.effects[4]}}})
        owned = self.engine._geometry(key, top["layout"], top)
        front = {**top, "component": {"spot": "front"}}
        with self.assertRaisesRegex(ValueError, "4 distinct"):
            self.engine.validate_zone_changes({key: front})
        self.assertIs(self.engine._geometry(key, top["layout"], top), owned)

    def test_automation_saves_inactive_selection_and_invalidates_undo(self):
        self.ctrl.set_scene({"effect": self.effects[0]})
        self.ctrl.automations.light_rule = {"id": "off", "then": {"type": "lights_off"}}
        self.ctrl.apply()
        self.assertEqual(self.store.state["scene"]["effect"], self.effects[0])
        self.assertTrue(self.ctrl.scene_status()["overridden"])
        self.assertIsNone(self.ctrl._scene_pending)
        self.engine.tick()
        self.assertEqual(self.engine.frames["d/z1"], "000000" * 6)
        self.ctrl.automations.light_rule = None
        self.ctrl.apply()
        self.assertFalse(self.ctrl.scene_status()["undo_available"])

    def test_identical_automation_takeover_does_not_leave_publication_gated(self):
        self.store.update_settings({"lighting_transition_ms": 0})
        self.ctrl.set_scene({"effect": self.effects[0], "brightness": 40})
        self.ctrl.automations.light_rule = {"id": "dim", "then": {"type": "dim", "brightness": 70}}
        self.ctrl.apply()
        self.supply()
        self.engine.tick()
        self.assertIsNone(self.ctrl._scene_pending)
        self.assertEqual(self.store.state["scene"]["effect"], self.effects[0])
        self.assertEqual(self.engine.frames["d/z1"], "000066" * 6)

    def test_shutdown_blacks_out_and_runs_failsafe_without_saving_pending_scene(self):
        self.ctrl.set_scene({"effect": self.effects[0]})
        with mock.patch.object(self.store, "save_state", side_effect=OSError("disk full")) as save, \
                mock.patch.object(self.ctrl, "failsafe") as failsafe:
            self.ctrl._on_shutdown()
        save.assert_not_called()
        failsafe.assert_called_once_with()
        self.assertEqual(self.dev.last("z1"), [(0, 0, 0)] * 6)
        self.assertEqual(self.ctrl.base_scene["effect"], "solid")
        self.assertIsNone(self.ctrl._scene_pending)

    def test_automation_applies_off_even_when_inactive_selection_cannot_be_saved(self):
        self.ctrl.set_scene({"effect": self.effects[0]})
        self.ctrl.automations.light_rule = {"id": "off", "then": {"type": "lights_off"}}
        with mock.patch.object(self.store, "save_state", side_effect=OSError("disk full")):
            self.ctrl.apply()
        self.engine.tick()
        self.assertEqual(self.engine.frames["d/z1"], "000000" * 6)
        self.assertEqual(self.ctrl.base_scene["effect"], "solid")
        self.assertIsNone(self.ctrl._scene_pending)
        self.assertIn("disk full", self.ctrl.scene_status()["error"])

    def test_timeout_restores_previous_scene_and_allows_explicit_retry(self):
        self.ctrl.set_scene({"effect": self.effects[0]})
        self.engine._transition["started"] -= 16
        self.engine.tick()
        self.assertEqual(self.ctrl.scene_status()["status"], "failed")
        self.assertEqual(self.ctrl.base_scene["effect"], "solid")
        self.ctrl.set_scene({"retry": True})
        self.assertEqual(self.ctrl.scene_status()["status"], "ready")

    def test_failed_restore_holds_last_frame_without_infinite_retry(self):
        self.store.update_settings({"lighting_transition_ms": 0})
        self.ctrl.set_scene({"effect": self.effects[0]})
        self.supply()
        self.engine.tick()
        self.ctrl.scene_status()
        self.engine.tick()
        before = self.engine.frames["d/z1"]
        self.ctrl.set_scene({"effect": self.effects[1]})
        iid = next(iter(self.host.instances))
        self.host.frames({"problems": {iid: {"reason": "error"}}})
        self.engine.tick()
        self.ctrl.scene_status()
        iid = next(iter(self.host.instances))
        self.host.frames({"problems": {iid: {"reason": "error"}}})
        self.engine.tick()
        self.assertIn("previous scene also", self.ctrl.scene_status()["error"])
        seq = self.host._seq
        for _ in range(5):
            self.engine.tick()
        self.assertEqual(self.host._seq, seq)
        self.assertEqual(self.engine.frames["d/z1"], before)

    def test_reconcile_leaves_crash_attribution_and_backoff_to_the_watcher(self):
        self.host.url = "http://localhost"
        proc = mock.Mock(pid=1234, returncode=1)
        proc.poll.return_value = None
        with mock.patch.object(self.host, "_spawn", return_value=proc) as spawn, \
                mock.patch.object(studio, "_tree_memory", return_value=0):
            self.ctrl.set_scene({"effect": self.effects[0]})
            self.engine.tick()
            proc.poll.return_value = 1
            self.engine.tick()
        self.assertEqual(spawn.call_count, 1)
        self.assertEqual(len(self.host._crash_times), 1)
        self.assertEqual(self.host.problems[self.effects[0]]["reason"], "crashed")

    def test_profile_cooling_waits_for_lighting_readiness(self):
        before = self.ctrl.base_curve
        self.ctrl.apply_preset({"id": "candidate", "name": "Candidate", "effect": self.effects[0],
                                "params": {}, "brightness": 100, "fan_curve": "silent"})
        self.assertEqual(self.ctrl.base_curve, before)
        self.supply()
        self.engine.tick()
        self.ctrl.scene_status()
        self.assertEqual(self.ctrl.base_curve, "silent")

    def test_use_saved_studio_effect_replaces_live_draft_and_persists_on_readiness(self):
        previous, draft, saved = self.effects[:3]
        self.ctrl.set_scene({"effect": previous, "brightness": 65})
        self.supply()
        self.engine.tick()
        self.assertEqual(self.ctrl.scene_status()["status"], "ready")
        self.ctrl.try_profile({"effect": draft, "brightness": 25}, "Live draft", kind="studio")
        self.engine.tick()
        with mock.patch.object(studioapi, "_lib", return_value=self.lib):
            reply = studioapi._route(self.ctrl, {}, "POST", "/api/studio/use", {"fx_id": saved}, {})
        self.assertEqual(reply["scene_status"]["status"], "pending")
        self.assertEqual(self.store.state["scene"]["effect"], previous)
        self.engine.tick()
        self.supply()
        self.engine.tick()
        self.assertEqual(self.ctrl.scene_status()["status"], "ready")
        self.assertIsNone(self.ctrl.trial)
        self.assertEqual(self.store.state["scene"]["effect"], saved)
        self.assertEqual(self.store.state["scene"]["brightness"], 65)

    def test_cooling_edit_during_pending_scene_is_immediate_and_survives_failure(self):
        self.ctrl.set_scene({"effect": self.effects[0]})
        self.ctrl.set_fan_curve("silent")
        self.assertEqual(self.ctrl.effective_curve, "silent")
        iid = next(iter(self.host.instances))
        self.host.frames({"problems": {iid: {"reason": "error"}}})
        self.engine.tick()
        self.ctrl.scene_status()
        self.assertEqual(self.ctrl.base_curve, "silent")
        self.assertEqual(self.store.state["fan_curve"], "silent")
        self.assertEqual(self.ctrl.base_scene["effect"], "solid")

    def test_four_real_instances_survive_zone_iteration_and_fifth_rejects(self):
        requirements = []
        for i in range(5):
            cfg = {**self.store.zones()["d/z1"]}
            z = self.engine._geometry(f"fake/{i}", cfg["layout"], cfg)
            eff = studio._provide(self.effects[i])
            requirements.append((eff, {}, z))
        self.host.reconcile(requirements[:4])
        ids = set(self.host.instances)
        for eff, params, z in requirements[:4]:
            self.host.render(eff, z, 0, params)
        self.host.reconcile(list(reversed(requirements[:4])))
        self.assertEqual(set(self.host.instances), ids)
        with self.assertRaises(ValueError):
            self.host.reconcile(requirements)
        self.assertEqual(set(self.host.instances), ids)

    def test_reconnecting_fifth_studio_requirement_keeps_existing_zones_running(self):
        late = self.dev
        devices = [FakeLightDevice(f"d{i}", f"Device {i}", zones=(("z1", 6),)) for i in range(1, 5)]
        self.manager.natives.update((device.id, device) for device in devices)
        self.manager.maintain(True)
        self.store.ensure_zones([d.info() for d in devices])
        for device, effect in zip([late, *devices], self.effects):
            key = f"{device.id}/z1"
            self.store.set_zone(key, {**self.store.zones()[key], "override": {"effect": effect}}, device.zones[0].max_leds)
        late.close()
        self.engine.tick()
        self.supply()
        self.engine.tick()
        self.assertEqual(len(self.host.instances), 4)
        active = set(self.host.instances)
        late.open()
        self.supply((0, 255, 0))
        self.engine.tick()
        self.assertEqual(set(self.host.instances), active)
        for device in devices:
            self.assertEqual(device.last("z1"), [(0, 255, 0)] * 6)
        self.assertEqual(late.last("z1"), [(0, 0, 0)] * 6)
        self.assertIn("four", self.ctrl.scene_status()["error"].lower())
        late.close()
        self.engine.tick()
        self.assertIsNone(self.ctrl.scene_status()["error"])

    def test_studio_scene_with_too_many_leds_for_one_frame_is_rejected_before_rendering(self):
        extra = FakeLightDevice("extra", "Extra", zones=(("z1", 6),))
        self.manager.natives[extra.id] = extra
        self.manager.maintain(True)
        self.store.ensure_zones([extra.info()])
        with mock.patch.object(studio, "MAX_INSTANCE_LEDS", 10, create=True):
            with self.assertRaisesRegex(ValueError, "10 LEDs"):
                self.ctrl.set_scene({"effect": self.effects[0]})
        self.assertEqual(self.host.instances, {})
        self.assertEqual(self.ctrl.base_scene["effect"], "solid")

    def test_four_maximum_studio_frames_fit_in_one_api_request(self):
        points = [None] * studio.MAX_INSTANCE_LEDS
        colors = bytes((17, 34, 51)) * len(points)
        segments = []
        for number in range(studio.MAX_INSTANCES):
            iid = str(number) * 255
            self.host.instances[iid] = SimpleNamespace(points=points, colors=None, gen=0)
            segments.append(b"\xff" + iid.encode("ascii") + len(colors).to_bytes(2, "little") + colors)
        body = b"".join(segments)
        handler = api.Handler.__new__(api.Handler)
        handler.headers = {"Content-Length": str(len(body)), "X-Airglow-Token": self.host.token}
        handler.rfile = io.BytesIO(body)
        self.assertEqual(handler._host_frames(self.host), {"ok": True})
        self.assertTrue(all(inst.colors == colors for inst in self.host.instances.values()))

    def test_reconnecting_a_large_zone_does_not_overflow_existing_studio_frame(self):
        late = self.dev
        late.close()
        keep = FakeLightDevice("keep", "Keep", zones=(("z1", 6),))
        self.manager.natives[keep.id] = keep
        keep.open()
        self.store.ensure_zones([keep.info()])
        self.store.update_settings({"lighting_transition_ms": 0})
        with mock.patch.object(studio, "MAX_INSTANCE_LEDS", 10, create=True):
            self.ctrl.set_scene({"effect": self.effects[0]})
            self.supply()
            self.engine.tick()
            self.ctrl.scene_status()
            active = set(self.host.instances)
            late.open()
            self.supply((0, 255, 0))
            self.engine.tick()
            self.assertIn("10 LEDs", self.ctrl.scene_status()["error"])
        self.assertEqual(set(self.host.instances), active)
        self.assertEqual(keep.last("z1"), [(0, 255, 0)] * 6)
        self.assertEqual(late.last("z1"), [(0, 0, 0)] * 6)
