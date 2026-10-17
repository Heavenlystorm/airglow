"""Updates through Velopack, from this repository's GitHub releases, and the app-meta.json switchboard.

Airglow goes online for this only when the user has turned on "Check automatically" (a minute after start-up, then
every 6 hours) or presses "Check for updates". A found update downloads in the background and is applied on the next
start, or right away with "Restart to update". Updates never touch %APPDATA%\\Airglow.

Copies Velopack didn't install (running from source, a test build in dist\\) can still check, but only link to the
release page. Scoop installs are updated by Scoop.
"""

import json
import logging
import re
import sys
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path

from . import __version__, paths

log = logging.getLogger(__name__)

# Where releases are published. Empty hides the check in the UI.
UPDATE_REPO = "Heavenlystorm/airglow"
META_URL = f"https://raw.githubusercontent.com/{UPDATE_REPO}/main/meta/app-meta.json"
CHANNELS = ("stable", "beta")
FIRST_CHECK = 60              # seconds after start-up
CHECK_EVERY = 6 * 3600
MAX_DELTAS = 10


# ---------------------------------------------------------------- versions

def parse_version(v):
    """SemVer ordering, enough for our tags: 1.6.0 > 1.6.0-beta.2 > 1.6.0-beta.1 > 1.5.0."""
    m = re.match(r"v?(\d+)(?:\.(\d+))?(?:\.(\d+))?(?:-([0-9A-Za-z.-]+))?", str(v).strip())
    if not m:
        return (0, 0, 0, (0,))
    core = tuple(int(x or 0) for x in m.group(1, 2, 3))
    if not m.group(4):
        return (*core, (1,))
    pre = tuple((0, int(p), "") if p.isdigit() else (1, 0, p) for p in m.group(4).split("."))
    return (*core, (0, *pre))


def newer(a, b):
    return parse_version(a) > parse_version(b)


# ---------------------------------------------------------------- the old-style check (not installed by Velopack)

