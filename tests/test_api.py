import http.client
import json
import tempfile
import threading
import time
import unittest
import urllib.parse
from unittest import mock

from airglow import api, config, tools
from airglow.controller import Controller
from tests.fakes import FakeLightDevice, FakeManager, FakeNzxt, signals
from tests.test_profiles import big_profile


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
        cls.designs = []
        cls.server = api.Server(0, cls.ctrl, cls.store, {"show_window": cls.shown.set,
                                                         "design_changed": cls.designs.append})
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

    def test_fan_channels_only_take_fans_lhm_lists(self):
        status, body = self.request("POST", "/api/fans/channel", {"id": "/lpc/nct6798d/0/control/0", "mode": "curve"})
        self.assertEqual(status, 404)
        status, body = self.request("POST", "/api/fans/channel", {"id": "/lpc/nct6798d/0/control/0", "mode": "off"})
        self.assertEqual((status, body), (200, {"channels": []}))         # leaving one alone always works
        status, body = self.request("POST", "/api/fans/channel", {"id": "nonsense", "mode": "off"})
        self.assertEqual(status, 400)

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

    def test_gallery_lists_and_fetches_from_the_configured_folder(self):
        import os
        from pathlib import Path
        old = os.environ.get("AIRGLOW_GALLERY_URL")
        os.environ["AIRGLOW_GALLERY_URL"] = (Path(__file__).resolve().parent.parent / "profiles").as_uri()
        try:
            status, data = self.request("GET", "/api/gallery")
            self.assertEqual(status, 200, data)
            self.assertEqual(data["profiles"][0]["id"], data["featured"][0])
            status, p = self.request("GET", f"/api/gallery/profile?id={data['profiles'][0]['id']}")
            self.assertEqual(status, 200, p)
            self.assertIn("badges", p)
            self.assertEqual(self.request("GET", "/api/gallery/profile?id=../secrets")[0], 400)
        finally:
            if old is None:
                os.environ.pop("AIRGLOW_GALLERY_URL", None)
            else:
                os.environ["AIRGLOW_GALLERY_URL"] = old

    def test_try_keep_and_share_a_profile(self):
        profile = {"airglow_profile": 1, "id": "api-test", "name": "API Test", "author": "me",
                   "lighting": {"effect": "solid", "params": {"color": "#00ff00"}, "brightness": 50},
                   "automations": [{"when": {"type": "idle", "minutes": 5}, "then": {"type": "preset", "preset": "self"}}]}
        before = dict(self.ctrl.base_scene)
        self.assertEqual(self.request("POST", "/api/profiles/try", {"profile": profile})[0], 200)
        self.assertEqual(self.request("GET", "/api/state")[1]["trial"], {"name": "API Test", "fans": False, "kind": "profile"})
        self.request("POST", "/api/profiles/discard", {})
        self.assertEqual(self.ctrl.base_scene, before)
        self.request("POST", "/api/profiles/try", {"profile": profile})
        status, r = self.request("POST", "/api/profiles/keep", {"profile": profile, "include_automations": True})
        self.assertEqual(status, 200, r)
        self.assertEqual(self.ctrl.active_preset, r["id"])
        self.assertTrue(any(x["then"].get("preset") == r["id"] for x in self.store.get_rules()))
        shown = []
        self.server.hooks["open_url"] = shown.append
        status, s = self.request("POST", "/api/profiles/share", {"id": r["id"], "author": "me", "include_automations": True})
        self.assertEqual(status, 200, s)
        self.assertTrue(s["filled"])
        self.assertEqual(shown, [s["url"]])
        self.assertEqual(s["profile"]["automations"][0]["then"]["preset"], "self")
        self.assertEqual(self.request("POST", "/api/profiles/share", {"id": r["id"]})[0], 400)      # needs an author
        self.store.set_rules([], lambda x: x)
        self.store.set_presets([p for p in self.store.get_presets() if p["id"] != r["id"]])
        self.ctrl.presets_changed()

    def test_share_a_profile_too_long_for_the_address(self):
        profile = big_profile()
        status, r = self.request("POST", "/api/profiles/keep", {"profile": profile, "include_fans": True,
                                                                 "include_automations": True})
        self.assertEqual(status, 200, r)
        shown = []
        self.server.hooks["open_url"] = shown.append
        status, s = self.request("POST", "/api/profiles/share", {"id": r["id"], "author": "me", "include_fans": True,
                                                                  "include_automations": True, "open": False})
        self.assertEqual(status, 200, s)
        self.assertFalse(s["filled"])
        self.assertEqual(shown, [])                                                  # the page opens it, once copied
        self.assertNotIn("value=", s["url"])
        self.assertEqual(json.loads(s["source"]), s["profile"])
        self.assertEqual(len(s["profile"]["automations"]), 10)
        self.assertEqual(s["path"], f"profiles/{s['profile']['id']}.json")
        self.assertEqual(self.request("POST", "/api/profiles/open-share", {"url": s["url"]})[0], 200)
        self.assertEqual(shown, [s["url"]])
        self.assertEqual(self.request("POST", "/api/profiles/open-share", {"url": "https://evil.example/"})[0], 400)
        self.assertEqual(shown, [s["url"]])
        self.store.set_rules([], lambda x: x)
        self.store.set_presets([p for p in self.store.get_presets() if p["id"] != r["id"]])
        self.ctrl.presets_changed()

    def test_preview_and_save_current_with_areas(self):
        profile = {"airglow_profile": 1, "id": "p", "name": "P", "lighting": {"effect": "solid", "params": {}}}
        self.assertEqual(self.request("POST", "/api/preview", {"profile": profile})[0], 200)
        self.ctrl.engine.tick()
        self.assertTrue(self.request("GET", "/api/preview/frames")[1]["zones"])
        self.request("POST", "/api/preview", {})
        self.assertEqual(self.request("GET", "/api/preview/frames")[1]["zones"], {})
        cfg = self.store.zones()["strip/z1"]
        self.store.set_zone("strip/z1", {**cfg, "override": {"effect": "solid", "params": {"color": "#0000ff"}},
                                         "component": {"id": None, "count": 1, "spot": "top"}}, 16)
        status, r = self.request("POST", "/api/presets/from-current", {"name": "With areas"})
        self.assertEqual(status, 200, r)
        saved = next(p for p in r["presets"] if p["id"] == r["id"])
        self.assertEqual(saved["areas"]["top"]["params"]["color"], "#0000ff")
        self.store.config["zones"] = {**self.store.config["zones"], "strip/z1": cfg}    # as it was, still "auto"
        self.store.set_presets([p for p in self.store.get_presets() if p["id"] != r["id"]])
        self.ctrl.presets_changed()

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

    def test_saving_the_same_name_twice_gives_two_profiles(self):
        ids = [self.request("POST", "/api/presets/from-current", {"name": "Twice"})[1]["id"] for _ in range(2)]
        self.assertEqual(ids, ["twice", "twice-2"])
        self.store.set_presets([p for p in self.store.get_presets() if p["id"] not in ids])

    def test_saving_past_24_profiles_says_why(self):
        before = self.store.get_presets()
        self.store.set_presets(before + [{"id": f"p{n}", "name": f"P{n}"} for n in range(24 - len(before))])
        try:
            status, r = self.request("POST", "/api/presets/from-current", {"name": "One more"})
            self.assertEqual(status, 400)
            self.assertIn("24 profiles", r["error"])
        finally:
            self.store.set_presets(before)

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

    def test_malformed_automations_are_refused_not_crashes(self):
        before = self.store.get_rules()
        lights_off = {"type": "lights_off"}
        for when, then in [({"type": "idle", "minutes": None}, lights_off),
                           ({"type": "temp_above", "temp": [80]}, lights_off),
                           ({"type": "locked"}, {"type": "dim", "brightness": {}}),
                           ({"type": "locked"}, {"type": "preset", "preset": ["gaming"]}),
                           ({"type": "locked"}, {"type": "fan_curve", "curve": {"x": 1}}),
                           ("locked", lights_off)]:
            status, r = self.request("POST", "/api/automations", {"rules": [{"id": "x", "when": when, "then": then}]})
            self.assertEqual(status, 400, (when, then, r))
        for minutes in (None, "soon", [5], True):
            self.assertEqual(self.request("POST", "/api/automations/pause", {"minutes": minutes})[0], 400, minutes)
        self.assertEqual(self.store.get_rules(), before)

    def test_settings_update(self):
        status, s = self.request("POST", "/api/settings", {"temp_unit": "F", "lights_off_on_lock": True})
        self.assertEqual((status, s["settings"]["temp_unit"], s["settings"]["lights_off_on_lock"]), (200, "F", True))
        self.request("POST", "/api/settings", {"temp_unit": "C", "lights_off_on_lock": False})
        self.assertEqual(self.request("POST", "/api/settings", {"fan_failsafe_duty": 5})[0], 400)

    def test_settings_take_a_design_and_an_energy_price(self):
        self.designs.clear()
        status, s = self.request("POST", "/api/settings", {"design": "amber", "energy_price": 0.41})
        self.assertEqual((status, s["settings"]["design"], s["settings"]["energy_price"]), (200, "amber", 0.41))
        self.assertEqual(self.request("POST", "/api/settings", {"design": "nope"})[0], 400)
        self.assertEqual(self.request("POST", "/api/settings", {"energy_price": 0})[0], 400)
        self.assertEqual(self.designs, ["amber"], "the window's title bar is told about the new design")
        self.request("POST", "/api/settings", {"design": "graphite", "energy_price": 0.3797})

    def test_settings_reset_puts_them_back(self):
        self.request("POST", "/api/settings", {"fan_failsafe_duty": 70, "temp_unit": "F"})
        status, s = self.request("POST", "/api/settings/reset", {})
        self.assertEqual(status, 200)
        self.assertEqual((s["settings"]["fan_failsafe_duty"], s["settings"]["temp_unit"]), (30, "C"))

    def test_openrgb_start_explains_when_it_cannot(self):
        with mock.patch("airglow.tools.launch", side_effect=tools.SetupError("OpenRGB isn't set up on this PC")):
            status, body = self.request("POST", "/api/openrgb/start", {})
        self.assertEqual(status, 409)
        self.assertIn("OpenRGB", body["error"])

    def test_state_says_whether_openrgb_is_reachable(self):
        state = self.request("GET", "/api/state")[1]
        self.assertIn("connected", state["openrgb"])
        self.assertIn("failsafe", state["fans"])

    def test_going_online_is_asked_once_and_off_by_default(self):
        self.assertFalse(self.store.settings()["online_checks"])
        self.assertFalse(self.request("GET", "/api/state")[1]["online_asked"])
        status, s = self.request("POST", "/api/settings", {"online_checks": False, "update_channel": "beta"})
        self.assertEqual((status, s["settings"]["update_channel"]), (200, "beta"))
        self.assertTrue(self.request("GET", "/api/state")[1]["online_asked"])
        self.assertEqual(self.request("POST", "/api/settings", {"update_channel": "nightly"})[0], 400)
        self.request("POST", "/api/settings", {"update_channel": "stable"})

    def test_update_routes_need_an_updater(self):
        self.assertEqual(self.request("POST", "/api/updates/check", {})[0], 404)
        self.assertEqual(self.request("POST", "/api/updates/restart", {})[0], 404)

    def test_onboarded_and_show_window(self):
        self.request("POST", "/api/onboarded", {})
        self.assertTrue(self.store.state["onboarded"])
        self.request("POST", "/api/window/show", {})
        self.assertTrue(self.shown.wait(1))

    def test_static_files_and_traversal(self):
        status, body = self.request("GET", "/")
        self.assertEqual(status, 200)
        self.assertIn(b"<!doctype html>", body.lower())
        # static assets are revalidated, not always re-downloaded
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=5)
        conn.request("GET", "/css/app.css", headers={"Host": f"127.0.0.1:{self.port}"})
        resp = conn.getresponse()
        self.assertEqual(resp.status, 200)
        etag = resp.getheader("ETag")
        self.assertTrue(etag)
        self.assertEqual(resp.getheader("Cache-Control"), "no-cache")
        resp.read()
        conn.close()
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=5)
        conn.request("GET", "/css/app.css", headers={"Host": f"127.0.0.1:{self.port}", "If-None-Match": etag})
        resp = conn.getresponse()
        self.assertEqual(resp.status, 304)
        self.assertEqual(resp.read(), b"")
        self.assertEqual(resp.getheader("ETag"), etag)
        conn.close()
        self.assertEqual(self.request("GET", "/../airglow/api.py")[0], 404)
        self.assertEqual(self.request("GET", "/%2e%2e/run.py")[0], 404)

    def test_a_rejected_post_body_is_not_run_as_a_second_request(self):
        # A page may POST here with a CORS-simple content type and no X-Airglow header. Its body must never be
        # read as the next request on that keep-alive connection, or it could smuggle that header in.
        import socket
        before = self.ctrl.base_curve
        inner_body = b'{"name": "silent"}'
        smuggled = (b"POST /api/fan_curve HTTP/1.1\r\nHost: 127.0.0.1:%d\r\nX-Airglow: 1\r\n"
                    b"Content-Type: application/json\r\nContent-Length: %d\r\n\r\n" % (self.port, len(inner_body)))
        smuggled += inner_body
        outer = (b"POST /api/scene HTTP/1.1\r\nHost: 127.0.0.1:%d\r\nContent-Type: text/plain\r\n"
                 b"Content-Length: %d\r\n\r\n" % (self.port, len(smuggled))) + smuggled
        sock = socket.create_connection(("127.0.0.1", self.port), timeout=5)
        try:
            sock.sendall(outer)
            sock.settimeout(2)
            data = b""
            try:
                while True:
                    chunk = sock.recv(65536)
                    if not chunk:
                        break
                    data += chunk
            except socket.timeout:
                pass
        finally:
            sock.close()
        self.assertIn(b"403", data)
        self.assertEqual(data.count(b"HTTP/1.1"), 1, "the body was parsed as a second request")
        self.assertEqual(self.ctrl.base_curve, before, "the smuggled request changed the fan curve")

    def test_the_effect_frame_may_only_load_this_servers_scripts(self):
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=5)
        conn.request("GET", "/runtime/frame.html", headers={"Host": f"127.0.0.1:{self.port}"})
        resp = conn.getresponse()
        csp = resp.getheader("Content-Security-Policy")
        conn.close()
        self.assertNotIn("127.0.0.1:*", csp, "an effect could load a script from any local port")
        self.assertIn(f"127.0.0.1:{self.port}", csp)

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
