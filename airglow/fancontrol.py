"""Optional: switch FanControl (Rem0o) profiles for the motherboard fans."""

import os
import subprocess
from pathlib import Path

GUESSES = [
    Path(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)")) / "FanControl" / "FanControl.exe",
    Path(os.environ.get("ProgramFiles", r"C:\Program Files")) / "FanControl" / "FanControl.exe",
]


def exe_path(settings):
    if settings.get("fancontrol_exe"):
        return Path(settings["fancontrol_exe"])
    return next((p for p in GUESSES if p.is_file()), None)


def configs_dir(settings):
    if settings.get("fancontrol_configs_dir"):
        return Path(settings["fancontrol_configs_dir"])
    exe = exe_path(settings)
    return exe.parent / "Configurations" if exe else None


def profiles(settings):
    d = configs_dir(settings)
    return sorted(p.name for p in d.glob("*.json")) if d and d.is_dir() else []


def apply_profile(settings, name):
    exe = exe_path(settings)
    if not exe or not exe.is_file():
        raise RuntimeError("FanControl isn't installed")
    cfg = configs_dir(settings) / name
    if not cfg.is_file():
        raise RuntimeError(f"FanControl profile '{name}' doesn't exist")
    subprocess.Popen([str(exe), "-c", str(cfg)], cwd=str(exe.parent),
                     creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
