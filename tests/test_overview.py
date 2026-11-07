"""Sensor catalog, history and the Overview layout (validation, storage, migration, API)."""

import copy
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from airglow import autostart, config, sensors
from airglow.history import SensorHistory

FIXTURE = json.loads((Path(__file__).parent / "fixtures" / "lhm_data.json").read_text(encoding="utf-8"))


class CatalogTests(unittest.TestCase):
    def setUp(self):
        self.flat = sensors.flatten(FIXTURE)
        self.readings = sensors.build(self.flat, {"ram_load": 41.0, "ram_used": 12.7, "ram_total": 31.2, "net_down": 2048.0})

    def test_every_sensor_has_lhms_stable_id(self):
        ids = [s["id"] for s in self.flat]
        self.assertIn("/amdcpu/0/power/0", ids)
        self.assertTrue(all(i.startswith("/") for i in ids))

    def test_shortcuts_resolve_like_the_summary(self):
        summary = sensors.summary(self.flat)
        self.assertEqual(self.readings["auto/cpu_temp"]["value"], summary["cpu_temp"])
        self.assertEqual(self.readings["auto/gpu_power"]["value"], summary["gpu_power"])
        self.assertEqual(self.readings["auto/total_power"]["value"], summary["total_power"])
        self.assertIn("9800X3D", self.readings["auto/cpu_temp"]["source"])

    def test_windows_readings_and_known_maximums(self):
        self.assertEqual(self.readings["auto/ram_load"]["value"], 41.0)
        self.assertEqual(self.readings["sys/net_down"]["value"], 2048.0)
        self.assertEqual(self.readings["auto/ram_used"]["max"], 31.2)
        self.assertEqual(self.readings["auto/vram_used"]["max"], 16303.0)

    def test_descriptive_entries_are_left_out_of_the_picker(self):
        names = {m["name"] for m in self.readings.values()}
        self.assertNotIn("Thermal Sensor High Limit", names)
        self.assertNotIn("tAA (CAS Latency Time)", names)
        self.assertIn("GPU Memory Junction", names)

    def test_throughput_is_normalised_to_bytes_per_second(self):
        tree = {"Text": "PC", "Children": [{"Text": "Ethernet", "HardwareId": "/nic/1", "ImageURL": "images_icon/nic.png", "Children": [
            {"Text": "Throughput", "Children": [
                {"Text": "Download Speed", "Value": "1,5 MB/s", "Type": "Throughput", "SensorId": "/nic/1/throughput/8", "Children": []},
                {"Text": "Upload Speed", "Value": "12,0 KB/s", "Type": "Throughput", "SensorId": "/nic/1/throughput/7", "Children": []}]}]}]}
        flat = {s["id"]: s for s in sensors.flatten(tree)}
        self.assertEqual(flat["/nic/1/throughput/8"]["value"], 1.5 * 1024 * 1024)
        self.assertEqual(flat["/nic/1/throughput/7"]["value"], 12 * 1024)
        self.assertEqual(flat["/nic/1/throughput/8"]["hw"], "network")

    def test_missing_lhm_still_gives_windows_readings(self):
        r = sensors.build([], {"ram_load": 50.0})
        self.assertIsNone(r["auto/cpu_temp"]["value"])
        self.assertEqual(r["auto/ram_load"]["value"], 50.0)

    def test_formatting_for_the_tray(self):
        m = {"unit": "°C"}
        self.assertEqual(sensors.fmt(m, 45.4), "45°C")
        self.assertEqual(sensors.fmt(m, 45.4, fahrenheit=True), "114°F")
        self.assertEqual(sensors.fmt({"unit": "B/s"}, 3 * 1024 * 1024), "3.0 MiB/s")
        self.assertEqual(sensors.fmt({"unit": "MHz"}, 4625), "4.62 GHz")
        self.assertEqual(sensors.fmt({"unit": "%"}, None), "n/a")

    def test_system_sensors_read_memory(self):
        s = sensors.SystemSensors()
        s.read()
        r = s.read()
        self.assertTrue(0 < r["ram_load"] <= 100)
        self.assertIn("net_down", r)


