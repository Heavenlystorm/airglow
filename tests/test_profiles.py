import json
import tempfile
import unittest
import urllib.parse
from pathlib import Path

from airglow import config, gallery, lightsetup, profiles
from airglow.controller import Controller
from airglow.lighting.engine import Engine
from tests.fakes import FakeLightDevice, FakeManager, FakeNzxt, signals

ROOT = Path(__file__).resolve().parent.parent

NEON = {"airglow_profile": 1, "id": "neon", "name": "Neon", "author": "someone", "description": "Pink and blue.",
        "lighting": {"effect": "gradient", "params": {"colors": ["#ff2fb4", "#2f5bff"]}, "brightness": 90},
        "areas": {"front": {"effect": "solid", "params": {"color": "#00ff00"}}},
        "fans": {"curve": [[40, 25], [70, 50], [85, 100]]},
        "automations": [{"when": {"type": "app_running", "exe": "game.exe"}, "then": {"type": "preset", "preset": "self"}},
                        {"when": {"type": "time", "start": "23:00", "end": "07:00"}, "then": {"type": "lights_off"}}]}


def big_profile():
    """A valid profile too long for GitHub's address: every area, ten automations, an eight-point fan curve."""
    return {**NEON, "id": "big", "name": "Big", "description": "d" * 200,
            "areas": {s: {"effect": "gradient", "params": {"colors": ["#ff2fb4", "#2f5bff", "#00ff00", "#ffffff"]}}
                      for s in lightsetup.SPOTS},
            "fans": {"curve": [[20 + i * 5, 20 + i * 7] for i in range(8)]},
            "automations": [{"when": {"type": "app_running", "exe": "g" * 74 + f"{i:02d}.exe"},
                             "then": {"type": "preset", "preset": "self"}} for i in range(10)]}


class FormatTests(unittest.TestCase):
    def test_a_full_profile_validates(self):
        p = profiles.validate_shared(NEON)
        self.assertEqual(p["areas"]["front"]["params"]["color"], "#00ff00")
        self.assertEqual(p["fans"]["curve"][0], [40, 25])
        self.assertEqual(len(p["automations"]), 2)
        self.assertEqual(profiles.badges(p), ["fans", "automations", "areas"])

    def test_bad_profiles_are_refused(self):
        bad = [{"airglow_profile": 2}, {"id": "Bad Id"}, {"name": ""}, {"lighting": None},
               {"lighting": {"effect": "lasers"}}, {"areas": {"roof": {"effect": "solid"}}},
               {"fans": {"curve": [[40, 5], [80, 100]]}},                       # below the fan minimum
               {"automations": [{"when": {"type": "idle", "minutes": 5}, "then": {"type": "preset", "preset": "other"}}]},
               {"automations": [NEON["automations"][1]] * 11},
               {"description": "x" * 201}]
        for b in bad:
            with self.assertRaises(ValueError, msg=b):
                profiles.validate_shared({**NEON, **b})

    def test_too_big_is_refused(self):
        with self.assertRaises(ValueError):
            profiles.validate_shared({**NEON, "padding": "x" * profiles.MAX_BYTES})

    def test_round_trip_through_a_local_profile(self):
        shared = profiles.validate_shared(NEON)
        local = config.validate_preset(profiles.to_preset(shared, "neon-2"), set(config.DEFAULT_CURVES))
        self.assertEqual(local["fan_points"], shared["fans"]["curve"])
        rules = profiles.rules_to_install(shared, "neon-2", set())
        self.assertEqual(rules[0]["then"], {"type": "preset", "preset": "neon-2"})
        back = profiles.to_shared(local, rules, author="Me Too")
        self.assertEqual(back["id"], "neon-me-too")                         # unique per author in the gallery
        self.assertEqual(back["areas"], shared["areas"])
        self.assertEqual(back["automations"][0]["then"]["preset"], "self")
        self.assertEqual(len(back["automations"]), 1)                     # only rules that switch to it come along
        no_extras = profiles.to_shared(local, rules, author="me", include_fans=False, include_automations=False)
        self.assertNotIn("fans", no_extras)
        self.assertNotIn("automations", no_extras)

    def test_named_curve_is_shared_as_points(self):
        p = config.validate_preset({"id": "x", "name": "X", "effect": "solid", "fan_curve": "silent"}, set(config.DEFAULT_CURVES))
        shared = profiles.to_shared(p, author="me", curves=config.DEFAULT_CURVES)
        self.assertEqual(shared["fans"]["curve"], config.DEFAULT_CURVES["silent"])

    def test_new_ids_dont_collide(self):
        self.assertEqual(profiles.new_id("Neon Dusk!", {"neon-dusk", "neon-dusk-2"}), "neon-dusk-3")

    def test_old_presets_still_load_unchanged(self):
        for p in config.DEFAULT_PRESETS:
            self.assertEqual(set(config.validate_preset(p, set(config.DEFAULT_CURVES))), set(p))


