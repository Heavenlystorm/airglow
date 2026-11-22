"""Native app window (pywebview on Edge WebView2). Closing hides it to the tray; Quit is in the tray menu."""

import logging
import threading

import webview

from . import APP_NAME, paths
from .deskdisplay import DeskWindow
from .windowlayout import foreground_window
from .windowtheme import TITLEBAR, style_caption

log = logging.getLogger(__name__)


class Window:
    def __init__(self, url, start_hidden=False, on_first_hide=None, design="firefly", layout=None):
        # WebView2 opens its native Save As dialog for files exported by the local UI.
        webview.settings["ALLOW_DOWNLOADS"] = True
        self.design = design if design in TITLEBAR else "graphite"
        self.desk = DeskWindow(url, webview.create_window, layout, self.design)
        self.quitting = False
        self._native_shown = False
        self._destroy_requested = False
        self._destroy_lock = threading.Lock()
        self._on_first_hide = on_first_hide
        self._hidden_once = False
        self._hwnd = None
        # Created visible unless launched in the background: showing a hidden window from another thread during
        # start-up can deadlock pythonnet/WinForms (seen in the frozen build).
        self.win = webview.create_window(
            APP_NAME, url, width=1280, height=840, min_size=(560, 440),
            hidden=start_hidden, background_color=TITLEBAR[self.design][0], text_select=False,
        )
        self.win.events.closing += self._closing
        self.win.events.shown += self._style_title_bar
        if layout:
            layout.watch("main", self.win)

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
        """Style the title bar as soon as there is a handle; the design is re-applied when it changes."""
        self._native_shown = True
        if self.quitting:
            self.destroy()
            return
        try:
            self._hwnd = self.win.native.Handle.ToInt64()
        except Exception as e:  # cosmetic only
            log.debug("title bar styling skipped: %s", e)
            return
        self.apply_design(self.design)

    def apply_design(self, design):
        """Match the Windows title bar to an interface design. Cosmetic: if DWM ignores it, the window still works."""
        self.design = design if design in TITLEBAR else "graphite"
        style_caption(self.win, self.design)
        self.desk.apply_design(self.design)

    def show(self):
        try:
            self.win.show()
            self._webview_visible(True)
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
        hwnd = self._hwnd or (self.win.native.Handle.ToInt64() if self.win.native else None)
        if hwnd:
            foreground_window(hwnd)

    def destroy(self):
        try:
            self.desk.shutdown()
        except Exception:
            log.exception("could not close Desk Display")
        with self._destroy_lock:
            self.quitting = True
            # pywebview ignores destroy() until it creates its native window. The shown event retries it.
            if not self._native_shown or self._destroy_requested or self.win.events.closed.is_set():
                return
            self._destroy_requested = True
        try:
            self.win.destroy()
        except Exception:
            with self._destroy_lock:
                self._destroy_requested = False
            log.exception("could not close the native window")

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
