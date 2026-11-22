import copy
import struct
import tempfile
import time
import unittest
from concurrent.futures import ThreadPoolExecutor
from types import SimpleNamespace
from unittest import mock

from airglow import api, config, hw, openrgb
from airglow.controller import Controller
from airglow.lighting.engine import Engine
from airglow.tray import Tray
from tests.fakes import FakeLightDevice, FakeManager, FakeNzxt, signals
from tests.test_openrgb import FakeServer


class WindDownTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.store = config.Store(tmp.name)
        self.nzxt, self.lamp = FakeNzxt(), FakeLightDevice("d", "Lamp", zones=(("z1", 6),))
        self.manager = FakeManager(self.nzxt, self.lamp)
        self.manager.maintain(True)
        self.store.ensure_zones([dev.info() for dev in self.manager.all().values()])
        self.store.update_settings({"lighting_transition_ms": 0})
        self.ctrl = Controller(self.store, manager=self.manager, read_sensors=lambda: {}, signals=signals,
                               async_writes=False)
        self.engine = self.ctrl.engine
        self.now = int(time.monotonic()) + 1
        clock = mock.patch("airglow.lighting.engine.time.monotonic", side_effect=lambda: self.now)
        clock.start()
        self.addCleanup(clock.stop)
        self.ctrl.set_scene({"effect": "solid", "params": {"color": "#ffffff"}, "brightness": 80, "on": True})
        self.engine.tick()
        self.ctrl.scene_response()

    def request(self, method="GET", **body):
        return api.route(SimpleNamespace(ctrl=self.ctrl, store=self.store, hooks={}), method,
                         "/api/lighting/wind-down", body)

    def check_pixel(self, rgb):
        self.engine.tick()
        self.assertEqual(self.lamp.last("z1"), [rgb] * 6)
        self.assertEqual(self.engine.frames["d/z1"], bytes(rgb).hex() * 6)

    def test_linear_absolute_ceiling_restores_intervening_edits_and_holds_at_zero(self):
        started = self.request("POST", action="start", minutes=5)
        self.assertEqual((started["initial"], started["remaining"]), (80, 300))
        self.now += 150
        self.check_pixel((102, 102, 102))
        self.ctrl.set_scene({"brightness": 20})
        self.check_pixel((51, 51, 51))
        self.ctrl.set_scene({"brightness": 90, "params": {"color": "#ff0000"}})
        self.check_pixel((102, 0, 0))
        self.now += 150
        self.check_pixel((0, 0, 0))
        self.assertEqual(self.request()["phase"], "held")
        self.ctrl.set_scene({"params": {"color": "#0000ff"}})
        self.check_pixel((0, 0, 0))
        count = len(self.lamp.writes)
        self.now += .5
        self.check_pixel((0, 0, 0))
        self.assertEqual(len(self.lamp.writes), count)
        self.request("POST", action="cancel", id=started["id"])
        self.check_pixel((0, 0, 230))

    def test_active_rules_saved_settings_revisions_and_fans(self):
        self.ctrl.automations.light_rule = {"id": "dim", "then": {"type": "dim", "brightness": 60}}
        self.ctrl.automations.fan_rule = {"id": "fan", "then": {"type": "fan_curve", "curve": "quiet"}}
        self.ctrl.apply()
        self.check_pixel((153, 153, 153))
        saved, revision = copy.deepcopy(self.store.state), (self.ctrl.rev, self.engine.scene_revision)
        duties, curve = list(self.nzxt.hub.duties), self.ctrl.effective_curve
        timer = self.request("POST", action="start", minutes=120)
        self.now += 3600
        self.check_pixel((77, 77, 77))
        state = self.ctrl.snapshot()
        self.assertEqual((state["scene"]["brightness"], state["effective"]["brightness"], state["wind_down"]["underlying"]),
                         (80, 30, 60))
        self.assertEqual((self.ctrl.rev, self.engine.scene_revision), revision)
        self.assertEqual(self.store.state, saved)
        self.ctrl.automations.light_rule = {"id": "off", "then": {"type": "lights_off"}}
        self.ctrl.apply()
        self.check_pixel((0, 0, 0))
        self.request("POST", action="cancel", id=timer["id"])
        self.check_pixel((0, 0, 0))
        self.assertEqual((self.ctrl.effective_curve, self.nzxt.hub.duties), (curve, duties))
        self.assertEqual(self.store.state, saved)

    def test_crossfade_uses_visible_level_and_does_not_cap_previous_frames_twice(self):
        self.engine.set_scene({**self.engine.scene, "brightness": 20}, transition_ms=1000)
        self.check_pixel((204, 204, 204))
        self.now += .5
        self.check_pixel((128, 128, 128))
        timer = self.request("POST", action="start", minutes=5)
        self.assertEqual(timer["initial"], 50)
        self.check_pixel((128, 128, 128))
        self.now += 150
        self.check_pixel((51, 51, 51))
        self.engine.set_scene({**self.engine.scene, "brightness": 100}, transition_ms=1000)
        self.check_pixel((51, 51, 51))
        self.now += .5
        self.check_pixel((64, 64, 64))
        self.engine.set_scene({**self.engine.scene, "brightness": 90}, transition_ms=1000)
        self.check_pixel((64, 64, 64))

    def test_power_and_lock_holds_take_precedence_and_restart_releases(self):
        self.store.update_settings({"lights_off_on_sleep": True, "lights_off_on_lock": True})
        timer = self.request("POST", action="start", minutes=5)
        self.now += 150
        self.ctrl._on_lock(True)
        self.check_pixel((0, 0, 0))
        self.ctrl._on_lock(False)
        self.check_pixel((102, 102, 102))
        self.ctrl._on_suspend()
        self.check_pixel((0, 0, 0))
        self.request("POST", action="cancel", id=timer["id"])
        self.check_pixel((0, 0, 0))
        with self.assertRaises(ValueError):
            self.request("POST", action="start", minutes=5)
        self.ctrl._on_resume()
        self.check_pixel((204, 204, 204))
        self.request("POST", action="start", minutes=5)
        self.now += 300
        self.check_pixel((0, 0, 0))
        fresh = Engine(self.manager, self.store, async_writes=False)
        self.assertEqual(fresh.wind_down()["phase"], "idle")
        self.assertNotEqual(fresh.wind_down()["session"], timer["session"])
        fresh.tick()
        self.assertEqual(self.lamp.last("z1"), [(204, 204, 204)] * 6)

    def test_full_length_setup_flash_is_cleared_and_cannot_override_power_or_timer_hold(self):
        self.store.update_settings({"lights_off_on_lock": True})
        timer = self.request("POST", action="start", minutes=5)
        self.engine.probe("d/z1", "count", 12)
        self.engine.tick()
        self.assertTrue(any(self.lamp.last("z1")[-1]))
        self.ctrl._on_lock(True)
        self.engine.tick()
        self.assertEqual(self.lamp.last("z1"), [(0, 0, 0)] * 12)
        with self.assertRaisesRegex(ValueError, "off"):
            self.engine.probe("d/z1", "count", 12)
        self.ctrl._on_lock(False)
        self.now += 300
        with self.assertRaisesRegex(ValueError, "Wind down"):
            self.engine.identify("d/z1")
        self.engine.probe("d/z1", "off")
        self.request("POST", action="cancel", id=timer["id"])
        self.check_pixel((204, 204, 204))

    def test_manual_power_remains_editable_and_tray_reports_the_actual_hold(self):
        self.store.update_settings({"lights_off_on_lock": True})
        tray = SimpleNamespace(ctrl=self.ctrl, store=self.store)
        self.ctrl.automations.light_rule = {"id": "dim", "then": {"type": "dim", "brightness": 60}}
        self.ctrl.apply()
        self.engine.tick()
        timer = self.request("POST", action="start", minutes=5)
        self.assertIn("Wind down active", Tray._lights_text(tray))
        self.now += 300
        self.check_pixel((0, 0, 0))
        self.assertIn("holding at zero", Tray._lights_text(tray))
        self.ctrl._on_lock(True)
        self.assertIn("while locked", Tray._lights_text(tray))
        self.ctrl._on_suspend()
        self.assertTrue(self.ctrl.snapshot()["override"]["power"])
        self.assertIn("Windows power", Tray._lights_text(tray))
        self.ctrl._on_resume()
        self.ctrl._on_lock(False)
        self.ctrl.set_scene({"on": False})
        self.request("POST", action="cancel", id=timer["id"])
        self.check_pixel((0, 0, 0))
        self.assertFalse(self.ctrl.scene_response()["scene"]["on"])

    def test_openrgb_protocol_fallback_and_reconnect_obey_the_cap_independently_of_missing_or_hot_readings(self):
        def connect():
            server = FakeServer(server_version=5, drift=True)
            server.start()
            self.addCleanup(server.join, 2)
            bridge = hw.OpenRGBBridge(port=server.port)
            self.addCleanup(bridge.client.close)
            self.manager.bridge = bridge
            bridge.refresh(True)
            self.assertEqual(len(bridge.devices), 1)
            self.store.ensure_zones([d.info() for d in bridge.devices.values()])
            requests = [struct.unpack("<I", data)[0] for _, pid, data in server.received if pid == openrgb.REQUEST_CONTROLLER_DATA]
            self.assertEqual(requests, [5, 3])
            return bridge, server

        def frames(bridge, server):
            # Read a server response after tick's socket writes. All prior packets are now in its trace.
            self.engine.tick()
            bridge.client.controllers()
            return [data for _, pid, data in server.received if pid == openrgb.UPDATE_LEDS]

        bridge, server = connect()
        self.ctrl.sensors = {"online": False, "cpu_temp": None, "gpu_temp": None}
        self.assertEqual(frames(bridge, server), [openrgb.encode_leds([(204, 204, 204)] * 24)])
        timer = self.request("POST", action="start", minutes=5)
        self.now += 150
        self.assertEqual(frames(bridge, server)[-1], openrgb.encode_leds([(102, 102, 102)] * 24))
        self.assertEqual(self.nzxt.hub.duties, [])
        self.ctrl.sensors = {"online": True, "cpu_temp": 95, "gpu_temp": 41}
        self.ctrl._run_fans()
        self.assertEqual(set(self.nzxt.hub.duties[-1].values()), {100})
        bridge.refresh(False)
        self.engine.tick()
        self.now += 150
        self.assertEqual(self.request()["phase"], "held")
        self.ctrl.sensors = {"online": False, "cpu_temp": None, "gpu_temp": None}
        self.ctrl._run_fans()
        self.assertEqual(set(self.nzxt.hub.duties[-1].values()), {100})
        before = list(self.nzxt.hub.duties)
        bridge, server = connect()
        dark = openrgb.encode_leds([(0, 0, 0)] * 24)
        self.assertEqual(frames(bridge, server), [dark])
        self.assertEqual(frames(bridge, server), [dark])
        self.request("POST", action="cancel", id=timer["id"])
        self.assertEqual(frames(bridge, server), [dark, openrgb.encode_leds([(204, 204, 204)] * 24)])
        self.assertEqual(self.nzxt.hub.duties, before)

    def test_racing_commands_and_stale_generations_never_replace_a_new_timer(self):
        idle = self.request()
        for invalid in [4, 121, 5.5, "30", True, None]:
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                self.request("POST", action="start", minutes=invalid)
        def start():
            return self.request("POST", action="start", minutes=5, session=idle["session"], version=idle["version"])
        with ThreadPoolExecutor(max_workers=2) as pool:
            attempts = [pool.submit(start) for _ in range(2)]
        self.assertEqual(sum(t.exception() is None for t in attempts), 1)
        first = self.request()
        self.request("POST", action="cancel", id=first["id"])
        with self.assertRaises(ValueError):
            start()
        second = self.request("POST", action="start", minutes=5)
        with self.assertRaises(ValueError):
            self.request("POST", action="cancel", id=first["id"])
        self.assertEqual(self.request()["id"], second["id"])
