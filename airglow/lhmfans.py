"""Fans and pumps that LibreHardwareMonitor can drive: the motherboard's fan headers (its Super I/O chip), and the
pumps and fans of the coolers and fan hubs LHM supports, such as NZXT Kraken X3, Z3 and 2023, Aquacomputer pumps
and hubs, MSI CoreLiquid, Arctic and Razer fan controllers and the NZXT Grid+.

LHM lists each one as a "Control" sensor and changes it through its web server (POST /Sensor?action=Set, LHM 0.8
and later). Nothing is driven until you choose to: a channel is left alone (the BIOS or the cooler keeps its own
curve), follows Airglow's fan curve, or holds a steady speed, which suits pumps. When Airglow quits, every channel it
drove is handed back to its own control (value=null), so the BIOS curve takes over again.

Graphics card fans are left out on purpose: they belong to the card, and a case curve would follow the wrong
temperature.
"""

import json
import logging
import re
import threading
import time
import urllib.error
import urllib.parse
import urllib.request

log = logging.getLogger(__name__)

MODES = ("curve", "steady")
MAX_CHANNELS = 32
_PUMP = re.compile(r"pump", re.I)


def channels(flat):
    """Every fan or pump LHM can drive, from the flattened sensor list (sensors.flatten): [{"id", "name",
    "device", "hw", "fan_id", "duty", "rpm", "pump"}]. duty is what the channel runs at now, in percent; fan_id
    is the RPM sensor that goes with it."""
    by_id = {s["id"]: s for s in flat}
    out = []
    for s in flat:
        if s["type"] not in ("control", "controls") or s["hw"] in ("gpu", "igpu"):
            continue
        fan = by_id.get(s["id"].replace("/control/", "/fan/")) if "/control/" in s["id"] else None
        out.append({"id": s["id"], "name": s["name"], "device": s["hw_name"] or "", "hw": s["hw"],
                    "fan_id": fan["id"] if fan else None,
                    "duty": s["value"], "rpm": round(fan["value"]) if fan and fan["value"] is not None else None,
                    "pump": bool(_PUMP.search(s["name"]) or _PUMP.search(fan["name"] if fan else ""))})
    return out


def validate(cfg):
    """One channel's setting: {"mode": "curve"} or {"mode": "steady", "duty": 20..100}."""
    from .config import MIN_FAN_DUTY
    if not isinstance(cfg, dict) or cfg.get("mode") not in MODES:
        raise ValueError("pick how this fan should run")
    out = {"mode": cfg["mode"]}
    if cfg["mode"] == "steady":
        try:
            duty = int(cfg.get("duty", 60))
        except (TypeError, ValueError):
            raise ValueError("speed must be a whole number") from None
        if not MIN_FAN_DUTY <= duty <= 100:
            raise ValueError(f"speed must be between {MIN_FAN_DUTY} and 100%")
        out["duty"] = duty
    return out


def validate_id(sid):
    sid = str(sid or "")
    if not 1 <= len(sid) <= 200 or not sid.startswith("/") or any(ord(c) < 32 for c in sid):
        raise ValueError("unknown fan")
    return sid


def http_set(base_url, sensor_id, value, headers=None, timeout=1.5):
    """Ask LHM to set one control: value in percent, or None to give it back to its own control."""
    query = urllib.parse.urlencode({"action": "Set", "id": sensor_id, "value": "null" if value is None else f"{value:.0f}"})
    req = urllib.request.Request(f"{base_url.rstrip('/')}/Sensor?{query}", data=b"", method="POST",
                                 headers=headers or {})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            reply = json.loads(resp.read().decode("utf-8") or "{}")
    except (urllib.error.URLError, OSError, ValueError) as e:
        raise OSError(f"LibreHardwareMonitor didn't answer ({e})") from None
    if reply.get("result") != "ok":
        # LHM sends the whole .NET exception; its first line says what went wrong
        raise OSError(str(reply.get("message") or "LibreHardwareMonitor refused").splitlines()[0][:200])


class LhmFans:
    """Sends LHM the speeds Airglow wants, only when they change (and again every RESEND seconds, because sleep or
    an LHM restart puts the hardware back on its own curve), and gives channels back when they're no longer ours."""

    RESEND = 30.0

    def __init__(self, base_url, headers=lambda: {}, send=http_set):
        self._base_url, self._headers, self._send = base_url, headers, send
        self.sent = {}          # sensor id -> (duty, monotonic time sent)
        self.errors = {}        # sensor id -> last error text
        # the controller's loop, the power events (failsafe at shutdown) and quitting all send
        self._lock = threading.RLock()

    def apply(self, targets):
        """targets: {sensor id: duty} for every channel Airglow drives right now. Channels driven before but missing
        now are handed back to their own control."""
        with self._lock:
            self._apply(targets)

    def _apply(self, targets):
        now = time.monotonic()
        for sid in [s for s in self.sent if s not in targets]:
            self._release(sid)
        for sid, duty in targets.items():
            last = self.sent.get(sid)
            if last and last[0] == duty and now - last[1] < self.RESEND:
                continue
            try:
                self._send(self._base_url(), sid, duty, self._headers())
                self.sent[sid] = (duty, now)
                self.errors.pop(sid, None)
            except OSError as e:
                if self.errors.get(sid) != str(e):
                    log.warning("fan %s: %s", sid, e)
                self.errors[sid] = str(e)
                self.sent.pop(sid, None)           # try again next time

    def release(self, sid):
        with self._lock:
            self._release(sid)

    def _release(self, sid):
        self.sent.pop(sid, None)
        self.errors.pop(sid, None)
        try:
            self._send(self._base_url(), sid, None, self._headers())
            log.info("fan %s handed back to its own control", sid)
        except OSError as e:
            log.warning("fan %s: couldn't hand it back: %s", sid, e)

    def release_all(self):
        with self._lock:
            for sid in list(self.sent):
                self._release(sid)

    def resend_all(self):
        """LHM went away: whatever it held is lost if it restarts, so send everything again once it's back."""
        with self._lock:
            self.sent = {sid: (duty, float("-inf")) for sid, (duty, _) in self.sent.items()}

    def failsafe(self, duty):
        """Everything Airglow drives to at least duty, now (a channel held faster keeps its speed). Returns how
        many channels that was."""
        with self._lock:
            targets = {sid: max(d, duty) for sid, (d, _) in self.sent.items()}
            self.resend_all()
            self._apply(targets)
            return len(targets)
