"""Airglow entry point."""

import ctypes
import logging
import logging.handlers
import os
import subprocess
import sys
import threading
import time
import urllib.request
import webbrowser

from . import APP_NAME, __version__, api, autostart, install, paths, system, updates, webview2
from .config import DEFAULT_CONFIG, Store
from .controller import Controller
from .transactions import RecoveryError

log = logging.getLogger("airglow")
MUTEX_NAME = "Local\\AirglowSingleInstance"
RESTART_WAIT = 60
_mutex = None


def setup_logging():
    handler = logging.handlers.RotatingFileHandler(paths.log_dir() / "airglow.log", maxBytes=1_000_000,
                                                   backupCount=3, encoding="utf-8")
    handler.setFormatter(logging.Formatter("%(asctime)s %(levelname)-7s %(name)s: %(message)s"))
    root = logging.getLogger()
    root.setLevel(logging.INFO)
    root.addHandler(handler)
    if sys.stderr is not None and not getattr(sys, "frozen", False):
        root.addHandler(logging.StreamHandler())


def single_instance():
    global _mutex
    if _mutex:
        return True
    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.CreateMutexW.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_wchar_p]
    k32.CreateMutexW.restype = ctypes.c_void_p
    ctypes.set_last_error(0)
    handle = k32.CreateMutexW(None, False, MUTEX_NAME)
    error = ctypes.get_last_error()
    if not handle:
        raise ctypes.WinError(error)
    if error == 183:                              # ERROR_ALREADY_EXISTS
        k32.CloseHandle.argtypes = [ctypes.c_void_p]
        k32.CloseHandle(handle)
        return False
    _mutex = handle
    return True


def close_mutex():
    """Release the instance only once its resources have stopped."""
    global _mutex
    handle, _mutex = _mutex, None
    if handle:
        k32 = ctypes.WinDLL("kernel32", use_last_error=True)
        k32.CloseHandle.argtypes = [ctypes.c_void_p]
        k32.CloseHandle(handle)


PORT_TRIES = 20               # if the saved port is taken by another program, try the next ones


def port_file():
    """Where the running copy writes the port it actually listens on (a second launch reads it)."""
    return paths.data_dir() / "port"


def running_port(default):
    try:
        return int(port_file().read_text(encoding="ascii").strip())
    except (OSError, ValueError):
        return default


def start_server(port, ctrl, store, hooks, updater=None, tries=PORT_TRIES):
    """Listen on the saved port, or the next free one. Returns the server, or None if they're all taken."""
    for p in range(port, min(port + tries, 65536)):
        try:
            server = api.Server(p, ctrl, store, hooks, updater)
        except OSError:
            continue
        if p != port:
            log.warning("port %d is used by another program; Airglow is on %d this time", port, p)
        try:
            port_file().write_text(str(p), encoding="ascii")
        except OSError as e:
            log.warning("could not record the port: %s", e)
        return server
    return None


def ask_running_instance_to_show(port, timeout=1.5):
    """Ask the copy that owns the mutex to show its window. True when it answered: this launch is done."""
    ctypes.windll.user32.AllowSetForegroundWindow(-1)   # ASFW_ANY: let the running copy take focus
    req = urllib.request.Request(f"http://127.0.0.1:{port}/api/window/show", data=b"{}", method="POST",
                                 headers={"X-Airglow": "1", "Content-Type": "application/json"})
    try:
        urllib.request.urlopen(req, timeout=timeout).close()
        return True
    except OSError as e:
        log.warning("could not reach the running instance: %s", e)
        return False


def claim_single_instance(port, wait=5.0, background=False):
    """True when this process is (or just became) the one running copy.

    A launch that lands in the last second of a quit or an update used to find the mutex taken, fail to reach the
    API that was already shutting down, and exit without doing anything. Now it waits for the old copy to let go and
    takes over, instead of needing a second click."""
    if single_instance():
        return True
    deadline = time.monotonic() + wait
    while time.monotonic() < deadline:
        if not background and ask_running_instance_to_show(port):
            return False                   # it's alive and showed itself
        time.sleep(0.25)
        if single_instance():
            log.info("took over from a copy that was quitting")
            return True
    return False


def message_box(text):
    ctypes.windll.user32.MessageBoxW(None, text, APP_NAME, 0x10)


