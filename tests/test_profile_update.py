"""Saving the current look must update the selected identity or fail without a partial save."""

import copy
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

from airglow import api, config
from airglow.automation import validate_rule
from airglow.controller import Controller
from tests.fakes import FakeLightDevice, FakeManager, signals


class ProfileUpdateTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.store = config.Store(self.tmp.name)
        self.device = FakeLightDevice("original", "Original strip")
        self.store.ensure_zones([self.device.info()])
        self.ctrl = Controller(self.store, manager=FakeManager(self.device), signals=signals,
                               read_sensors=dict, async_writes=False)
        self.server = SimpleNamespace(ctrl=self.ctrl, store=self.store, hooks={})

    def save(self, **body):
        return api.route(self.server, "POST", "/api/presets/from-current", body)

    def test_update_retains_identity_and_automation_reference_after_adjustment(self):
        self.store.set_rules([{"id": "night-time", "when": {"type": "locked"},
                               "then": {"type": "preset", "preset": "night"}}], validate_rule)
        self.ctrl.apply_preset(next(p for p in self.store.get_presets() if p["id"] == "night"))
        self.ctrl.set_scene({"brightness": 42})
        self.assertIsNone(self.ctrl.active_preset)
        result = self.save(id="night", name="Evening", include_fans=False)
        self.assertEqual(result["id"], "night")
        self.assertEqual(len(result["presets"]), 4)
        saved = next(p for p in result["presets"] if p["id"] == "night")
        self.assertEqual((saved["name"], saved["brightness"], saved["fan_curve"]), ("Evening", 42, None))
        again = config.Store(self.tmp.name)
        self.assertEqual(again.get_rules()[0]["then"]["preset"], "night")
        self.assertEqual(again.state["active_preset"], "night")

    def test_failed_selection_save_restores_profiles_and_live_selection(self):
        before = self.store.get_presets(), copy.deepcopy(self.store.state), self.ctrl.active_preset
        files = {p.name: p.read_bytes() for p in Path(self.tmp.name).glob("*.json")}
        write = self.store._write

        def fail_selection(name, value):
            if name == "state.json":
                raise OSError("selection locked")
            return write(name, value)

        with mock.patch.object(self.store, "_write", side_effect=fail_selection):
            with self.assertRaisesRegex(OSError, "selection locked"):
                self.save(name="Unsaved", include_fans=False)
        self.assertEqual((self.store.get_presets(), self.store.state, self.ctrl.active_preset), before)
        self.assertEqual({p.name: p.read_bytes() for p in Path(self.tmp.name).glob("*.json")}, files)

    def test_explicit_area_scope_can_exclude_zone_overrides(self):
        key = "original/z1"
        self.store.set_zone(key, {**self.store.zones()[key], "enabled": True,
                                 "override": {"effect": "solid", "params": {"color": "#123456"}}}, 16)
        result = self.save(name="Global only", include_areas=False, include_fans=False)
        saved = next(p for p in result["presets"] if p["id"] == result["id"])
        self.assertNotIn("areas", saved)
        self.assertEqual(self.store.zones()[key]["override"]["params"]["color"], "#123456")

    def test_rollback_keeps_original_line_endings_and_formatting(self):
        path = self.store.dir / "presets.json"
        original = path.read_bytes().replace(b"\n", b"\r\n") + b"\r\n"
        path.write_bytes(original)
        write = self.store._write

        def fail_selection(name, value):
            if name == "state.json":
                raise OSError("locked")
            return write(name, value)

        with mock.patch.object(self.store, "_write", side_effect=fail_selection):
            with self.assertRaises(OSError):
                self.save(name="No partial write")
        self.assertEqual(path.read_bytes(), original)

    def test_update_unknown_id_never_creates_a_replacement(self):
        before = self.store.get_presets()
        with self.assertRaisesRegex(ValueError, "no longer exists"):
            self.save(id="removed", name="No duplicate")
        self.assertEqual(self.store.get_presets(), before)

    def test_update_including_fans_retains_its_existing_fancontrol_assignment(self):
        presets = self.store.get_presets()
        presets[0]["fan_profile"] = "Existing.json"
        self.store.set_presets(presets)
        result = self.save(id="silent", name="Silent adjusted", include_fans=True)
        self.assertEqual(result["presets"][0]["fan_profile"], "Existing.json")
        result = self.save(id="silent", name="Lighting only", include_fans=False)
        self.assertIsNone(result["presets"][0]["fan_profile"])

    def test_keeping_an_installed_trial_clears_overrides_only_after_state_is_saved(self):
        method = getattr(self.store, "save_trial_state", None)
        self.assertTrue(callable(method), "compound trial state persistence is missing")
        key = "original/z1"
        override = {"effect": "solid", "params": {"color": "#123456"}}
        self.store.set_zone(key, {**self.store.zones()[key], "override": override}, 16)
        before, revision = copy.deepcopy(self.store.state), self.store.zones_rev
        write = self.store._write

        def fail_selection(name, value):
            if name == "state.json":
                raise OSError("selection locked")
            return write(name, value)

        with mock.patch.object(self.store, "_write", side_effect=fail_selection):
            with self.assertRaisesRegex(OSError, "selection locked"):
                method({"active_preset": "night"}, [key])
        self.assertEqual(self.store.state, before)
        self.assertEqual(self.store.zones_rev, revision)
        self.assertEqual(config.Store(self.tmp.name).zones()[key]["override"], override)
        method({"active_preset": "night"}, [key])
        self.assertIsNone(self.store.zones()[key]["override"])
        self.assertEqual(config.Store(self.tmp.name).state["active_preset"], "night")

    def own_profile(self, *, named=None):
        profile = {"id": "own", "name": "Own", "effect": "solid", "params": {"color": "#123456"},
                   "brightness": 75, "fan_curve": named, "fan_profile": None}
        if named is None:
            profile["fan_points"] = [[30, 75], [88, 100]]
        self.store.set_presets([profile])
        return self.store.get_presets()[0]

    def run_profile_automation(self):
        self.store.set_rules([{"id": "current-cooling", "when": {"type": "locked"},
                               "then": {"type": "preset", "preset": "own"}}], validate_rule)
        self.ctrl.automations._signals = lambda: signals(locked=True)
        self.ctrl.automations.evaluate()
        self.ctrl.apply()
        self.assertEqual(self.ctrl.automations.fan_rule["id"], "current-cooling")

    def test_removing_owned_fans_keeps_manual_cooling_independently_across_restart(self):
        profile = self.own_profile()
        self.ctrl.apply_preset(profile)
        result = self.save(id="own", name="Lighting only", include_fans=False)
        self.assertNotIn("fan_points", result["presets"][0])
        self.assertIsNone(result["presets"][0]["fan_curve"])
        self.assertEqual(self.ctrl.curve_points(self.ctrl.base_curve), [[30, 75], [88, 100]])
        self.assertFalse(self.ctrl.base_curve.startswith("profile:"))
        self.assertEqual(result["retained_curve"], {"id": self.ctrl.base_curve, "points": [[30, 75], [88, 100]]})
        self.ctrl.apply()
        self.assertEqual(self.ctrl.curve_points(self.ctrl.effective_curve), [[30, 75], [88, 100]])
        again = config.Store(self.tmp.name)
        restarted = Controller(again, manager=FakeManager(), signals=signals, read_sensors=dict, async_writes=False)
        self.assertEqual(restarted.curve_points(restarted.effective_curve), [[30, 75], [88, 100]])
        self.assertEqual(again.state["fan_curve"], self.ctrl.base_curve)

    def test_failed_detachment_restores_all_files_and_live_curve_references(self):
        self.ctrl.apply_preset(self.own_profile())
        self.ctrl.set_scene({"brightness": 40})
        files = {p.name: p.read_bytes() for p in self.store.dir.glob("*.json")}
        before = copy.deepcopy(self.store.config), self.store.get_presets(), copy.deepcopy(self.store.state)
        live = self.ctrl.base_curve, self.ctrl.effective_curve, self.ctrl.active_preset
        write = self.store._write
        for failure in ("presets.json", "state.json"):
            with self.subTest(failure=failure):
                def fail(name, data):
                    if name == failure:
                        raise OSError("later write locked")
                    return write(name, data)
                with mock.patch.object(self.store, "_write", side_effect=fail):
                    with self.assertRaisesRegex(OSError, "later write locked"):
                        self.save(id="own", name="Lighting only", include_fans=False)
                self.assertEqual((self.store.config, self.store.get_presets(), self.store.state), before)
                self.assertEqual((self.ctrl.base_curve, self.ctrl.effective_curve, self.ctrl.active_preset), live)
                self.assertEqual({p.name: p.read_bytes() for p in self.store.dir.glob("*.json")}, files)
                self.assertEqual(self.ctrl.curve_points(self.ctrl.effective_curve), [[30, 75], [88, 100]])

    def test_active_owned_or_named_automation_refuses_removed_fans_before_writes(self):
        for named in (None, "performance"):
            with self.subTest(named=named):
                self.own_profile(named=named)
                self.run_profile_automation()
                points = copy.deepcopy(self.ctrl.curve_points(self.ctrl.effective_curve))
                files = {p.name: p.read_bytes() for p in self.store.dir.glob("*.json")}
                before = self.store.get_presets(), copy.deepcopy(self.store.state), self.ctrl.active_preset
                with self.assertRaisesRegex(ValueError, "current-cooling.*[Pp]ause"):
                    self.save(id="own", name="Lighting only", include_fans=False)
                self.assertEqual((self.store.get_presets(), self.store.state, self.ctrl.active_preset), before)
                self.assertEqual({p.name: p.read_bytes() for p in self.store.dir.glob("*.json")}, files)
                self.ctrl.automations.evaluate()
                self.ctrl.apply()
                self.assertEqual(self.ctrl.curve_points(self.ctrl.effective_curve), points)
                again = config.Store(self.tmp.name)
                restarted = Controller(again, manager=FakeManager(), signals=lambda: signals(locked=True),
                                       read_sensors=dict, async_writes=False)
                restarted.automations.evaluate()
                restarted.apply()
                self.assertEqual(restarted.curve_points(restarted.effective_curve), points)

    def test_active_automation_refuses_replacing_its_curve_with_the_manual_base(self):
        self.own_profile(named="performance")
        self.run_profile_automation()
        self.assertEqual(self.ctrl.base_curve, "balanced")
        with self.assertRaisesRegex(ValueError, "current-cooling.*[Pp]ause"):
            self.save(id="own", name="Still performance", include_fans=True)
        self.assertEqual(self.store.get_presets()[0]["fan_curve"], "performance")

    def test_active_automation_allows_lighting_update_when_fan_points_are_equivalent(self):
        for named in (None, "performance"):
            with self.subTest(named=named):
                profile = self.own_profile(named=named)
                self.ctrl.apply_preset(profile)
                self.run_profile_automation()
                points = copy.deepcopy(self.ctrl.curve_points(self.ctrl.effective_curve))
                self.ctrl.set_scene({"brightness": 51})
                result = self.save(id="own", name="New lighting", include_fans=True)
                self.assertEqual(result["presets"][0]["brightness"], 51)
                self.ctrl.automations.evaluate()
                self.ctrl.apply()
                self.assertEqual(self.ctrl.curve_points(self.ctrl.effective_curve), points)

    def test_equivalent_automation_curve_identity_change_keeps_immediate_resolution_valid(self):
        profile = self.own_profile()
        profile["fan_points"] = copy.deepcopy(config.DEFAULT_CURVES["balanced"])
        self.store.set_presets([profile])
        self.run_profile_automation()
        self.assertEqual(self.ctrl.effective_curve, "profile:own")
        self.assertEqual(self.ctrl.base_curve, "balanced")
        result = self.save(id="own", name="Same cooling", include_fans=True)
        self.assertEqual(result["presets"][0]["fan_curve"], "balanced")
        self.assertEqual(self.ctrl.curve_points(self.ctrl.effective_curve), config.DEFAULT_CURVES["balanced"])
        self.assertEqual(self.ctrl.effective_curve, "balanced")

    def test_detached_curve_does_not_satisfy_a_dangling_automation_reference(self):
        self.ctrl.apply_preset(self.own_profile())
        self.store.set_rules([{"id": "unresolved", "when": {"type": "locked"},
                               "then": {"type": "fan_curve", "curve": "own-saved"}}], validate_rule)
        self.save(id="own", name="Lighting only", include_fans=False)
        self.assertNotEqual(self.ctrl.base_curve, "own-saved")
        self.assertNotIn("own-saved", self.store.curves())
        self.assertEqual(self.ctrl.curve_points(self.ctrl.base_curve), [[30, 75], [88, 100]])

    def test_detachment_keeps_the_previous_curve_when_an_existing_trial_is_discarded(self):
        self.ctrl.apply_preset(self.own_profile())
        self.ctrl.try_profile(config.DEFAULT_SCENE, "Preview", fan_points=[[30, 55], [88, 100]])
        self.save(id="own", name="Lighting only", include_fans=False)
        self.assertEqual(self.ctrl.curve_points(self.ctrl.effective_curve), [[30, 55], [88, 100]])
        self.ctrl.end_trial()
        self.assertEqual(self.ctrl.curve_points(self.ctrl.effective_curve), [[30, 75], [88, 100]])
        again = config.Store(self.tmp.name)
        self.assertEqual(again.curves()[again.state["fan_curve"]], [[30, 75], [88, 100]])


if __name__ == "__main__":
    unittest.main()
