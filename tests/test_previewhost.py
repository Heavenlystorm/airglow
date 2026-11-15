"""Screen previews use a separate, watched effect process and never drive hardware."""

import base64
import importlib
import time
import unittest
from types import SimpleNamespace
from unittest import mock

import numpy as np

from airglow import api
from airglow.lighting import effects, geometry, studio
from airglow.lighting.engine import Engine


class PreviewHostTests(unittest.TestCase):
    def setUp(self):
        module = importlib.import_module("airglow.previewhost")
        self.spawned = []
        def spawn(url):
            self.spawned.append(url)
            return SimpleNamespace(pid=99999999, poll=lambda: None)
        self.preview = module.PreviewHost("http://127.0.0.1:12345", spawn=spawn)
        self.addCleanup(self.preview.close)

    def load(self):
        return self.preview.route("load", {"effect": {"kind": "code", "code": "airglow.effect({point(){return [1,0,0]}})",
                                                      "params": {}}, "points": [[10, 20, None, 0, 0, "test"]]})["id"]

    def test_load_uses_a_separate_process_and_profile_mode(self):
        iid = self.load()
        self.assertIn("&preview=1", self.spawned[0])
        self.assertIsNot(self.preview.host, studio.HOST)
        self.assertIn(iid, self.preview.host.instances)
        self.preview.route("stop", {"id": iid})
        self.assertNotIn(iid, self.preview.host.instances)

    def test_frame_exchange_and_late_updates_are_isolated_to_the_preview(self):
        iid = self.load()
        self.preview.frames({"frames": {iid: base64.b64encode(bytes([255, 0, 0])).decode()},
                             "details": {iid: {"ms": 1.2, "logs": ["hello"]}}})
        result = self.preview.route("tick", {"id": iid, "time": 2, "dt": 1 / 30,
                                             "updates": {"params": {"speed": 2}}})
        self.assertEqual(result["colors"], "/wAA")
        self.assertEqual(result["logs"], ["hello"])
        self.assertEqual(self.preview.host.instances[iid].params, {"speed": 2})
        self.assertEqual(self.preview.route("tick", {"id": iid, "time": 3})["logs"], [])

    def test_memory_watchdog_stops_preview_without_closing_the_main_effect_host(self):
        iid = self.load()
        self.preview.host._last_mem_check = 0
        with mock.patch.object(studio, "_tree_memory", return_value=studio.MEMORY_LIMIT + 1), \
                mock.patch.object(self.preview.host, "stop") as stop, mock.patch.object(studio.HOST, "stop") as lights:
            self.preview.maintain()
        stop.assert_called_once()
        lights.assert_not_called()
        result = self.preview.route("tick", {"id": iid})
        self.assertEqual(result["problem"]["reason"], "memory")

    def test_closed_or_idle_previews_do_not_accumulate_instances(self):
        iid = self.load()
        self.preview.host.instances[iid].last_used = time.monotonic() - 20
        self.preview.maintain()
        self.assertNotIn(iid, self.preview.host.instances)

    def test_malformed_points_and_oversized_code_are_rejected_before_spawn(self):
        for points, code in (([[float("inf"), 0]], ""), ("bad", ""), ([], "x" * 33000)):
            with self.subTest(points=points), self.assertRaises(ValueError):
                self.preview.route("load", {"effect": {"kind": "code", "code": code}, "points": points})
        self.assertEqual(self.spawned, [])

    def builtin(self, effect="solid", points=None, params=None, brightness=100):
        return self.preview.route("load", {
            "effect": {"kind": "builtin", "id": effect, "params": params or {}, "brightness": brightness},
            "points": [[0, 0]] if points is None else points,
        })["id"]

    def test_every_builtin_matches_engine_colours_on_the_same_geometry_and_inputs(self):
        layout = {"shape": "fans", "fans": 2, "leds_per_fan": 5, "x": 0.12, "y": 0.8, "size": 0.4}
        canvas, local, fans = geometry.positions(layout)
        points = [[float(x) * 200, float(y) * 200, "front", float(t), int(fan)]
                  for (x, y), t, fan in zip(canvas, local, fans)]
        audio = {"bands": [.3, .55, .7, .9], "bass": .62}

        def screen_sample(pos):
            return np.column_stack((pos[:, 0], pos[:, 1], 1 - pos[:, 0]))

        context = SimpleNamespace(cpu_temp=60, gpu_temp=60, screen=SimpleNamespace(sample=screen_sample),
                                  audio=SimpleNamespace(bands=np.array(audio["bands"]), bass=audio["bass"]))
        zone = SimpleNamespace(pos=geometry.normalised(canvas), local=local, fan=fans)
        for effect_id, renderer in effects.EFFECTS.items():
            with self.subTest(effect_id=effect_id):
                iid = self.builtin(effect_id, points, brightness=73)
                params = effects.clean_params(effect_id, {})
                expected = Engine._to_bytes(renderer.render(zone, 2.6, params, context), 73).tobytes()
                frame = self.preview.route("tick", {"id": iid, "time": 2.6, "audio": audio})
                self.assertEqual(base64.b64decode(frame["colors"]), expected)
                self.preview.route("stop", {"id": iid})
        self.assertEqual(self.spawned, [])

    def test_builtin_and_code_share_only_preview_capacity(self):
        live = {f"live-{i}": object() for i in range(studio.MAX_INSTANCES)}
        with mock.patch.object(studio.HOST, "instances", live):
            leases = [self.builtin(), self.load(), self.builtin(), self.load()]
            with self.assertRaisesRegex(ValueError, "Too many screen previews"):
                self.builtin()
            self.preview.route("stop", {"id": leases[0]})
            self.builtin()
            self.assertEqual(list(studio.HOST.instances), list(live))

    def test_builtin_lease_expires_and_stops(self):
        started = time.monotonic()
        iid = self.builtin()
        with mock.patch("airglow.previewhost.time.monotonic", return_value=started + 11):
            self.preview.maintain()
            self.assertEqual(self.preview.route("tick", {"id": iid})["problem"]["reason"], "stopped")
            iid = self.builtin()
            self.preview.route("stop", {"id": iid})
            self.assertEqual(self.preview.route("tick", {"id": iid})["problem"]["reason"], "stopped")

    def test_scene_lease_composes_zone_effects_using_the_same_brightness(self):
        effect = {"kind": "scene", "id": "solid", "brightness": 25, "zones": [
            {"key": "front", "count": 2, "id": "solid", "params": {"color": "#ff0000"}},
            {"key": "rear", "count": 1, "id": "solid", "params": {"color": "#00ff00"}},
        ]}
        points = [[0, 0], [100, 0], [150, 50]]
        iid = self.preview.route("load", {"effect": effect, "points": points})["id"]
        frame = self.preview.route("tick", {"id": iid})
        self.assertEqual(base64.b64decode(frame["colors"]), b"\x40\x00\x00" * 2 + b"\x00\x40\x00")
        self.preview.route("stop", {"id": iid})
        self.assertEqual(self.preview.route("tick", {"id": iid})["problem"]["reason"], "stopped")
        with self.assertRaisesRegex(ValueError, "LED"):
            self.preview.route("load", {"effect": effect, "points": points[:-1]})

    def test_scene_preview_stops_its_studio_children_without_releasing_live_slots(self):
        renderer = self.studio_effect()
        live = {f"live-{i}": object() for i in range(studio.MAX_INSTANCES)}
        effect = {"kind": "scene", "id": renderer.id, "brightness": 50, "zones": [
            {"key": "top", "count": 1, "id": renderer.id},
            {"key": "rear", "count": 1, "id": "solid", "params": {"color": "#00ff00"}},
        ]}
        with mock.patch.object(effects.EFFECTS, "provider", return_value=renderer), \
             mock.patch.object(studio.HOST, "instances", live):
            iid = self.preview.route("load", {"effect": effect, "points": [[0, 0], [100, 0]]})["id"]
            (child,) = self.preview.host.instances
            self.assertIsNone(self.preview.route("tick", {"id": iid})["colors"])
            self.preview.frames({"frames": {child: "/wAA"}})
            frame = self.preview.route("tick", {"id": iid})
            self.assertEqual(base64.b64decode(frame["colors"]), b"\x80\x00\x00\x00\x80\x00")
            self.preview.route("stop", {"id": iid})
            self.assertEqual(self.preview.host.instances, {})
            self.assertEqual(list(studio.HOST.instances), list(live))

    def test_scene_studio_points_match_the_engine_including_offstage_paint_addresses(self):
        renderer = self.studio_effect()
        layout = {"shape": "fans", "fans": 2, "leds_per_fan": 7, "x": 0.03162, "y": 0.01234,
                  "size": 0.23123, "angle": 17}
        canvas, local, fans = geometry.positions(layout)
        zone = SimpleNamespace(pos=geometry.normalised(canvas), local=local, fan=fans, canvas=canvas,
                               key="front", spot="top", n=len(canvas))
        expected = studio._points(zone)
        visible = [[round(float(x), 4) * 200, round(float(y), 4) * 200, "top", round(float(t), 4), int(fan)]
                   for (x, y), t, fan in zip(canvas, local, fans)]
        effect = {"kind": "scene", "id": renderer.id,
                  "zones": [{"key": "front", "count": len(visible), "id": renderer.id, "layout": layout}]}
        with mock.patch.object(effects.EFFECTS, "provider", return_value=renderer):
            self.preview.route("load", {"effect": effect, "points": visible})
        (child,) = self.preview.host.instances.values()
        self.assertTrue(any(x < 0 or y < 0 for *_, (x, y) in expected))
        self.assertEqual(child.points, expected)

    def test_builtin_rejects_unbounded_geometry_and_audio_before_render(self):
        for points in ([[10 ** 400, 0]], [[0, 0, None, "bad"]], [[0, 0, None, .5, -1]], [[0, 0]] * 4097):
            with self.subTest(points=str(points)[:30]), self.assertRaisesRegex(ValueError, "LED|preview"):
                self.builtin(points=points)
        iid = self.builtin("spectrum")
        for audio in ({"bands": []}, {"bands": [1] * 1025}, {"bands": [float("nan")]},
                      {"bands": [.5], "bass": -1}, {"bands": [.5], "beat": "false"}, ["bad"]):
            with self.subTest(audio=str(audio)[:30]), self.assertRaisesRegex(ValueError, "audio"):
                self.preview.route("tick", {"id": iid, "audio": audio})
        self.assertIsNotNone(self.preview.route("tick", {"id": iid, "audio": {"bands": [.5]}})["colors"])

    def profile_server(self):
        device = SimpleNamespace(online=True, id="strip", zones=[SimpleNamespace(id="leds")])
        controller = SimpleNamespace(
            engine=mock.Mock(spec=Engine), manager=SimpleNamespace(all=lambda: {"strip": device}))
        layout = {"shape": "strip", "x": .8, "y": .5, "length": 1, "leds": 2}
        store = SimpleNamespace(zones=lambda: {"strip/leds": {"enabled": True, "layout": layout}})
        return SimpleNamespace(ctrl=controller, store=store, hooks={}, previews=self.preview)

    def test_profile_gallery_frames_do_not_use_the_live_engine(self):
        server = self.profile_server()
        profile = {"airglow_profile": 1, "id": "test", "name": "Test", "lighting": {
            "effect": "solid", "params": {"color": "#00ff00"}, "brightness": 50}}
        self.assertEqual(api.route(server, "POST", "/api/preview", {"profile": profile}), {"ok": True})
        result = api.route(server, "GET", "/api/preview/frames", {})
        self.assertEqual(result["zones"], {"strip/leds": "008000008000"})
        api.route(server, "POST", "/api/preview", {})
        self.assertEqual(api.route(server, "GET", "/api/preview/frames", {})["zones"], {})
        self.assertEqual(server.ctrl.engine.mock_calls, [])

    def test_profile_preview_uses_its_area_and_ignores_live_overrides(self):
        server = self.profile_server()
        cfg = server.store.zones()
        cfg["strip/leds"].update(component={"spot": "top"},
                                 override={"effect": "solid", "params": {"color": "#0000ff"}})
        server.store.zones = lambda: cfg
        profile = {"airglow_profile": 1, "id": "test", "name": "Test",
                   "lighting": {"effect": "solid", "params": {"color": "#ff0000"}, "brightness": 60},
                   "areas": {"top": {"effect": "solid", "params": {"color": "#00ff00"}}}}
        api.route(server, "POST", "/api/preview", {"profile": profile})
        self.assertEqual(api.route(server, "GET", "/api/preview/frames", {})["zones"],
                         {"strip/leds": "009900009900"})

    def studio_effect(self):
        return studio.JsEffect({
            "fx_id": "fx:me.profile-test", "name": "Red", "description": "", "schema": [],
            "permissions": [], "kind": "code", "code": "airglow.effect({point(){return [1,0,0]}})",
            "layers": None, "sha256": "fixture", "origin": "mine", "author": "", "tags": [], "remix_of": None,
        })

    def test_profile_studio_preview_draws_with_four_live_slots_already_taken(self):
        server = self.profile_server()
        renderer = self.studio_effect()
        profile = {"airglow_profile": 1, "id": "test", "name": "Test", "lighting": {
            "effect": renderer.id, "params": {}, "brightness": 100}}
        live = {f"live-{i}": object() for i in range(studio.MAX_INSTANCES)}
        with mock.patch.object(effects.EFFECTS, "provider", return_value=renderer), \
             mock.patch.object(studio.HOST, "instances", live):
            api.route(server, "POST", "/api/preview", {"profile": profile})
            (iid,) = self.preview.host.instances
            self.preview.frames({"frames": {iid: "/wAA/wAA"}})
            result = api.route(server, "GET", "/api/preview/frames", {})
            self.assertEqual(result["zones"], {"strip/leds": "ff0000ff0000"})
            self.assertEqual(list(studio.HOST.instances), list(live))
        self.assertEqual(server.ctrl.engine.mock_calls, [])
