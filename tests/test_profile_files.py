import tempfile
import unittest
from concurrent.futures import ThreadPoolExecutor
from threading import RLock
from types import SimpleNamespace
from unittest.mock import Mock

from airglow import api, config, profileapi
from airglow.api import ApiError
from tests.test_profiles import NEON


class ProfileFileTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.store = config.Store(temp.name)
        self.ctrl = Mock()
        self.server = SimpleNamespace(store=self.store, ctrl=self.ctrl, hooks={})

    def call(self, verb, **body):
        return profileapi.route(self.server, "POST", "/api/profiles/" + verb, body)

    def test_import_copies_a_profile_locally_without_selecting_it_and_can_add_paused_rules(self):
        initial = self.store.state.copy()
        first = self.call("import", profile=NEON, include_fans=True, include_automations=True)
        p = next(p for p in first["presets"] if p["id"] == first["id"])
        rules = self.store.get_rules()
        self.assertTrue(p["fan_points"])
        self.assertGreater(first["paused_rules"], 0)
        self.assertTrue(all(not r["enabled"] for r in rules if r["id"].startswith(first["id"])))
        self.assertEqual(initial, self.store.state)
        self.assertEqual(self.ctrl.mock_calls, [])
        exported = self.call("export", id=first["id"])
        self.assertEqual(exported["profile"]["fans"], NEON["fans"])
        self.assertNotIn("automations", exported["profile"])
        self.assertEqual(self.call("inspect-file", profile=exported["profile"]), exported["profile"])
        self.assertEqual(exported["filename"], exported["profile"]["id"] + ".airglow-profile.json")
        clean = self.call("import", profile=exported["profile"])
        self.assertNotEqual(clean["id"], first["id"])
        self.assertFalse(next(p for p in clean["presets"] if p["id"] == clean["id"]).get("fan_points"))

    def test_simultaneous_imports_keep_both_copies_with_distinct_identity(self):
        with ThreadPoolExecutor(max_workers=2) as pool:
            ids = list(pool.map(lambda _: self.call("import", profile=NEON)["id"], range(2)))
        self.assertEqual(len(set(ids)), 2)
        self.assertTrue(set(ids) <= {p["id"] for p in self.store.get_presets()})

    def test_export_checks_the_reviewed_profile_and_linked_rules(self):
        result = self.call("import", profile=NEON, include_automations=True)
        pid = result["id"]
        preset = next(p for p in result["presets"] if p["id"] == pid)
        linked = [r for r in result["rules"] if r["then"].get("preset") == pid]
        self.assertTrue(linked)
        for changed in ({"reviewed": {**preset, "brightness": -1}},
                        {"include_automations": True, "reviewed_rules": []}):
            with self.subTest(changed=changed), self.assertRaises(ApiError) as conflict:
                self.call("export", id=pid, **changed)
            self.assertEqual(conflict.exception.status, 409)
        exported = self.call("export", id=pid, reviewed=preset, include_automations=True, reviewed_rules=linked)
        self.assertEqual(len(exported["profile"]["automations"]), len(linked))
        self.assertNotIn("automations", self.call("export", id=pid, reviewed_rules=[])["profile"])

    def test_export_reports_portable_rule_limit_without_losing_the_remaining_rules(self):
        result = self.call("import", profile=NEON, include_automations=True)
        pid = result["id"]
        rule = next(r for r in result["rules"] if r["then"].get("preset") == pid)
        api.route(self.server, "POST", "/api/automations", {"rules": [{**rule, "id": f"linked-{i}"} for i in range(11)]})
        with self.assertRaisesRegex(ApiError, "11 linked rules") as caught:
            self.call("export", id=pid, include_automations=True)
        self.assertEqual(caught.exception.status, 400)
        self.assertEqual(len(self.store.get_rules()), 11)
        self.assertNotIn("automations", self.call("export", id=pid)["profile"])

    def test_apply_and_export_check_reviewed_named_curve_points_including_missing(self):
        preset = next(p for p in self.store.get_presets() if p["id"] == "silent")
        name = preset["fan_curve"]
        old = self.store.curves()[name]
        changed = [[old[0][0], old[0][1] + 1], *old[1:]]
        self.ctrl._lock = RLock()
        self.ctrl.apply_preset.return_value = [{"step": "Lighting", "ok": True}]
        self.ctrl.scene_status.return_value = {}

        def apply(**check):
            return profileapi.route(self.server, "POST", "/api/presets/silent/apply", {"reviewed": preset, **check})

        def export(**check):
            return self.call("export", id=preset["id"], reviewed=preset, **check)["profile"]

        def assert_conflict(expected):
            for action in (apply, export):
                with self.subTest(action=action.__name__, expected=expected), self.assertRaises(ApiError) as caught:
                    action(reviewed_curve_points=expected)
                self.assertEqual(caught.exception.status, 409)
            self.ctrl.apply_preset.assert_not_called()

        self.store.set_curve(name, changed)
        assert_conflict(old)
        assert_conflict(None)
        self.assertEqual(export(reviewed_curve_points=changed)["fans"]["curve"], changed)
        self.assertTrue(apply(reviewed_curve_points=changed)["ok"])
        self.ctrl.apply_preset.assert_called_once_with(preset)
        self.ctrl.apply_preset.reset_mock()
        # An older profile can outlive its named curve. The review distinguishes absence from an omitted check.
        with self.store._lock:
            self.store.config["fan_curves"].pop(name)
        assert_conflict(changed)
        self.assertNotIn("fans", export(reviewed_curve_points=None))
        self.assertTrue(apply(reviewed_curve_points=None)["ok"])
        self.assertTrue(apply()["ok"])

    def test_import_and_whole_list_edit_preserve_rules_in_either_order(self):
        for import_first in (True, False):
            with self.subTest(import_first=import_first):
                before = self.store.get_rules()
                edit = before + [{"id": f"rule-{int(import_first)}", "enabled": False,
                                  "when": {"type": "locked"}, "then": {"type": "lights_off"}}]
                def write():
                    return api.route(self.server, "POST", "/api/automations", {"rules": edit, "expected_rules": before})
                if import_first:
                    result = self.call("import", profile=NEON, include_automations=True)
                    with self.assertRaises(ApiError) as conflict:
                        write()
                    self.assertEqual(conflict.exception.status, 409)
                    self.assertEqual(conflict.exception.details["rules"], result["rules"])
                    self.assertEqual(conflict.exception.details["presets"], result["presets"])
                    self.assertEqual(self.store.get_rules(), result["rules"])
                else:
                    saved = write()
                    result = self.call("import", profile=NEON, include_automations=True)
                    self.assertEqual(result["rules"][:len(saved)], saved)
                    self.assertEqual(len(result["rules"]), len(saved) + len(NEON["automations"]))

    def test_invalid_file_and_missing_export_leave_the_library_intact(self):
        before = self.store.get_presets()
        with self.assertRaises(ValueError):
            self.call("import", profile={"name":"Not a profile"})
        with self.assertRaises(ApiError) as caught:
            self.call("export", id="missing")
        self.assertEqual(caught.exception.status, 404)
        self.assertEqual(before,self.store.get_presets())
