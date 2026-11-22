"""Remember where each Airglow window lived, relative to the usable area of its monitor."""

import ctypes
from ctypes import wintypes
import json
import logging
import math
import threading

from .fileio import atomic_write_text

log = logging.getLogger(__name__)


def foreground_window(handle):
    """Recalling a visible or hidden maximized window must keep its Windows layout."""
    hwnd = ctypes.c_void_p(handle)
    user32 = ctypes.windll.user32
    if user32.IsIconic(hwnd):
        user32.ShowWindow(hwnd, 9)  # SW_RESTORE honors Windows' restore-to-maximized flag.
    user32.SetForegroundWindow(hwnd)


class _WindowPlacement(ctypes.Structure):
    _fields_ = [("length", wintypes.UINT), ("flags", wintypes.UINT), ("show_command", wintypes.UINT),
                ("min_position", wintypes.POINT), ("max_position", wintypes.POINT), ("normal_position", wintypes.RECT)]


def _will_restore_maximized(form):
    saved = _WindowPlacement()
    saved.length = ctypes.sizeof(saved)
    hwnd = ctypes.c_void_p(form.Handle.ToInt64())
    return bool(ctypes.windll.user32.GetWindowPlacement(hwnd, ctypes.byref(saved)) and saved.flags & 2)


def placement(rect, work):
    x, y, width, height = rect
    left, top, room_w, room_h = work
    if min(width, height, room_w, room_h) <= 0:
        raise ValueError("invalid window bounds")
    clamp = lambda n: max(0.0, min(1.0, n))
    return [clamp((x-left)/max(1, room_w-width)), clamp((y-top)/max(1, room_h-height)),
            clamp(width/room_w), clamp(height/room_h)]


def fit(position, work, minimum):
    if (not isinstance(position, list) or len(position) != 4
            or any(type(n) not in (int, float) or not math.isfinite(n) or not 0 <= n <= 1 for n in position)
            or min(position[2:]) <= 0):
        raise ValueError("invalid saved window position")
    left, top, room_w, room_h = work
    width = min(room_w, max(minimum[0], round(room_w * position[2])))
    height = min(room_h, max(minimum[1], round(room_h * position[3])))
    return (left + round((room_w - width) * position[0]), top + round((room_h - height) * position[1]), width, height)


def _rect(bounds):
    return (bounds.X, bounds.Y, bounds.Width, bounds.Height)


class WindowLayout:
    def __init__(self, file):
        self.file = file
        self._lock = threading.Lock()
        self._records = {}
        try:
            if file.stat().st_size <= 8192:
                saved = json.loads(file.read_text(encoding="utf-8"))
                if isinstance(saved, dict):
                    self._records = {k:v for k,v in saved.items() if k in ("main", "desk") and isinstance(v, dict)}
        except (FileNotFoundError, ValueError, OSError):
            pass

    def desk_pinned(self):
        with self._lock:
            return self._records.get("desk", {}).get("pinned") is True

    def watch(self, key, window):
        """Restore once, even if the webpage reloads. Save on hide to tray or exit, never during a drag."""
        restored = threading.Event()

        def restore():
            if restored.is_set():
                return
            restored.set()
            try:
                from System import Action
                from System.Drawing import Rectangle
                from System.Windows.Forms import FormWindowState, Screen
                form = window.native
                with self._lock:
                    saved = self._records.get(key)
                def apply():
                    try:
                        if form.IsDisposed:
                            return
                        if not saved:
                            screen = Screen.FromControl(form)
                            area = _rect(screen.WorkingArea)
                            current = _rect(form.Bounds)
                            if current[2] <= area[2] and current[3] <= area[3]:
                                return
                            relative = placement(current, area)
                        else:
                            screen = next((s for s in Screen.AllScreens if s.DeviceName == saved.get("monitor")),
                                          Screen.PrimaryScreen)
                            area = _rect(screen.WorkingArea)
                            relative = saved.get("position")
                        bounds = fit(relative, area, (form.MinimumSize.Width, form.MinimumSize.Height))
                        form.Bounds = Rectangle(*bounds)
                        if saved and saved.get("maximized") is True:
                            form.WindowState = FormWindowState.Maximized
                    except Exception as exc:
                        log.debug("Window placement skipped: %s", exc)
                form.BeginInvoke(Action(apply))
            except Exception as exc:
                log.debug("Window placement unavailable: %s", exc)

        def save():
            if not restored.is_set():
                return
            try:
                from System.Windows.Forms import FormWindowState, Screen
                form = window.native
                state = form.WindowState
                normal = form.Bounds if state == FormWindowState.Normal else form.RestoreBounds
                screen = Screen.FromRectangle(normal)
                maximized = state == FormWindowState.Maximized or state == FormWindowState.Minimized and _will_restore_maximized(form)
                record = {"monitor": str(screen.DeviceName), "position": placement(_rect(normal), _rect(screen.WorkingArea)),
                          "maximized": maximized}
                if key == "desk":
                    record["pinned"] = bool(window.on_top)
                with self._lock:
                    if self._records.get(key) != record:
                        updated = {**self._records, key: record}
                        atomic_write_text(self.file, json.dumps(updated, ensure_ascii=False))
                        self._records = updated
            except Exception as exc:
                log.debug("Could not remember %s window: %s", key, exc)

        window.events.loaded += restore
        window.events.closing += save
        if window.events.loaded.is_set():
            restore()
