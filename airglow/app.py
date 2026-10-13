"""Airglow entry point."""

import ctypes
import logging
import logging.handlers
import os
import sys
import threading
import urllib.request

from . import APP_NAME, __version__, api, autostart, paths
from .config import Store
from .controller import Controller

log = logging.getLogger("airglow")
MUTEX_NAME = "Local\\AirglowSingleInstance"
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
    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.CreateMutexW.restype = ctypes.c_void_p
    _mutex = k32.CreateMutexW(None, False, MUTEX_NAME)
    return ctypes.get_last_error() != 183          # ERROR_ALREADY_EXISTS


def ask_running_instance_to_show(port):
    ctypes.windll.user32.AllowSetForegroundWindow(-1)   # ASFW_ANY: let the running copy take focus
    req = urllib.request.Request(f"http://127.0.0.1:{port}/api/window/show", data=b"{}", method="POST",
                                 headers={"X-Airglow": "1", "Content-Type": "application/json"})
    try:
        urllib.request.urlopen(req, timeout=3).close()
    except OSError as e:
        log.warning("could not reach the running instance: %s", e)


def message_box(text):
    ctypes.windll.user32.MessageBoxW(None, text, APP_NAME, 0x10)


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
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
    background = "--background" in argv
    store = Store()
    port = store.config["port"]

    if not single_instance():
        if not background:
            ask_running_instance_to_show(port)
        return 0

    try:
        ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID("Airglow.App")
    except (AttributeError, OSError):
        pass

    log.info("%s %s starting (data in %s)", APP_NAME, __version__, paths.data_dir())
    if autostart.repair():
        log.info("start-up entry updated to this install")

    ctrl = Controller(store, start_power_events=True)

    def crash_hook(exc_type, exc, tb):
        log.critical("unhandled error", exc_info=(exc_type, exc, tb))
        ctrl.failsafe()

    sys.excepthook = crash_hook
    threading.excepthook = lambda a: crash_hook(a.exc_type, a.exc_value, a.exc_traceback)

    # imported here so the tests and a second instance don't load the GUI stack
    from .tray import Tray
    from .window import Window

    window = None
    hooks = {"show_window": lambda: window and window.show()}
    try:
        server = api.Server(port, ctrl, store, hooks)
    except OSError:
        message_box(f"Port {port} is already used by another program, so Airglow can't start.\n\n"
                    f"Change \"port\" in {paths.data_dir() / 'config.json'} and try again.")
        return 1

    ctrl.start()
    threading.Thread(target=server.serve_forever, name="api", daemon=True).start()

    tray = None

    def quit_app():
        log.info("quitting")
        tray and tray.stop()
        window.destroy()

    window = Window(f"http://127.0.0.1:{port}/", start_hidden=background,
                    on_first_hide=lambda: tray and tray.notify("Still running in the tray. Right-click the Airglow icon to quit."))
    tray = Tray(ctrl, store, show_window=window.show, quit_app=quit_app)
    tray.start()

    try:
        Window.run(None)            # blocks until the window is destroyed (Quit)
    finally:
        server.shutdown()
        ctrl.shutdown()
        tray.stop()
        log.info("stopped")
    return 0


if __name__ == "__main__":
    os._exit(main() or 0)
