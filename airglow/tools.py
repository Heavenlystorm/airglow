"""Helper programs: download, check and set up LibreHardwareMonitor and OpenRGB, so temperatures and most RGB devices
work without manual steps.

Two halves. The app (never elevated) downloads the pinned files from data/tools.json and checks their size and
SHA-256, then starts a copy of itself with "--setup-tools <job file>" behind one Windows admin prompt. That elevated
copy copies the files into an admin-only folder (Program Files\\Airglow Tools), checks them again there, unpacks them,
installs the PawnIO driver and the Visual C++ runtime if they're missing, turns on LibreHardwareMonitor's web server
(on 127.0.0.1 only, with a password only Airglow knows), and registers a sign-in task per tool that starts it with
admin rights. Nothing that runs as admin is ever taken from a folder the user's other programs can write to.
"""

import ctypes
import hashlib
import json
import logging
import os
import secrets
import shutil
import subprocess
import sys
import threading
import time
import urllib.request
import zipfile
from ctypes import wintypes
from pathlib import Path
from xml.sax.saxutils import escape

from . import __version__, paths

log = logging.getLogger(__name__)

LHM_PORT = 8085
LHM_URL = f"http://127.0.0.1:{LHM_PORT}"    # LHM's listener matches the Host header, so "localhost" would be refused
LHM_USER = "airglow"
CREATE_NO_WINDOW = 0x08000000
VCREDIST_OK = {0, 1638, 3010}               # done, a newer one is already installed, done but wants a restart


class SetupError(Exception):
    pass


def spec():
    return json.loads((paths.PACKAGE_DIR / "data" / "tools.json").read_text(encoding="utf-8"))


def tools_dir():
    return Path(os.environ.get("ProgramW6432") or os.environ.get("ProgramFiles") or r"C:\Program Files") / "Airglow Tools"


_installed_cache = (0.0, {})


def installed(tid):
    """True when Airglow's own copy of a helper program is set up. Asked for by the state every two seconds, so the
    check is cached for a few seconds."""
    global _installed_cache
    now = time.monotonic()
    if now - _installed_cache[0] > 5.0:
        root = tools_dir()
        _installed_cache = (now, {k: (root / t["folder"] / t["exe"]).is_file() for k, t in spec()["tools"].items()})
    return bool(_installed_cache[1].get(tid))


def _start_task(name, run=subprocess.run):
    """Start a sign-in task now. Windows gives it the rights the task was registered with, without asking."""
    try:
        done = run(["schtasks", "/Run", "/TN", name], capture_output=True, creationflags=CREATE_NO_WINDOW)
    except OSError:
        return False
    return getattr(done, "returncode", 1) == 0


def _run_elevated(exe, args):
    """Start a program with administrator rights, and don't wait for it. True when Windows started it."""
    try:
        started = ctypes.windll.shell32.ShellExecuteW(None, "runas", str(exe), subprocess.list2cmdline(args),
                                                      str(Path(exe).parent), 1)
    except (AttributeError, OSError):
        return False
    return started > 32                    # ShellExecuteW returns an error code up to 32


def launch(tid, run=subprocess.run, elevate=_run_elevated, **subs):
    """Start a helper program Airglow set up (OpenRGB, usually).

    The rights matter: OpenRGB reaches graphics card, memory and onboard RGB over I2C/SMBus, which goes through the
    PawnIO driver and therefore needs administrator rights. Started normally it shows "one or more I2C/SMBus interfaces
    failed to initialize" and those devices are missing. Its sign-in task already runs with highest rights, so it's
    started through the task; without one (a copy someone installed themselves) Windows is asked once.
    """
    t = spec()["tools"].get(tid)
    if not t:
        raise SetupError(f"unknown helper program {tid}")
    exe = tools_dir() / t["folder"] / t["exe"]
    if not exe.is_file():
        raise SetupError(f"{t['name']} isn't set up on this PC; set it up in Settings, Helper programs")
    if _processes(t["exe"]):
        raise SetupError(f"{t['name']} is already running, but Airglow can't reach it; close it and let Airglow start it")
    if t.get("task") and _start_task(t["task"], run=run):
        log.info("started %s through its sign-in task", t["name"])
        return True
    args = t.get("args", "").format(**subs).split() if t.get("args") else []
    if not elevate(exe, args):
        raise SetupError(f"{t['name']} needs administrator rights to drive graphics card and memory lighting, and "
                         f"Windows didn't give them")
    log.info("started %s with administrator rights", exe)
    return True


