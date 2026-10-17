import http.client
import json
import tempfile
import threading
import time
import unittest
import urllib.parse

from airglow import api, config
from airglow.controller import Controller
from tests.fakes import FakeLightDevice, FakeManager, FakeNzxt, signals


class ApiTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        cls.store = config.Store(cls.tmp.name)
        cls.manager = FakeManager(FakeNzxt(), FakeLightDevice("strip", "Strip"))
        cls.ctrl = Controller(cls.store, manager=cls.manager, signals=lambda: signals(),
                              read_sensors=lambda: {"online": True, "cpu_temp": 45.0, "gpu_temp": 35.0}, async_writes=False)
        cls.manager.maintain(True)
        cls.ctrl.tick()
        cls.ctrl.engine.tick()
        cls.shown = threading.Event()
        cls.server = api.Server(0, cls.ctrl, cls.store, {"show_window": cls.shown.set})
        cls.port = cls.server.server_address[1]
        threading.Thread(target=cls.server.serve_forever, daemon=True).start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.tmp.cleanup()

    def request(self, method, path, body=None, headers=None, host=None):
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=5)
        h = {"Host": host or f"127.0.0.1:{self.port}"}
        if method == "POST":
            h.update({"Content-Type": "application/json", "X-Airglow": "1"})
        h.update(headers or {})
        conn.request(method, path, json.dumps(body).encode() if body is not None else None, h)
        resp = conn.getresponse()
        data = resp.read()
        conn.close()
        try:
            return resp.status, json.loads(data)
        except ValueError:
            return resp.status, data

    def test_event_stream_sends_state_then_frames(self):
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=5)
        conn.request("GET", "/api/stream?frames=1", headers={"Host": f"127.0.0.1:{self.port}"})
        resp = conn.getresponse()
        self.assertEqual(resp.status, 200)
        self.assertTrue(resp.getheader("Content-Type").startswith("text/event-stream"))
        events, current = {}, None
        while len(events) < 2:
            line = resp.fp.readline().decode().rstrip("\n")
            if line.startswith("event: "):
                current = line[7:]
            elif line.startswith("data: ") and current:
                events[current] = json.loads(line[6:])
        conn.close()
        self.assertIn("scene", events["state"])
        self.assertIn("zones", events["frames"])

    def test_event_stream_skips_frames_that_did_not_change(self):
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=5)
        conn.request("GET", "/api/stream?frames=1", headers={"Host": f"127.0.0.1:{self.port}"})
        resp = conn.getresponse()

        def next_frames():
            current = None
            while True:
                line = resp.fp.readline().decode().rstrip("\n")
                if line.startswith("event: "):
                    current = line[7:]
                elif line.startswith("data: ") and current == "frames":
                    return json.loads(line[6:])

        engine = self.ctrl.engine
        saved = engine.frames
        try:
            next_frames()
            for _ in range(4):                     # new frame ids, same colours: nothing to send
                engine.frames = dict(engine.frames)
                engine.frame_id += 1
                time.sleep(api.STREAM_FRAME_EVERY * 2)
            engine.frames = {**engine.frames, "test/z": "ff0000"}
            engine.frame_id += 1
            self.assertEqual(next_frames()["zones"].get("test/z"), "ff0000")
        finally:
            conn.close()
            engine.frames = saved

    def test_setup_lists_pending_zones_catalog_and_spots(self):
        status, data = self.request("GET", "/api/setup")
        self.assertEqual(status, 200)
        self.assertIn("strip/z1", data["pending"])
        self.assertTrue(any(p["id"] == "generic-argb-strip" for p in data["catalog"]))
        self.assertIn("front", [s["id"] for s in data["spots"]])
        self.assertGreaterEqual(self.request("GET", "/api/state")[1]["setup_pending"], 1)

    def test_setup_probe_checks_its_input(self):
        ok = self.request("POST", "/api/setup/probe", {"zone": "strip/z1", "mode": "count", "n": 3})
        self.assertEqual(ok, (200, {"ok": True, "max_leds": 16}))
        self.assertEqual(self.ctrl.engine._probe["strip/z1"][:2], ("count", 3))
        self.request("POST", "/api/setup/probe", {"zone": "strip/z1", "mode": "off"})
        self.assertNotIn("strip/z1", self.ctrl.engine._probe)
        for body in ({"zone": "strip/z1", "mode": "rave"}, {"zone": "strip/z1", "mode": "count", "n": 17},
                     {"zone": "strip/z1", "mode": "count", "n": "3"}):
            self.assertEqual(self.request("POST", "/api/setup/probe", body)[0], 400, body)
        self.assertEqual(self.request("POST", "/api/setup/probe", {"zone": "nope/z1", "mode": "off"})[0], 404)

    def test_setup_apply_saves_the_zone_and_clears_it_from_pending(self):
        status, data = self.request("POST", "/api/setup/apply",
                                    {"zone": "strip/z2", "component": "generic-argb-strip", "leds": 7, "spot": "top"})
        self.assertEqual(status, 200, data)
        self.assertEqual((data["config"]["name"], data["config"]["layout"]["leds"]), ("Top strip", 7))
        self.assertNotIn("strip/z2", self.request("GET", "/api/setup")[1]["pending"])
        bad = self.request("POST", "/api/setup/apply", {"zone": "strip/z2", "component": "generic-argb-strip",
                                                        "leds": 99, "spot": "top"})
        self.assertEqual(bad[0], 400)

    def test_event_stream_checks_the_host_too(self):
        self.assertEqual(self.request("GET", "/api/stream", host="evil.example")[0], 403)

    def test_state(self):
        status, s = self.request("GET", "/api/state")
        self.assertEqual(status, 200)
        for key in ("scene", "effective", "devices", "fans", "conflicts", "onboarded", "temp_unit"):
            self.assertIn(key, s)

    def test_state_carries_the_overviews_readings(self):
        status, s = self.request("GET", "/api/state")
        self.assertIn("auto/cpu_temp", s["readings"])
        self.assertIn("auto/ram_load", s["readings"])

    def test_sensors_and_history(self):
        status, cat = self.request("GET", "/api/sensors")
        self.assertEqual(status, 200)
        ids = {m["id"] for m in cat["sensors"]}
        self.assertIn("auto/gpu_temp", ids)
        self.assertIn("app/fan_duty", ids)
        status, h = self.request("GET", "/api/sensors/history?ids=auto/ram_load,app/fan_duty,unknown")
        self.assertEqual(len(h["series"]["auto/ram_load"]), len(h["t"]))
        self.assertNotIn("unknown", h["series"])

    def test_overview_round_trip_and_reset(self):
        layout = {"tiles": [{"id": "t", "type": "sensor", "sensors": ["sys/net_down"], "style": "graph", "w": 2, "h": 1}],
                  "pins": ["sys/net_down"]}
        status, saved = self.request("POST", "/api/overview", layout)
        self.assertEqual(status, 200)
        self.assertEqual(self.request("GET", "/api/overview")[1], saved)
        status, err = self.request("POST", "/api/overview", {"tiles": [{"id": "t", "type": "nope"}]})
        self.assertEqual(status, 400)
        status, reset = self.request("POST", "/api/overview/reset", {})
        self.assertEqual(reset, config.DEFAULT_OVERVIEW)

    def test_effects_catalog(self):
        status, cat = self.request("GET", "/api/effects")
        self.assertIn("rainbow", {e["id"] for e in cat})

    def test_scene_and_validation(self):
        status, r = self.request("POST", "/api/scene", {"effect": "solid", "params": {"color": "#00ff88"}, "brightness": 40})
        self.assertEqual((status, r["scene"]["params"]["color"]), (200, "#00ff88"))
        status, err = self.request("POST", "/api/scene", {"params": {"color": "nope"}})
        self.assertEqual(status, 400)
        self.assertIn("colour", err["error"])

    def test_zones_and_identify(self):
        status, z = self.request("GET", "/api/zones")
        keys = [zone["key"] for zone in z["zones"]]
        self.assertIn("nzxt-hub/ch1", keys)
        ch1 = next(zone for zone in z["zones"] if zone["key"] == "nzxt-hub/ch1")
        self.assertEqual(len(ch1["positions"]), 24)
        enc = urllib.parse.quote("nzxt-hub/ch1", safe="")
        cfg = dict(ch1["config"], name="Front fans")
        cfg["layout"] = dict(cfg["layout"], x=0.4)
        status, r = self.request("POST", f"/api/zones/{enc}", cfg)
        self.assertEqual((status, r["config"]["name"]), (200, "Front fans"))
        self.assertEqual(self.request("POST", f"/api/zones/{enc}/identify", {})[0], 200)
        self.assertEqual(self.request("POST", "/api/zones/nope%2Fz9", cfg)[0], 404)

    def test_frames(self):
        status, f = self.request("GET", "/api/frames")
        self.assertEqual(status, 200)
        self.assertTrue(all(len(v) % 6 == 0 for v in f["zones"].values()))

    def test_post_without_header_is_rejected(self):
        status, _ = self.request("POST", "/api/scene", {"on": False}, headers={"X-Airglow": ""})
        self.assertEqual(status, 403)
        self.assertTrue(self.ctrl.base_scene["on"])

    def test_foreign_host_is_rejected(self):
        self.assertEqual(self.request("GET", "/api/state", host="evil.example:80")[0], 403)

    def test_curves(self):
        status, r = self.request("POST", "/api/curves/silent", {"points": [[30, 20], [80, 90]]})
        self.assertEqual((status, r["points"]), (200, [[30, 20], [80, 90]]))
        self.assertEqual(self.request("POST", "/api/curves/silent", {"points": [[30, 5]]})[0], 400)
        self.assertEqual(self.request("POST", "/api/curves/silent/reset", {})[1]["points"], config.DEFAULT_CURVES["silent"])

    def test_presets_apply_and_save_current(self):
        status, r = self.request("POST", "/api/presets/gaming/apply", {})
        self.assertTrue(r["ok"])
        self.assertEqual(self.ctrl.active_preset, "gaming")
        self.assertEqual(self.request("POST", "/api/presets/nope/apply", {})[0], 404)
        status, r = self.request("POST", "/api/presets/from-current", {"name": "My look"})
        self.assertEqual(status, 200)
        self.assertIn(r["id"], [p["id"] for p in r["presets"]])
        self.store.set_presets([p for p in self.store.get_presets() if p["id"] != r["id"]])

    def test_automations(self):
        rules = [{"id": "night", "when": {"type": "time", "start": "23:00", "end": "07:00"}, "then": {"type": "lights_off"}}]
        status, saved = self.request("POST", "/api/automations", {"rules": rules})
        self.assertEqual((status, saved[0]["enabled"]), (200, True))
        bad = [{"id": "x", "when": {"type": "app_running", "exe": "nope"}, "then": {"type": "lights_off"}}]
        self.assertEqual(self.request("POST", "/api/automations", {"rules": bad})[0], 400)
        status, r = self.request("POST", "/api/automations/pause", {"minutes": 30})
        self.assertTrue(r["paused_until"])
        self.request("POST", "/api/automations/pause", {"minutes": 0})
        self.request("POST", "/api/automations", {"rules": []})

    def test_settings_update(self):
        status, s = self.request("POST", "/api/settings", {"temp_unit": "F", "lights_off_on_lock": True})
        self.assertEqual((status, s["settings"]["temp_unit"], s["settings"]["lights_off_on_lock"]), (200, "F", True))
        self.request("POST", "/api/settings", {"temp_unit": "C", "lights_off_on_lock": False})
        self.assertEqual(self.request("POST", "/api/settings", {"fan_failsafe_duty": 5})[0], 400)

    def test_onboarded_and_show_window(self):
        self.request("POST", "/api/onboarded", {})
        self.assertTrue(self.store.state["onboarded"])
        self.request("POST", "/api/window/show", {})
        self.assertTrue(self.shown.wait(1))

    def test_static_files_and_traversal(self):
        status, body = self.request("GET", "/")
        self.assertEqual(status, 200)
        self.assertIn(b"<!doctype html>", body.lower())
        self.assertEqual(self.request("GET", "/../airglow/api.py")[0], 404)
        self.assertEqual(self.request("GET", "/%2e%2e/run.py")[0], 404)

    def test_non_object_body_is_a_400(self):
        status, r = self.request("POST", "/api/automations/pause", [1, 2])
        self.assertEqual(status, 400)
        self.assertIn("object", r["error"])



class PortFallbackTests(unittest.TestCase):
    def test_taken_port_moves_to_the_next_and_is_recorded(self):
        import socket
        from pathlib import Path
        from unittest import mock
        from airglow import app
        with tempfile.TemporaryDirectory() as tmp:
            store = config.Store(tmp)
            ctrl = Controller(store, manager=FakeManager(), signals=signals, read_sensors=dict, async_writes=False)
            blocker = socket.socket()
            if hasattr(socket, "SO_EXCLUSIVEADDRUSE"):
                blocker.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
            blocker.bind(("127.0.0.1", 0))
            blocker.listen(1)
            taken = blocker.getsockname()[1]
            record = Path(tmp, "port")
            with mock.patch.object(app, "port_file", return_value=record):
                server = app.start_server(taken, ctrl, store, {}, tries=5)
                try:
                    self.assertIsNotNone(server)
                    self.assertGreater(server.server_address[1], taken)
                    self.assertEqual(app.running_port(1), server.server_address[1])
                finally:
                    server.server_close()
                    blocker.close()


if __name__ == "__main__":
    unittest.main()