def offer_webview2_download():
    """The window needs the WebView2 runtime (see webview2.py). Say so, and offer Microsoft's download page."""
    MB_YESNO, MB_ICONERROR, IDYES = 0x4, 0x10, 6
    answer = ctypes.windll.user32.MessageBoxW(
        None, "Airglow needs the Microsoft Edge WebView2 Runtime to show its window, and this PC doesn't have it.\n\n"
              "Open Microsoft's download page now? Install the Evergreen Bootstrapper there, then start Airglow again.",
        APP_NAME, MB_YESNO | MB_ICONERROR)
    if answer == IDYES:
        webbrowser.open(webview2.DOWNLOAD_URL)


def wait_for_restart(argv):
    try:
        pid, started = map(int, argv[1:3])
        if not 0 < pid <= 0xFFFFFFFF or pid == os.getpid() or not 0 < started < 1 << 64:
            raise ValueError("invalid process identity")
        if system.wait_for_process_exit(pid, started, RESTART_WAIT):
            return True
    except (OSError, ValueError):
        log.exception("could not check the previous Airglow process")
        message_box("Airglow could not verify that the previous instance closed. "
                    "Quit it from the tray if it is still running, then open Airglow again.")
    else:
        log.error("Airglow restart timed out waiting %d seconds for process %d", RESTART_WAIT, pid)
        message_box(f"Airglow did not close within {RESTART_WAIT} seconds. "
                    "Wait for it to close, then open Airglow again.")
    return False


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    install.run_hooks()             # when Velopack started us for an install/uninstall hook, this runs it and exits
    if argv and argv[0] == "--after-setup":
        # the install hook found that Setup had ended a running copy (see install.after_install)
        if not install.wait_for_setup(argv[1] if len(argv) > 1 else ""):
            return 0                # Setup started the new version itself
        argv = ["--background"]
    if argv and argv[0] == "--effect-host":
        # runs Studio effects for the real lights in its own process (lighting/studio.py starts it)
        from . import effecthost
        return effecthost.main(argv)
    if argv and argv[0] in ("--setup-tools", "--remove-tools"):
        # the elevated half of helper-program setup (see tools.py): no window, no tray, no second app instance
        from . import tools
        return tools.elevated_main(argv)
    setup_logging()
    import faulthandler
    if os.environ.get("AIRGLOW_DEBUG_CRASHES"):
        # Opt-in only: the .NET runtime behind the window raises and handles access violations on purpose, and
        # faulthandler can't tell those from real crashes, so in normal use this would only log false alarms.
        faulthandler.enable(file=open(paths.log_dir() / "crash.log", "a", encoding="utf-8"), all_threads=True)
    if os.environ.get("AIRGLOW_DEBUG_STACKS"):
        # Diagnostics: dump every thread's stack to the log folder after N seconds (works even if the GIL is stuck).
        import faulthandler
        faulthandler.dump_traceback_later(int(os.environ["AIRGLOW_DEBUG_STACKS"]), exit=False,
                                          file=open(paths.log_dir() / "stacks.txt", "w", encoding="utf-8"))
    if argv and argv[0] == "--restart-from-pid":
        if not wait_for_restart(argv):
            return 1
        argv = argv[3:]            # Velopack keeps normal launch arguments, including --background

    background = "--background" in argv
    try:
        if not claim_single_instance(running_port(DEFAULT_CONFIG["port"]), wait=0.0 if background else 5.0,
                                     background=background):
            return 0
        updates.apply_pending(argv)     # applies a pending update and starts again (doesn't return)
        if not webview2.installed():
            log.error("Install the Microsoft Edge WebView2 Evergreen Runtime from %s, then start Airglow again.",
                      webview2.DOWNLOAD_URL)
            if not background:
                offer_webview2_download()
            return 1

        store = Store()
        try:
            ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID("Airglow.App")
        except (AttributeError, OSError):
            pass
        log.info("%s %s starting (data in %s)", APP_NAME, __version__, paths.data_dir())
        if autostart.repair():
            log.info("start-up entry updated to this install")
        return _run_app(store, store.config["port"], background)
    except RecoveryError as exc:
        log.error("%s", exc)
        if not background:
            message_box(str(exc))
        return 1
    except Exception:
        log.exception("Airglow could not start or its window failed")
        if not background:
            message_box(f"Airglow could not start. Try opening it again. If the problem persists, see "
                        f"{paths.log_dir() / 'airglow.log'} for details.")
        return 1
    finally:
        close_mutex()