def downloads_dir():
    d = paths.data_dir() / "downloads"
    d.mkdir(parents=True, exist_ok=True)
    return d


# ---------------------------------------------------------------- LibreHardwareMonitor's password

def auth_file():
    return paths.data_dir() / "lhm-auth.json"


_auth_cache = (None, {})


def lhm_headers():
    """The Authorization header for an LHM that Airglow set up ({} otherwise). Not part of settings, so bug reports
    never contain it."""
    global _auth_cache
    f = auth_file()
    try:
        mtime = f.stat().st_mtime
    except OSError:
        return {}
    if _auth_cache[0] != mtime:
        try:
            a = json.loads(f.read_text(encoding="utf-8"))
            import base64
            token = base64.b64encode(f"{a['user']}:{a['password']}".encode()).decode()
            _auth_cache = (mtime, {"Authorization": f"Basic {token}"})
        except (OSError, ValueError, KeyError, TypeError):
            _auth_cache = (mtime, {})
    return _auth_cache[1]


# ---------------------------------------------------------------- what's on this PC

def _hklm_value(key, name):
    import winreg
    for view in (winreg.KEY_WOW64_64KEY, winreg.KEY_WOW64_32KEY):
        try:
            with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, key, 0, winreg.KEY_READ | view) as k:
                return winreg.QueryValueEx(k, name)[0]
        except OSError:
            continue
    return None


def _version_tuple(v):
    try:
        return tuple(int(x) for x in str(v).split(".")[:3])
    except ValueError:
        return None


