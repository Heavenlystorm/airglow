"""Keep Airglow unit tests away from the installed app and its hardware."""

import atexit
import ctypes
import os
import platform
import re
import socket
import subprocess
import sys
import tempfile
import threading
import weakref
from contextlib import contextmanager
from contextvars import ContextVar
from pathlib import Path

_child_command = ContextVar("airglow_test_child", default=None)
_bound_sockets = weakref.WeakSet()
_socket_lock = threading.RLock()


def _blocked(operation):
    raise AssertionError(f"{operation} is blocked in Airglow unit tests; provide a test fixture")


def _owns_endpoint(address):
    if not isinstance(address, tuple) or address[0] not in ("127.0.0.1", "::1"):
        return False
    with _socket_lock:
        for sock in list(_bound_sockets):
            try:
                if sock.getsockname()[:2] == address[:2]:
                    return True
            except OSError:
                continue
    return False


def _guard(event, args):
    if event == "socket.bind":
        with _socket_lock:
            _bound_sockets.add(args[0])
    elif event == "socket.connect":
        if not _owns_endpoint(args[1]):
            _blocked(f"{event} to {args[1]!r}")
    elif event == "socket.getaddrinfo":
        if args[0] not in (None, "localhost", "127.0.0.1", "::1"):
            _blocked(f"{event} for {args[0]!r}")
    elif event == "subprocess.Popen":
        if _child_command.get() is None or args[1] != _child_command.get():
            _blocked(event)
    elif event in {
        "os.exec", "os.posix_spawn", "os.spawn", "os.startfile", "os.system",
        "winreg.CreateKey", "winreg.DeleteKey", "winreg.DeleteValue", "winreg.LoadKey",
        "winreg.SaveKey", "winreg.SetValue", "winreg.SetValueEx",
    }:
        _blocked(event)


def _fixture_process(start, command, **kwargs):
    audited = subprocess.list2cmdline(command) if os.name == "nt" else command
    token = _child_command.set(audited)
    try:
        return start(command, **kwargs)
    finally:
        _child_command.reset(token)


def popen_python(code, *args):
    """Start a synthetic Python child with the isolated environment."""
    return _fixture_process(subprocess.Popen, [sys.executable, "-B", "-c", code, *args])


def run_python(code, *args, cwd=None):
    """Run a synthetic Python child and capture its output."""
    return _fixture_process(subprocess.run, [sys.executable, "-B", "-c", code, *args],
                            cwd=cwd, capture_output=True, text=True, check=True, timeout=20)


@contextmanager
def refusing_port():
    """Reserve a loopback port without listening, so no live service can answer."""
    with socket.socket() as sock:
        if hasattr(socket, "SO_EXCLUSIVEADDRUSE"):
            sock.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
        sock.bind(("127.0.0.1", 0))
        yield sock.getsockname()[1]


def run_authenticode_probe(command, **kwargs):
    """Allow the signature test's read-only probe and no installer scripts."""
    prefix = ["powershell", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command"]
    signature = (
        r"\$s = Get-AuthenticodeSignature -LiteralPath '(?:[^']|'')*'; "
        r"if \(\$s.Status -eq 'Valid' -and \$s.SignerCertificate.Subject -like "
        r"'\*O=Microsoft Corporation\*'\) \{ exit 0 \} exit 1"
    )
    if command[:-1] != prefix or not re.fullmatch(signature, command[-1]):
        _blocked("PowerShell command")
    powershell = Path(os.environ["SystemRoot"]) / "System32" / "WindowsPowerShell" / "v1.0" / "powershell.exe"
    return _fixture_process(subprocess.run, [str(powershell), *command[1:]], timeout=20, **kwargs)


def _deny(operation):
    def denied(*args, **kwargs):
        _blocked(operation)

    return denied


def isolate():
    profile = tempfile.TemporaryDirectory(prefix="airglow-tests-")
    atexit.register(profile.cleanup)
    home = Path(profile.name).resolve()  # Windows runner TEMP can use an 8.3 alias such as RUNNER~1.
    os.environ.update({
        "HOME": str(home),
        "USERPROFILE": str(home),
        "HOMEDRIVE": home.drive,
        "HOMEPATH": str(home)[len(home.drive):],
        "APPDATA": str(home / "AppData" / "Roaming"),
        "LOCALAPPDATA": str(home / "AppData" / "Local"),
        "ProgramFiles": str(home / "Program Files"),
        "ProgramFiles(x86)": str(home / "Program Files (x86)"),
        "ProgramW6432": str(home / "Program Files"),
        "SCOOP": str(home / "scoop"),
    })
    sys.addaudithook(_guard)

    # Win32 known folders use the real account rather than USERPROFILE.
    if os.name == "nt":
        from unittest.mock import patch

        # CPython can use the built-in Windows version API when its shell probe is unavailable.
        def no_version_shell(system="", release="", version=""):
            return system, release, version

        with patch.object(platform, "_syscmd_ver", no_version_shell):
            platform.uname()
        shell = ctypes.windll.shell32
        shell.SHGetFolderPathW = lambda *args: -1
        shell.ShellExecuteW = _deny("ShellExecuteW")
        shell.ShellExecuteExW = _deny("ShellExecuteExW")

    import hid
    from airglow import dynlight
    from airglow.lighting import inputs

    hid.device = _deny("HID device open")
    hid.enumerate = _deny("HID device enumeration")
    dynlight.DynamicLightingBridge._scan = staticmethod(_deny("Dynamic Lighting device open"))
    inputs.ScreenSampler.run = _deny("screen capture")
    inputs.AudioAnalyzer.run = _deny("audio capture")
