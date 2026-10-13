import tempfile
import time
import unittest

from airglow import config
from airglow.controller import Controller
from tests.fakes import FakeLightDevice, FakeManager, FakeNzxt, signals


class ControllerTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.store = config.Store(self.tmp.name)
        self.nzxt, self.strip = FakeNzxt(), FakeLightDevice("strip", "Strip")
        self.manager = FakeManager(self.nzxt, self.strip)
        self.temps = {"online": True, "cpu_temp": 50.0, "gpu_temp": 40.0, "cpu_power": 60.0, "gpu_power": 90.0}
        self.sig = signals()
        self.ctrl = Controller(self.store, manager=self.manager, read_sensors=lambda: dict(self.temps),
                               signals=lambda: self.sig, async_writes=False)
        self.manager.maintain(True)
        self.store.ensure_zones([d.info() for d in self.manager.all().values()])

    def tearDown(self):
        self.tmp.cleanup()

    def test_tick_runs_fans_on_connected_ports_and_records_history(self):
        self.ctrl.tick()
        self.assertEqual(set(self.nzxt.hub.duties[-1]), {0, 1, 4})
        self.assertEqual(len(self.ctrl.history), 1)

    def test_new_zones_get_default_settings(self):
        zones = self.store.zones()
        self.assertIn("nzxt-hub/ch1", zones)
        self.assertTrue(zones["nzxt-hub/ch1"]["enabled"])

    def test_set_scene_validates_and_clears_preset(self):
        self.ctrl.apply_preset(self.store.get_presets()[0])
        self.ctrl.set_scene({"effect": "comet"})
        self.assertEqual(self.ctrl.base_scene["effect"], "comet")
        self.assertEqual(self.ctrl.base_scene["params"]["tail"], 45)          # defaults for the new effect
        self.assertIsNone(self.ctrl.active_preset)
        with self.assertRaises(ValueError):
            self.ctrl.set_scene({"effect": "nope"})

    def test_lights_off_keeps_preset(self):
        self.ctrl.apply_preset(self.store.get_presets()[1])
        self.ctrl.set_scene({"on": False})
        self.assertFalse(self.ctrl.effective_scene["on"])
        self.assertEqual(self.ctrl.active_preset, self.store.get_presets()[1]["id"])

    def test_fans_follow_curve_with_ramp_limit(self):
        self.ctrl.set_fan_curve("balanced")
        self.ctrl.tick()
        first = self.ctrl.fan_duty
        self.temps["cpu_temp"] = 85.0
        self.ctrl.tick(); self.ctrl.tick()                      # sensors/fans run every other tick
        self.assertEqual(self.ctrl.fan_duty, first + self.store.settings()["fan_max_step"])

    def test_sensor_loss_goes_to_failsafe_speed(self):
        self.temps = {"online": False, "cpu_temp": None, "gpu_temp": None}
        for _ in range(40):
            self.ctrl.tick()
        self.assertEqual(self.ctrl.fan_duty, self.store.settings()["fan_failsafe_duty"])

    def test_automation_overrides_then_releases(self):
        self.store.set_rules([{"id": "night", "when": {"type": "locked"}, "then": {"type": "lights_off"}},
                              {"id": "quiet", "when": {"type": "locked"}, "then": {"type": "fan_curve", "curve": "silent"}}],
                             lambda r: r | {"enabled": True})
        self.sig = signals(locked=True)
        self.ctrl.tick()
        self.assertFalse(self.ctrl.effective_scene["on"])
        self.assertEqual(self.ctrl.effective_curve, "silent")
        self.assertTrue(self.ctrl.base_scene["on"])                         # the user's own setting is untouched
        self.sig = signals(locked=False)
        self.ctrl.tick()
        self.assertTrue(self.ctrl.effective_scene["on"])
        self.assertEqual(self.ctrl.effective_curve, self.ctrl.base_curve)

    def test_automation_preset_and_pause(self):
        gaming = next(p for p in self.store.get_presets() if p["id"] == "gaming")
        self.store.set_rules([{"id": "game", "enabled": True, "when": {"type": "app_running", "exe": "game.exe"},
                               "then": {"type": "preset", "preset": "gaming"}}], lambda r: r)
        self.sig = signals(running=frozenset({"game.exe"}))
        self.ctrl.tick()
        self.assertEqual(self.ctrl.effective_scene["effect"], gaming["effect"])
        self.assertEqual(self.ctrl.effective_curve, gaming["fan_curve"])
        self.ctrl.automations.pause(60)
        self.ctrl.tick()
        self.assertEqual(self.ctrl.effective_scene["effect"], self.ctrl.base_scene["effect"])

    def test_apply_preset_partial_failure(self):
        preset = {"id": "x", "name": "X", "effect": "solid", "params": {"color": "#00ff00"}, "brightness": 100,
                  "fan_curve": "silent", "fan_profile": "Missing.json"}
        self.store.update_settings({"fancontrol_exe": "C:\\nope\\FanControl.exe"})
        steps = self.ctrl.apply_preset(preset)
        self.assertEqual([s["ok"] for s in steps], [True, True, False])
        self.assertIsNone(self.ctrl.active_preset)
        self.assertEqual(self.ctrl.base_curve, "silent")

    def test_sleep_blacks_out_and_wake_restores(self):
        self.ctrl.engine.tick()
        self.ctrl._on_suspend()
        self.assertEqual(set(map(tuple, self.strip.last("z1"))), {(0, 0, 0)})
        self.assertFalse(self.ctrl.effective_scene["on"])
        self.ctrl._on_resume()
        self.assertTrue(self.ctrl.effective_scene["on"])
        self.assertGreaterEqual(self.manager.resets, 1)

    def test_lock_turns_lights_off_only_when_enabled(self):
        self.ctrl._on_lock(True)
        self.assertTrue(self.ctrl.effective_scene["on"])
        self.store.update_settings({"lights_off_on_lock": True})
        self.ctrl._on_lock(True)
        self.assertFalse(self.ctrl.effective_scene["on"])
        self.ctrl._on_lock(False)
        self.assertTrue(self.ctrl.effective_scene["on"])

    def test_loop_failsafe_after_repeated_errors(self):
        self.ctrl.TICK = 0.01
        self.ctrl._read_sensors = lambda: 1 / 0
        self.ctrl.start()
        time.sleep(0.4)
        self.ctrl.shutdown()
        failsafe = self.store.settings()["fan_failsafe_duty"]
        self.assertIn({0: failsafe, 1: failsafe, 4: failsafe}, self.nzxt.hub.duties)

    def test_snapshot_shape(self):
        self.ctrl.tick()
        s = self.ctrl.snapshot()
        for key in ("scene", "effective", "override", "fans", "sensors", "devices", "openrgb", "inputs", "healthy"):
            self.assertIn(key, s)
        self.assertTrue(s["fans"]["hub_online"])

    def test_app_readings_have_short_labels(self):
        self.ctrl.tick()
        for sid in ("app/fan_duty", "app/fan_rpm"):
            self.assertTrue(self.ctrl.readings[sid]["short"])

    def test_editing_a_preset_in_use_by_a_rule_takes_effect(self):
        self.store.set_rules([{"id": "game", "enabled": True, "when": {"type": "app_running", "exe": "game.exe"},
                               "then": {"type": "preset", "preset": "gaming"}}], lambda r: r)
        self.sig = signals(running=frozenset({"game.exe"}))
        self.ctrl.tick()
        presets = self.store.get_presets()
        next(p for p in presets if p["id"] == "gaming")["brightness"] = 33
        self.store.set_presets(presets)
        self.ctrl.presets_changed()
        self.assertEqual(self.ctrl.effective_scene["brightness"], 33)

    def test_deleting_the_active_preset_clears_it(self):
        self.ctrl.apply_preset(self.store.get_presets()[0])
        self.store.set_presets(self.store.get_presets()[1:])
        self.ctrl.presets_changed()
        self.assertIsNone(self.ctrl.active_preset)
        self.assertIsNone(self.store.state["active_preset"])

    def test_mark_preset_is_saved(self):
        self.ctrl.mark_preset("balanced")
        self.assertEqual(config.Store(self.tmp.name).state["active_preset"], "balanced")


if __name__ == "__main__":
    unittest.main()
