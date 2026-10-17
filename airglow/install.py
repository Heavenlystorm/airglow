"""Install, update and uninstall hooks (Velopack), and moving an older Inno Setup install out of the way.

Velopack runs Airglow.exe with a hook argument right after installing and right before uninstalling; velopack.App()
calls the matching function below and exits. These hooks must finish quickly (Velopack stops waiting after about
30 seconds), so anything that waits for the user runs as a detached script instead.
"""

import ctypes
import logging
import os
import shutil
import subprocess
import sys
import tempfile
import time
import winreg
from pathlib import Path

from . import APP_NAME, autostart, paths

log = logging.getLogger(__name__)

# Where 1.0 to 1.5 (Inno Setup) installed, and its "Apps & features" entry (AppId from installer.iss)
LEGACY_DIR = Path(os.environ.get("LOCALAPPDATA", "")) / "Programs" / "Airglow"
LEGACY_UNINSTALL_KEY = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{982CBB98-0B0F-43C0-A986-30BC3E9EA9F0}_is1"

CSIDL_PROGRAMS = 0x02
CSIDL_DESKTOPDIRECTORY = 0x10
CREATE_NO_WINDOW = 0x08000000
CREATE_BREAKAWAY_FROM_JOB = 0x01000000
SETUP_WAIT = 120              # seconds; Setup is normally done a moment after the install hook


def run_hooks():
    """First thing at start-up: lets Velopack run its hooks (and exit) when it started us for one."""
    if not getattr(sys, "frozen", False):
        return
    try:
        import velopack
    except ImportError:
        return
    (velopack.App()
        .set_auto_apply_on_startup(False)          # updates.apply_pending() does it, keeping our arguments
        .on_after_install_fast_callback(after_install)
        .on_before_uninstall_fast_callback(before_uninstall)
        .run())


def _hook_logging():
    handler = logging.FileHandler(paths.log_dir() / "install.log", encoding="utf-8")
    handler.setFormatter(logging.Formatter("%(asctime)s %(levelname)-7s %(name)s: %(message)s"))
    root = logging.getLogger()
    root.setLevel(logging.INFO)
    root.addHandler(handler)


def after_install(version):
    _hook_logging()
    log.info("installed %s %s", APP_NAME, version)
    existing_user = (paths.data_dir() / "config.json").exists()
    had_startup = autostart.is_enabled()
    try:
        moved = remove_legacy_install()
    except Exception:  # never fail the install over the old copy
        log.exception("couldn't remove the old install completely")
        moved = True
    # A fresh install starts with Windows, like the old installer's default; a moved one keeps the user's choice.
    if had_startup or not existing_user:
        autostart.enable()
    log.info("start with Windows: %s%s", autostart.is_enabled(), " (moved from the old install)" if moved else "")
    if should_start_after_setup(existing_user or moved, running_marker().exists(), had_startup):
        log.info("starting %s again once Setup has finished", APP_NAME)
        try:
            start_after_setup(os.getppid())
        except OSError:
            log.exception("couldn't start %s again", APP_NAME)


def before_uninstall(version):
    _hook_logging()
    log.info("uninstalling %s %s", APP_NAME, version)
    autostart.disable()
    clear_running()
    from . import tools
    if tools.tools_dir().exists():
        try:
            offer_tool_removal()
        except OSError:
            log.exception("couldn't offer to remove the helper programs")


# ---------------------------------------------------------------- starting again after Setup

def running_marker():
    return paths.data_dir() / "running"


def mark_running():
    """Called once Airglow is up; clear_running() when it quits cleanly. A marker still there at install time means
    Setup ended a running copy (it stops everything running from the install folder, without a clean shutdown)."""
    try:
        running_marker().write_text(str(os.getpid()), encoding="ascii")
    except OSError as e:
        log.warning("could not record that Airglow is running: %s", e)


def clear_running():
    try:
        running_marker().unlink(missing_ok=True)
    except OSError as e:
        log.warning("could not clear the running marker: %s", e)


def should_start_after_setup(existing_install, was_running, had_startup):
    """Setup ends a running copy to replace it, and a silent one (winget) doesn't start the new version. Start it again
    when the user had it running or starting with Windows; a fresh install is left to the user."""
    return existing_install and (was_running or had_startup)


def start_after_setup(setup_pid):
    """Start "Airglow.exe --after-setup <pid>" from the new install, outliving the hook and Setup."""
    cmd = [sys.executable, "--after-setup", str(setup_pid)]
    cwd = Path(sys.executable).parent
    try:        # Setup may run the hook in a job that ends with it
        subprocess.Popen(cmd, cwd=cwd, creationflags=CREATE_BREAKAWAY_FROM_JOB, close_fds=True)
    except OSError:
        subprocess.Popen(cmd, cwd=cwd, close_fds=True)


def wait_for_setup(setup_pid, timeout=SETUP_WAIT):
    """For "--after-setup": wait until Setup has exited, then say whether Airglow still needs starting. It doesn't when
    Setup started it itself (every install that isn't silent)."""
    import psutil
    try:
        psutil.Process(int(setup_pid)).wait(timeout)
    except (ValueError, psutil.NoSuchProcess, psutil.AccessDenied, psutil.TimeoutExpired):
        pass
    return not other_copies_running()


