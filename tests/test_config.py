import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from airglow import config


class ValidationTests(unittest.TestCase):
    def test_hex(self):
        self.assertEqual(config.parse_hex("#FF8000"), [255, 128, 0])
        self.assertEqual(config.norm_hex("FF8000"), "#ff8000")
        with self.assertRaises(ValueError):
            config.parse_hex("red")

    def test_curve_sorted_and_bounded(self):
        self.assertEqual(config.validate_curve([[70, 80], [40, 30]]), [[40, 30], [70, 80]])
        for bad in ([[40, 30]], [[40, 10], [70, 80]], [[40, 30], [40, 50]], [[40, 30], [120, 80]], "x"):
            with self.assertRaises(ValueError, msg=bad):
                config.validate_curve(bad)

    def test_settings(self):
        self.assertEqual(config.validate_settings({"fan_failsafe_duty": "70"}), {"fan_failsafe_duty": 70})
        self.assertEqual(config.validate_settings({"lhm_url": "http://localhost:8085/"}), {"lhm_url": "http://localhost:8085"})
        self.assertEqual(config.validate_settings({"openrgb_port": "6742"}), {"openrgb_port": 6742})
        for bad in ({"fan_failsafe_duty": 10}, {"temp_unit": "K"}, {"nope": 1}, {"lhm_url": "file:///c:/"},
                    {"openrgb_host": "bad host!"}):
            with self.assertRaises(ValueError, msg=bad):
                config.validate_settings(bad)

    def test_preset_from_pre_2_colour_preset(self):
        p = config.validate_preset({"id": "a-1", "name": " Night ", "color": "00FF00", "brightness": 20,
                                    "fan_curve": "silent"}, config.DEFAULT_CURVES)
        self.assertEqual((p["name"], p["effect"], p["params"]), ("Night", "solid", {"color": "#00ff00"}))
        with self.assertRaises(ValueError):
            config.validate_preset({"id": "a", "name": "x", "fan_curve": "turbo"}, config.DEFAULT_CURVES)
        with self.assertRaises(ValueError):
            config.validate_preset({"id": "Bad Id", "name": "x"}, config.DEFAULT_CURVES)

    def test_scene(self):
        s = config.validate_scene({"effect": "comet", "params": {"speed": 70}, "brightness": "40"})
        self.assertEqual((s["params"]["speed"], s["params"]["tail"], s["brightness"], s["on"]), (70, 45, 40, True))
        with self.assertRaises(ValueError):
            config.validate_scene({"effect": "comet", "brightness": 140})

    def test_zone(self):
        z = config.validate_zone({"name": "Front fans", "layout": {"shape": "fans", "fans": 3, "leds_per_fan": 8,
                                                                   "x": 0.3, "y": 0.5}}, 40)
        self.assertEqual((z["enabled"], z["order"], z["override"]), (True, "RGB", None))
        with self.assertRaises(ValueError):
            config.validate_zone({"name": "", "layout": {"shape": "strip", "leds": 4, "x": 0, "y": 0}}, 40)
        with self.assertRaises(ValueError):
            config.validate_zone({"name": "x", "order": "XYZ", "layout": {"shape": "strip", "leds": 4, "x": 0, "y": 0}}, 40)


class StoreTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name) / "data"

    def tearDown(self):
        self.tmp.cleanup()

    def test_defaults_when_empty(self):
        s = config.Store(self.dir)
        self.assertEqual([p["id"] for p in s.get_presets()], ["silent", "balanced", "gaming", "night"])
        self.assertEqual(s.settings()["port"], 8787)
        self.assertEqual(s.state["scene"]["effect"], "solid")
        self.assertFalse(s.state["onboarded"])

    def test_round_trip(self):
        s = config.Store(self.dir)
        s.update_settings({"temp_unit": "F"})
        s.set_curve("silent", [[30, 20], [90, 100]])
        s.save_state(scene={"effect": "solid", "params": {"color": "#123456"}, "brightness": 50, "on": True})
        s2 = config.Store(self.dir)
        self.assertEqual(s2.settings()["temp_unit"], "F")
        self.assertEqual(s2.curves()["silent"], [[30, 20], [90, 100]])
        self.assertEqual(s2.state["scene"]["params"]["color"], "#123456")
        self.assertFalse(list(self.dir.glob("*.tmp")))

    def test_upgrades_unversioned_state_and_presets_once(self):
        self.dir.mkdir(parents=True)
        (self.dir / "state.json").write_text(json.dumps({"color": "#ff7a1a", "brightness": 35, "lights_on": False,
                                                         "fan_curve": "silent", "active_preset": "silent"}), encoding="utf-8")
        (self.dir / "presets.json").write_text(json.dumps([{"id": "silent", "name": "Silent", "color": "#ff7a1a",
                                                            "brightness": 35, "fan_curve": "silent", "fan_profile": None}]), encoding="utf-8")
        s = config.Store(self.dir)
        self.assertEqual(s.state["scene"], {"effect": "solid", "params": {"color": "#ff7a1a"}, "brightness": 35, "on": False})
        self.assertEqual(s.get_presets()[0]["effect"], "solid")
        saved = json.loads((self.dir / "presets.json").read_text(encoding="utf-8"))
        self.assertEqual(saved[0]["params"], {"color": "#ff7a1a"})                 # rewritten in the new format
        self.assertEqual(json.loads((self.dir / "config.json").read_text(encoding="utf-8"))["schema"],
                         config.SCHEMA_VERSION)
        self.assertNotIn("schema", s.settings())

    def test_newer_settings_are_not_rewritten(self):
        self.dir.mkdir(parents=True)
        raw = json.dumps({"schema": config.SCHEMA_VERSION + 1, "fan_max_step": 4})
        (self.dir / "config.json").write_text(raw, encoding="utf-8")
        s = config.Store(self.dir)
        self.assertEqual(s.settings()["fan_max_step"], 4)
        self.assertEqual((self.dir / "config.json").read_text(encoding="utf-8"), raw)

    def test_ensure_zones_defaults(self):
        s = config.Store(self.dir)
        devs = [{"id": "nzxt-hub", "kind": "native", "type": "Fan and lighting hub", "name": "NZXT",
                 "zones": [{"id": "ch1", "name": "Channel 1", "default_leds": 24, "shape": "fans", "fans": 1},
                           {"id": "ch3", "name": "Channel 3", "default_leds": 0, "shape": "strip", "fans": 1}]},
                {"id": "openrgb:kb", "kind": "openrgb", "type": "Keyboard", "name": "Keychron K2 HE",
                 "zones": [{"id": "z0", "name": "Keyboard", "default_leds": 85, "shape": "strip", "fans": 1}]}]
        self.assertTrue(s.ensure_zones(devs))
        z = s.zones()
        self.assertEqual(z["nzxt-hub/ch1"]["layout"]["leds_per_fan"], 24)
        self.assertFalse(z["nzxt-hub/ch3"]["enabled"])                  # nothing plugged in
        self.assertFalse(z["openrgb:kb/z0"]["enabled"])                 # peripherals start off
        self.assertEqual(z["openrgb:kb/z0"]["name"], "Keychron K2 HE")             # single-zone device: just its name
        self.assertFalse(s.ensure_zones(devs))                          # idempotent

    def test_auto_zones_follow_detection_but_user_zones_dont(self):
        s = config.Store(self.dir)
        before = [{"id": "nzxt-hub", "kind": "native", "type": "Fan and lighting hub", "name": "NZXT",
                   "zones": [{"id": "ch1", "name": "Channel 1", "default_leds": 40, "shape": "strip", "fans": 1},
                             {"id": "ch2", "name": "Channel 2", "default_leds": 40, "shape": "strip", "fans": 1}]}]
        s.ensure_zones(before)
        # the user renames channel 2 (saving drops the auto mark)
        s.set_zone("nzxt-hub/ch2", {**s.zones()["nzxt-hub/ch2"], "name": "Top strip"}, 40)
        detected = [{**before[0], "zones": [{"id": "ch1", "name": "Channel 1", "default_leds": 24, "shape": "fans", "fans": 1},
                                            {"id": "ch2", "name": "Channel 2", "default_leds": 8, "shape": "fans", "fans": 1}]}]
        self.assertTrue(s.ensure_zones(detected))
        z = s.zones()
        self.assertEqual((z["nzxt-hub/ch1"]["layout"]["shape"], z["nzxt-hub/ch1"]["layout"]["leds_per_fan"]), ("fans", 24))
        self.assertEqual(z["nzxt-hub/ch2"]["name"], "Top strip")
        self.assertEqual(z["nzxt-hub/ch2"]["layout"]["leds"], 40)          # the user's zone is left alone
        self.assertNotIn("auto", z["nzxt-hub/ch2"])

    def test_provisional_zones_are_not_saved(self):
        s = config.Store(self.dir)
        dev = [{"id": "nzxt-hub", "kind": "native", "type": "Fan and lighting hub", "name": "NZXT",
                "zones": [{"id": "ch1", "name": "Channel 1", "default_leds": 40, "shape": "strip", "fans": 1, "provisional": True}]}]
        self.assertFalse(s.ensure_zones(dev))
        self.assertNotIn("nzxt-hub/ch1", s.zones())
        rev = s.zones_rev
        dev[0]["zones"][0].update(provisional=False, default_leds=24, shape="fans")
        self.assertTrue(s.ensure_zones(dev))
        self.assertGreater(s.zones_rev, rev)

    def test_corrupt_files_fall_back(self):
        self.dir.mkdir(parents=True)
        (self.dir / "config.json").write_text("{not json", encoding="utf-8")
        (self.dir / "presets.json").write_text(json.dumps([{"id": "ok", "name": "Ok"}, {"name": ""}]), encoding="utf-8")
        s = config.Store(self.dir)
        self.assertEqual(s.settings()["fan_failsafe_duty"], 60)
        self.assertEqual([p["id"] for p in s.get_presets()], ["ok"])

    def test_duplicate_preset_ids_rejected(self):
        s = config.Store(self.dir)
        with self.assertRaises(ValueError):
            s.set_presets([{"id": "a", "name": "A"}, {"id": "a", "name": "B"}])


    def test_duplicate_automation_ids_rejected(self):
        s = config.Store(self.dir)
        rule = {"id": "night", "when": {"type": "time", "start": "23:00", "end": "07:00"}, "then": {"type": "lights_off"}}
        with self.assertRaises(ValueError):
            s.set_rules([rule, dict(rule)], lambda r: dict(r))
        self.assertEqual(s.get_rules(), [])

    def test_a_briefly_locked_file_is_written_on_a_retry(self):
        s = config.Store(self.dir)
        real, calls = Path.replace, []

        def flaky(path, target):
            calls.append(target)
            if len(calls) < 3:
                raise PermissionError(13, "The process cannot access the file")
            return real(path, target)
        with mock.patch.object(Path, "replace", flaky), mock.patch.object(config.time, "sleep"):
            s.update_settings({"temp_unit": "F"})
        self.assertEqual(len(calls), 3)
        self.assertEqual(config.Store(self.dir).settings()["temp_unit"], "F")

    def test_a_file_that_stays_locked_still_fails(self):
        s = config.Store(self.dir)
        with mock.patch.object(Path, "replace", side_effect=PermissionError(13, "locked")),                 mock.patch.object(config.time, "sleep") as sleep:
            with self.assertRaises(PermissionError):
                s.update_settings({"temp_unit": "F"})
        self.assertEqual(sleep.call_count, 4)


class RegressionTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.store = config.Store(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_uneven_fan_chain_is_placed_once(self):
        # 24 + 8 + 8 LEDs on one NZXT channel: 40 LEDs over 3 fans used to be "stale" on every tick
        dev = {"id": "nzxt-hub", "name": "NZXT Control Hub", "vendor": "NZXT", "kind": "native",
               "type": "Fan and lighting hub",
               "zones": [{"id": "ch1", "name": "Channel 1", "max_leds": 40, "default_leds": 40, "shape": "fans",
                          "fans": 3, "resizable": True, "provisional": False}]}
        self.assertTrue(self.store.ensure_zones([dev]))
        self.assertFalse(self.store.ensure_zones([dev]))
        self.assertEqual(self.store.zones_rev, 1)

    def test_fan_profile_must_be_a_file_name(self):
        ok = config.validate_preset({"id": "a", "name": "A", "fan_profile": "Game.json"}, config.DEFAULT_CURVES)
        self.assertEqual(ok["fan_profile"], "Game.json")
        for bad in ("..\..\evil.json", "C:/x/y.json", "notes.txt"):
            with self.assertRaises(ValueError, msg=bad):
                config.validate_preset({"id": "a", "name": "A", "fan_profile": bad}, config.DEFAULT_CURVES)

    def test_invalid_saved_automations_are_dropped(self):
        good = {"id": "night", "when": {"type": "time", "start": "23:00", "end": "07:00"}, "then": {"type": "lights_off"}}
        (Path(self.tmp.name) / "automations.json").write_text(json.dumps([good, {"id": "x", "when": "junk"}, 5]),
                                                              encoding="utf-8")
        s = config.Store(self.tmp.name)
        self.assertEqual([r["id"] for r in s.get_rules()], ["night"])


if __name__ == "__main__":
    unittest.main()
