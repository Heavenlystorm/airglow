"""Windows caption colours shared by the main and companion windows."""

import ctypes
import logging

log = logging.getLogger(__name__)

# design id -> (window/caption ground, caption text, dark title bar). The palette's ground, so a light design doesn't
# sit in a black frame. test_designs keeps these ids in step with the design lists.
TITLEBAR = {
    "firefly": ("#221d30", "#fff4e4", True),
    "graphite": ("#111315", "#f2f0e9", True),
    "ascii": ("#060806", "#d9ffd9", True),
    "paper": ("#e8e4db", "#1c1d19", False),
    "amber": ("#0c0903", "#ffd39a", True),
    "contrast": ("#000000", "#ffffff", True),
    "neon": ("#07070d", "#f1edff", True),
    "kiln": ("#0b0605", "#f2eadd", True),
    "lumen": ("#060510", "#f3f0f8", True),
    "midnight": ("#02071d", "#e7f0f9", True),
    "spore": ("#0f0a06", "#efe7d8", True),
    "rim": ("#030201", "#efebe4", True),
    "darkroom": ("#090604", "#e9e4dc", True),
}


def _colorref(hex_color):
    r, g, b = (int(hex_color[i:i + 2], 16) for i in (1, 3, 5))
    return ctypes.c_int(r | (g << 8) | (b << 16))


def style_caption(window, design):
    try:
        if window.native is None:
            return
        handle = window.native.Handle.ToInt64()
        if not isinstance(handle, int):
            return
        hwnd = ctypes.c_void_p(handle)
        ground, text, dark = TITLEBAR[design]
        for attribute, value in ((20, ctypes.c_int(dark)), (35, _colorref(ground)), (36, _colorref(text))):
            ctypes.windll.dwmapi.DwmSetWindowAttribute(hwnd, attribute, ctypes.byref(value), ctypes.sizeof(value))
    except Exception as exc:
        log.debug("Windows caption styling skipped: %s", exc)
