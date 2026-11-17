import tempfile
import unittest
from concurrent.futures import ThreadPoolExecutor

from airglow.config import Store


class QuickActionsTest(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.directory = directory.name
        self.store = Store(directory.name)

    def test_pins_persist_by_kind_and_id_independently_of_recents(self):
        for kind in ("effect", "profile", "effect"):
            self.store.update_quick_action("pin", kind, "night")
        expected = [{"kind": kind, "id": "night"} for kind in ("effect", "profile")]
        self.assertEqual(Store(self.directory).settings()["quick_actions"], {"favorites": expected, "recents": []})
        self.store.update_quick_action("use", "profile", "night")
        saved = self.store.update_quick_action("unpin", "profile", "night")
        self.assertEqual(saved, {"favorites": expected[:1], "recents": expected[1:]})

    def test_recents_move_to_front_and_keep_eight_unique_entries(self):
        for i in range(11):
            self.store.update_quick_action("use", "effect", str(i))
        saved = self.store.update_quick_action("use", "effect", "5")
        self.assertEqual([item["id"] for item in saved["recents"]], ["5", "10", "9", "8", "7", "6", "4", "3"])

    def test_simultaneous_pins_survive_and_invalid_items_leave_preferences_intact(self):
        with ThreadPoolExecutor(max_workers=4) as pool:
            list(pool.map(lambda i: self.store.update_quick_action("pin", "effect", str(i)), range(12)))
        saved = self.store.settings()["quick_actions"]
        self.assertEqual(len(saved["favorites"]), 12)
        for change in [("pin", "page", "fans"), ("pin", "effect", ""), ("remove", "effect", "0")]:
            with self.assertRaises(ValueError):
                self.store.update_quick_action(*change)
        self.assertEqual(Store(self.directory).settings()["quick_actions"], saved)
