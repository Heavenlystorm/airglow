"""One user-selected RegisterHotKey binding. Each registration belongs to its own Windows message queue."""

import ctypes
import logging
import secrets
import threading
from ctypes import wintypes

from . import config, system

log = logging.getLogger(__name__)
WM_HOTKEY, WM_QUIT = 0x312, 0x12
CTRL_SHIFT, NO_REPEAT = 0x0006, 0x4000
_ID = 1
u32 = system.user32
u32.RegisterHotKey.argtypes = [wintypes.HWND, ctypes.c_int, wintypes.UINT, wintypes.UINT]
u32.UnregisterHotKey.argtypes = [wintypes.HWND, ctypes.c_int]
u32.PostThreadMessageW.argtypes = [wintypes.DWORD, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
u32.PeekMessageW.argtypes = [ctypes.POINTER(wintypes.MSG), wintypes.HWND, wintypes.UINT, wintypes.UINT, wintypes.UINT]


class ShortcutUnavailable(RuntimeError):
    pass


class NativeBinding:
    def __init__(self, key, pressed, *, modifiers=CTRL_SHIFT):
        self._vk, self._mods = 0x70 + int(key[1:]) - 1, modifiers
        self._pressed = pressed
        self._ready, self._stop = threading.Event(), threading.Event()
        self._thread = threading.Thread(target=self._listen, name="window-shortcut", daemon=True)
        self.registered, self.error = False, ""

    def start(self):
        self._thread.start()
        if not self._ready.wait(3):
            self.stop()
            raise ShortcutUnavailable("The Windows shortcut service did not answer.")
        if not self.registered:
            raise ShortcutUnavailable(self.error or "Windows shortcut registration stopped.")

    def _listen(self):
        msg = wintypes.MSG()
        try:
            # A thread has no message queue until its first USER call. Create it before signaling readiness.
            u32.PeekMessageW(ctypes.byref(msg), None, 0, 0, 0)
            if self._stop.is_set():
                return
            if not u32.RegisterHotKey(None, _ID, self._mods | NO_REPEAT, self._vk):
                code = ctypes.get_last_error()
                self.error = ("This shortcut is already in use. Choose another combination." if code == 1409
                              else f"Windows could not register this shortcut (error {code}).")
                return
            self.registered = True
            self._ready.set()
            while not self._stop.is_set():
                result = u32.GetMessageW(ctypes.byref(msg), None, 0, 0)
                if result <= 0:
                    if result < 0:
                        self.error = "Windows stopped delivering shortcut messages. Try registering it again."
                    break
                if msg.message == WM_HOTKEY and msg.wParam == _ID and msg.lParam >> 16 == self._vk and (
                        msg.lParam & 0x000f) == self._mods:
                    self._pressed(self)
        except OSError as exc:
            self.error = f"Windows shortcut registration is unavailable. {exc}"
        finally:
            if self.registered:
                u32.UnregisterHotKey(None, _ID)
            self.registered = False
            self._ready.set()

    def stop(self):
        self._stop.set()
        if self._thread.is_alive():
            u32.PostThreadMessageW(self._thread.native_id, WM_QUIT, 0, 0)
            self._thread.join(3)


class WindowShortcut:
    def __init__(self, store, show_main, show_desk, factory=NativeBinding):
        self.store, self._factory = store, factory
        self._actions = {"main": show_main, "desk": show_desk}
        self._changes, self._opening = threading.RLock(), threading.Lock()
        self._active = None
        self._started, self._closed, self._error = False, False, ""
        self._session, self._version = secrets.token_hex(8), 0

    def start(self):
        with self._changes:
            if self._closed or self._started:
                return
            self._started = True
            choice = self.store.settings()["windows_shortcut"]
            try:
                self._replace(choice)
            except ShortcutUnavailable as exc:
                self._error = str(exc)

    def _pressed(self, sender):
        # GUI calls can block on WinForms. Leave the registration thread free to receive its quit message.
        active = self._active
        if self._closed or not active or sender is not active[0] or not self._opening.acquire(False):
            return

        def open_window():
            try:
                if not self._closed and self._active is active:
                    self._actions[active[1]["action"]]()
            except Exception:
                log.exception("could not open the shortcut window")
            finally:
                self._opening.release()
        try:
            threading.Thread(target=open_window, name="shortcut-open-window", daemon=True).start()
        except RuntimeError:
            self._opening.release()
            log.exception("could not dispatch the Windows shortcut")

    def status(self):
        with self._changes:
            saved = self.store.settings()["windows_shortcut"]
            binding = self._active[0] if self._active else None
            return {"available": self._started and not self._closed,
                    "registered": bool(binding and binding.registered), "saved": saved,
                    "session": self._session, "version": self._version, "keys": list(config.SHORTCUT_KEYS),
                    "error": self._error or (binding.error if binding else "")}

    def change(self, choice, session, version):
        choice = config.validate_settings({"windows_shortcut": choice})["windows_shortcut"]
        with self._changes:
            if not self._started or self._closed:
                raise ShortcutUnavailable("The Windows shortcut service is unavailable.")
            if session != self._session or type(version) is not int or version != self._version:
                raise ShortcutUnavailable("The shortcut changed in another window. Review the current setting.")
            self._replace(choice, lambda: self.store.update_settings({"windows_shortcut": choice}))
            return self.status()

    def reset_settings(self):
        with self._changes:
            self._replace(config.SHORTCUT_OFF, self.store.reset_settings)

    def _replace(self, choice, save=None):
        old = self._active[0] if self._active else None
        candidate = None
        if choice["key"] != "off":
            if old and old.registered and self._active[1]["key"] == choice["key"]:
                candidate = old
            else:
                candidate = self._factory(choice["key"], self._pressed)
                try:
                    candidate.start()  # Keep the previous registration until the replacement was accepted and saved.
                except (OSError, RuntimeError) as exc:
                    candidate.stop()
                    raise ShortcutUnavailable(str(exc)) from None
        try:
            if save:
                save()
        except Exception:
            if candidate and candidate is not old:
                candidate.stop()
            raise
        self._active = (candidate, choice) if candidate else None
        if old and old is not candidate:
            old.stop()
        self._error = ""
        self._version += 1

    def stop(self):
        with self._changes:
            self._closed = True
            if self._active:
                binding, self._active = self._active[0], None
                binding.stop()
