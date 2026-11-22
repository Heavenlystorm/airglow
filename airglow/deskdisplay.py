"""Read-only desk instruments and the optional companion WebView window."""

import threading
import time

from .windowlayout import foreground_window
from .windowtheme import TITLEBAR, style_caption


def display_view(store, ctrl, native=None, reading_ids=None):
    prefs = store.desk_preferences()
    pins = prefs.pop("pins")
    if reading_ids is not None:
        if len(reading_ids) > 4 or any(not isinstance(i, str) or not 1 <= len(i) <= 256 for i in reading_ids):
            raise ValueError("Choose up to four readings for Desk Display.")
        pins = list(dict.fromkeys(reading_ids))
    readings = ctrl.readings if pins else {}
    history = ctrl.readings_history.get(pins) if pins else {"t": [], "series": {}}
    now = time.time()
    # Share one timeline across the four traces. History stores rounded Unix timestamps.
    indices = [i for i, timestamp in enumerate(history["t"]) if now - 60 <= timestamp <= now + 1]
    instruments = []
    for sensor_id in pins:
        meta = readings.get(sensor_id) or {}
        values = history["series"].get(sensor_id, ())
        instruments.append({"id": sensor_id, "name": meta.get("name") or sensor_id,
                            "short": meta.get("short"), "unit": meta.get("unit", ""), "known": bool(meta),
                            "value": meta.get("value"), "samples": [values[i] if i < len(values) else None for i in indices]})
    return {**prefs, "readings": instruments, "time": now,
            "times": [history["t"][i] for i in indices],
            "window": native.status() if native else {"native": False, "open": False, "pinned": False}}


class DeskWindow:
    """Own one independently closable companion. Requests never accept an arbitrary URL or window handle."""

    def __init__(self, origin, create, layout=None, design="firefly"):
        self._url = origin.rstrip("/") + "/desk.html"
        self._create = create
        self._layout = layout
        self._design = design if design in TITLEBAR else "graphite"
        self._lock = threading.RLock()
        self._window = None
        self._quitting = False

    def _current(self):
        window = self._window
        return window if window is not None and not window.events.closed.is_set() else None

    def status(self):
        with self._lock:
            window = self._current()
            return {"native": True, "open": window is not None, "pinned": bool(window and window.on_top)}

    def open(self):
        with self._lock:
            if self._quitting:
                raise RuntimeError("Airglow is closing.")
            window = self._current()
            if window is None:
                window = self._create("Airglow · Desk Display", self._url, width=660, height=440,
                                      min_size=(320, 240), background_color=TITLEBAR[self._design][0], text_select=False,
                                      on_top=self._layout.desk_pinned() if self._layout else False)
                if window is None:
                    raise RuntimeError("The display window could not open.")
                self._window = window

                def paint():
                    with self._lock:
                        if self._current() is window:
                            style_caption(window, self._design)

                window.events.loaded += paint
                if window.events.loaded.is_set():
                    paint()
                if self._layout:
                    self._layout.watch("desk", window)

                def closed():
                    with self._lock:
                        if self._window is window:
                            self._window = None

                window.events.closed += closed
            else:
                form = window.native
                if form is not None:
                    window.show()
                    handle = form.Handle.ToInt64()
                    if isinstance(handle, int):
                        foreground_window(handle)
            return self.status()

    def apply_design(self, design):
        with self._lock:
            self._design = design if design in TITLEBAR else "graphite"
            window = self._current()
            if window is not None:
                style_caption(window, self._design)

    def pin(self, pinned):
        with self._lock:
            window = self._current()
            if window is None or self._quitting:
                raise RuntimeError("Open Desk Display before pinning its window.")
            window.on_top = pinned
            return self.status()

    def shutdown(self):
        with self._lock:
            self._quitting = True
            window = self._current()
            self._window = None
        if window is not None:
            window.destroy()
