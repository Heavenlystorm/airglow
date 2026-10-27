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
        self.assertEqual(config.validate_settings({"lhm_url": "http://localhost:8085/"}), {"lhm_url": "http://127.0.0.1:8085"})
        self.assertEqual(config.validate_settings({"lhm_url": "http://localhost.lan:8085"}), {"lhm_url": "http://localhost.lan:8085"})
        self.assertEqual(config.validate_settings({"openrgb_port": "6742"}), {"openrgb_port": 6742})
        for bad in ({"fan_failsafe_duty": 10}, {"temp_unit": "K"}, {"nope": 1}, {"lhm_url": "file:///c:/"},
                    {"openrgb_host": "bad host!"}):
            with self.assertRaises(ValueError, msg=bad):
                config.validate_settings(bad)

    def test_design_and_energy_price(self):
        self.assertEqual(config.validate_settings({"design": "ascii"}), {"design": "ascii"})
        self.assertEqual(config.validate_settings({"energy_price": "0.41"}), {"energy_price": 0.41})
        self.assertEqual(config.validate_settings({"energy_price": 0.37971}), {"energy_price": 0.3797})
        for bad in ({"design": "neon2"}, {"energy_price": 0}, {"energy_price": 5.01},
                    {"energy_price": "free"}, {"energy_price": float("nan")}, {"energy_price": float("inf")}):
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

    def test_a_studio_effect_that_is_gone_is_kept_with_its_values(self):
        s = config.validate_scene({"effect": "fx:me.gone", "params": {"speed": 40, "tint": "#ff0000", "x": {"deep": 1}},
                                   "areas": {"front": {"effect": "fx:jane.wave", "params": {}}}})
        self.assertEqual(s["params"], {"speed": 40, "tint": "#ff0000"})       # plain values only
        self.assertEqual(s["areas"]["front"]["effect"], "fx:jane.wave")
        for bad in ("nope", "fx:", "fx:Bad Id", "fx:me/../x"):
            with self.assertRaises(ValueError, msg=bad):
                config.validate_scene({"effect": bad})

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

    def test_profiles_using_a_deleted_studio_effect_survive_a_restart(self):
        s = config.Store(self.dir)
        s.set_presets(s.get_presets() + [{"id": "mine", "name": "My look", "effect": "fx:me.gone",
                                          "params": {"speed": 7}, "brightness": 80}])
        again = config.Store(self.dir)
        mine = next(p for p in again.get_presets() if p["id"] == "mine")
        self.assertEqual((mine["effect"], mine["params"]), ("fx:me.gone", {"speed": 7}))

    def test_a_saved_localhost_sensor_address_becomes_127_0_0_1(self):
        self.dir.mkdir(parents=True)
        (self.dir / "config.json").write_text(json.dumps({"lhm_url": "http://localhost:8085"}), encoding="utf-8")
        self.assertEqual(config.Store(self.dir).settings()["lhm_url"], "http://127.0.0.1:8085")
        self.assertEqual(config.DEFAULT_CONFIG["lhm_url"], "http://127.0.0.1:8085")

    def test_an_unknown_saved_design_falls_back_to_graphite(self):
        self.dir.mkdir(parents=True)
        (self.dir / "config.json").write_text(json.dumps({"design": "vaporwave"}), encoding="utf-8")
        self.assertEqual(config.Store(self.dir).settings()["design"], "graphite")

    def test_defaults_when_empty(self):
        s = config.Store(self.dir)
        self.assertEqual([p["id"] for p in s.get_presets()], ["silent", "balanced", "gaming", "night"])
        self.assertEqual(s.settings()["port"], 8787)
        self.assertEqual(s.state["scene"]["effect"], "solid")
        self.assertFalse(s.state["onboarded"])

    def test_reset_settings_keeps_zones_presets_and_curves(self):
        s = config.Store(self.dir)
        s.update_settings({"fan_failsafe_duty": 70, "temp_unit": "F", "online_checks": True})
        s.set_curve("balanced", [[40, 35], [80, 90]])
        s.set_presets(s.get_presets() + [{"id": "mine", "name": "Mine", "effect": "solid", "params": {"color": "#123456"}}])
        s.reset_settings()
        settings = s.settings()
        self.assertEqual(settings["fan_failsafe_duty"], 30)
        self.assertEqual(settings["temp_unit"], "C")
        self.assertFalse(settings["online_checks"])
        self.assertEqual(settings["port"], 8787)                       # the port Airglow is listening on stays
        self.assertEqual(s.curves()["balanced"], [[40, 35], [80, 90]])  # what was built stays too
        self.assertIn("mine", [p["id"] for p in s.get_presets()])
        again = config.Store(self.dir)
        self.assertEqual(again.settings()["fan_failsafe_duty"], 30)    # and it survived the save

    def test_design_and_price_persist_and_reset(self):
        s = config.Store(self.dir)
        s.update_settings({"design": "paper", "energy_price": 0.42})
        again = config.Store(self.dir)
        self.assertEqual((again.settings()["design"], again.settings()["energy_price"]), ("paper", 0.42))
        again.reset_settings()
        self.assertEqual((again.settings()["design"], again.settings()["energy_price"]), ("graphite", 0.3797))

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

    def test_default_fan_zones_never_overlap(self):
        # two F360 kits and two F120s on one hub: each kit gets a column of its own, the single fans fill the gaps
        s = config.Store(self.dir)
        zones = [{"id": f"ch{i}", "name": f"Channel {i}", "default_leds": 8 * n, "shape": "fans", "fans": n}
                 for i, n in ((1, 3), (2, 3), (3, 1), (4, 1))]
        s.ensure_zones([{"id": "nzxt-hub", "kind": "native", "type": "Fan and lighting hub", "name": "NZXT",
                         "zones": zones}])
        from airglow.lighting import geometry
        circles = []
        for key, cfg in s.zones().items():
            L = cfg["layout"]
            pos, _, fan = geometry.positions(L)
            for f in range(L["fans"]):
                circles.append((key, pos[fan == f].mean(axis=0), L["size"] / 2))
        for i, (ka, ca, ra) in enumerate(circles):
            for kb, cb, rb in circles[i + 1:]:
                if ka != kb:
                    self.assertGreaterEqual(float(((ca - cb) ** 2).sum() ** 0.5), ra + rb, f"{ka} overlaps {kb}")
            # inside the case, and left of where strips and the board start, like front intake fans
            self.assertTrue(0 <= ca[1] - ra and ca[1] + ra <= 1 and ca[0] + ra < 0.65, f"{ka} sits at {ca}")

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

    def test_unreported_fallback_does_not_rewrite_saved_zones(self):
        # A start where the hub's accessory report was lost used to flip empty channels to enabled 40-LED strips
        s = config.Store(self.dir)
        zone = {"id": "ch3", "name": "Channel 3", "default_leds": 0, "shape": "strip", "fans": 1}
        dev = [{"id": "nzxt-hub", "kind": "native", "type": "Fan and lighting hub", "name": "NZXT", "zones": [zone]}]
        self.assertTrue(s.ensure_zones(dev))
        saved = s.zones()["nzxt-hub/ch3"]
        self.assertFalse(saved["enabled"])
        zone.update(default_leds=40, guessed=True)                   # this start: no report, 40-LED fallback
        self.assertFalse(s.ensure_zones(dev))
        self.assertEqual(s.zones()["nzxt-hub/ch3"], saved)
        zone.update(default_leds=24, shape="fans", guessed=False)    # something really was plugged in
        self.assertTrue(s.ensure_zones(dev))
        self.assertTrue(s.zones()["nzxt-hub/ch3"]["enabled"])

    def test_unreported_fallback_still_places_new_zones(self):
        s = config.Store(self.dir)
        dev = [{"id": "nzxt-hub", "kind": "native", "type": "Fan and lighting hub", "name": "NZXT",
                "zones": [{"id": "ch1", "name": "Channel 1", "default_leds": 40, "shape": "strip", "fans": 1,
                           "guessed": True}]}]
        self.assertTrue(s.ensure_zones(dev))
        self.assertIn("nzxt-hub/ch1", s.zones())

    def test_corrupt_files_fall_back(self):
        self.dir.mkdir(parents=True)
        (self.dir / "config.json").write_text("{not json", encoding="utf-8")
        (self.dir / "presets.json").write_text(json.dumps([{"id": "ok", "name": "Ok"}, {"name": ""}]), encoding="utf-8")
        s = config.Store(self.dir)
        self.assertEqual(s.settings()["fan_failsafe_duty"], 30)
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
        for bad in (r"..\..\evil.json", "C:/x/y.json", "notes.txt"):
            with self.assertRaises(ValueError, msg=bad):
                config.validate_preset({"id": "a", "name": "A", "fan_profile": bad}, config.DEFAULT_CURVES)

    def test_invalid_saved_automations_are_dropped(self):
        good = {"id": "night", "when": {"type": "time", "start": "23:00", "end": "07:00"}, "then": {"type": "lights_off"}}
        (Path(self.tmp.name) / "automations.json").write_text(json.dumps([good, {"id": "x", "when": "junk"}, 5]),
                                                              encoding="utf-8")
        s = config.Store(self.tmp.name)
        self.assertEqual([r["id"] for r in s.get_rules()], ["night"])