def check(repo=None, current=__version__, timeout=6):
    """Latest GitHub release, for copies that can't update themselves."""
    repo = repo or UPDATE_REPO
    if not repo:
        raise RuntimeError("Update checks aren't set up for this build")
    req = urllib.request.Request(f"https://api.github.com/repos/{repo}/releases/latest",
                                 headers={"Accept": "application/vnd.github+json", "User-Agent": f"Airglow/{current}"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            data = json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        raise RuntimeError("No published release found yet" if e.code == 404 else f"GitHub answered {e.code}") from e
    except (urllib.error.URLError, OSError) as e:
        raise RuntimeError("Couldn't reach GitHub. Check your internet connection.") from e
    latest = data.get("tag_name", "")
    return {"current": current, "latest": latest.lstrip("v"), "newer": newer(latest, current),
            "url": data.get("html_url"), "notes": (data.get("body") or "")[:2000]}


# ---------------------------------------------------------------- app-meta.json

def clean_meta(raw):
    """Validate app-meta.json; raises ValueError if it isn't usable."""
    if not isinstance(raw, dict) or raw.get("schema") != 1:
        raise ValueError("app-meta.json needs \"schema\": 1")
    min_supported = raw.get("minSupported")
    if not isinstance(min_supported, str) or not re.fullmatch(r"\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?", min_supported):
        raise ValueError("minSupported must be a version like 1.6.0")
    api = raw.get("effectApi")
    if not isinstance(api, list) or not api or not all(isinstance(a, int) and a > 0 for a in api):
        raise ValueError("effectApi must be a list of major versions")
    notice = raw.get("notice")
    if notice is not None and (not isinstance(notice, str) or len(notice) > 300):
        raise ValueError("notice must be text of at most 300 characters, or null")
    return {"schema": 1, "minSupported": min_supported, "effectApi": api, "notice": notice or None}


def _meta_cache():
    return paths.data_dir() / "cache" / "app-meta.json"


def cached_meta():
    try:
        return clean_meta(json.loads(_meta_cache().read_text(encoding="utf-8")))
    except (OSError, ValueError):
        return None


def fetch_meta(url=META_URL, timeout=6, opener=urllib.request.urlopen):
    """Fetch and cache app-meta.json. On any failure the cached copy (or None) is returned instead."""
    try:
        req = urllib.request.Request(url, headers={"User-Agent": f"Airglow/{__version__}"})
        with opener(req, timeout=timeout) as resp:
            meta = clean_meta(json.loads(resp.read(65536).decode("utf-8")))
    except (urllib.error.URLError, OSError, ValueError) as e:
        log.info("app-meta.json not updated: %s", e)
        return cached_meta()
    try:
        _meta_cache().parent.mkdir(exist_ok=True)
        _meta_cache().write_text(json.dumps(meta), encoding="utf-8")
    except OSError as e:
        log.warning("couldn't cache app-meta.json: %s", e)
    return meta


# ---------------------------------------------------------------- what kind of install this is

def install_kind():
    """"velopack" (updates itself), "scoop" (Scoop updates it) or "manual" (from source, or a loose build)."""
    if not getattr(sys, "frozen", False):
        return "manual"
    try:
        import velopack
        mgr = velopack.UpdateManager(_source(False))
    except (ImportError, RuntimeError):
        return "manual"
    if mgr.get_is_portable() and "\\scoop\\apps\\" in str(Path(sys.executable)).lower():
        return "scoop"
    return "velopack"


def _source(prerelease):
    import velopack
    return velopack.GithubSource(f"https://github.com/{UPDATE_REPO}", None, prerelease)


def velopack_managers(channel):
    """One update manager per feed to look at. Beta also looks at stable, so a beta user gets the next stable release."""
    import velopack
    managers = [velopack.UpdateManager(_source(False), velopack.UpdateOptions(False, MAX_DELTAS, "stable"))]
    if channel == "beta":
        managers.append(velopack.UpdateManager(_source(True), velopack.UpdateOptions(False, MAX_DELTAS, "beta")))
    return managers


def apply_pending(argv):
    """At start-up: if an update finished downloading last time, apply it now and start again with the same
    arguments (so a start at sign-in stays in the tray). Doesn't return in that case."""
    if not getattr(sys, "frozen", False):
        return
    try:
        import velopack
        mgr = velopack.UpdateManager(_source(False))
        pending = mgr.get_update_pending_restart()
    except (ImportError, RuntimeError):
        return
    if pending:
        log.info("applying the downloaded update to %s", pending.Version)
        mgr.apply_updates_and_restart_with_args(pending, list(argv))


# ---------------------------------------------------------------- the updater

class Updater:
    """Checks, downloads and applies updates on its own thread. status() is what the UI shows."""

    def __init__(self, store, kind=None, managers=velopack_managers, fetch_meta=fetch_meta, manual_check=check,
                 clock=time.monotonic):
        self.store = store
        self.kind = kind or install_kind()
        self._managers, self._fetch_meta, self._manual_check, self._clock = managers, fetch_meta, manual_check, clock
        self.quit_app = None                  # set by app.main; "Restart to update" quits through it
        self._lock = threading.Lock()
        self._wake = threading.Event()
        self._stop = threading.Event()
        self._thread = None
        self._ready = None                    # (manager, UpdateInfo) once downloaded
        self._last_check = None
        self.state = "idle"                   # idle, checking, downloading, ready, latest, error
        self.latest = self.url = self.error = None
        self.notes = ""
        self.progress = 0
        self.meta = cached_meta()

    # -- what the UI sees
    def status(self):
        meta = self.meta or {}
        return {"kind": self.kind, "state": self.state, "current": __version__, "latest": self.latest,
                "notes": self.notes, "progress": self.progress, "error": self.error, "url": self.url,
                "notice": meta.get("notice"),
                "unsupported": bool(meta) and newer(meta["minSupported"], __version__)}

    def _set(self, **kw):
        with self._lock:
            for k, v in kw.items():
                setattr(self, k, v)

    # -- background schedule
    def start(self):
        self._thread = threading.Thread(target=self._loop, name="updates", daemon=True)
        self._thread.start()

    def stop(self):
        self._stop.set()
        self._wake.set()

    def settings_changed(self):
        """Call when "Check automatically" or the channel changes, so the schedule reacts now."""
        self._wake.set()

    def due(self):
        if not self.store.settings().get("online_checks") or self.state in ("checking", "downloading", "ready"):
            return False
        return self._last_check is None or self._clock() - self._last_check >= CHECK_EVERY

    def _loop(self):
        if self._stop.wait(FIRST_CHECK):
            return
        while not self._stop.is_set():
            if self.due():
                self.check_now()
            self._wake.wait(300)            # re-evaluate every few minutes, or as soon as a setting changes
            self._wake.clear()

    # -- checking
    def _begin(self):
        """Mark a check as started; False if one is already running (or there's nothing for it to do)."""
        with self._lock:
            if self.kind == "scoop" or self.state in ("checking", "downloading", "ready"):
                return False
            self._last_check = self._clock()
            self.state, self.error, self.progress = "checking", None, 0
            return True

    def check_now(self):
        """Check (and for Velopack installs, download), synchronously."""
        if self._begin():
            self._run_check()
        return self.status()

    def check_in_background(self):
        """For the "Check for updates" button: answers at once with state "checking"; the UI follows the state."""
        if self._begin():
            threading.Thread(target=self._run_check, name="update-check", daemon=True).start()
        return self.status()

    def _run_check(self):
        self.meta = self._fetch_meta() or self.meta
        try:
            if self.kind == "velopack":
                self._check_velopack()
            else:
                r = self._manual_check()
                self._set(state="available" if r["newer"] else "latest", latest=r["latest"], url=r["url"],
                          notes=r["notes"])
        except Exception as e:  # never let an update problem reach the app
            log.warning("update check failed: %s", e)
            self._set(state="error", error=str(e) or type(e).__name__)

    def _check_velopack(self):
        channel = self.store.settings().get("update_channel", "stable")
        found = []
        for mgr in self._managers(channel):
            info = mgr.check_for_updates()
            if info is not None:
                found.append((mgr, info))
        if not found:
            self._set(state="latest", latest=__version__)
            return
        mgr, info = max(found, key=lambda f: parse_version(f[1].TargetFullRelease.Version))
        target = info.TargetFullRelease
        self._set(state="downloading", latest=target.Version, notes=(target.NotesMarkdown or "")[:4000])
        log.info("downloading Airglow %s (%s)", target.Version, "delta" if info.DeltasToTarget else "full")
        mgr.download_updates(info, lambda p: self._set(progress=int(p)))
        self._ready = (mgr, info)
        self._set(state="ready", progress=100)
        log.info("Airglow %s is ready; it's applied on the next start", target.Version)

    # -- applying
    def restart_to_update(self):
        if self.state != "ready" or not self._ready:
            raise RuntimeError("There's no downloaded update to install.")
        mgr, info = self._ready
        # Update.exe waits for Airglow to quit (fans go to the failsafe speed, devices are released), applies the
        # update and starts the new version.
        mgr.wait_exit_then_apply_updates(info, silent=True, restart=True, restart_args=[])
        if self.quit_app:
            self.quit_app()