class GalleryFolderTests(unittest.TestCase):
    def test_every_profile_in_the_repo_is_valid_and_named_after_its_id(self):
        files = [f for f in (ROOT / "profiles").glob("*.json") if f.name not in ("index.json", "featured.json")]
        self.assertGreater(len(files), 0)
        for f in files:
            p = profiles.validate_shared(json.loads(f.read_text(encoding="utf-8")))
            self.assertEqual(f.stem, p["id"])
            self.assertTrue(p["author"])
        featured = json.loads((ROOT / "profiles" / "featured.json").read_text(encoding="utf-8"))
        self.assertTrue(set(featured) <= {f.stem for f in files})

    def test_share_url_is_githubs_new_file_page_with_the_profile(self):
        shared = profiles.validate_shared(NEON)
        url, filled = gallery.share_url(shared, repo="someone/airglow")
        self.assertTrue(filled)
        self.assertTrue(gallery.is_share_url(url, repo="someone/airglow"))
        q = urllib.parse.parse_qs(urllib.parse.urlsplit(url).query)
        self.assertEqual(q["filename"], ["neon.json"])
        self.assertEqual(json.loads(q["value"][0])["name"], "Neon")
        self.assertFalse(gallery.is_share_url("https://evil.example/new/main/profiles?x", repo="someone/airglow"))
        self.assertFalse(gallery.is_share_url("https://github.com/someone/airglow/new/main/effects/x?x", repo="someone/airglow"))

    def test_a_profile_too_long_for_the_address_gets_only_the_file_name(self):
        shared = profiles.validate_shared(big_profile())
        url, filled = gallery.share_url(shared)
        self.assertFalse(filled)
        self.assertLessEqual(len(url), gallery.MAX_URL)
        self.assertTrue(gallery.is_share_url(url))
        self.assertEqual(urllib.parse.parse_qs(urllib.parse.urlsplit(url).query), {"filename": ["big.json"]})
        self.assertEqual(json.loads(gallery.share_file(shared)), shared)


class AreaTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.store = config.Store(self.tmp.name)
        self.dev = FakeLightDevice("d", "Dev", zones=(("front", 4), ("top", 4)))
        self.manager = FakeManager(self.dev)
        self.manager.maintain(True)
        self.store.ensure_zones([self.dev.info()])
        for key, spot in (("d/front", "front"), ("d/top", "top")):
            cfg = self.store.zones()[key]
            self.store.set_zone(key, {**cfg, "component": {"id": None, "count": 1, "spot": spot}}, 8)
        self.engine = Engine(self.manager, self.store, async_writes=False)

    def tearDown(self):
        self.tmp.cleanup()

    def test_areas_light_the_zones_that_sit_there(self):
        self.engine.set_scene(config.validate_scene({"effect": "solid", "params": {"color": "#ff0000"}, "brightness": 100,
                                                     "areas": {"front": {"effect": "solid", "params": {"color": "#00ff00"}}}}))
        self.engine.tick()
        self.assertEqual(self.dev.last("front"), [(0, 255, 0)] * 4)
        self.assertEqual(self.dev.last("top"), [(255, 0, 0)] * 4)

    def test_your_own_zone_effect_wins_over_an_area(self):
        cfg = self.store.zones()["d/front"]
        self.store.set_zone("d/front", {**cfg, "override": {"effect": "solid", "params": {"color": "#0000ff"}}}, 8)
        self.engine.set_scene(config.validate_scene({"effect": "solid", "params": {"color": "#ff0000"},
                                                     "areas": {"front": {"effect": "solid", "params": {"color": "#00ff00"}}}}))
        self.engine.tick()
        self.assertEqual(self.dev.last("front"), [(0, 0, 255)] * 4)

    def test_zone_without_a_spot_takes_the_nearest(self):
        layout = {"shape": "strip", "leds": 3, "x": 0.2, "y": 0.5, "angle": 90, "length": 0.4}
        self.assertEqual(lightsetup.zone_spot({"layout": layout}), "front")
        self.assertEqual(lightsetup.zone_spot({"layout": {**layout, "x": 1.4, "y": 0.27}}), "rear")

    def test_preview_renders_without_touching_the_lights(self):
        self.engine.set_scene(config.validate_scene({"effect": "solid", "params": {"color": "#ff0000"}}))
        self.engine.preview(profiles.scene_of(profiles.validate_shared(NEON)))
        self.engine.tick()
        self.assertEqual(self.engine.preview_frames["d/front"], "00e600" * 4)        # at the profile's 90%
        self.assertEqual(self.dev.last("front"), [(255, 0, 0)] * 4)
        self.engine.preview(None)
        self.assertEqual(self.engine.preview_frames, {})


class TrialTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.store = config.Store(self.tmp.name)
        self.nzxt, self.strip = FakeNzxt(), FakeLightDevice("strip", "Strip")
        self.manager = FakeManager(self.nzxt, self.strip)
        self.ctrl = Controller(self.store, manager=self.manager, signals=lambda: signals(),
                               read_sensors=lambda: {"online": True, "cpu_temp": 70.0, "gpu_temp": 40.0}, async_writes=False)
        self.manager.maintain(True)
        self.store.ensure_zones([d.info() for d in self.manager.all().values()])
        self.shared = profiles.validate_shared(NEON)

    def tearDown(self):
        self.tmp.cleanup()

    def test_discard_puts_everything_back(self):
        self.ctrl.set_scene({"effect": "solid", "params": {"color": "#123456"}})
        key = "strip/z1"
        cfg = self.store.zones()[key]
        mine = {"effect": "solid", "params": {"color": "#abcdef"}}
        self.store.set_zone(key, {**cfg, "override": mine, "component": {"id": None, "count": 1, "spot": "front"}}, 16)
        before_scene, before_curve = dict(self.ctrl.base_scene), self.ctrl.base_curve
        self.ctrl.try_profile(profiles.scene_of(self.shared), "Neon", fan_points=self.shared["fans"]["curve"], clear_zones=[key])
        self.assertEqual(self.ctrl.base_scene["effect"], "gradient")
        self.assertEqual(self.ctrl.effective_curve, "trial")
        self.assertIsNone(self.store.zones()[key]["override"])
        self.ctrl.tick()
        self.assertEqual(self.ctrl.fan_target, 50)                        # the tried curve at 70 °C
        self.ctrl.end_trial()
        self.assertEqual(self.ctrl.base_scene, before_scene)
        self.assertEqual(self.ctrl.base_curve, before_curve)
        self.assertEqual(self.store.zones()[key]["override"], mine)
        self.assertIsNone(self.ctrl.trial)

    def test_keep_switches_to_the_installed_profile_and_its_curve(self):
        self.ctrl.try_profile(profiles.scene_of(self.shared), "Neon", fan_points=self.shared["fans"]["curve"])
        preset = config.validate_preset(profiles.to_preset(self.shared, "neon"), set(self.store.curves()))
        self.store.set_presets(self.store.get_presets() + [preset])
        self.ctrl.end_trial(keep_preset=preset)
        self.assertEqual(self.ctrl.active_preset, "neon")
        self.assertEqual(self.ctrl.base_curve, "profile:neon")
        self.assertEqual(self.ctrl.base_scene["areas"]["front"]["effect"], "solid")
        self.ctrl.tick()
        self.assertEqual(self.ctrl.fan_target, 50)
        self.assertEqual(self.store.state["fan_curve"], "profile:neon")   # remembered across restarts

    def test_picking_a_preset_while_trying_ends_the_trial(self):
        self.ctrl.set_fan_curve("silent")
        key = "strip/z1"
        cfg = self.store.zones()[key]
        mine = {"effect": "solid", "params": {"color": "#abcdef"}}
        self.store.set_zone(key, {**cfg, "override": mine, "component": {"id": None, "count": 1, "spot": "front"}}, 16)
        self.ctrl.try_profile(profiles.scene_of(self.shared), "Neon", fan_points=self.shared["fans"]["curve"], clear_zones=[key])
        night = next(p for p in self.store.get_presets() if p["id"] == "night")
        self.assertTrue(all(s["ok"] for s in self.ctrl.apply_preset({**night, "fan_curve": None})))  # the tray
        self.assertIsNone(self.ctrl.trial)
        self.assertEqual(self.ctrl.active_preset, "night")
        self.assertEqual(self.ctrl.base_curve, "silent")                  # the tried curve is gone, yours is back
        self.assertEqual(self.store.zones()[key]["override"], mine)
        self.assertEqual(self.store.state["active_preset"], "night")      # saved, like any preset you pick
        self.ctrl.end_trial()                                             # a late Discard has nothing to undo
        self.assertEqual(self.ctrl.active_preset, "night")
        self.assertEqual(self.ctrl.base_scene["effect"], night["effect"])

    def test_changes_while_trying_are_never_saved(self):
        self.ctrl.set_scene({"effect": "solid", "params": {"color": "#123456"}})
        self.ctrl.set_fan_curve("silent")
        self.ctrl.try_profile(profiles.scene_of(self.shared), "Neon", fan_points=self.shared["fans"]["curve"])
        self.ctrl.set_scene({"brightness": 40})                 # the Overview slider or the tray, while trying
        self.assertEqual(self.ctrl.base_scene["brightness"], 40)
        self.assertEqual(self.store.state["scene"]["effect"], "solid")
        self.assertEqual(self.store.state["fan_curve"], "silent")
        # a restart while trying comes back with what you had
        again = Controller(config.Store(self.tmp.name), manager=FakeManager(), signals=lambda: signals(),
                           read_sensors=lambda: {}, async_writes=False)
        self.assertEqual(again.base_scene["params"]["color"], "#123456")
        self.assertEqual(again.base_curve, "silent")
        self.ctrl.end_trial()
        self.assertEqual(self.store.state["scene"]["params"]["color"], "#123456")

    def test_automation_using_a_profile_brings_its_areas_and_curve(self):
        preset = config.validate_preset(profiles.to_preset(self.shared, "neon"), set(self.store.curves()))
        self.store.set_presets(self.store.get_presets() + [preset])
        from airglow.automation import validate_rule
        self.store.set_rules([{"id": "night", "when": {"type": "locked"}, "then": {"type": "preset", "preset": "neon"}}],
                             lambda r: validate_rule(r, {"neon"}, set(self.store.curves())) | {"enabled": True})
        self.ctrl.automations._signals = lambda: signals(locked=True)
        self.ctrl.tick()
        self.assertEqual(self.ctrl.effective_scene.get("areas", {}).get("front", {}).get("effect"), "solid")
        self.assertEqual(self.ctrl.effective_curve, "profile:neon")


if __name__ == "__main__":
    unittest.main()
