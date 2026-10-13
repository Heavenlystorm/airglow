"""Windows signals for automations: idle time, foreground app, running apps, lock state, and a hidden window
that receives sleep / wake / lock / shutdown notifications."""

import ctypes
import logging
import os
import threading
import time
from ctypes import wintypes

log = logging.getLogger(__name__)

user32 = ctypes.WinDLL("user32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)


class LASTINPUTINFO(ctypes.Structure):
    _fields_ = [("cbSize", wintypes.UINT), ("dwTime", wintypes.DWORD)]


def idle_seconds():
    info = LASTINPUTINFO(ctypes.sizeof(LASTINPUTINFO), 0)
    if not user32.GetLastInputInfo(ctypes.byref(info)):
        return 0.0
    return ((kernel32.GetTickCount() - info.dwTime) & 0xFFFFFFFF) / 1000.0


def _exe_of_pid(pid):
    PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
    h = kernel32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
    if not h:
        return None
    try:
        buf = ctypes.create_unicode_buffer(1024)
        size = wintypes.DWORD(len(buf))
        if kernel32.QueryFullProcessImageNameW(h, 0, buf, ctypes.byref(size)):
            return os.path.basename(buf.value).lower()
    finally:
        kernel32.CloseHandle(h)
    return None


def foreground_exe():
    hwnd = user32.GetForegroundWindow()
    if not hwnd:
        return None
    pid = wintypes.DWORD()
    user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
    return _exe_of_pid(pid.value) if pid.value else None


_proc_cache = (0.0, frozenset())

# Windows plumbing that nobody wants to pick as an automation trigger.
BACKGROUND_EXES = frozenset({
    "system", "registry", "idle", "smss.exe", "csrss.exe", "wininit.exe", "winlogon.exe", "services.exe", "lsass.exe",
    "svchost.exe", "fontdrvhost.exe", "dwm.exe", "sihost.exe", "taskhostw.exe", "explorer.exe", "ctfmon.exe",
    "runtimebroker.exe", "searchhost.exe", "startmenuexperiencehost.exe", "shellexperiencehost.exe", "textinputhost.exe",
    "conhost.exe", "dllhost.exe", "spoolsv.exe", "audiodg.exe", "wmiprvse.exe", "securityhealthservice.exe",
    "securityhealthsystray.exe", "smartscreen.exe", "applicationframehost.exe", "lockapp.exe", "memory compression",
    "msmpeng.exe", "nissrv.exe", "searchindexer.exe", "wudfhost.exe", "airglow.exe", "python.exe", "pythonw.exe",
    "msedgewebview2.exe", "crashpad_handler.exe", "backgroundtaskhost.exe", "useroobebroker.exe", "widgets.exe",
    "phoneexperiencehost.exe", "gamebar.exe", "gamebarftserver.exe", "nvcontainer.exe", "nvdisplay.container.exe",
})


def running_exes():
    """Lower-case names of running executables (cached for 3 seconds)."""
    global _proc_cache
    if time.monotonic() - _proc_cache[0] < 3.0:
        return _proc_cache[1]
    import psutil
    names = set()
    for p in psutil.process_iter(["name"]):
        n = p.info.get("name")
        if n:
            names.add(n.lower())
    _proc_cache = (time.monotonic(), frozenset(names))
    return _proc_cache[1]


def windowed_exes():
    """Programs with a visible, titled top-level window: the ones people recognise (games, browsers, chat)."""
    pids = set()
    proc = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

    def collect(hwnd, _):
        if user32.IsWindowVisible(hwnd) and user32.GetWindowTextLengthW(hwnd) > 0:
            pid = wintypes.DWORD()
            user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
            pids.add(pid.value)
        return True

    user32.EnumWindows(proc(collect), 0)
    return sorted({n for n in (_exe_of_pid(p) for p in pids) if n and n not in BACKGROUND_EXES})


def is_locked():
    """True when the lock screen (or another secure desktop) has the input."""
    DESKTOP_SWITCHDESKTOP = 0x0100
    h = user32.OpenInputDesktop(0, False, DESKTOP_SWITCHDESKTOP)
    if not h:
        return True
    user32.CloseDesktop(h)
    return False


def snapshot():
    """Everything automations look at, gathered once per tick."""
    return {"idle": idle_seconds(), "foreground": foreground_exe(), "running": running_exes(),
            "locked": is_locked(), "now": time.localtime()}


# ---------------------------------------------------------------- power / session events

WNDPROC = ctypes.WINFUNCTYPE(ctypes.c_ssize_t, wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM)


class WNDCLASSW(ctypes.Structure):
    _fields_ = [("style", wintypes.UINT), ("lpfnWndProc", WNDPROC), ("cbClsExtra", ctypes.c_int),
                ("cbWndExtra", ctypes.c_int), ("hInstance", wintypes.HINSTANCE), ("hIcon", wintypes.HICON),
                ("hCursor", wintypes.HANDLE), ("hbrBackground", wintypes.HBRUSH), ("lpszMenuName", wintypes.LPCWSTR),
                ("lpszClassName", wintypes.LPCWSTR)]


WM_POWERBROADCAST, WM_QUERYENDSESSION, WM_ENDSESSION, WM_WTSSESSION_CHANGE = 0x0218, 0x0011, 0x0016, 0x02B1
PBT_APMSUSPEND, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND = 0x4, 0x12, 0x7
WTS_SESSION_LOCK, WTS_SESSION_UNLOCK = 0x7, 0x8


class PowerEvents:
    """Hidden top-level window (message-only windows don't get broadcasts) on its own thread.
    handlers: dict with optional suspend, resume, shutdown, lock, unlock callables."""

    def __init__(self, handlers):
        self.handlers = handlers
        self._proc = WNDPROC(self._wndproc)        # keep a reference so it isn't garbage-collected
        self._hwnd = None

    def _fire(self, name):
        fn = self.handlers.get(name)
        if fn:
            try:
                fn()
            except Exception:
                log.exception("power event handler %s failed", name)

    def _wndproc(self, hwnd, msg, wparam, lparam):
        if msg == WM_POWERBROADCAST:
            if wparam == PBT_APMSUSPEND:
                self._fire("suspend")
            elif wparam in (PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND):
                self._fire("resume")
            return 1
        if msg == WM_QUERYENDSESSION:
            self._fire("shutdown")
            return 1
        if msg == WM_ENDSESSION and wparam:
            self._fire("shutdown")
            return 0
        if msg == WM_WTSSESSION_CHANGE:
            if wparam == WTS_SESSION_LOCK:
                self._fire("lock")
            elif wparam == WTS_SESSION_UNLOCK:
                self._fire("unlock")
            return 0
        return user32.DefWindowProcW(hwnd, msg, wparam, ctypes.c_ssize_t(lparam))

    def _run(self):
        user32.DefWindowProcW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, ctypes.c_ssize_t]
        user32.DefWindowProcW.restype = ctypes.c_ssize_t
        hinst = kernel32.GetModuleHandleW(None)
        wc = WNDCLASSW(lpfnWndProc=self._proc, hInstance=hinst, lpszClassName="AirglowPowerEvents")
        user32.RegisterClassW(ctypes.byref(wc))
        user32.CreateWindowExW.restype = wintypes.HWND
        self._hwnd = user32.CreateWindowExW(0, "AirglowPowerEvents", "Airglow power events", 0,
                                            0, 0, 0, 0, None, None, hinst, None)
        if not self._hwnd:
            log.warning("power events unavailable (window creation failed)")
            return
        try:
            ctypes.WinDLL("wtsapi32").WTSRegisterSessionNotification(self._hwnd, 0)   # NOTIFY_FOR_THIS_SESSION
        except OSError:
            pass
        msg = wintypes.MSG()
        while user32.GetMessageW(ctypes.byref(msg), None, 0, 0) > 0:
            user32.TranslateMessage(ctypes.byref(msg))
            user32.DispatchMessageW(ctypes.byref(msg))

    def start(self):
        threading.Thread(target=self._run, name="power-events", daemon=True).start()
