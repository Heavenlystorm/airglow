"""Where things live: bundled resources (read-only) and per-user data (config, presets, logs)."""

import os
import sys
from pathlib import Path


def resource_dir() -> Path:
    """Folder holding web/ and the icon; inside the PyInstaller bundle when frozen."""
    if getattr(sys, "frozen", False):
        return Path(getattr(sys, "_MEIPASS", Path(sys.executable).parent))
    return Path(__file__).resolve().parent.parent


def _appdata() -> Path:
    return Path(os.environ.get("APPDATA") or Path.home() / "AppData" / "Roaming")


def data_dir() -> Path:
    d = _appdata() / "Airglow"
    d.mkdir(parents=True, exist_ok=True)
    return d


def log_dir() -> Path:
    d = data_dir() / "logs"
    d.mkdir(parents=True, exist_ok=True)
    return d


WEB_DIR = resource_dir() / "web"
ICON_FILE = resource_dir() / "airglow.ico"
