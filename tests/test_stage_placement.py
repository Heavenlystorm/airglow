import copy
import tempfile
import threading
import unittest
from types import SimpleNamespace
from unittest.mock import Mock, patch

from airglow import api, config
from airglow.stageplacement import clean_placements
from tests.fakes import FakeLightDevice, FakeManager


class StagePlacementTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.store = config.Store(tmp.name)
        self.device = FakeLightDevice()
        manager = FakeManager(self.device)
        manager.maintain(True)
        self.store.ensure_zones([self.device.info()])
        self.ctrl = SimpleNamespace(_lock=threading.RLock(), manager=manager, engine=Mock())
        self.server = SimpleNamespace(store=self.store, ctrl=self.ctrl, hooks={})
        self.keys = list(self.store.zones())
        self.items = []
        for i, key in enumerate(self.keys):
            expected = {axis: self.store.zones()[key]["layout"][axis] for axis in ("x", "y", "angle")}
            self.items.append({"key": key, "expected": expected, "position": {"x": .25, "y": .1 + .1 * i, "angle": 45}})

    def save(self):
        return api.route(self.server, "POST", "/api/stage/placement", {"placements": self.items})

    def test_one_atomic_write_preserves_later_wiring_and_replays(self):
        key = self.keys[0]
        self.store.set_zone(key, {**self.store.zones()[key], "order": "GRB", "name": "Renamed"}, 100)
        revision = self.store.zones_rev
        with patch.object(self.store, "_save_config", wraps=self.store._save_config) as persist:
            self.assertEqual(self.save()["changed"], self.keys)
            self.assertEqual(self.save()["changed"], [])
            self.assertEqual(persist.call_count, 1)
        self.assertEqual(self.store.zones_rev, revision + 1)
        self.assertEqual(self.store.zones()[key]["order"], "GRB")
        self.assertEqual(self.store.zones()[key]["name"], "Renamed")
        self.assertEqual(self.ctrl.engine.invalidate.call_count, len(self.keys))

    def test_concurrent_move_rejects_the_whole_group(self):
        before = copy.deepcopy(self.store.zones())
        self.items[-1]["expected"]["x"] = -0.1
        with self.assertRaises(api.ApiError) as error:
            self.save()
        self.assertEqual(error.exception.status, 409)
        self.assertEqual(self.store.zones(), before)
        self.ctrl.engine.invalidate.assert_not_called()

    def test_failed_persistence_and_disconnection_keep_previous_positions(self):
        before = copy.deepcopy(self.store.zones())
        revision = self.store.zones_rev
        with patch.object(self.store, "_save_config", side_effect=OSError("disk full")), self.assertRaises(OSError):
            self.save()
        self.device.close()
        with self.assertRaises(api.ApiError) as error:
            self.save()
        self.assertEqual(error.exception.status, 409)
        self.assertEqual(self.store.zones(), before)
        self.assertEqual(self.store.zones_rev, revision)
        self.ctrl.engine.invalidate.assert_not_called()

    def test_malformed_or_repeated_input_rejected_before_storage(self):
        with self.assertRaises(ValueError):
            clean_placements({"placements": self.items * 2})
        for invalid in (float("nan"), None, True, 80):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                clean_placements({"placements": [{**self.items[0], "position": {"x": invalid, "y": 0, "angle": 0}}]})
