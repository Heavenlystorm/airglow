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
        self._webview_visible(False)
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
            self._webview_visible(True)
            self.win.restore()
            self._foreground()
        except Exception:
            log.exception("could not show window")

    def _webview_visible(self, visible):
        """Tell WebView2 whether the window is on screen. Hiding the window doesn't: the page went on believing it
        was visible, so in the tray it kept the live LED stream open and redrew the stage 15 times a second. Hidden,
        the page gets visibilitychange (and closes its stream), and WebView2 trims its memory."""
        try:
            from Microsoft.Web.WebView2.Core import CoreWebView2MemoryUsageTargetLevel
            from System import Action
            from System.Reflection import BindingFlags

            form = self.win.native
            view = form.browser.webview

            def apply():
                # the WinForms control keeps its CoreWebView2Controller private, and that's what holds IsVisible
                flags = BindingFlags.NonPublic | BindingFlags.Instance
                field = view.GetType().GetField("_coreWebView2Controller", flags)
                controller = field.GetValue(view) if field else None
                if controller is not None:
                    controller.IsVisible = visible
                if view.CoreWebView2 is not None:
                    level = CoreWebView2MemoryUsageTargetLevel
                    view.CoreWebView2.MemoryUsageTargetLevel = level.Normal if visible else level.Low

            if form.InvokeRequired:
                form.Invoke(Action(apply))
            else:
                apply()
        except Exception as e:  # pywebview or WebView2 changed: everything still works, it just costs more in the tray
            log.warning("couldn't tell WebView2 the window is %s: %s", "shown" if visible else "hidden", e)

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
        quiet_requests()
        webview.start(on_start, gui="edgechromium", private_mode=False,
                      storage_path=str(paths.data_dir() / "webview"), icon=str(paths.ICON_FILE))


def quiet_requests():
    """pywebview hands every request a page makes, and every response, to Python (WebResourceRequested on every URL)
    for events Airglow doesn't use. Each is a round trip between WebView2 and this process: for the effect host's 30
    frame POSTs a second that was about 5% of a core. Unsubscribe as soon as WebView2 is ready. Call before
    webview.start."""
    try:
        from webview.platforms import edgechromium
    except Exception:  # not on WebView2 (pywebview fell back to something else)
        return
    chrome = edgechromium.EdgeChrome
    if getattr(chrome, "_airglow_quiet", False):
        return
    ready = chrome.on_webview_ready

    def on_webview_ready(self, sender, args):
        ready(self, sender, args)
        if not args.IsSuccess:
            return
        try:
            core = sender.CoreWebView2
            core.RemoveWebResourceRequestedFilter("*", edgechromium.CoreWebView2WebResourceContext.All)
            core.WebResourceRequested -= self.on_web_resource_request
            core.WebResourceResponseReceived -= self.on_web_resource_response
        except Exception as e:
            log.warning("couldn't stop pywebview watching every request: %s", e)

    chrome.on_webview_ready = on_webview_ready
    chrome._airglow_quiet = True
