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

from . import APP_NAME, __version__, api, autostart, install, paths, updates, webview2
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
    close_mutex()
    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.CreateMutexW.restype = ctypes.c_void_p
    _mutex = k32.CreateMutexW(None, False, MUTEX_NAME)
    return ctypes.get_last_error() != 183          # ERROR_ALREADY_EXISTS


def close_mutex():
    """Let go of the mutex before asking again: holding it ourselves would keep it alive after the owning copy quits."""
    global _mutex
    if _mutex:
        k32 = ctypes.WinDLL("kernel32", use_last_error=True)
        k32.CloseHandle.argtypes = [ctypes.c_void_p]
        k32.CloseHandle(_mutex)
        _mutex = None


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
    # The tray's "Restart Airglow" starts this copy while the old one is still quitting, and asking that one to show
    # its window would just flash it on screen before it goes: wait for it instead (see Tray._menu).
    handoff = bool(os.environ.get("AIRGLOW_RESTARTING"))
    deadline = time.monotonic() + wait
    while time.monotonic() < deadline:
        if not background and not handoff and ask_running_instance_to_show(port):
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
    background = "--background" in argv
    store = Store()
    port = store.config["port"]

    if not claim_single_instance(running_port(port), wait=0.0 if background else 5.0, background=background):
        return 0
    updates.apply_pending(argv)     # an update downloaded last time: apply it and start again (doesn't return)

    if not webview2.installed():
        log.error("the Microsoft Edge WebView2 runtime isn't installed, so the window can't open")
        if not background:
            offer_webview2_download()
            return 1
        # started with Windows: carry on, the lights and fans don't need the window

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
    hooks = {"show_window": lambda: window and window.show(),
             # show a saved diagnostics report in Explorer, selected
             "reveal": lambda f: subprocess.Popen(["explorer", "/select,", str(f)]),
             # the gallery's share page (only GitHub's new-file page for the profiles folder; api.py checks)
             "open_url": lambda url: webbrowser.open(url)}
    updater = updates.Updater(store)
    server = start_server(port, ctrl, store, hooks, updater)
    if server is None:
        message_box(f"Ports {port} to {port + PORT_TRIES - 1} are all used by other programs, so Airglow can't "
                    f"start.\n\nChange \"port\" in {paths.data_dir() / 'config.json'} and try again.")
        return 1
    port = server.server_address[1]
    from .lighting import studio
    studio.HOST.url = f"http://127.0.0.1:{port}"     # the effect host loads its page from here

    ctrl.start()
    threading.Thread(target=server.serve_forever, name="api", daemon=True).start()
    updater.start()

    tray = None

    def quit_app():
        log.info("quitting")
        tray and tray.stop()
        window.destroy()

    window = Window(f"http://127.0.0.1:{port}/", start_hidden=background,
                    on_first_hide=lambda: tray and tray.notify("Still running in the tray. Right-click the Airglow icon to quit."))
    updater.quit_app = quit_app
    tray = Tray(ctrl, store, show_window=window.show, quit_app=quit_app, updater=updater)
    tray.start()
    install.mark_running()

    try:
        Window.run(None)            # blocks until the window is destroyed (Quit)
    finally:
        # Close the Studio host before anything else: the lighting thread keeps drawing a few more frames while it
        # stops, and a spawn there used to leave a fresh effect host running for a moment after Airglow quit.
        studio.HOST.close()
        updater.stop()
        ctrl.shutdown()
        server.shutdown()
        tray.stop()
        install.clear_running()
        log.info("stopped")
    return 0


if __name__ == "__main__":
    os._exit(main() or 0)
