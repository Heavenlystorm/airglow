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

kernel32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
kernel32.OpenProcess.restype = wintypes.HANDLE
kernel32.GetCurrentProcess.restype = wintypes.HANDLE
kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
kernel32.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
kernel32.WaitForSingleObject.restype = wintypes.DWORD
kernel32.GetProcessTimes.argtypes = [wintypes.HANDLE, *([ctypes.POINTER(wintypes.FILETIME)] * 4)]


def _process_started_at(handle):
    created, exited, kernel, user = (wintypes.FILETIME() for _ in range(4))
    if not kernel32.GetProcessTimes(handle, ctypes.byref(created), ctypes.byref(exited),
                                    ctypes.byref(kernel), ctypes.byref(user)):
        raise ctypes.WinError(ctypes.get_last_error())
    return (created.dwHighDateTime << 32) | created.dwLowDateTime


def current_process_started_at():
    return _process_started_at(kernel32.GetCurrentProcess())


def wait_for_process_exit(pid, started_at, timeout):
    """Wait for the process that sent the restart, even if its PID has since been reused."""
    access = 0x100000 | 0x1000                    # SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION
    handle = kernel32.OpenProcess(access, False, pid)
    if not handle:
        error = ctypes.get_last_error()
        if error == 87:                         # ERROR_INVALID_PARAMETER: the PID no longer exists
            return True
        raise ctypes.WinError(error)
    try:
        if _process_started_at(handle) != started_at:
            return True
        result = kernel32.WaitForSingleObject(handle, timeout * 1000)
        if result == 0:                         # WAIT_OBJECT_0
            return True
        if result == 258:                       # WAIT_TIMEOUT
            return False
        raise ctypes.WinError(ctypes.get_last_error())
    finally:
        kernel32.CloseHandle(handle)


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
WM_CLOSE = 0x0010
PBT_APMSUSPEND, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND = 0x4, 0x12, 0x7
WTS_SESSION_LOCK, WTS_SESSION_UNLOCK = 0x7, 0x8


class PowerEvents:
    """Hidden top-level window (message-only windows don't get broadcasts) on its own thread.
    handlers: dict with optional suspend, resume, shutdown, shutdown_cancelled, lock, unlock callables."""

    # Windows sends WM_QUERYENDSESSION ("may I end the session?") and then WM_ENDSESSION ("ending it now") for the same
    # shutdown. Without this, Airglow turned the lights off and set the fans to the failsafe speed twice, a few hundred
    # milliseconds apart. A cancel clears the mark, so the next real shutdown still goes through.
    SHUTDOWN_QUIET = 30.0

    def __init__(self, handlers):
        self.handlers = handlers
        self._proc = WNDPROC(self._wndproc)        # keep a reference so it isn't garbage-collected
        self._hwnd = None
        self._thread = None
        self._stop = threading.Event()
        self._shutdown_at = 0.0

    def _fire(self, name):
        fn = self.handlers.get(name)
        if fn and not self._stop.is_set():
            try:
                fn()
            except Exception:
                log.exception("power event handler %s failed", name)

    def _fire_shutdown(self):
        now = time.monotonic()
        if now - self._shutdown_at < self.SHUTDOWN_QUIET:
            return
        self._shutdown_at = now
        self._fire("shutdown")

    def _wndproc(self, hwnd, msg, wparam, lparam):
        if msg == WM_CLOSE:
            user32.PostQuitMessage(0)
            return 0
        if msg == WM_POWERBROADCAST:
            if wparam == PBT_APMSUSPEND:
                self._fire("suspend")
            elif wparam in (PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND):
                self._fire("resume")
            return 1
        if msg == WM_QUERYENDSESSION:
            self._fire_shutdown()
            return 1
        if msg == WM_ENDSESSION:
            # wparam FALSE: something cancelled the shutdown announced by WM_QUERYENDSESSION
            if wparam:
                self._fire_shutdown()
            else:
                self._shutdown_at = 0.0
                self._fire("shutdown_cancelled")
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
        kernel32.GetModuleHandleW.argtypes = [wintypes.LPCWSTR]
        kernel32.GetModuleHandleW.restype = wintypes.HMODULE
        hinst = kernel32.GetModuleHandleW(None)
        wc = WNDCLASSW(lpfnWndProc=self._proc, hInstance=hinst, lpszClassName="AirglowPowerEvents")
        if not user32.RegisterClassW(ctypes.byref(wc)):
            log.warning("power events unavailable (window class registration failed)")
            return
        user32.CreateWindowExW.restype = wintypes.HWND
        user32.CreateWindowExW.argtypes = [wintypes.DWORD, wintypes.LPCWSTR, wintypes.LPCWSTR, wintypes.DWORD,
                                          ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, wintypes.HWND,
                                          wintypes.HMENU, wintypes.HINSTANCE, wintypes.LPVOID]
        user32.DestroyWindow.argtypes = [wintypes.HWND]
        user32.UnregisterClassW.argtypes = [wintypes.LPCWSTR, wintypes.HINSTANCE]
        wts = None
        try:
            ex_style = 0x08000000 | 0x00000080      # WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW
            self._hwnd = user32.CreateWindowExW(ex_style, wc.lpszClassName, "Airglow power events", 0,
                                                0, 0, 0, 0, None, None, hinst, None)
            if not self._hwnd:
                log.warning("power events unavailable (window creation failed)")
                return
            try:
                wts = ctypes.WinDLL("wtsapi32")
                wts.WTSRegisterSessionNotification.argtypes = [wintypes.HWND, wintypes.DWORD]
                wts.WTSUnRegisterSessionNotification.argtypes = [wintypes.HWND]
                if not wts.WTSRegisterSessionNotification(self._hwnd, 0):  # NOTIFY_FOR_THIS_SESSION
                    wts = None
            except OSError:
                pass
            msg = wintypes.MSG()
            while not self._stop.is_set() and user32.GetMessageW(ctypes.byref(msg), None, 0, 0) > 0:
                user32.TranslateMessage(ctypes.byref(msg))
                user32.DispatchMessageW(ctypes.byref(msg))
        finally:
            hwnd, self._hwnd = self._hwnd, None
            if hwnd:
                if wts is not None:
                    wts.WTSUnRegisterSessionNotification(hwnd)
                user32.DestroyWindow(hwnd)
            user32.UnregisterClassW(wc.lpszClassName, hinst)

    def start(self):
        self._thread = threading.Thread(target=self._run, name="power-events", daemon=True)
        self._thread.start()

    def stop(self):
        if self._stop.is_set():
            return
        self._stop.set()
        hwnd = self._hwnd
        if hwnd:
            user32.PostMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
            user32.PostMessageW(hwnd, WM_CLOSE, 0, 0)
        thread = self._thread
        if thread and thread.ident is not None and thread is not threading.current_thread():
            thread.join(timeout=2)
            if thread.is_alive():
                log.warning("power event thread is still stopping")
