"""Native app window (pywebview on Edge WebView2). Closing hides it to the tray; Quit is in the tray menu."""

import ctypes
import logging

import webview

from . import APP_NAME, paths

log = logging.getLogger(__name__)

BACKGROUND = "#121418"


def _colorref(hex_color):
    r, g, b = (int(hex_color[i:i + 2], 16) for i in (1, 3, 5))
    return ctypes.c_int(r | (g << 8) | (b << 16))


class Window:
    def __init__(self, url, start_hidden=False, on_first_hide=None):
        self.quitting = False
        self._on_first_hide = on_first_hide
        self._hidden_once = False
        self._hwnd = None
        # Created visible unless launched in the background: showing a hidden window from another thread during
        # start-up can deadlock pythonnet/WinForms (seen in the frozen build).
        self.win = webview.create_window(
            APP_NAME, url, width=1280, height=840, min_size=(1040, 720),
            hidden=start_hidden, background_color=BACKGROUND, text_select=False,
        )
        self.win.events.closing += self._closing
        self.win.events.shown += self._style_title_bar

    def _closing(self):
        if self.quitting:
            return True
        self.win.hide()
        if not self._hidden_once and self._on_first_hide:
            self._hidden_once = True
            self._on_first_hide()
        return False          # keep running in the tray

    def _style_title_bar(self):
        """Dark Windows 11 title bar in the app's own background colour."""
        try:
            hwnd = self._hwnd = self.win.native.Handle.ToInt64()
            dwm = ctypes.windll.dwmapi
            on = ctypes.c_int(1)
            dwm.DwmSetWindowAttribute(hwnd, 20, ctypes.byref(on), ctypes.sizeof(on))          # immersive dark mode
            color = _colorref(BACKGROUND)
            dwm.DwmSetWindowAttribute(hwnd, 35, ctypes.byref(color), ctypes.sizeof(color))    # caption colour
            text = _colorref("#e8eaed")
            dwm.DwmSetWindowAttribute(hwnd, 36, ctypes.byref(text), ctypes.sizeof(text))      # caption text
        except Exception as e:  # cosmetic only
            log.debug("title bar styling skipped: %s", e)

    def show(self):
        try:
            self.win.show()
            self.win.restore()
            self._foreground()
        except Exception:
            log.exception("could not show window")

    def _foreground(self):
        """Bring the window forward with plain Win32 calls (no cross-thread WinForms Invoke)."""
        user32 = ctypes.windll.user32
        hwnd = self._hwnd or user32.FindWindowW(None, APP_NAME)
        if hwnd:
            user32.ShowWindow(hwnd, 9)               # SW_RESTORE
            user32.SetForegroundWindow(hwnd)

    def destroy(self):
        self.quitting = True
        try:
            self.win.destroy()
        except Exception:
            pass

    @staticmethod
    def run(on_start):
        webview.start(on_start, gui="edgechromium", private_mode=False,
                      storage_path=str(paths.data_dir() / "webview"), icon=str(paths.ICON_FILE))
