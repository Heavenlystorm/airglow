"""Screen previews use a separate, watched effect process and never drive hardware."""

import base64
import importlib
import time
import unittest
from types import SimpleNamespace
from unittest import mock

from airglow.lighting import studio


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
