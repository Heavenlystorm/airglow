"""Portable rig migration uses two isolated rigs and never takes over cooling or activates rules."""

import copy
import importlib
import json
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

from airglow import api, config
from airglow.automation import validate_rule
from airglow.controller import Controller
from tests.fakes import FakeLightDevice, FakeManager, signals


class RigMigrationTests(unittest.TestCase):
    def setUp(self):
        self.assertIsNotNone(importlib.util.find_spec("airglow.rigbundle"), "rig bundle migration is missing")
        self.source = self.rig("source", (("front", 8), ("top", 4)))
        self.target = self.rig("target", (("main", 8), ("short", 2)))
        source = self.source.store
        zone = source.zones()["source/front"]
        source.set_zone("source/front", {**zone, "name": "Front strip", "enabled": True,
                                        "component": {"id": "generic-strip", "count": 1, "spot": "front"}}, 16)
        source.set_curve("silent", [[40, 40], [88, 100]])
        source.update_settings({"design": "paper", "temp_unit": "F"})
        source.set_rules([{"id": "night-time", "enabled": True, "when": {"type": "locked"},
                           "then": {"type": "preset", "preset": "silent"}},
                          {"id": "warm", "when": {"type": "temp_above", "temp": 60},
                           "then": {"type": "fan_curve", "curve": "silent"}}], validate_rule)
        target = self.target.store
        zone = target.zones()["target/main"]
        target.set_zone("target/main", {**zone, "enabled": False}, 16)
        target.set_fan_channel("/fan/0/control/0", {"mode": "steady", "duty": 67})
        target.update_settings({"fan_failsafe_duty": 55, "fan_max_step": 3})
        self.bundle = self.call(self.source, "GET", "/api/rig/export")
        self.mapping = {"source/front": "target/main", "source/top": None}

    def rig(self, name, zones):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        store = config.Store(tmp.name)
        device = FakeLightDevice(name, name.title(), zones=zones)
        store.ensure_zones([device.info()])
        ctrl = Controller(store, manager=FakeManager(device), signals=signals, read_sensors=dict, async_writes=False)
        return SimpleNamespace(store=store, ctrl=ctrl, hooks={}, updater=None)

    def call(self, server, method, path, body=None):
        return api.route(server, method, path, body)

    def preview(self, mapping=None, bundle=None):
        return self.call(self.target, "POST", "/api/rig/preview",
                         {"bundle": bundle or self.bundle, "mapping": mapping if mapping is not None else {}})

    def apply(self, preview, mapping=None, bundle=None):
        return self.call(self.target, "POST", "/api/rig/apply",
                         {"bundle": bundle or self.bundle, "mapping": self.mapping if mapping is None else mapping,
                          "preview_token": preview.get("preview_token")})

    def files(self):
        return {p.name: p.read_bytes() for p in self.target.store.dir.glob("*.json")}

    def test_export_is_versioned_and_excludes_ownership_and_helper_details(self):
        self.assertEqual((self.bundle["format"], self.bundle["version"]), ("airglow-rig", 1))
        self.assertLess(len(json.dumps(self.bundle, ensure_ascii=False).encode()), 192 * 1024)
        self.assertEqual(set(self.bundle["settings"]), {"design", "temp_unit", "energy_price"})
        text = json.dumps(self.bundle)
        for private in ("fan_channels", "fancontrol_exe", "fan_profile", "lhm_url", "device_ids", "active_preset",
                        "fan_failsafe_duty", "fan_max_step", "start_with_windows"):
            self.assertNotIn(private, text)
        self.assertNotIn("enabled", self.bundle["zones"][0]["config"])

    def test_preview_requires_an_explicit_choice_for_each_zone_and_writes_nothing(self):
        files = self.files()
        result = self.preview()
        self.assertFalse(result["ready"])
        self.assertEqual(set(result["unresolved"]), {"source/front", "source/top"})
        self.assertIsNone(result["preview_token"])
        self.assertEqual(self.files(), files)
        with self.assertRaisesRegex(ValueError, "mapping|preview"):
            self.apply(result, mapping={})

    def test_import_maps_second_rig_and_remaps_references_without_activating_cooling(self):
        before = copy.deepcopy(self.target.store.config)
        state = copy.deepcopy(self.target.store.state)
        live = self.target.ctrl.base_scene, self.target.ctrl.base_curve, self.target.ctrl.active_preset
        result = self.preview(self.mapping)
        self.assertTrue(result["ready"])
        self.assertEqual(result["skipped"], ["source/top"])
        self.assertIn({"key": "design", "before": "graphite", "after": "paper"}, result["changes"]["settings"])
        result = self.apply(result)
        self.assertTrue(result["applied"])
        again = config.Store(self.target.store.dir)
        mapped = again.zones()["target/main"]
        self.assertEqual((mapped["name"], mapped["component"]["spot"], mapped["enabled"]),
                         ("Front strip", "front", False))
        self.assertNotIn("source/front", again.zones())
        self.assertEqual(again.fan_channels(), before["fan_channels"])
        for key in ("fan_failsafe_duty", "fan_max_step", "temp_source", "device_ids", "device_params",
                    "fancontrol_exe", "fancontrol_configs_dir", "lhm_url", "openrgb_enabled"):
            self.assertEqual(again.config[key], before[key])
        self.assertEqual(again.state, state)
        self.assertEqual((self.target.ctrl.base_scene, self.target.ctrl.base_curve, self.target.ctrl.active_preset), live)
        self.assertEqual(again.curves()["silent"], config.DEFAULT_CURVES["silent"])
        new_id = result["id_map"]["profiles"]["silent"]
        new_curve = result["id_map"]["curves"]["silent"]
        self.assertNotEqual(new_id, "silent")
        self.assertNotEqual(new_curve, "silent")
        imported = next(p for p in again.get_presets() if p["id"] == new_id)
        self.assertEqual(imported["fan_curve"], new_curve)
        self.assertTrue(all(not r["enabled"] for r in again.get_rules()))
        self.assertEqual(again.get_rules()[0]["then"]["preset"], new_id)
        self.assertEqual(again.get_rules()[1]["then"]["curve"], new_curve)
        self.assertEqual(self.target.ctrl.manager.get("target").writes, [])

    def test_duplicate_and_too_small_targets_fail_before_any_write(self):
        files = self.files()
        for mapping in ({"source/front": "target/main", "source/top": "target/main"},
                        {"source/front": "target/short", "source/top": None}):
            with self.subTest(mapping=mapping), self.assertRaises(ValueError):
                self.preview(mapping)
            self.assertEqual(self.files(), files)

    def test_import_cannot_reactivate_existing_rules_with_dangling_references(self):
        self.target.store.set_rules([
            {"id": "waiting-profile", "when": {"type": "locked"},
             "then": {"type": "preset", "preset": "silent-2"}},
            {"id": "waiting-curve", "when": {"type": "locked"},
             "then": {"type": "fan_curve", "curve": "silent-2"}},
        ], validate_rule)
        result = self.preview(self.mapping)
        self.assertEqual(result["id_map"]["profiles"]["silent"], "silent-3")
        self.assertEqual(result["id_map"]["curves"]["silent"], "silent-3")
        self.apply(result)
        self.assertTrue(self.target.store.get_rules()[0]["enabled"])
        self.assertEqual(self.target.store.get_rules()[0]["then"]["preset"], "silent-2")

    def test_imported_curve_can_be_edited_without_a_factory_reset(self):
        result = self.apply(self.preview(self.mapping))
        curve = result["id_map"]["curves"]["silent"]
        response = self.call(self.target, "POST", f"/api/curves/{curve}", {"points": [[40, 45], [88, 100]]})
        self.assertEqual(response["points"], [[40, 45], [88, 100]])
        with self.assertRaisesRegex(ValueError, "no factory default"):
            self.call(self.target, "POST", f"/api/curves/{curve}/reset", {})

    def test_later_write_failure_rolls_back_every_file_and_keeps_memory(self):
        before = copy.deepcopy(self.target.store.config), self.target.store.get_presets(), self.target.store.get_rules()
        files, revision = self.files(), self.target.store.zones_rev
        preview = self.preview(self.mapping)
        write = self.target.store._write

        def fail_rules(name, value):
            if name == "automations.json":
                raise OSError("rules locked")
            return write(name, value)

        with mock.patch.object(self.target.store, "_write", side_effect=fail_rules):
            with self.assertRaisesRegex(OSError, "rules locked"):
                self.apply(preview)
        self.assertEqual(self.files(), files)
        self.assertEqual((self.target.store.config, self.target.store.get_presets(), self.target.store.get_rules()), before)
        self.assertEqual(self.target.store.zones_rev, revision)

    def test_stale_preview_is_rejected_instead_of_overwriting_a_newer_edit(self):
        result = self.preview(self.mapping)
        self.target.store.update_settings({"design": "amber"})
        with self.assertRaisesRegex(ValueError, "changed|preview"):
            self.apply(result)
        self.assertEqual(self.target.store.settings()["design"], "amber")

    def test_protected_target_or_source_files_are_not_overwritten_or_exported(self):
        files = self.files()
        result = self.preview(self.mapping)
        self.target.store._protected_files.add("automations.json")
        with self.assertRaisesRegex(ValueError, "read|restart|Restart"):
            self.apply(result)
        self.assertEqual(self.files(), files)
        self.source.store._protected_files.add("presets.json")
        with self.assertRaisesRegex(ValueError, "read|restart|Restart"):
            self.call(self.source, "GET", "/api/rig/export")

    def test_unknown_versions_oversized_and_unsafe_content_are_refused(self):
        files = self.files()
        invalid = []
        version = copy.deepcopy(self.bundle)
        version["version"] = 2
        invalid.append(version)
        huge = copy.deepcopy(self.bundle)
        huge["notes"] = "x" * (192 * 1024)
        invalid.append(huge)
        unsafe = copy.deepcopy(self.bundle)
        unsafe["settings"]["fan_failsafe_duty"] = 100
        invalid.append(unsafe)
        reference = copy.deepcopy(self.bundle)
        reference["automations"][0]["then"]["preset"] = "missing"
        invalid.append(reference)
        for bundle in invalid:
            with self.subTest(bundle_keys=list(bundle)), self.assertRaises(ValueError):
                self.preview(self.mapping, bundle)
        self.assertEqual(self.files(), files)


if __name__ == "__main__":
    unittest.main()
