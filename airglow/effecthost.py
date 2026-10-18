"""The effect host process: "Airglow.exe --effect-host <url> --parent <pid>".

A hidden WebView2 window with its own data folder, showing web/runtime/host.html, which runs Studio effects in
sandboxed frames for the real lights (lighting/studio.py has the other side). It quits when Airglow does, and Airglow
ends it when it's not needed, hangs, or uses too much memory.
"""

import logging
import re
import threading
import time

from . import paths

log = logging.getLogger(__name__)


def main(argv):
    handler = logging.FileHandler(paths.log_dir() / "effect-host.log", encoding="utf-8")
    handler.setFormatter(logging.Formatter("%(asctime)s %(levelname)-7s %(name)s: %(message)s"))
    logging.getLogger().addHandler(handler)
    logging.getLogger().setLevel(logging.INFO)
    url = argv[1] if len(argv) > 1 else ""
    if not re.fullmatch(r"http://127\.0\.0\.1:\d{2,5}/runtime/host\.html#token=[\w\-]+", url):
        log.error("refusing to start: unexpected address")
        return 2
    parent = int(argv[3]) if len(argv) > 3 and argv[2] == "--parent" and argv[3].isdigit() else None

    import webview

    from .window import quiet_requests
    quiet_requests()                       # 30 frame POSTs a second: don't route each one through this process
    win = webview.create_window("Airglow effects", url, hidden=True, width=320, height=200)

    def watch_parent():
        import psutil
        while parent is None or psutil.pid_exists(parent):
            time.sleep(2)
        log.info("Airglow has quit; closing the effect host")
        win.destroy()

    log.info("effect host starting")
    webview.start(lambda: threading.Thread(target=watch_parent, daemon=True).start(), gui="edgechromium",
                  private_mode=True, storage_path=str(paths.data_dir() / "effect-host"))
    return 0
