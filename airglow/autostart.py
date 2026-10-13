"""Start with Windows via HKCU\...\Run (per-user, no admin needed)."""

import os
import sys
import winreg
from pathlib import Path

RUN_KEY = r"Software\Microsoft\Windows\CurrentVersion\Run"
VALUE = "Airglow"


def command():
    if getattr(sys, "frozen", False):
        return f'"{sys.executable}" --background'
    pythonw = Path(sys.executable).with_name("pythonw.exe")
    run_py = Path(__file__).resolve().parent.parent / "run.py"
    return f'"{pythonw}" "{run_py}" --background'


def current(name=VALUE):
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, RUN_KEY) as k:
            return winreg.QueryValueEx(k, name)[0]
    except FileNotFoundError:
        return None


def is_enabled():
    return current() is not None


def enable():
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, RUN_KEY, 0, winreg.KEY_SET_VALUE) as k:
        winreg.SetValueEx(k, VALUE, 0, winreg.REG_SZ, command())


def disable(name=VALUE):
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, RUN_KEY, 0, winreg.KEY_SET_VALUE) as k:
            winreg.DeleteValue(k, name)
    except FileNotFoundError:
        pass


INSTALL_DIR = Path(os.environ.get("LOCALAPPDATA", "")) / "Programs" / "Airglow"


def is_installed_copy():
    return getattr(sys, "frozen", False) and Path(sys.executable).resolve().parent == INSTALL_DIR.resolve()


def repair():
    """If start-up is on but points somewhere else (an older or moved install), point it at this copy.
    Only the installed copy does this, so running a test build from dist\\ never hijacks start-up."""
    if not is_installed_copy():
        return False
    if is_enabled() and current() != command():
        enable()
        return True
    return False