class DamagedFileTests(unittest.TestCase):
    """A file that exists but cannot be read must never be replaced by the defaults."""

    def test_a_damaged_config_is_set_aside_not_overwritten(self):
        with tempfile.TemporaryDirectory() as tmp:
            cfg = Path(tmp, "config.json")
            cfg.write_text(json.dumps({"schema": config.SCHEMA_VERSION, "design": "amber"}), encoding="utf-8")
            self.assertEqual(config.Store(tmp).settings()["design"], "amber")
            cfg.write_text("{ not json", encoding="utf-8")
            store = config.Store(tmp)
            self.assertEqual(store.settings()["design"], "graphite")       # defaults in memory, so it starts
            self.assertEqual(Path(tmp, "config.json.corrupt").read_text(encoding="utf-8"), "{ not json")
            self.assertFalse(cfg.exists(), "the damaged file was written over with the defaults")

    def test_an_unreadable_config_is_left_alone(self):
        with tempfile.TemporaryDirectory() as tmp:
            cfg = Path(tmp, "config.json")
            good = json.dumps({"schema": config.SCHEMA_VERSION, "design": "amber"})
            cfg.write_text(good, encoding="utf-8")
            real = Path.read_text

            def blocked(self, *args, **kwargs):
                if self.name == "config.json":
                    raise PermissionError(13, "in use by another process")
                return real(self, *args, **kwargs)

            with mock.patch.object(Path, "read_text", blocked):
                store = config.Store(tmp)
            self.assertEqual(store.settings()["design"], "graphite")
            self.assertEqual(cfg.read_text(encoding="utf-8"), good, "an unreadable file was overwritten")

            # Startup continues with device discovery, and later the user may try to change a setting.
            from tests.fakes import FakeNzxt
            self.assertTrue(store.ensure_zones([FakeNzxt().info()]))
            self.assertTrue(store.zones())
            self.assertFalse(store.ensure_zones([FakeNzxt().info()]))
            with self.assertRaisesRegex(ValueError, "Restart Airglow"):
                store.update_settings({"temp_unit": "F"})
            self.assertEqual(cfg.read_text(encoding="utf-8"), good)
            store.save_state(active_preset=None)             # an unrelated readable file can still be saved
            self.assertTrue(Path(tmp, "state.json").exists())
            self.assertEqual(config.Store(tmp).settings()["design"], "amber")

    def test_a_failed_quarantine_cannot_be_overwritten_by_discovery(self):
        with tempfile.TemporaryDirectory() as tmp:
            cfg = Path(tmp, "config.json")
            cfg.write_text("{ damaged", encoding="utf-8")
            with mock.patch.object(Path, "replace", side_effect=PermissionError("locked")):
                store = config.Store(tmp)
            from tests.fakes import FakeNzxt
            store.ensure_zones([FakeNzxt().info()])
            self.assertEqual(cfg.read_text(encoding="utf-8"), "{ damaged")
            with self.assertRaisesRegex(ValueError, "original file has been kept"):
                store.update_settings({"design": "amber"})

    def test_a_quarantined_file_allows_new_settings_without_losing_the_original(self):
        with tempfile.TemporaryDirectory() as tmp:
            cfg = Path(tmp, "config.json")
            cfg.write_text("{ damaged", encoding="utf-8")
            store = config.Store(tmp)
            store.update_settings({"design": "amber"})
            self.assertEqual(json.loads(cfg.read_text(encoding="utf-8"))["design"], "amber")
            self.assertEqual(Path(tmp, "config.json.corrupt").read_text(encoding="utf-8"), "{ damaged")


if __name__ == "__main__":
    unittest.main()
