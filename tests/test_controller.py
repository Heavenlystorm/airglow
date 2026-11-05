import tempfile
import time
import unittest
from unittest import mock

from airglow import config, devices, hw, sensors
from airglow.plugins import nzxt_hue2
from airglow.controller import Controller
from tests.fakes import FakeHidBackend, FakeHidDevice, FakeLightDevice, FakeManager, FakeNzxt, signals


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

    def test_tick_runs_fans_on_every_port_and_records_history(self):
        self.ctrl.tick()
        self.assertEqual(set(self.nzxt.hub.duties[-1]), {0, 1, 2, 3, 4})
        self.assertEqual(len(self.ctrl.history), 1)

    def test_a_tick_never_waits_on_lhm(self):
        ctrl = Controller(self.store, manager=self.manager, signals=lambda: self.sig, async_writes=False)
        with mock.patch.object(sensors, "read_tree", side_effect=AssertionError("read on the controller thread")):
            started = time.monotonic()
            ctrl.tick()
            ctrl.tick()
        self.assertLess(time.monotonic() - started, 1.0)
        self.assertFalse(ctrl.sensors["online"])
        ctrl.lhm.tree = {"Text": "PC", "Children": []}          # what the poller thread read
        ctrl.lhm.last_success = time.monotonic()
        ctrl.tick()
        ctrl.tick()
        self.assertTrue(ctrl.sensors["online"])

    def test_a_stalled_fan_still_gets_full_speed_when_hot(self):
        self.nzxt.hub.fans[1]["rpm"] = 0                     # stopped at a low speed: reads 0 RPM
        self.temps["cpu_temp"] = 95.0
        self.ctrl.tick()
        self.assertEqual(self.nzxt.hub.duties[-1][1], 100)
        self.ctrl.failsafe()
        self.assertEqual(self.nzxt.hub.duties[-1][1], 100)

    def test_failsafe_preserves_last_higher_duty_without_a_temperature(self):
        self.ctrl.fan_duty = 80
        self.ctrl.failsafe()
        self.assertEqual(set(self.nzxt.hub.duties[-1].values()), {80})

    def test_failsafe_uses_known_hot_temperature_before_the_first_fan_tick(self):
        self.ctrl.sensors = {"cpu_temp": 95.0, "gpu_temp": 40.0}
        self.ctrl.failsafe()
        self.assertEqual(set(self.nzxt.hub.duties[-1].values()), {100})

    def test_failed_fan_only_controller_is_closed_and_can_reconnect(self):
        for operation in ("read", "write"):
            with self.subTest(operation=operation):
                handle = FakeHidDevice()
                hub = devices.NzxtControlHub(FakeHidBackend(device=handle), pid=0x2011,
                                            led_channels=0, fan_ports=3)
                dev = nzxt_hue2.NzxtDevice(hub, product=next(p for p in nzxt_hue2.PRODUCTS if p.pid == 0x2011))
                manager = hw.DeviceManager(natives=[dev])
                manager.maintain(False, force=True)
                ctrl = Controller(self.store, manager=manager, read_sensors=lambda: dict(self.temps),
                                  signals=lambda: self.sig, async_writes=False)
                with mock.patch.object(handle, operation, side_effect=OSError("USB disconnected")):
                    ctrl.tick()
                self.assertFalse(dev.online)
                self.assertTrue(handle.closed)
                self.assertIn("USB disconnected", dev.error)
                manager.maintain(False, force=True)
                self.assertTrue(dev.online)
                self.assertIsNone(dev.error)
                manager.close()

    def test_new_zones_get_default_settings(self):
        zones = self.store.zones()
        self.assertIn("nzxt-hub/ch1", zones)
        self.assertTrue(zones["nzxt-hub/ch1"]["enabled"])

    def test_startup_uses_the_saved_openrgb_endpoint(self):
        self.store.update_settings({"openrgb_host": "192.0.2.5", "openrgb_port": 7777})
        with mock.patch("hid.enumerate", return_value=[]):
            ctrl = Controller(config.Store(self.tmp.name), signals=lambda: self.sig, async_writes=False)
        self.assertEqual((ctrl.manager.bridge.client.host, ctrl.manager.bridge.client.port), ("192.0.2.5", 7777))

    def test_start_delegates_initial_device_io_to_the_device_worker(self):
        with mock.patch.object(self.manager, "maintain", side_effect=AssertionError("blocking device open")), \
             mock.patch.object(self.manager, "start") as start, \
             mock.patch.object(self.ctrl.engine, "start"), \
             mock.patch.object(self.ctrl, "_warm_up_inputs"), mock.patch.object(self.ctrl, "_loop"):
            self.ctrl.start()
            self.ctrl._thread.join(1)
        start.assert_called_once()

    def test_first_sensor_success_is_published_without_an_extra_fan_cycle(self):
        import threading
        gate = threading.Event()
        self.ctrl._read_sensors = None
        self.ctrl.TICK = 0.5
        self.ctrl.lhm._read = lambda *a, **kw: (gate.wait(2) and {"Text": "PC", "Children": []}) or None
        with mock.patch.object(self.ctrl.engine, "start"), mock.patch.object(self.ctrl, "_warm_up_inputs"):
            try:
                self.ctrl.start()
                deadline = time.monotonic() + 1
                while not self.ctrl._ticks and time.monotonic() < deadline:
                    time.sleep(0.005)
                before = len(self.nzxt.hub.duties)
                gate.set()
                deadline = time.monotonic() + 0.25
                while not self.ctrl.sensors["online"] and time.monotonic() < deadline:
                    time.sleep(0.005)
                self.assertTrue(self.ctrl.sensors["online"])
                self.assertEqual(len(self.nzxt.hub.duties), before)
            finally:
                gate.set()
                self.ctrl.shutdown()

    def test_catalog_revision_tracks_identity_not_changing_values(self):
        with mock.patch.object(self.ctrl._system, "read", side_effect=[
                {"ram_total": 32, "cpu_load": 10}, {"ram_total": 32, "cpu_load": 20}]):
            self.ctrl.tick()
            first = self.ctrl.snapshot()["catalog_rev"]
            self.ctrl.tick(); self.ctrl.tick()
            self.assertEqual(self.ctrl.snapshot()["catalog_rev"], first)
        with mock.patch.object(self.ctrl._system, "read", return_value={"ram_total": 64}):
            self.ctrl._update_readings([])
        self.assertGreater(self.ctrl.snapshot()["catalog_rev"], first)

    def test_set_scene_validates_and_clears_preset(self):
        self.ctrl.apply_preset(self.store.get_presets()[0])
        self.ctrl.set_scene({"effect": "comet"})
        self.assertEqual(self.ctrl.base_scene["effect"], "comet")
        self.assertEqual(self.ctrl.base_scene["params"]["tail"], 45)          # defaults for the new effect
        self.assertIsNone(self.ctrl.active_preset)
        with self.assertRaises(ValueError):
            self.ctrl.set_scene({"effect": "nope"})

    def test_failed_manual_save_keeps_live_scene_curve_and_selection(self):
        self.ctrl.apply_preset(self.store.get_presets()[0])
        before = (self.ctrl.base_scene, self.ctrl.base_curve, self.ctrl.active_preset,
                  self.ctrl.effective_scene, self.ctrl.effective_curve)
        for action in (lambda: self.ctrl.set_scene({"effect": "comet"}),
                       lambda: self.ctrl.set_fan_curve("silent"),
                       lambda: self.ctrl.mark_preset("different"),
                       lambda: self.ctrl.apply_preset(self.store.get_presets()[1])):
            with self.subTest(action=action), mock.patch.object(self.store, "_write", side_effect=OSError("disk full")):
                with self.assertRaises(OSError):
                    action()
            self.assertEqual((self.ctrl.base_scene, self.ctrl.base_curve, self.ctrl.active_preset,
                              self.ctrl.effective_scene, self.ctrl.effective_curve), before)

    def test_control_reports_completed_cycles_and_sensor_age(self):
        self.assertIn("control", self.ctrl.snapshot())
        self.assertIsNone(self.ctrl.snapshot()["control"]["controller"]["last_success_at"])
        self.ctrl.tick()
        first = self.ctrl.snapshot()["control"]
        self.assertIsNotNone(first["controller"]["last_success_at"])
        self.assertIsNotNone(first["sensors"]["last_success_at"])
        with mock.patch.object(self.ctrl, "_run_fans", side_effect=RuntimeError("blocked output")):
            self.ctrl.tick()  # every other cycle has no fan work
            completed = self.ctrl.snapshot()["control"]["controller"]["last_success_at"]
            with self.assertRaises(RuntimeError):
                self.ctrl.tick()
        self.assertEqual(self.ctrl.snapshot()["control"]["controller"]["last_success_at"], completed)

    def test_lightweight_tick_does_not_claim_a_failed_fan_cycle_recovered(self):
        self.ctrl.tick()
        self.ctrl.tick()
        with mock.patch.object(self.ctrl, "_run_fans", side_effect=RuntimeError("blocked output")):
            with self.assertRaises(RuntimeError):
                self.ctrl.tick()
        self.ctrl.tick()  # no sensors/fans on this tick
        self.assertEqual(self.ctrl.snapshot()["control"]["controller"]["state"], "failed")
        self.ctrl.tick()
        self.assertEqual(self.ctrl.snapshot()["control"]["controller"]["state"], "healthy")

    def test_recording_and_replay_do_not_issue_fan_commands(self):
        self.assertTrue(hasattr(self.ctrl, "cooling"))
        self.ctrl.cooling.start("Idle baseline")
        self.assertEqual(self.nzxt.hub.duties, [])
        self.ctrl.tick()
        commands = list(self.nzxt.hub.duties)
        session = self.ctrl.cooling.stop()
        replay = self.ctrl.cooling.replay(session["session"]["id"], [[20, 30], [80, 100]], "cpu")
        self.assertEqual(len(replay["samples"]), 1)
        self.assertEqual(self.nzxt.hub.duties, commands)

    def test_failed_trial_discard_preserves_the_live_trial(self):
        self.ctrl.try_profile({"effect": "solid", "params": {"color": "#ff0000"}, "brightness": 70}, "Test")
        before = self.ctrl.base_scene
        with mock.patch.object(self.store, "_write", side_effect=OSError("disk full")):
            with self.assertRaises(OSError):
                self.ctrl.end_trial()
        self.assertIsNotNone(self.ctrl.trial)
        self.assertEqual(self.ctrl.base_scene, before)

    def test_failed_keep_of_installed_profile_preserves_trial_and_zone_override(self):
        key = "strip/z1"
        override = {"effect": "solid", "params": {"color": "#abcdef"}}
        self.store.set_zone(key, {**self.store.zones()[key], "override": override}, 16)
        self.ctrl.try_profile({"effect": "solid", "params": {"color": "#ff0000"}, "brightness": 70},
                              "Test", clear_zones=[key])
        before = self.ctrl.base_scene
        write = self.store._write
        def fail_state(name, data):
            if name == "state.json":
                raise OSError("disk full")
            return write(name, data)
        with mock.patch.object(self.store, "_write", side_effect=fail_state):
            with self.assertRaises(OSError):
                self.ctrl.end_trial(keep_preset=self.store.get_presets()[0])
        self.assertIsNotNone(self.ctrl.trial)
        self.assertEqual(self.ctrl.base_scene, before)
        self.assertEqual(self.store.zones()[key]["override"], override)

    def test_recording_failure_does_not_stop_fan_cycles(self):
        self.ctrl.cooling.start("Baseline")
        with mock.patch.object(self.ctrl.cooling, "sample", side_effect=OSError("recording failed")):
            self.ctrl.tick()
        self.assertEqual(len(self.nzxt.hub.duties), 1)
        self.assertEqual(self.ctrl.snapshot()["control"]["controller"]["state"], "healthy")
        self.assertTrue(any(e["resource"] == "cooling" for e in self.ctrl.control.events()))

    def test_recording_shutdown_failure_cannot_skip_fan_failsafe(self):
        self.ctrl.cooling.start("Baseline")
        with mock.patch.object(self.ctrl.cooling, "stop", side_effect=OSError("disk unavailable")):
            self.ctrl.shutdown()
        self.assertEqual(len(self.nzxt.hub.duties), 1)
        self.assertEqual(set(self.nzxt.hub.duties[-1].values()), {self.store.settings()["fan_failsafe_duty"]})
        self.assertFalse(self.nzxt.online)

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
        self.ctrl.tick()
        self.temps = {"online": False, "cpu_temp": None, "gpu_temp": None}
        for _ in range(40):
            self.ctrl.tick()
        self.assertEqual(self.ctrl.fan_duty, self.store.settings()["fan_failsafe_duty"])

    def test_start_without_temperatures_runs_fans_quietly(self):
        # LibreHardwareMonitor not answering yet at sign-in: bottom of the curve, not the failsafe speed
        self.ctrl.set_fan_curve("silent")
        self.temps = {"online": False, "cpu_temp": None, "gpu_temp": None}
        self.ctrl.tick()
        lowest = self.store.curves()["silent"][0][1]
        self.assertEqual(self.ctrl.fan_duty, lowest)
        self.assertLess(lowest, self.store.settings()["fan_failsafe_duty"])
        self.assertEqual(set(self.nzxt.hub.duties[-1].values()), {lowest})

    def test_start_goes_to_failsafe_if_temperatures_never_come(self):
        self.temps = {"online": False, "cpu_temp": None, "gpu_temp": None}
        self.ctrl.tick()
        self.ctrl._started -= Controller.STARTUP_GRACE
        for _ in range(40):
            self.ctrl.tick()
        self.assertEqual(self.ctrl.fan_duty, self.store.settings()["fan_failsafe_duty"])

    def test_first_temperature_takes_over_from_the_start_speed(self):
        self.ctrl.set_fan_curve("balanced")
        self.temps = {"online": False, "cpu_temp": None, "gpu_temp": None}
        self.ctrl.tick()
        self.temps = {"online": True, "cpu_temp": 40.0, "gpu_temp": 35.0}
        self.ctrl.tick(); self.ctrl.tick()
        self.assertEqual(self.ctrl.fan_temp, 40.0)
        self.assertEqual(self.ctrl.fan_target, self.store.curves()["balanced"][0][1])

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

    def test_shutdown_lights_stay_dark_on_later_frames(self):
        self.ctrl.set_scene({"effect": "rainbow"})
        self.ctrl.engine.tick()
        self.ctrl._on_shutdown()
        self.ctrl.engine.tick()                               # the engine keeps running until the process ends
        self.assertEqual(set(map(tuple, self.strip.last("z1"))), {(0, 0, 0)})
        self.ctrl.engine._last = {k: (raw, sent - 10) for k, (raw, sent) in self.ctrl.engine._last.items()}
        self.ctrl.engine.tick()                               # keep-alive re-send
        self.assertEqual(set(map(tuple, self.strip.last("z1"))), {(0, 0, 0)})
        self.assertTrue(self.ctrl.base_scene["on"])           # not saved as off

    def test_cancelled_shutdown_brings_lights_back(self):
        self.ctrl._on_shutdown()
        self.assertFalse(self.ctrl.effective_scene["on"])
        self.ctrl._on_shutdown_cancelled()
        self.assertTrue(self.ctrl.effective_scene["on"])

    def test_shutdown_leaves_lights_alone_when_setting_is_off(self):
        self.store.update_settings({"lights_off_on_sleep": False})
        self.ctrl._on_shutdown()
        self.assertTrue(self.ctrl.effective_scene["on"])

    def test_lock_turns_lights_off_only_when_enabled(self):
        self.ctrl._on_lock(True)
        self.assertTrue(self.ctrl.effective_scene["on"])
        self.store.update_settings({"lights_off_on_lock": True})
        self.ctrl._on_lock(True)
        self.assertFalse(self.ctrl.effective_scene["on"])
        self.ctrl._on_lock(False)
        self.assertTrue(self.ctrl.effective_scene["on"])

    def _run_loop_ticks(self, count):
        waits = []
        clock = [time.monotonic()]

        def wait(_timeout):
            waits.append(self.ctrl._ticks)
            clock[0] += _timeout
            if len(waits) == count:
                self.ctrl._stop.set()

        with mock.patch.object(self.ctrl._sensor_ready, "wait", side_effect=wait), \
             mock.patch("airglow.controller.time.monotonic", side_effect=lambda: clock[0]):
            self.ctrl._loop()

    def test_loop_failsafe_after_repeated_sensor_errors_before_shutdown(self):
        self.ctrl._read_sensors = mock.Mock(side_effect=ValueError("bad sensor response"))
        with self.assertLogs("airglow.controller", "ERROR"):
            self._run_loop_ticks(9)                         # five failed reads, with four successful even ticks
        failsafe = self.store.settings()["fan_failsafe_duty"]
        self.assertEqual(self.nzxt.hub.duties, [{p: failsafe for p in range(5)}])

    def test_loop_failsafe_after_repeated_fan_errors_before_shutdown(self):
        with mock.patch.object(self.ctrl, "_run_fans", side_effect=ValueError("bad fan response")), \
                self.assertLogs("airglow.controller", "ERROR"):
            self._run_loop_ticks(9)
        failsafe = self.store.settings()["fan_failsafe_duty"]
        self.assertEqual(self.nzxt.hub.duties, [{p: failsafe for p in range(5)}])

    def test_successful_fan_cycle_resets_the_failure_streak(self):
        self.ctrl._read_sensors = mock.Mock(side_effect=(
            [ValueError("bad sensor response")] * 4 + [dict(self.temps)] + [ValueError("bad sensor response")] * 4))
        with mock.patch.object(self.ctrl, "failsafe") as failsafe, self.assertLogs("airglow.controller", "ERROR"):
            self._run_loop_ticks(17)
        failsafe.assert_not_called()
        self.assertEqual(len(self.nzxt.hub.duties), 1)       # the one successful fan cycle, no shutdown involved

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

    def test_fan_readings_average_reported_curve_and_steady_fans_including_zero_rpm(self):
        self.nzxt.close()
        self.ctrl.fan_duty = 33
        self.ctrl.fan_channels = [
            {"id": "/fan/steady", "fan_id": "/rpm/steady", "mode": "steady", "pump": False, "duty": 80, "rpm": 0},
            {"id": "/fan/curve", "fan_id": "/rpm/curve", "mode": "curve", "pump": False, "duty": 40, "rpm": 900},
            {"id": "/fan/bios", "mode": "bios", "pump": False, "duty": 100, "rpm": 2000},
            {"id": "/pump", "mode": "steady", "pump": True, "duty": 100, "rpm": 2000},
        ]
        readings = {r["id"]: r for r in self.ctrl._app_readings()}
        self.assertEqual(readings["app/fan_duty"]["value"], 60)
        self.assertEqual(readings["app/fan_rpm"]["value"], 450)
        self.assertEqual(readings["app/fan_duty"]["source_ids"], ["/fan/steady", "/fan/curve"])

    def test_only_a_steady_pump_does_not_report_the_unused_case_fan_curve(self):
        self.nzxt.close()
        self.ctrl.fan_duty = 33
        self.ctrl.fan_channels = [{"id": "/pump", "mode": "steady", "pump": True, "duty": 80, "rpm": 2000}]
        readings = {r["id"]: r for r in self.ctrl._app_readings()}
        self.assertIsNone(readings["app/fan_duty"]["value"])
        self.assertIsNone(readings["app/fan_rpm"]["value"])

    def test_cost_per_hour_follows_power_and_the_tariff(self):
        self.store.update_settings({"energy_price": 0.3797})
        self.ctrl.tick()
        cost = self.ctrl.readings["app/energy_cost"]
        self.assertAlmostEqual(cost["value"], round(150 / 1000 * 0.3797, 3))     # cpu 60 + gpu 90
        self.assertEqual((cost["short"], cost["source"], cost["needs_lhm"]), ("Cost", "CPU + GPU", True))
        self.temps["total_power"] = 200.0                                        # an explicit total wins
        self.ctrl.tick()                                                         # sensors are read every other tick
        self.ctrl.tick()
        self.assertAlmostEqual(self.ctrl.readings["app/energy_cost"]["value"], round(0.2 * 0.3797, 3))

    def test_cost_per_hour_requires_both_power_readings_including_zero(self):
        for cpu, gpu, expected in ((65, None, None), (None, 20, None), (None, None, None), (0, 0, 0)):
            with self.subTest(cpu=cpu, gpu=gpu):
                self.ctrl.sensors = {"cpu_power": cpu, "gpu_power": gpu, "total_power": None}
                cost = next(r for r in self.ctrl._app_readings() if r["id"] == "app/energy_cost")
                self.assertEqual(cost["value"], expected)

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