class HistoryTests(unittest.TestCase):
    def test_disappeared_sensors_expire_after_the_history_window(self):
        h = SensorHistory(size=3)
        h.add(1, {"gone": 10, "live": 20})
        for t in range(2, 5):
            h.add(t, {"live": 20})
        self.assertEqual(h.get(["gone", "live"])["series"], {"live": [20, 20, 20]})
        self.assertNotIn("gone", h._misses)

    def test_returning_sensor_resets_its_absence_window(self):
        h = SensorHistory(size=3)
        h.add(1, {"a": 10})
        h.add(2, {})
        h.add(3, {})
        h.add(4, {"a": 20})
        h.add(5, {})
        h.add(6, {})
        self.assertEqual(h.get(["a"])["series"], {"a": [20, None, None]})
        h.add(7, {})
        self.assertNotIn("a", h.get(["a"])["series"])

    def test_series_line_up_with_timestamps(self):
        h = SensorHistory(size=4)
        h.add(1, {"a": 1.0})
        h.add(2, {"a": 2.0, "b": 5.0})          # b appears late: padded at the start
        h.add(3, {"b": 6.0})                    # a missing: recorded as a gap
        got = h.get(["a", "b", "nope"])
        self.assertEqual(got["t"], [1, 2, 3])
        self.assertEqual(got["series"]["a"], [1.0, 2.0, None])
        self.assertEqual(got["series"]["b"], [None, 5.0, 6.0])
        self.assertNotIn("nope", got["series"])

    def test_keeps_only_the_window(self):
        h = SensorHistory(size=3)
        for t in range(10):
            h.add(t, {"a": float(t)})
        self.assertEqual(h.get(["a"])["series"]["a"], [7.0, 8.0, 9.0])

    def test_disappeared_sensor_expires_after_its_history_window(self):
        h = SensorHistory(size=3)
        h.add(0, {"gone": 1.0, "present": 2.0})
        h.add(1, {"present": 3.0})
        h.add(2, {"present": 4.0})
        self.assertEqual(h.get(["gone"])["series"]["gone"], [1.0, None, None])
        h.add(3, {"present": 5.0})
        self.assertNotIn("gone", h.series)
        self.assertNotIn("gone", h._misses)
        self.assertEqual(h.get(["present"])["series"]["present"], [3.0, 4.0, 5.0])

    def test_returning_sensor_resets_expiry_and_lines_up_after_reappearance(self):
        h = SensorHistory(size=3)
        h.add(0, {"a": 1.0})
        h.add(1, {})
        h.add(2, {"a": 2.0})
        h.add(3, {})
        h.add(4, {})
        self.assertEqual(h.get(["a"])["series"]["a"], [2.0, None, None])
        h.add(5, {})
        self.assertNotIn("a", h.series)
        h.add(6, {"a": 3.0})
        self.assertEqual(h.get(["a"]), {"t": [4, 5, 6], "series": {"a": [None, None, 3.0]}})

    def test_sensor_churn_does_not_accumulate_expired_histories(self):
        h = SensorHistory(size=3)
        for t in range(100):
            h.add(t, {str(t): float(t)})
        self.assertEqual(set(h.series), {"97", "98", "99"})
        self.assertEqual(set(h._misses), set(h.series))


class LayoutTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)

    def test_default_is_valid(self):
        self.assertEqual(config.validate_overview(config.DEFAULT_OVERVIEW), config.DEFAULT_OVERVIEW)

    def test_tile_presentation_roundtrips_in_active_and_saved_workspaces(self):
        tiles = [
            {"id": "rig", "type": "lighting", "w": 3, "h": 2, "view": "3d",
             "appearance": {"emphasis": "focus", "accent": "#AABB12"}},
            {"id": "cpu", "type": "sensor", "sensors": ["auto/cpu_temp"], "style": "auto", "w": 1, "h": 1,
             "appearance": {"emphasis": "quiet", "accent": None}},
            {"id": "fans", "type": "fans", "w": 2, "h": 2, "appearance": {}},
        ]
        original = {"tiles": tiles, "pins": ["auto/cpu_temp"], "active_workspace": "desk",
                    "workspaces": [{"id": "desk", "name": "Desk", "tiles": copy.deepcopy(tiles), "pins": ["auto/cpu_temp"]}]}
        before = copy.deepcopy(original)
        expected = copy.deepcopy(original)
        for layout in (expected, expected["workspaces"][0]):
            layout["tiles"][0]["appearance"]["accent"] = "#aabb12"
            layout["tiles"][2]["appearance"] = {"emphasis": "standard", "accent": None}
        store = config.Store(self.tmp.name)
        self.assertEqual(store.set_overview(original), expected)
        self.assertEqual(config.Store(self.tmp.name).get_overview(), expected)
        self.assertEqual(original, before)

    def test_omitted_tile_presentation_keeps_legacy_document_shape(self):
        original = copy.deepcopy(config.DEFAULT_OVERVIEW)
        self.assertEqual(config.validate_overview(original), original)
        self.assertTrue(all("appearance" not in tile and "view" not in tile for tile in original["tiles"]))

    def test_invalid_tile_presentation_fails_before_any_saved_state_changes(self):
        store = config.Store(self.tmp.name)
        good = {"tiles": [{"id": "rig", "type": "lighting", "w": 2, "h": 2}], "pins": [],
                "active_workspace": "desk", "workspaces": [
                    {"id": "desk", "name": "Desk", "tiles": [{"id": "fans", "type": "fans", "w": 1, "h": 1}], "pins": []}]}
        before = store.set_overview(good)
        disk_before = Path(self.tmp.name, "overview.json").read_bytes()
        invalid = [
            {"appearance": None}, {"appearance": []}, {"appearance": "focus"},
            {"appearance": {"emphasis": None}}, {"appearance": {"emphasis": "loud"}},
            {"appearance": {"accent": "red"}}, {"appearance": {"accent": "#abc"}},
            {"appearance": {"accent": "#aabbcc;"}}, {"appearance": {"accent": 0}},
            {"appearance": {"accent": "#aabbcc\n"}},
            {"appearance": {"accent": False}}, {"appearance": {"effect": "rainbow"}},
            {"view": None}, {"view": "4d"}, {"type": "fans", "view": "2d"},
        ]
        for change in invalid:
            for saved_workspace in (False, True):
                bad = copy.deepcopy(before)
                layout = bad["workspaces"][0] if saved_workspace else bad
                layout["tiles"][0].update(change)
                with self.subTest(change=change, saved_workspace=saved_workspace):
                    with self.assertRaises(ValueError):
                        store.set_overview(bad)
                    self.assertEqual(store.get_overview(), before)
                    self.assertEqual(Path(self.tmp.name, "overview.json").read_bytes(), disk_before)

    def test_explicit_null_sensor_style_is_invalid(self):
        with self.assertRaises(ValueError):
            config.validate_overview({"tiles": [{"id": "cpu", "type": "sensor", "sensors": ["auto/cpu_temp"], "style": None}], "pins": []})

    def test_rejects_bad_tiles(self):
        good = copy.deepcopy(config.DEFAULT_OVERVIEW)
        for bad in (
            {**good, "tiles": good["tiles"] + [dict(good["tiles"][1])]},                        # duplicate id
            {**good, "tiles": [{"id": "x", "type": "clock"}]},
            {**good, "tiles": [{"id": "x", "type": "sensor", "sensors": ["a"], "style": "pie"}]},
            {**good, "tiles": [{"id": "x", "type": "sensor", "sensors": ["a", "b", "c"]}]},
            {**good, "tiles": [{"id": "x", "type": "sensor", "sensors": ["a"], "w": 5}]},
            {**good, "pins": ["a", "b", "c", "d", "e"]},
            {**good, "tiles": [{"id": "Bad Id", "type": "fans"}]},
        ):
            with self.assertRaises(ValueError):
                config.validate_overview(bad)

    def test_saved_and_tracked(self):
        s = config.Store(self.tmp.name)
        s.set_overview({"tiles": [{"id": "ram", "type": "sensor", "sensors": ["auto/ram_load"], "style": "bar", "w": 2},
                                  {"id": "fans", "type": "fans"}], "pins": ["auto/cpu_temp", "auto/cpu_temp"]})
        s2 = config.Store(self.tmp.name)
        self.assertEqual(s2.get_overview()["tiles"][0]["w"], 2)
        self.assertEqual(s2.get_overview()["pins"], ["auto/cpu_temp"])            # duplicates dropped
        self.assertEqual(s2.tracked_sensors(), ["auto/ram_load", "auto/cpu_temp"])
        self.assertEqual(s2.reset_overview(), config.DEFAULT_OVERVIEW)

    def test_broken_file_falls_back_to_default(self):
        Path(self.tmp.name, "overview.json").write_text(json.dumps({"tiles": "nope"}), encoding="utf-8")
        self.assertEqual(config.Store(self.tmp.name).get_overview(), config.DEFAULT_OVERVIEW)


class StartupEntryTests(unittest.TestCase):
    def test_moved_install_repoints_the_startup_entry(self):
        reg = {"Airglow": '"C:\\old\\Airglow.exe" --background'}
        with mock.patch.object(autostart, "is_installed_copy", return_value=True), \
             mock.patch.object(autostart, "current", side_effect=lambda name=autostart.VALUE: reg.get(name)), \
             mock.patch.object(autostart, "is_enabled", side_effect=lambda: autostart.VALUE in reg), \
             mock.patch.object(autostart, "enable", side_effect=lambda: reg.__setitem__(autostart.VALUE, autostart.command())):
            self.assertTrue(autostart.repair())
        self.assertEqual(reg["Airglow"], autostart.command())

    def test_no_startup_entry_is_created_when_there_was_none(self):
        with mock.patch.object(autostart, "is_installed_copy", return_value=True), \
             mock.patch.object(autostart, "current", return_value=None), \
             mock.patch.object(autostart, "is_enabled", return_value=False), \
             mock.patch.object(autostart, "enable", side_effect=AssertionError("must not write")):
            self.assertFalse(autostart.repair())


if __name__ == "__main__":
    unittest.main()
