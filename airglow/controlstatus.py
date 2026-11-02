"""Bounded, thread-safe evidence of completed work. Queues are never acknowledgements."""

from collections import OrderedDict, deque
import threading
import time


class ControlStatus:
    MAX_DEVICES = 128
    MAX_EVENTS = 200
    COMPONENTS = ("controller", "sensors", "lighting")

    def __init__(self, monotonic=time.monotonic, epoch=time.time):
        self._mono, self._epoch = monotonic, epoch
        self._lock = threading.RLock()
        self._rows = OrderedDict()
        self._events = deque(maxlen=self.MAX_EVENTS)
        self._sequence = 0

    def _event(self, key, kind, message):
        self._sequence += 1
        self._events.append({"id": self._sequence, "at": self._epoch(), "resource": key,
                             "kind": kind, "message": message})

    def event(self, key, kind, message):
        with self._lock:
            self._event(key, kind, str(message)[:400])

    def observe(self, key, label, *, kind="device", owner="Airglow", online=True,
                stale_after=15.0, expected=True, detail=None, action=None):
        with self._lock:
            row = self._rows.get(key)
            presence = ("control channel detected" if online else "control channel unavailable") if kind == "lhm-fan" else (
                "connected" if online else "disconnected")
            if row is None:
                row = self._rows[key] = {"id": key, "label": str(label)[:200], "kind": kind,
                    "owner": owner, "online": online, "last_success_at": None, "success_mono": None,
                    "pending": False, "error": None, "duration_ms": None, "stale_after": stale_after,
                    "expected": expected, "stale_reported": False, "observed_mono": self._mono()}
                self._event(key, "connection", f"{label}: {presence}")
            else:
                if row["owner"] != owner:
                    self._event(key, "ownership", f"{label}: {row['owner']} → {owner}")
                if row["online"] != online:
                    self._event(key, "connection", f"{label}: {presence}")
                row.update(label=str(label)[:200], kind=kind, owner=owner, online=online,
                           expected=expected, stale_after=stale_after)
            row.update(detail=detail, action=action)
            self._rows.move_to_end(key)
            device_keys = [k for k in self._rows if k not in self.COMPONENTS]
            while len(device_keys) > self.MAX_DEVICES:
                # Prefer forgotten/disconnected identities, retaining the newest evidence on reconnect.
                victim = next((k for k in device_keys if not self._rows[k]["online"]), device_keys[0])
                self._rows.pop(victim)
                device_keys.remove(victim)

    def reconcile(self, kind, present):
        """Keep a short disconnected history, without allowing abandoned identities to grow forever."""
        with self._lock:
            for key, row in list(self._rows.items()):
                if row["kind"] == kind and key not in present and row["online"]:
                    row.update(online=False, pending=False)
                    self._event(key, "connection", f"{row['label']}: disconnected")

    def queued(self, key):
        with self._lock:
            if key in self._rows:
                self._rows[key]["pending"] = True

    def succeeded(self, key, *, duration=None, pending=False, at=None, mono=None):
        with self._lock:
            row = self._rows.get(key)
            if row is None:
                return
            if row["error"] or row["stale_reported"]:
                self._event(key, "recovery", f"{row['label']}: completed successfully again")
            elif row["last_success_at"] is None:
                self._event(key, "success", f"{row['label']}: first completed work")
            row.update(last_success_at=self._epoch() if at is None else at,
                       success_mono=self._mono() if mono is None else mono, error=None, pending=pending,
                       stale_reported=False)
            if duration is not None:
                row["duration_ms"] = round(max(0.0, duration) * 1000, 2)

    def failed(self, key, error):
        with self._lock:
            row = self._rows.get(key)
            if row is None:
                return
            error = str(error)[:300]
            if row["error"] != error:
                self._event(key, "failure", f"{row['label']}: {error}")
            row.update(error=error, pending=False)

    def clear_failure(self, key):
        """A cancelled request no longer needs recovery; this is not a hardware acknowledgement."""
        with self._lock:
            row = self._rows.get(key)
            if row is not None and row["error"]:
                self._event(key, "recovery", f"{row['label']}: no Airglow control requested; previous error cleared")
                row.update(error=None, pending=False, stale_reported=False)

    def snapshot(self, compact=False):
        with self._lock:
            now = self._mono()
            rows = {}
            for key, row in self._rows.items():
                age = None if row["success_mono"] is None else round(max(0, now - row["success_mono"]), 2)
                if not row["online"]:
                    state = "disconnected"
                elif row["error"]:
                    state = "failed"
                elif not row["expected"]:
                    state = "unmanaged"
                elif row["stale_after"] is not None and (age if age is not None else now - row["observed_mono"]) > row["stale_after"]:
                    state = "stale"
                elif row["pending"] or age is None:
                    state = "pending"
                else:
                    state = "healthy"
                if state == "stale" and not row["stale_reported"]:
                    row["stale_reported"] = True
                    self._event(key, "stale", f"{row['label']}: completed work is stale")
                rows[key] = {k: v for k, v in row.items() if k not in ("success_mono", "stale_after", "expected", "stale_reported", "observed_mono")}
                rows[key].update(state=state, age_seconds=age)
            devices = [v for k, v in rows.items() if k not in self.COMPONENTS]
            counts = {state: sum(r["state"] == state for r in rows.values())
                      for state in ("healthy", "pending", "stale", "failed", "disconnected", "unmanaged")}
            state = next((s for s in ("failed", "stale", "disconnected", "pending") if counts[s]), "healthy")
            out = {"state": state, **{k: rows.get(k) for k in self.COMPONENTS}, "counts": counts,
                   "event_count": len(self._events)}
            if not compact:
                out["devices"] = devices
            return out

    def events(self, after=0):
        with self._lock:
            return [dict(e) for e in self._events if e["id"] > after]
