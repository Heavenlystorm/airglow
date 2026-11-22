import tempfile
import threading
import unittest
from concurrent.futures import ThreadPoolExecutor
from types import SimpleNamespace
from unittest import mock

from airglow import api, config, windowshortcut as ws


class FakeBinding:
    def __init__(self, key, pressed, owner):
        self.key, self.pressed, self.owner = key, pressed, owner
        self.registered, self.error = False, ""

    def start(self):
        if self.key in self.owner:
            raise ws.ShortcutUnavailable("Shortcut is already in use.")
        self.owner[self.key] = self
        self.registered = True

    def stop(self):
        self.registered = False
        if self.owner.get(self.key) is self:
            del self.owner[self.key]


class WindowShortcutTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.store = config.Store(tmp.name)
        self.keys, self.main, self.desk = {}, threading.Event(), threading.Event()
        self.ctrl = ws.WindowShortcut(self.store, self.main.set, self.desk.set,
                                      lambda key, cb: FakeBinding(key, cb, self.keys))
        self.addCleanup(self.ctrl.stop)
        self.server = SimpleNamespace(ctrl=None, store=self.store, hooks={"window_shortcut": self.ctrl})

    def change(self, key, action="main", *, previous=None):
        status = previous or self.ctrl.status()
        return api.route(self.server, "POST", "/api/windows-shortcut",
                         {"choice": {"key": key, "action": action}, "session": status["session"], "version": status["version"]})

    def test_disabled_default_changes_target_without_reregister_and_shutdown(self):
        self.ctrl.start()
        self.assertEqual(self.keys, {})
        active = self.change("F7")
        self.assertTrue(active["registered"])
        first = self.keys["F7"]
        first.pressed(first)
        self.assertTrue(self.main.wait(1))
        with self.ctrl._opening:
            pass  # Wait until the first dispatched action has completely returned.
        self.change("F7", "desk")
        self.assertIs(self.keys["F7"], first)
        first.pressed(first)
        self.assertTrue(self.desk.wait(1))
        self.change("F8")
        self.assertEqual(set(self.keys), {"F8"})
        self.main.clear()
        first.pressed(first)
        self.assertFalse(self.main.is_set())
        self.ctrl.stop()
        self.assertFalse(self.ctrl.status()["registered"])
        self.assertEqual(self.keys, {})
        self.assertEqual(self.store.settings()["windows_shortcut"], {"key": "F8", "action": "main"})
        with self.assertRaises(api.ApiError):
            self.change("F9")

    def test_conflict_and_failed_save_preserve_original_os_and_file(self):
        self.ctrl.start()
        self.change("F7", "desk")
        self.keys["F8"] = "another Windows application"
        with self.assertRaisesRegex(api.ApiError, "already in use"):
            self.change("F8")
        with mock.patch.object(self.store, "_write", side_effect=OSError("disk full")):
            with self.assertRaisesRegex(OSError, "disk full"):
                self.change("F9")
            with self.assertRaisesRegex(OSError, "disk full"):
                self.ctrl.reset_settings()
        self.assertEqual(set(self.keys), {"F7", "F8"})
        self.assertEqual(self.store.settings()["windows_shortcut"], {"key": "F7", "action": "desk"})
        self.assertTrue(self.ctrl.status()["registered"])
        self.ctrl.reset_settings()
        self.assertEqual(self.keys, {"F8": "another Windows application"})
        self.assertEqual(self.store.settings()["windows_shortcut"], config.SHORTCUT_OFF)

    def test_startup_conflict_can_be_retried_and_generic_settings_cannot_bypass_registration(self):
        self.store.update_settings({"windows_shortcut": {"key": "F8", "action": "desk"}})
        self.keys["F8"] = object()
        self.ctrl.start()
        status = self.ctrl.status()
        self.assertFalse(status["registered"])
        self.assertEqual(status["saved"]["key"], "F8")
        self.assertIn("already in use", status["error"])
        del self.keys["F8"]
        self.assertTrue(self.change("F8", "desk")["registered"])
        with self.assertRaises(api.ApiError):
            api.route(self.server, "POST", "/api/settings", {"windows_shortcut": {"key": "F9", "action": "main"}})
        self.assertEqual(set(self.keys), {"F8"})

    def test_racing_settings_and_browser_only_fallback(self):
        self.ctrl.start()
        before = self.ctrl.status()
        with ThreadPoolExecutor(max_workers=2) as pool:
            edits = [pool.submit(self.change, key, previous=before) for key in ("F7", "F8")]
        self.assertEqual(sum(e.exception() is None for e in edits), 1)
        self.assertEqual(len(self.keys), 1)
        self.server.hooks.clear()
        status = api.route(self.server, "GET", "/api/windows-shortcut", {})
        self.assertFalse(status["available"])
        self.assertFalse(status["registered"])
        with self.assertRaises(api.ApiError):
            self.change("off")

    def test_invalid_keys_actions_and_tokens_cannot_register(self):
        self.ctrl.start()
        for key, action in [("F12", "main"), ("Ctrl+K", "desk"), ("F8", "shell"), (None, "desk")]:
            with self.subTest(key=key, action=action), self.assertRaises(ValueError):
                self.change(key, action)
        with self.assertRaises(api.ApiError):
            self.change("F8", previous={"session": "old-process", "version": self.ctrl.status()["version"]})
        self.assertEqual(self.keys, {})


class NativeWindowShortcutTests(unittest.TestCase):
    def test_private_f23_message_conflict_delivery_and_release(self):
        # No keyboard input is generated. Only the owned fixture queue receives this synthetic WM_HOTKEY.
        # The fixture uses Ctrl+Alt+Shift+F23, outside all offered combinations, and yields to any existing owner.
        pressed, mods, vk = threading.Event(), 7, 0x86
        first = ws.NativeBinding("F23", lambda _: pressed.set(), modifiers=mods)
        try:
            first.start()
        except ws.ShortcutUnavailable as exc:
            if "already in use" in str(exc):
                self.skipTest("The fixture chord already has an owner.")
            raise
        self.addCleanup(first.stop)
        second = ws.NativeBinding("F23", lambda _: None, modifiers=mods)
        with self.assertRaisesRegex(ws.ShortcutUnavailable, "already in use"):
            second.start()
        self.assertTrue(ws.u32.PostThreadMessageW(first._thread.native_id, ws.WM_HOTKEY, 1, (vk << 16) | mods))
        self.assertTrue(pressed.wait(1))
        first.stop()
        third = ws.NativeBinding("F23", lambda _: None, modifiers=mods)
        self.addCleanup(third.stop)
        third.start()
        self.assertTrue(third.registered)