def _stop_resource(name, stop):
    try:
        stop()
    except Exception:
        log.exception("stopping %s failed", name)


def _run_app(store, port, background):
    # Import the GUI only after WebView2 is confirmed and this copy owns the instance.
    from .lighting import studio
    from .tray import Tray
    from .window import Window
    from .windowlayout import WindowLayout
    from .windowshortcut import WindowShortcut

    ctrl = updater = server = api_thread = window = tray = shortcut = None
    marked_running = False
    quit_lock = threading.Lock()
    quitting = False
    previous_hooks = sys.excepthook, threading.excepthook

    def crash_hook(exc_type, exc, tb):
        log.critical("unhandled error", exc_info=(exc_type, exc, tb))
        if not quitting:
            ctrl.failsafe()

    def show_window():
        if window is not None and not quitting:
            window.show()

    def quit_app():
        nonlocal quitting
        with quit_lock:
            if not quitting:
                quitting = True
                log.info("quitting")
        if shortcut is not None:
            _stop_resource("the Windows shortcut", shortcut.stop)
        if window is not None:
            _stop_resource("the window", window.destroy)

    try:
        ctrl = Controller(store, start_power_events=True)
        sys.excepthook = crash_hook
        threading.excepthook = lambda a: crash_hook(a.exc_type, a.exc_value, a.exc_traceback)
        hooks = {"show_window": show_window,
                 # show a saved diagnostics report in Explorer, selected
                 "reveal": lambda f: subprocess.Popen(["explorer", "/select,", str(f)]),
                 # the gallery's share page (only GitHub's new-file page for the profiles folder; api.py checks)
                 "open_url": lambda url: webbrowser.open(url),
                 "design_changed": lambda design: window and not quitting and window.apply_design(design)}
        updater = updates.Updater(store)
        server = start_server(port, ctrl, store, hooks, updater)
        if server is None:
            message_box(f"Ports {port} to {min(port + PORT_TRIES - 1, 65535)} are all used by other programs, so "
                        f"Airglow can't start.\n\nChange \"port\" in {paths.data_dir() / 'config.json'} and try again.")
            return 1
        origin = f"http://127.0.0.1:{server.server_address[1]}"
        studio.HOST.url = origin
        window = Window(f"{origin}/", start_hidden=background,
                        on_first_hide=lambda: tray and tray.notify("Still running in the tray. Right-click the Airglow icon to quit."),
                        design=store.settings()["design"], layout=WindowLayout(store.dir / "window-layout.json"))
        server.hooks["desk_display"] = window.desk
        shortcut = WindowShortcut(store, show_window, lambda: not quitting and window.desk.open())
        window.win.events.loaded += shortcut.start
        server.hooks["window_shortcut"] = shortcut
        updater.quit_app = quit_app
        tray = Tray(ctrl, store, show_window=show_window, quit_app=quit_app, updater=updater, open_desk=window.desk.open)

        ctrl.start()
        api_thread = threading.Thread(target=server.serve_forever, name="api", daemon=True)
        api_thread.start()
        updater.start()
        tray.start()
        marked_running = True
        install.mark_running()
        Window.run(None)
    finally:
        if not quitting:
            quit_app()
        if shortcut is not None:
            _stop_resource("the Windows shortcut", shortcut.stop)
        # Stop accepting work before tearing down the device controller. shutdown() blocks if serve_forever never ran.
        if server is not None:
            if api_thread is not None and api_thread.is_alive():
                _stop_resource("the API", server.shutdown)
            _stop_resource("the API listener", server.server_close)
        if tray is not None:
            _stop_resource("the tray", tray.stop)
        if updater is not None:
            _stop_resource("the updater", updater.stop)
        if ctrl is not None:
            # Prevent another frame from spawning a replacement effect host during controller shutdown.
            _stop_resource("the effect host", studio.HOST.close)
            _stop_resource("the controller", ctrl.shutdown)
        if marked_running:
            _stop_resource("the running marker", install.clear_running)
        sys.excepthook, threading.excepthook = previous_hooks
        log.info("stopped")
    return 0


if __name__ == "__main__":
    os._exit(main() or 0)
