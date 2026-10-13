"""Launcher (also the PyInstaller entry point)."""

import os
import sys

from airglow.app import main

if __name__ == "__main__":
    code = main()
    sys.stdout and sys.stdout.flush()
    os._exit(code or 0)   # don't wait on library threads (tray, webview) once we've shut down cleanly
