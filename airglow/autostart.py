"""Start with Windows via HKCU\...\Run (per-user, no admin needed)."""

import os
import sys
import winreg
from pathlib import Path

RUN_KEY = r"Software\Microsoft\Windows\CurrentVersion\Run"
VALUE = "Airglow"


def stable_exe(exe):
    """Scoop keeps each version in its own folder (scoop\\apps\\airglow\\1.6.0) and points apps\\airglow\\current at
    the newest, so start-up has to go through that link or it would stay on the version that turned it on."""
    parts = list(Path(exe).parts)
    lower = [p.lower() for p in parts]
    for i in range(len(parts) - 3):
        if lower[i:i + 3] == ["scoop", "apps", "airglow"]:
            parts[i + 3] = "current"
            return str(Path(*parts))
    return str(exe)


def command():
    if getattr(sys, "frozen", False):
        return f'"{stable_exe(sys.executable)}" --background'
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


# Velopack installs the app into %LOCALAPPDATA%\Airglow\current, and updates replace what's in there in place
INSTALL_DIR = Path(os.environ.get("LOCALAPPDATA", "")) / "Airglow" / "current"


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
