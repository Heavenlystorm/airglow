"""Personal display preferences remain bounded, durable, and independent of hardware."""
import copy
import tempfile
import unittest
from unittest import mock
from types import SimpleNamespace

from airglow import api, config


LOOK = {"id": "my-desk", "name": "My desk", "design": "paper", "appearance": {
    "accent": "#1166AA", "density": "compact", "scale": 110, "rig": "frame", "motion": "reduced"}}


def workspace_layout():
    layout = copy.deepcopy(config.DEFAULT_OVERVIEW)
    layout["workspaces"] = [{"id": "daily", "name": "Daily", **copy.deepcopy(layout)},
                            {"id": "focus", "name": "Focus", "tiles": [], "pins": []}]
    layout["active_workspace"] = "daily"
    return layout


class PersonalizationTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.directory = directory.name
        self.store = config.Store(directory.name)

    def test_appearance_and_named_looks_survive_restart_without_changing_control_settings(self):
        before = self.store.settings()
        self.store.update_settings({"appearance": LOOK["appearance"], "looks": [LOOK]})
        again = config.Store(self.directory).settings()
        self.assertEqual(again["appearance"]["accent"], "#1166aa")
        self.assertEqual(again["looks"][0]["name"], "My desk")
        for key in ("temp_source", "fan_failsafe_duty", "fan_max_step", "openrgb_enabled"):
            self.assertEqual(again[key], before[key])

    def test_rejects_unbounded_and_malformed_appearance(self):
        for appearance in ([], {"accent": "url(x)"}, {"accent": 123}, {"scale": 101}, {"scale": True},
                           {"density": "tiny"}, {"motion": "always"}, {"rig": "3d"}, {"css": "*{}"}):
            with self.subTest(appearance=appearance), self.assertRaises(ValueError):
                config.validate_settings({"appearance": appearance})

    def test_saved_look_names_and_identifiers_are_bounded_and_unique(self):
        bad = [[LOOK, LOOK], [{**LOOK, "id": "../bad"}], [{**LOOK, "name": " "}],
               [{**LOOK, "name": "x" * 41}], [{**LOOK, "design": "unknown"}],
               [{**LOOK, "fan_curve": "silent"}], [{**LOOK, "id": str(i)} for i in range(13)]]
        for looks in bad:
            with self.subTest(looks=looks), self.assertRaises(ValueError):
                config.validate_settings({"looks": looks})

    def test_settings_snapshot_does_not_expose_mutable_saved_look_data(self):
        self.store.update_settings({"looks": [LOOK], "appearance": LOOK["appearance"]})
        snapshot = self.store.settings()
        snapshot["looks"][0]["name"] = "mutated"
        snapshot["appearance"]["scale"] = 90
        self.assertEqual(self.store.settings()["looks"][0]["name"], "My desk")
        self.assertEqual(self.store.settings()["appearance"]["scale"], 110)

    def test_reset_appearance_retains_saved_looks(self):
        self.store.update_settings({"appearance": LOOK["appearance"], "looks": [LOOK]})
        self.store.reset_settings()
        settings = self.store.settings()
        self.assertEqual(settings["appearance"]["scale"], 100)
        self.assertIsNone(settings["appearance"]["accent"])
        self.assertEqual(settings["looks"][0]["id"], "my-desk")

    def test_old_overview_documents_remain_unchanged(self):
        self.assertEqual(self.store.get_overview(), config.DEFAULT_OVERVIEW)
        self.assertEqual(config.validate_overview(config.DEFAULT_OVERVIEW), config.DEFAULT_OVERVIEW)

    def test_workspaces_roundtrip_and_active_edits_update_only_active_snapshot(self):
        self.store.set_overview(workspace_layout())
        edited = self.store.get_overview()
        edited["tiles"][0]["w"] = 3
        self.store.set_overview(edited)
        result = config.Store(self.directory).get_overview()
        self.assertEqual(result["active_workspace"], "daily")
        self.assertEqual(result["workspaces"][0]["tiles"][0]["w"], 3)
        self.assertEqual(result["workspaces"][1]["tiles"], [])

    def test_old_client_save_retains_workspace_collection(self):
        self.store.set_overview(workspace_layout())
        self.store.set_overview({"tiles": [], "pins": ["auto/cpu_temp"]})
        result = self.store.get_overview()
        self.assertEqual(len(result["workspaces"]), 2)
        self.assertEqual(result["workspaces"][0]["tiles"], [])
        self.assertEqual(result["active_workspace"], "daily")

    def test_reset_current_layout_preserves_other_workspace(self):
        layout = workspace_layout()
        layout.update(tiles=[], pins=[], active_workspace="focus")
        self.store.set_overview(layout)
        result = self.store.reset_overview()
        self.assertEqual(result["tiles"], config.DEFAULT_OVERVIEW["tiles"])
        self.assertEqual(result["active_workspace"], "focus")
        self.assertEqual(len(result["workspaces"]), 2)
        self.assertEqual(result["workspaces"][1]["tiles"], config.DEFAULT_OVERVIEW["tiles"])

    def test_workspace_validation_rejects_duplicates_nested_collections_and_dangling_selection(self):
        valid = workspace_layout()
        for bad in ({**valid, "workspaces": valid["workspaces"] * 2},
                    {**valid, "active_workspace": "missing"},
                    {**valid, "workspaces": [{**valid["workspaces"][0], "workspaces": []}]},
                    {**valid, "workspaces": [{**valid["workspaces"][0], "name": ""}]},
                    {**valid, "workspaces": [{**valid["workspaces"][0], "id": str(i)} for i in range(9)]}):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                config.validate_overview(bad)

    def test_failed_workspace_write_keeps_saved_and_in_memory_layout(self):
        self.store.set_overview(workspace_layout())
        before = self.store.get_overview()
        edited = copy.deepcopy(before)
        edited["tiles"] = []
        with mock.patch.object(self.store, "_write", side_effect=OSError("disk full")):
            with self.assertRaises(OSError):
                self.store.set_overview(edited)
        self.assertEqual(self.store.get_overview(), before)
        self.assertEqual(config.Store(self.directory).get_overview(), before)

    def test_failed_look_write_keeps_original_preferences(self):
        before = self.store.settings()
        with mock.patch.object(self.store, "_write", side_effect=OSError("disk full")):
            with self.assertRaises(OSError):
                self.store.update_settings({"appearance": LOOK["appearance"], "looks": [LOOK]})
        self.assertEqual(self.store.settings(), before)

    def test_appearance_api_persists_without_reapplying_hardware(self):
        # Exercise the real route; fail loudly if presentation invokes control work.
        def unexpected_control():
            raise AssertionError("appearance must not reapply hardware")
        server = SimpleNamespace(store=self.store, ctrl=SimpleNamespace(apply=unexpected_control),
                                 hooks={}, updater=None)
        with mock.patch.object(api, "_settings_view", side_effect=lambda store, updater: {"settings": store.settings()}):
            result = api.route(server, "POST", "/api/settings", {"appearance": LOOK["appearance"], "looks": [LOOK]})
        self.assertEqual(result["settings"]["appearance"]["scale"], 110)
        self.assertEqual(config.Store(self.directory).settings()["looks"][0]["id"], "my-desk")


if __name__ == "__main__":
    unittest.main()