def other_copies_running(exe=None):
    import psutil
    exe = Path(exe or sys.executable).resolve()
    for p in psutil.process_iter(["exe"]):
        try:
            if p.pid != os.getpid() and p.info.get("exe") and Path(p.info["exe"]).resolve() == exe:
                return True
        except OSError:
            continue
    return False


# ---------------------------------------------------------------- the old Inno Setup install

def _known_folder(csidl):
    buf = ctypes.create_unicode_buffer(260)
    if ctypes.windll.shell32.SHGetFolderPathW(None, csidl, None, 0, buf) != 0:
        return None
    return Path(buf.value)


def points_into(lnk, folder):
    """Whether a shortcut file refers to something inside folder. .lnk files keep their target path both as ANSI and
    as UTF-16 text, so looking for the folder in either is enough to tell ours from Velopack's."""
    try:
        data = Path(lnk).read_bytes().lower()
    except OSError:
        return False
    needle = str(folder).lower().rstrip("\\") + "\\"
    return needle.encode("utf-16-le") in data or needle.encode("mbcs", "ignore") in data


def _stop_running_from(folder):
    import psutil
    for p in psutil.process_iter(["exe"]):
        exe = p.info.get("exe")
        if exe and Path(exe).resolve().is_relative_to(folder.resolve()) and p.pid != os.getpid():
            try:
                p.terminate()
                p.wait(timeout=5)
            except (psutil.Error, OSError):
                pass


def remove_legacy_install(folder=LEGACY_DIR, shortcut_dirs=None):
    """Remove what the 1.x Inno installer left: running copies, the folder, its shortcuts and its uninstall entry.
    The old uninstaller isn't used, because it ends every Airglow.exe, including the one that's being installed.
    Returns whether there was an old install."""
    folder = Path(folder)
    if not (folder / "Airglow.exe").exists():
        return False
    log.info("removing the old install in %s", folder)
    _stop_running_from(folder)
    dirs = shortcut_dirs if shortcut_dirs is not None else [_known_folder(CSIDL_PROGRAMS), _known_folder(CSIDL_DESKTOPDIRECTORY)]
    for d in filter(None, dirs):
        lnk = Path(d) / f"{APP_NAME}.lnk"
        if lnk.exists() and points_into(lnk, folder):
            lnk.unlink(missing_ok=True)
    if shortcut_dirs is None:
        try:
            winreg.DeleteKey(winreg.HKEY_CURRENT_USER, LEGACY_UNINSTALL_KEY)
        except FileNotFoundError:
            pass
    for _ in range(5):                 # a just-ended process can hold its files for a moment
        shutil.rmtree(folder, ignore_errors=True)
        if not folder.exists():
            break
        time.sleep(1)
    return True


# ---------------------------------------------------------------- helper programs on uninstall

def _ps(text):
    return "'" + str(text).replace("'", "''") + "'"


def removal_scripts(root=None):
    """Two PowerShell scripts: one asks (as the user), the other removes the helper programs (as admin). They don't
    need anything from Airglow's own folder, which is being deleted while they run."""
    from . import tools
    s, root = tools.spec(), Path(root or tools.tools_dir())
    lines = ["$ErrorActionPreference = 'SilentlyContinue'"]
    for t in s["tools"].values():
        lines += [f"schtasks /End /TN {_ps(t['task'])} | Out-Null",
                  (f"Get-Process | Where-Object {{ $_.Path -and $_.Path.StartsWith({_ps(str(root) + chr(92))}, "
                   "'OrdinalIgnoreCase') } | Stop-Process -Force"),
                  f"schtasks /Delete /TN {_ps(t['task'])} /F | Out-Null"]
    lines += ["for ($i = 0; $i -lt 5; $i++) {",
              f"  Remove-Item -LiteralPath {_ps(root)} -Recurse -Force",
              f"  if (-not (Test-Path -LiteralPath {_ps(root)})) {{ break }}",
              "  Start-Sleep -Seconds 1", "}"]
    remove = "\r\n".join(lines) + "\r\n"
    ask = "\r\n".join([
        "Add-Type -AssemblyName PresentationFramework",
        ("$text = 'Airglow set up LibreHardwareMonitor and OpenRGB on this PC. Remove them too?' + "
         "[Environment]::NewLine + [Environment]::NewLine + 'Windows will ask for permission.'"),
        f"$answer = [System.Windows.MessageBox]::Show($text, {_ps(APP_NAME)}, 'YesNo', 'Question')",
        "if ($answer -eq 'Yes') {",
        "  $script = Join-Path $PSScriptRoot 'remove.ps1'",
        ("  Start-Process powershell -Verb RunAs -WindowStyle Hidden -ArgumentList "
         "@('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $script)"),
        "}"]) + "\r\n"
    return ask, remove


def offer_tool_removal():
    ask, remove = removal_scripts()
    folder = Path(tempfile.mkdtemp(prefix="airglow-uninstall-"))
    (folder / "ask.ps1").write_text(ask, encoding="utf-8-sig")
    (folder / "remove.ps1").write_text(remove, encoding="utf-8-sig")
    cmd = ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-File", str(folder / "ask.ps1")]
    try:        # outlive the hook even if Velopack runs it in a job that ends with it
        subprocess.Popen(cmd, creationflags=CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB, close_fds=True)
    except OSError:
        subprocess.Popen(cmd, creationflags=CREATE_NO_WINDOW, close_fds=True)