def pawnio_needed(s=None):
    s = s or spec()
    have = _version_tuple(_hklm_value(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\PawnIO", "DisplayVersion"))
    return have is None or have < _version_tuple(s["pawnio"]["min_version"])


def vcredist_needed():
    return _hklm_value(r"SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64", "Installed") != 1


def _processes(exe_name):
    """Running copies of an exe: [(pid, path or None)]. The path of an elevated process can be unreadable."""
    import psutil
    out = []
    for p in psutil.process_iter(["name"]):
        if (p.info.get("name") or "").lower() == exe_name.lower():
            try:
                out.append((p.pid, p.exe()))
            except (psutil.AccessDenied, psutil.NoSuchProcess, OSError):
                out.append((p.pid, None))
    return out


def _inside(path, folder):
    try:
        Path(path).resolve().relative_to(Path(folder).resolve())
        return True
    except (ValueError, OSError):
        return False


def status(ctrl):
    s, root = spec(), tools_dir()
    working = {"lhm": bool(ctrl.sensors.get("online")), "openrgb": bool(ctrl.manager.bridge.client.connected)}
    out = []
    for tid, t in s["tools"].items():
        exe = root / t["folder"] / t["exe"]
        installed = exe.is_file()
        procs = _processes(t["exe"])
        # a copy we can't see the path of counts as ours when ours is installed
        ours = any(p and _inside(p, root) for _, p in procs) or (installed and any(p is None for _, p in procs))
        out.append({"id": tid, "name": t["name"], "purpose": t["purpose"], "version": t["version"], "size": t["size"],
                    "licence": t["licence"], "home": t["home"], "installed": installed,
                    "running": bool(procs), "working": working.get(tid, False),
                    # someone's own copy is running but Airglow can't reach it: setting ours up next to it would
                    # make two programs fight over the same hardware, so the UI explains what to turn on instead
                    "other_copy": bool(procs) and not ours and not working.get(tid, False)})
    return {"tools": out, "folder": str(root), "job": JOB.snapshot(),
            "setup_available": any(not t["working"] and not t["other_copy"] for t in out),
            "installed_any": any(t["installed"] for t in out)}


# ---------------------------------------------------------------- downloads

def sha256_of(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def verify(path, size, sha256):
    actual = Path(path).stat().st_size
    if actual != size:
        raise SetupError(f"{Path(path).name} is {actual} bytes, expected {size}")
    if sha256_of(path).lower() != sha256.lower():
        raise SetupError(f"{Path(path).name} doesn't match its checksum, so it wasn't used")


def download(url, dest, size=None, sha256=None, progress=None, opener=urllib.request.urlopen):
    """Fetch url to dest (via a .part file). With size and sha256, a mismatch deletes the file and raises."""
    dest = Path(dest)
    part = dest.with_name(dest.name + ".part")
    req = urllib.request.Request(url, headers={"User-Agent": f"Airglow/{__version__}"})
    try:
        with opener(req, timeout=30) as resp, open(part, "wb") as f:
            total = size or int(resp.headers.get("Content-Length") or 0)
            done = 0
            while chunk := resp.read(1 << 16):
                f.write(chunk)
                done += len(chunk)
                if size and done > size:
                    raise SetupError(f"{dest.name} is larger than expected")
                if progress and total:
                    progress(min(done / total, 1.0))
        if size and sha256:
            verify(part, size, sha256)
        os.replace(part, dest)
    except OSError as e:
        part.unlink(missing_ok=True)
        raise SetupError(f"couldn't download {dest.name}: {e}") from None
    except SetupError:
        part.unlink(missing_ok=True)
        raise
    return dest


# ---------------------------------------------------------------- elevated work (runs as admin)

def extract(zip_path, dest, strip=""):
    """Unpack the entries under `strip` into dest, refusing any that would land outside it."""
    dest = Path(dest)
    dest.mkdir(parents=True, exist_ok=True)
    root = dest.resolve()
    with zipfile.ZipFile(zip_path) as z:
        for info in z.infolist():
            name = info.filename.replace("\\", "/")
            if not name.startswith(strip):
                continue
            rel = name[len(strip):]
            if not rel:
                continue
            target = (dest / rel).resolve()
            if target != root and root not in target.parents:
                raise SetupError(f"{Path(zip_path).name} contains an unsafe path: {name}")
            if name.endswith("/"):
                target.mkdir(parents=True, exist_ok=True)
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            with z.open(info) as src, open(target, "wb") as out:
                shutil.copyfileobj(src, out)


def lhm_config(password_sha256, port=LHM_PORT):
    """LibreHardwareMonitor.config: web server on, this PC only, password required, starts quietly in the tray. It
    also reads less than LHM does by default, since Airglow asks for readings every 2 s: no network adapters, power
    supplies or batteries (Airglow takes network and disk throughput from Windows), every 2 s rather than every
    second, drive SMART data every 30 s (enough for the SSD temperature) and 30 s of history kept in memory instead
    of 12 hours. Key names and values as LHM 0.9.6 reads them (UI/MainForm.cs, Utilities/PersistentSettings.cs)."""
    values = {"runWebServerMenuItem": "true", "listenerIp": "127.0.0.1", "listenerPort": str(port),
              "authenticationEnabled": "true", "authenticationUserName": LHM_USER,
              "authenticationPassword": password_sha256,
              "startMinMenuItem": "true", "minTrayMenuItem": "true", "minCloseMenuItem": "true",
              "nicMenuItem": "false", "psuMenuItem": "false", "batteryMenuItem": "false",
              "updateIntervalMenuItem": "3",          # 250 ms, 500 ms, 1 s, 2 s, 5 s, 10 s
              "throttleAtaUpdateMenuItem": "true",
              "sensorValuesTimeWindow": "0"}          # 30 s, 1 min, ... 24 h
    quote = {'"': "&quot;"}
    rows = "".join(f'    <add key="{k}" value="{escape(v, quote)}" />\n' for k, v in values.items())
    return f'<?xml version="1.0" encoding="utf-8"?>\n<configuration>\n  <appSettings>\n{rows}  </appSettings>\n</configuration>\n'


def task_xml(user_sid, exe, args, workdir, description):
    """A sign-in task for one user that starts exe with the highest rights that user has, with no time limit."""
    e = escape
    return f"""<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Author>Airglow</Author><Description>{e(description)}</Description></RegistrationInfo>
  <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{e(user_sid)}</UserId><Delay>PT5S</Delay></LogonTrigger></Triggers>
  <Principals><Principal id="Author"><UserId>{e(user_sid)}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>5</Priority>
    <IdleSettings><StopOnIdleEnd>false</StopOnIdleEnd><RestartOnIdle>false</RestartOnIdle></IdleSettings>
  </Settings>
  <Actions Context="Author"><Exec><Command>{e(str(exe))}</Command><Arguments>{e(args)}</Arguments><WorkingDirectory>{e(str(workdir))}</WorkingDirectory></Exec></Actions>
</Task>
"""


def _run(cmd, run=subprocess.run, **kw):
    log.info("running %s", " ".join(str(c) for c in cmd))
    return run([str(c) for c in cmd], capture_output=True, creationflags=CREATE_NO_WINDOW, **kw)


def _stop(t, root, run):
    """End our task and any copy running from our folder, so its files can be replaced or removed."""
    _run(["schtasks", "/End", "/TN", t["task"]], run)
    import psutil
    for pid, p in _processes(t["exe"]):
        if p and _inside(p, root):
            try:
                proc = psutil.Process(pid)
                proc.terminate()
                proc.wait(timeout=5)
            except (psutil.Error, OSError):
                pass


def _ps_literal(text):
    """text as a PowerShell single-quoted string."""
    return "'" + str(text).replace("'", "''") + "'"


def _microsoft_signed(path, run):
    # The path goes into the script itself. Arguments after "-Command" don't reach $args: PowerShell glues them onto the
    # end of the script text, which made this check fail for every file (and would split a path with spaces, like
    # "Program Files"). The script is the last argument, so there is nothing to glue.
    ps = (f"$s = Get-AuthenticodeSignature -LiteralPath {_ps_literal(path)}; "
          "if ($s.Status -eq 'Valid' -and $s.SignerCertificate.Subject -like '*O=Microsoft Corporation*') { exit 0 } exit 1")
    # Started from PowerShell 7 (a terminal, CI), PSModulePath lists PowerShell 7's own modules, and Windows PowerShell
    # then can't load the one Get-AuthenticodeSignature lives in. Without it, Windows PowerShell uses its own defaults.
    env = {k: v for k, v in os.environ.items() if k.upper() != "PSMODULEPATH"}
    return _run(["powershell", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", ps],
                run, env=env).returncode == 0


def openrgb_settings_file():
    return paths._appdata() / "OpenRGB" / "OpenRGB.json"


def disable_openrgb_detectors(names, path=None):
    """Switch off OpenRGB's detectors for controllers Airglow lights itself, so OpenRGB never opens them. Merges into
    an existing OpenRGB.json (everything else in it stays); one that can't be read is left alone. Returns whether
    the file now has them off."""
    path = Path(path or openrgb_settings_file())
    if not names:
        return True
    try:
        data = json.loads(path.read_text(encoding="utf-8")) if path.exists() else {}
    except (OSError, ValueError) as e:
        log.warning("left %s alone: couldn't read it (%s)", path, e)
        return False
    section = data.get("Detectors") if isinstance(data, dict) else None
    detectors = section.get("detectors") if isinstance(section, dict) else {} if section is None else None
    if not isinstance(data, dict) or not isinstance(detectors, dict):
        log.warning("left %s alone: its detector list isn't in the expected format", path)
        return False
    if all(detectors.get(n) is False for n in names):
        return True
    data["Detectors"] = {**(section or {}), "detectors": {**detectors, **{n: False for n in names}}}
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        tmp = path.with_name(path.name + ".airglow-tmp")
        tmp.write_text(json.dumps(data, indent=4), encoding="utf-8")
        tmp.replace(path)
    except OSError as e:
        log.warning("couldn't update %s: %s", path, e)
        return False
    log.info("OpenRGB won't open what Airglow lights: switched off its %s detector%s", ", ".join(names),
             "" if len(names) == 1 else "s")
    return True


def install(job, root=None, run=subprocess.run):
    """The elevated part of setup. Returns the list of steps done; raises SetupError on the first failure."""
    s, root = spec(), Path(root or tools_dir())
    stage = root / ".staging"
    shutil.rmtree(stage, ignore_errors=True)
    stage.mkdir(parents=True)
    done = []

    def staged(src, name):
        dst = stage / name
        shutil.copyfile(src, dst)
        return dst

    try:
        if job.get("vcredist"):
            f = staged(job["vcredist"], "vc_redist.x64.exe")
            if not _microsoft_signed(f, run):
                raise SetupError("the Visual C++ runtime download isn't signed by Microsoft, so it wasn't installed")
            code = _run([f, "/install", "/quiet", "/norestart"], run).returncode
            if code not in VCREDIST_OK:
                raise SetupError(f"the Visual C++ runtime installer failed (code {code})")
            done.append("vcredist")
        if job.get("pawnio"):
            p = s["pawnio"]
            f = staged(job["pawnio"], "PawnIO_setup.exe")
            verify(f, p["size"], p["sha256"])
            _run([f, "-install"], run)
            if pawnio_needed(s):
                raise SetupError("the PawnIO driver didn't install")
            done.append("pawnio")
        for tid in job["tools"]:
            t = s["tools"][tid]
            f = staged(job["files"][tid], f"{tid}.zip")
            verify(f, t["size"], t["sha256"])
            folder = root / t["folder"]
            _stop(t, root, run)
            shutil.rmtree(folder, ignore_errors=True)
            extract(f, folder, t["strip"])
            exe = folder / t["exe"]
            if not exe.is_file():
                raise SetupError(f"{t['name']} wasn't in its download where expected")
            if tid == "lhm":
                (folder / "LibreHardwareMonitor.config").write_text(lhm_config(job["lhm_password_sha256"]), encoding="utf-8")
            args = t["args"].format(openrgb_port=int(job.get("openrgb_port", 6742)))
            xml = stage / f"{tid}-task.xml"
            xml.write_text(task_xml(job["user_sid"], exe, args, folder,
                                    f"Starts {t['name']} when you sign in, for Airglow."), encoding="utf-16")
            r = _run(["schtasks", "/Create", "/TN", t["task"], "/XML", xml, "/F"], run)
            if r.returncode != 0:
                raise SetupError(f"couldn't register {t['name']} to start at sign-in")
            _run(["schtasks", "/Run", "/TN", t["task"]], run)
            done.append(tid)
    finally:
        shutil.rmtree(stage, ignore_errors=True)
    return done


def remove(root=None, run=subprocess.run):
    """The elevated part of removal: tasks, running copies and the folder. PawnIO and the C++ runtime stay, as
    other programs may use them."""
    s, root = spec(), Path(root or tools_dir())
    for t in s["tools"].values():
        _stop(t, root, run)
        _run(["schtasks", "/Delete", "/TN", t["task"], "/F"], run)
    for _ in range(5):                 # a just-stopped process can hold its files for a moment
        shutil.rmtree(root, ignore_errors=True)
        if not root.exists():
            break
        time.sleep(1)
    if root.exists():
        raise SetupError(f"couldn't delete {root} completely")


RESULT_FILES = {"--setup-tools": "setup-result.json", "--remove-tools": "remove-result.json"}


def result_file(job_path, command):
    """Where the elevated copy reports back: a fixed name next to the job file, never a path the job file names."""
    return Path(job_path).with_name(RESULT_FILES[command])


def elevated_main(argv):
    """Entry for "Airglow.exe --setup-tools <job>" and "--remove-tools [<job>]", started through the admin prompt."""
    handler = logging.FileHandler(paths.log_dir() / "tools-setup.log", encoding="utf-8")
    handler.setFormatter(logging.Formatter("%(asctime)s %(levelname)-7s %(name)s: %(message)s"))
    logging.getLogger().addHandler(handler)
    logging.getLogger().setLevel(logging.INFO)
    job_path = Path(argv[1]) if len(argv) > 1 else None
    job = json.loads(job_path.read_text(encoding="utf-8")) if job_path else {}
    result = {"ok": False, "done": [], "error": None}
    try:
        if argv[0] == "--setup-tools":
            result["done"] = install(job)
        else:
            remove()
        result["ok"] = True
    except (SetupError, OSError, KeyError, ValueError) as e:
        log.exception("helper setup failed")
        result["error"] = str(e)
    if job_path:
        result_file(job_path, argv[0]).write_text(json.dumps(result), encoding="utf-8")
    return 0 if result["ok"] else 1


# ---------------------------------------------------------------- starting the elevated copy

class SHELLEXECUTEINFOW(ctypes.Structure):
    _fields_ = [("cbSize", wintypes.DWORD), ("fMask", ctypes.c_ulong), ("hwnd", wintypes.HWND),
                ("lpVerb", wintypes.LPCWSTR), ("lpFile", wintypes.LPCWSTR), ("lpParameters", wintypes.LPCWSTR),
                ("lpDirectory", wintypes.LPCWSTR), ("nShow", ctypes.c_int), ("hInstApp", wintypes.HINSTANCE),
                ("lpIDList", ctypes.c_void_p), ("lpClass", wintypes.LPCWSTR), ("hkeyClass", wintypes.HKEY),
                ("dwHotKey", wintypes.DWORD), ("hIconOrMonitor", wintypes.HANDLE), ("hProcess", wintypes.HANDLE)]


def self_command(args):
    """This program with extra arguments: the exe when installed, run.py when running from source."""
    if getattr(sys, "frozen", False):
        return sys.executable, args
    py = Path(sys.executable)
    pyw = py.with_name("pythonw.exe")
    return str(pyw if pyw.exists() else py), [str(paths.resource_dir() / "run.py"), *args]


def run_elevated(args):
    """Start this program as admin and wait. Returns its exit code, or None if the user said no."""
    exe, params = self_command(args)
    info = SHELLEXECUTEINFOW(cbSize=ctypes.sizeof(SHELLEXECUTEINFOW), fMask=0x40 | 0x100,  # NOCLOSEPROCESS | NOASYNC
                             lpVerb="runas", lpFile=exe, lpParameters=subprocess.list2cmdline(params), nShow=0)
    if not ctypes.windll.shell32.ShellExecuteExW(ctypes.byref(info)):
        if ctypes.GetLastError() == 1223:                      # ERROR_CANCELLED: "No" on the admin prompt
            return None
        raise SetupError(f"couldn't start the setup step (error {ctypes.GetLastError()})")
    k32 = ctypes.windll.kernel32
    k32.WaitForSingleObject(info.hProcess, 0xFFFFFFFF)
    code = wintypes.DWORD()
    k32.GetExitCodeProcess(info.hProcess, ctypes.byref(code))
    k32.CloseHandle(info.hProcess)
    return code.value


def current_user_sid(run=subprocess.run):
    out = _run(["whoami", "/user", "/fo", "csv", "/nh"], run, text=True).stdout.strip()
    sid = out.rsplit(",", 1)[-1].strip().strip('"')
    if not sid.startswith("S-1-"):
        raise SetupError("couldn't find your Windows account's id")
    return sid


# ---------------------------------------------------------------- the job the UI follows

class Job:
    """One setup or removal at a time, run on its own thread; the UI polls snapshot()."""

    def __init__(self):
        self._lock = threading.Lock()
        self._s = {"state": "idle", "kind": None, "step": "", "progress": 0.0, "error": None}

    def snapshot(self):
        with self._lock:
            return dict(self._s)

    def update(self, **kw):
        with self._lock:
            self._s.update(kw)

    def busy(self):
        return self.snapshot()["state"] in ("working", "waiting")

    def start(self, kind, target):
        with self._lock:
            if self._s["state"] in ("working", "waiting"):
                raise SetupError("setup is already running")
            self._s = {"state": "working", "kind": kind, "step": "Starting", "progress": 0.0, "error": None}

        def body():
            try:
                outcome = target(self)
                self.update(state=outcome or "done", step="")
            except SetupError as e:
                log.warning("helper %s failed: %s", kind, e)
                self.update(state="failed", error=str(e), step="")
            except Exception as e:                              # noqa: BLE001 - reported to the UI, not lost
                log.exception("helper %s failed", kind)
                self.update(state="failed", error=f"Something went wrong ({e}). The log has details.", step="")

        threading.Thread(target=body, name=f"tools-{kind}", daemon=True).start()


JOB = Job()


def _elevate(job, args, result):
    result.unlink(missing_ok=True)
    job.update(state="waiting", step="Waiting for you to allow the changes in the Windows prompt")
    code = run_elevated(args)
    if code is None:
        return None
    try:
        r = json.loads(result.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        r = {"ok": code == 0, "error": f"the setup step stopped (code {code})"}
    finally:
        result.unlink(missing_ok=True)
    if not r.get("ok"):
        raise SetupError(r.get("error") or "the setup step failed")
    return r


def start_setup(ctrl, store, ids=None):
    st = status(ctrl)
    wanted = ids or [t["id"] for t in st["tools"] if not t["working"] and not t["other_copy"]]
    s = spec()
    wanted = [i for i in wanted if i in s["tools"]]
    if not wanted:
        raise SetupError("everything is already working")

    def work(job):
        dl = downloads_dir()
        files = {}
        need_pawnio = pawnio_needed(s)
        need_vc = any(i in s["vcredist"]["needed_by"] for i in wanted) and vcredist_needed()
        steps = [*wanted, *(["pawnio"] if need_pawnio else []), *(["vcredist"] if need_vc else [])]
        for n, tid in enumerate(wanted):
            t = s["tools"][tid]
            job.update(step=f"Downloading {t['name']} {t['version']}")
            files[tid] = str(download(t["url"], dl / f"{tid}.zip", t["size"], t["sha256"],
                                      lambda f, n=n: job.update(progress=(n + f) / len(steps))))
        job_file = {"tools": wanted, "files": files, "user_sid": current_user_sid(),
                    "openrgb_port": store.settings()["openrgb_port"]}
        if need_pawnio:
            p = s["pawnio"]
            job.update(step="Downloading the PawnIO driver")
            job_file["pawnio"] = str(download(p["url"], dl / "PawnIO_setup.exe", p["size"], p["sha256"]))
        if need_vc:
            job.update(step="Downloading the Visual C++ runtime")
            job_file["vcredist"] = str(download(s["vcredist"]["url"], dl / "vc_redist.x64.exe"))
        password = None
        if "lhm" in wanted:
            password = secrets.token_urlsafe(24)
            job_file["lhm_password_sha256"] = hashlib.sha256(password.encode()).hexdigest()
        job.update(progress=1.0)
        if "openrgb" in wanted:
            # before OpenRGB first starts, and here rather than in the elevated step: an administrator's
            # elevation can run as another account, with another %APPDATA%
            from .hw import openrgb_detectors_to_disable
            disable_openrgb_detectors(openrgb_detectors_to_disable(ctrl.manager.natives.values()))
        path = dl / "setup-job.json"
        path.write_text(json.dumps(job_file), encoding="utf-8")
        try:
            r = _elevate(job, ["--setup-tools", str(path)], result_file(path, "--setup-tools"))
        finally:
            path.unlink(missing_ok=True)
        if r is None:
            return "cancelled"
        job.update(state="working", step="Starting the programs")
        if password:
            auth_file().write_text(json.dumps({"user": LHM_USER, "password": password}), encoding="utf-8")
            store.update_settings({"lhm_url": LHM_URL})
        if "lhm" in wanted and hasattr(ctrl, "lhm"):
            ctrl.lhm.wake()                                     # read it as soon as it answers, not in 12 s
        if "openrgb" in wanted:
            store.update_settings({"openrgb_enabled": True})
            ctrl.manager.bridge._last_try = 0.0                 # connect as soon as OpenRGB's server is up
        shutil.rmtree(dl, ignore_errors=True)
        log.info("helper programs set up: %s", ", ".join(r.get("done", [])))
        return "done"

    JOB.start("setup", work)
    return status(ctrl)


def start_remove(ctrl):
    def work(job):
        dl = downloads_dir()
        path = dl / "remove-job.json"
        path.write_text(json.dumps({}), encoding="utf-8")
        try:
            r = _elevate(job, ["--remove-tools", str(path)], result_file(path, "--remove-tools"))
        finally:
            path.unlink(missing_ok=True)
        if r is None:
            return "cancelled"
        auth_file().unlink(missing_ok=True)
        shutil.rmtree(dl, ignore_errors=True)
        return "done"

    JOB.start("remove", work)
    return status(ctrl)
