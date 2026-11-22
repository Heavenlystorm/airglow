"""Opt-in, local cooling recordings. This module has no device or controller write access."""

import copy
from collections import deque
import csv
import io
import json
import math
from pathlib import Path
import threading
import time
import uuid

from . import fans, sessionnotes
from .config import SAFETY_TEMP, validate_curve
from .fileio import atomic_write_text


def _number(value):
    if not isinstance(value, (int, float)) or isinstance(value, bool):
        return None
    try:
        value = float(value)
        return round(value, 3) if math.isfinite(value) else None
    except OverflowError:
        return None


class Recorder:
    MAX_SAMPLES = 1800
    MAX_SESSIONS = 12
    MAX_SENSORS = 6
    INTERVAL = 2.0
    CHECKPOINT_INTERVAL = 60
    MAX_FILE_BYTES = 64 * 1024 * 1024
    NOTE = "Workload and ambient conditions affect comparisons. RPM is not a noise measurement."

    def __init__(self, directory, monotonic=time.monotonic, epoch=time.time):
        self.path = Path(directory) / "cooling-sessions.json"
        self._mono, self._epoch = monotonic, epoch
        self._lock = threading.RLock()
        # API commands serialize independently of the short memory lock used by fan ticks and status reads.
        self._command_lock = threading.Lock()
        self._jobs = deque()
        self._writer = None
        self._pending_writes = 0
        self._writer_idle = threading.Event()
        self._writer_idle.set()
        self._sessions, self._active = [], None
        self._summaries = {}
        self._started = self._last_sample = self._last_checkpoint = None
        self.save_error = self.load_error = None
        self.sample_error = None
        self._load()

    def _load(self):
        if not self.path.exists():
            return
        try:
            if self.path.stat().st_size > self.MAX_FILE_BYTES:
                raise ValueError("recordings file is too large")
            data = json.loads(self.path.read_text(encoding="utf-8"))
            if not isinstance(data, dict) or type(data.get("version")) is not int or data["version"] != 1:
                raise ValueError("unknown recording format")
            sessions = data.get("sessions", [])
            active = data.get("active")
            if (not isinstance(sessions, list) or active is not None and not isinstance(active, dict)
                    or len(sessions) + (active is not None) > self.MAX_SESSIONS):
                raise ValueError("invalid session count")
            identities = set()
            for session in sessions + ([active] if active is not None else []):
                self._validate_session(session, active=session is active)
                if session["id"] in identities:
                    raise ValueError("duplicate recording identity")
                identities.add(session["id"])
            if active is not None:
                samples, markers = len(active["samples"]), len(active.get("markers", []))
                if samples or markers:
                    saved = f"{samples} sample{'s' if samples != 1 else ''} and {markers} event marker{'s' if markers != 1 else ''}"
                    recovered = f"Airglow stopped while recording. Recovered {saved}. Later activity may be missing."
                else:
                    recovered = "Airglow stopped before this recording was saved. Only the start marker survived."
                active.update(ended_at=None, ended_reason="interrupted", duration_seconds=sessionnotes.duration(active), recovery_note=recovered)
                sessions.append(active)
            self._sessions = sessions
        except (OSError, ValueError, TypeError) as e:
            self.load_error = f"Cooling history could not be read: {e}. The original file is protected."

    @classmethod
    def _validate_session(cls, session, *, active):
        """Validate every shape read by status/export/replay before exposing any persisted history."""
        def text(value, maximum, required=False):
            return isinstance(value, str) and (not required or bool(value)) and len(value) <= maximum
        def numeric(value, nullable=True):
            return nullable and value is None or _number(value) is not None
        if (not isinstance(session, dict) or not text(session.get("id"), 64, True)
                or not text(session.get("annotation"), 1000) or not numeric(session.get("started_at"), False)
                or "ended_reason" not in session or "ended_at" not in session
                or not numeric(session["ended_at"])
                or not isinstance(session.get("samples"), list) or len(session["samples"]) > cls.MAX_SAMPLES
                or "recovery_note" in session and not text(session["recovery_note"], 1000)):
            raise ValueError("invalid recorded session metadata")
        if active:
            if session["ended_reason"] is not None or session["ended_at"] is not None:
                raise ValueError("invalid active recording metadata")
        elif (session["ended_reason"] not in ("stopped", "sample_limit", "interrupted")
              or session["ended_at"] is None and session["ended_reason"] != "interrupted"):
            raise ValueError("invalid completed recording metadata")
        fields = session.get("sensors", [])
        if (not isinstance(fields, list) or len(fields) > cls.MAX_SENSORS
                or any(not isinstance(f, dict) or not text(f.get("id"), 512, True)
                       or not text(f.get("name"), 200, True) or not text(f.get("unit"), 24)
                       or not text(f.get("group"), 200) for f in fields)
                or len({f["id"] for f in fields}) != len(fields)):
            raise ValueError("invalid recording sensors")
        if "duration_seconds" in session and (not numeric(session["duration_seconds"], False) or session["duration_seconds"] < 0):
            raise ValueError("invalid recording duration")
        sessionnotes.validate(session)
        previous = -1
        for sample in session["samples"]:
            if (not isinstance(sample, dict) or not numeric(sample.get("t"), False)
                    or not numeric(sample.get("elapsed"), False) or sample["elapsed"] < previous
                    or not isinstance(sample.get("channels"), list) or len(sample["channels"]) > 64
                    or any(not numeric(sample.get(k)) for k in ("cpu", "gpu", "liquid", "temp", "duty", "rpm"))):
                raise ValueError("invalid recorded sample")
            previous = sample["elapsed"]
            values = sample.get("readings", [])
            if not isinstance(values, list) or len(values) != len(fields) or any(not numeric(v) for v in values):
                raise ValueError("invalid recorded sensor values")
            curve = sample.get("curve")
            if (not isinstance(curve, dict) or not text(curve.get("name"), 200, True)
                    or curve.get("source") not in ("cpu", "gpu", "liquid", "max")
                    or not numeric(curve.get("failsafe"), False)):
                raise ValueError("invalid recorded curve")
            validate_curve(curve.get("points"))
            for channel in sample["channels"]:
                if (not isinstance(channel, dict) or not text(channel.get("id"), 200, True)
                        or not text(channel.get("name"), 100) or channel.get("mode") not in (None, "curve", "steady")
                        or any(not numeric(channel.get(k)) for k in ("rpm", "target", "duty"))):
                    raise ValueError("invalid recorded channel")

    def _write(self, data):
        """Worker only: JSON encoding and filesystem operations never hold the recorder memory lock."""
        if self.load_error:
            raise ValueError(self.load_error)
        text = json.dumps(data, indent=2, ensure_ascii=False, allow_nan=False)
        if len(text.encode("utf-8")) > self.MAX_FILE_BYTES:
            raise ValueError("Cooling history is full; export and delete a recording first")
        atomic_write_text(self.path, text)

    @staticmethod
    def _session_snapshot(session):
        # Published sample dictionaries are immutable. Copy the append-only list and mutable session header;
        # copying/encoding the potentially large sample contents can then happen without holding _lock.
        return {**session, "samples": list(session["samples"])} if session is not None else None

    def _snapshot(self, sessions, active):
        return {"version": 1, "sessions": [self._session_snapshot(s) for s in sessions],
                "active": self._session_snapshot(active)}

    def _enqueue(self, kind, argument=None):
        """At most one serialized API job and one automatic finish wait behind the current write."""
        with self._lock:
            if kind == "save":
                pending = next((job for job in self._jobs if job["kind"] == "save"), None)
                if pending is not None:
                    return pending  # it will snapshot the newest memory state when it begins
            if len(self._jobs) >= 2:
                raise RuntimeError("Cooling save queue is full")
            job = {"kind": kind, "argument": argument, "done": threading.Event(), "error": None}
            self._jobs.append(job)
            self._pending_writes += 1
            self._writer_idle.clear()
            if self._writer is None:
                self._writer = threading.Thread(target=self._write_jobs, name="cooling-save", daemon=True)
                self._writer.start()
            return job

    def _write_jobs(self):
        while True:
            with self._lock:
                if not self._jobs:
                    self._writer = None
                    self._writer_idle.set()
                    return
                job = self._jobs.popleft()
            error = None
            try:
                with self._lock:
                    kind, argument = job["kind"], job["argument"]
                    if kind == "start":
                        self._check_start()
                        data = self._snapshot(self._sessions, argument)
                    elif kind == "delete":
                        if self._active and self._active["id"] == argument:
                            raise ValueError("Stop recording before deleting it")
                        sessions = [s for s in self._sessions if s["id"] != argument]
                        if len(sessions) == len(self._sessions):
                            raise ValueError("Recording not found")
                        data = self._snapshot(sessions, self._active)
                    elif kind == "metadata":
                        session_id, patch = argument
                        data = self._snapshot(self._sessions, self._active)
                        self._find(session_id, data["sessions"], data["active"]).update(patch)
                    else:
                        data = self._snapshot(self._sessions, self._active)
                self._write(data)
                with self._lock:
                    if kind == "start":
                        self._active = argument
                        self._started = self._last_checkpoint = self._mono()
                        self._last_sample = None
                    elif kind == "delete":
                        # Sampling may have completed a different session while this write was in flight.
                        self._sessions = [s for s in self._sessions if s["id"] != argument]
                        self._summaries.pop(argument, None)
                    elif kind == "metadata":
                        self._find(session_id, self._sessions, self._active).update(patch)
                        self._summaries.pop(session_id, None)
                    self.save_error = None
            except Exception as e:
                error = e
                with self._lock:
                    if job["kind"] == "save":
                        self.save_error = f"Recording is kept in memory; save failed: {e}"
            finally:
                with self._lock:
                    job["error"] = error
                    self._pending_writes -= 1
                    job["done"].set()

    @staticmethod
    def _await_job(job, *, raise_error=False):
        job["done"].wait()
        if raise_error and job["error"] is not None:
            raise job["error"]
        return job["error"] is None

    def wait_for_save(self, timeout):
        """Bounded shutdown drain, called only after hardware failsafe/release has completed."""
        return self._writer_idle.wait(timeout)

    @staticmethod
    def summary(session):
        samples = session["samples"]
        out = {k: session.get(k) for k in ("id", "annotation", "started_at", "ended_at", "ended_reason", "recovery_note")}
        if "sensors" in session:
            out["sensors"] = session["sensors"]
        out.update(sample_count=len(samples), duration_seconds=sessionnotes.duration(session),
                   name=session.get("name", ""), marker_count=len(session.get("markers", [])),
                   metadata_revision=session.get("metadata_revision", 0))
        for key in ("cpu", "gpu", "liquid", "temp", "duty", "rpm"):
            values = [v for s in samples if (v := _number(s.get(key))) is not None]
            out[key + "_mean"] = round(sum(values) / len(values), 2) if values else None
            out[key + "_max"] = max(values) if values else None
            out[key + "_count"] = len(values)
        return out

    def status(self):
        with self._lock:
            return {"active": self.active_summary(),
                    "sessions": [self._summary(s) for s in reversed(self._sessions)],
                    "save_error": self.save_error, "load_error": self.load_error, "sample_error": self.sample_error,
                    "saving": self._pending_writes > 0,
                    "limits": {"samples": self.MAX_SAMPLES, "sessions": self.MAX_SESSIONS,
                               "interval_seconds": self.INTERVAL}, "note": self.NOTE}

    def _summary(self, session):
        key = session["id"]
        cached = self._summaries.get(key)
        revision = (len(session["samples"]), session["ended_reason"])
        if cached is None or cached[0] != revision:
            cached = self._summaries[key] = (revision, self.summary(session))
        return dict(cached[1])

    def active_summary(self):
        with self._lock:
            return self._summary(self._active) if self._active else None

    @classmethod
    def select_sensors(cls, ids, catalog):
        if not isinstance(ids, list) or len(ids) > cls.MAX_SENSORS:
            raise ValueError(f"Choose up to {cls.MAX_SENSORS} additional sensors")
        fields = []
        seen = set()
        for sid in ids:
            if not isinstance(sid, str) or not 0 < len(sid) <= 512 or sid in seen or sid not in catalog:
                raise ValueError("Choose distinct sensors from the current sensor catalog")
            meta = catalog[sid]
            fields.append({"id": sid, "name": (meta.get("name") or sid)[:200],
                           "unit": (meta.get("unit") or "")[:24], "group": (meta.get("group") or "")[:200]})
            seen.add(sid)
        return fields

    def start(self, annotation="", *, sensors=None):
        if not isinstance(annotation, str) or len(annotation) > 1000:
            raise ValueError("Workload annotation must be at most 1,000 characters")
        with self._command_lock:
            with self._lock:
                self._check_start()
            candidate = {"id": uuid.uuid4().hex, "annotation": annotation.strip(), "started_at": self._epoch(),
                         "ended_at": None, "ended_reason": None, "samples": []}
            if sensors:
                candidate["sensors"] = sensors
            # Publish only after the durable marker is written, without making controller/status wait for it.
            self._await_job(self._enqueue("start", candidate), raise_error=True)
            return self.status()

    def _check_start(self):
        if self.load_error:
            raise ValueError(self.load_error)
        if self._active:
            raise ValueError("A cooling recording is already running")
        if self.save_error:
            raise ValueError("Retry saving the previous recording before starting another")
        if len(self._sessions) >= self.MAX_SESSIONS:
            raise ValueError("12 recordings are retained; export and delete one before starting another")

    def sample(self, values):
        with self._lock:
            now = self._mono()
            if self._active is None or (self._last_sample is not None and now - self._last_sample < self.INTERVAL):
                return False
            sample = {key: _number(values.get(key)) for key in ("cpu", "gpu", "liquid", "temp", "duty", "rpm")}
            sample.update(t=self._epoch(), elapsed=round(max(0, now - self._started), 3),
                          curve=copy.deepcopy(values.get("curve")), channels=[])
            if fields := self._active.get("sensors"):
                current = values.get("readings", {})
                sample["readings"] = []
                for field in fields:
                    meta = current.get(field["id"], {})
                    sample["readings"].append(_number(meta.get("value")) if meta.get("unit") == field["unit"] else None)
            for channel in values.get("channels", [])[:64]:
                sample["channels"].append({"id": str(channel.get("id", ""))[:200],
                    "name": str(channel.get("name", ""))[:100], "mode": channel.get("mode"),
                    **{key: _number(channel.get(key)) for key in ("rpm", "target", "duty")}})
            self._active["samples"].append(sample)
            self._last_sample = now
            if len(self._active["samples"]) >= self.MAX_SAMPLES:
                self._finish("sample_limit")
            elif now - self._last_checkpoint >= self.CHECKPOINT_INTERVAL and not self._pending_writes:
                self._last_checkpoint = now
                self._enqueue("save")
            return True

    def stop(self, reason="stopped", *, expected_id=None):
        if reason not in ("stopped", "sample_limit", "interrupted"):
            raise ValueError("Unknown recording stop reason")
        if reason != "stopped":
            # Automatic caps and controller shutdown must never wait for API or filesystem locks.
            session, _ = self._finish(reason)
            return {"saved": False, **self.status(), "session": self.summary(session)}
        with self._command_lock:
            session, job = self._finish(reason, expected_id)
            saved = self._await_job(job)
            return {"saved": saved, **self.status(), "session": self.summary(session)}

    def _finish(self, reason, expected_id=None):
        with self._lock:
            if self._active is None:
                raise ValueError("No cooling recording is running")
            if expected_id is not None and self._active["id"] != expected_id:
                raise ValueError("The active recording changed. Check its name before stopping it.")
            session = self._active
            session.update(ended_at=self._epoch(), ended_reason=reason, duration_seconds=round(max(0, self._mono() - self._started), 3))
            self._sessions.append(session)
            self._active = None
            return session, self._enqueue("save")

    def retry_save(self):
        with self._command_lock:
            saved = self._await_job(self._enqueue("save"))
            return {"saved": saved, **self.status()}

    def get(self, session_id):
        with self._lock:
            session = self._find(session_id, self._sessions, self._active)
            snapshot = self._session_snapshot(session)
        return copy.deepcopy(snapshot)

    def delete(self, session_id):
        with self._command_lock:
            self._await_job(self._enqueue("delete", session_id), raise_error=True)
            return self.status()

    def trace(self, session_id, after=0):
        """Only graph columns and new rows, so a live graph need not copy channel and curve metadata every tick."""
        with self._lock:
            session = self._find(session_id, self._sessions, self._active)
            rows = session["samples"]
            if after < 0 or after > len(rows):
                raise ValueError("Invalid sample offset")
            duration = sessionnotes.duration(session)
            if session is self._active:
                duration = max(0, self._mono() - self._started)
            result = {k: v for k, v in session.items() if k != "samples"}
            result.update(offset=after, sample_count=len(rows), duration_seconds=duration)
            samples = rows[after:]
        result["samples"] = [{k: s[k] for k in ("t", "elapsed", "cpu", "gpu", "liquid", "duty", "rpm", "readings") if k in s}
                             for s in samples]
        return result

    @staticmethod
    def _find(session_id, sessions, active):
        for session in [active, *sessions]:
            if session and session["id"] == session_id:
                return session
        raise ValueError("Recording not found")

    def _edit_metadata(self, session_id, change):
        with self._command_lock:
            with self._lock:
                session = self._find(session_id, self._sessions, self._active)
                if self.load_error:
                    raise ValueError(self.load_error)
                patch = change(session)
                if not patch:
                    return sessionnotes.metadata(session)
                patch["metadata_revision"] = session.get("metadata_revision", 0) + 1
                job = self._enqueue("metadata", (session_id, patch))
            try:
                self._await_job(job, raise_error=True)
            except OSError as error:
                raise ValueError(f"Recording annotation could not be saved: {error}") from error
            with self._lock:
                return sessionnotes.metadata(self._find(session_id, self._sessions, self._active))

    def annotate(self, session_id, name, notes, expected):
        return self._edit_metadata(session_id, lambda s: sessionnotes.text_patch(s, name, notes, expected))

    def mark(self, session_id, marker_id, label, elapsed=None):
        kind = "live" if elapsed is None else "placed"
        # Timestamp the incoming user action before waiting on a previous disk write.
        with self._lock:
            run = self._find(session_id, self._sessions, self._active)
            if kind == "live":
                if run is self._active:
                    elapsed = max(0, self._mono() - self._started)
                elif any(m["id"] == marker_id for m in run.get("markers", [])):
                    elapsed = 0  # A retried live marker retains its original timestamp.
                else:
                    raise ValueError("That recording has stopped. Place an event at a time in its timeline.")
            marker = sessionnotes.new_marker(marker_id, label, elapsed, kind, self._epoch())
        return self._edit_metadata(session_id, lambda s: sessionnotes.add_patch(s, marker))

    def unmark(self, session_id, marker_id):
        return self._edit_metadata(session_id, lambda s: sessionnotes.remove_patch(s, marker_id))

    def compare(self, first, second):
        a, b = self.summary(self.get(first)), self.summary(self.get(second))
        keys = [key for key in a if key.endswith(("_mean", "_max"))]
        return {"first": a, "second": b, "delta": {
            k: round(b[k] - a[k], 2) if a[k] is not None and b[k] is not None else None for k in keys}, "note": self.NOTE}

    def replay(self, session_id, points, source="max"):
        curve = validate_curve(points)
        if source not in ("cpu", "gpu", "liquid", "max"):
            raise ValueError("Choose a valid temperature source")
        session = self.get(session_id)
        samples = []
        for sample in session["samples"]:
            temps = {"cpu_temp": sample.get("cpu"), "gpu_temp": sample.get("gpu"), "liquid_temp": sample.get("liquid")}
            temp = fans.source_temp(temps, source)
            hottest = fans.source_temp(temps, "max")
            # Missing readings stay missing. This is a target comparison, not a simulated thermal response.
            target = None if temp is None else fans.target_duty(curve, temp, 60)
            if hottest is not None and hottest >= SAFETY_TEMP:
                target = 100
            samples.append({"elapsed": sample["elapsed"], "t": sample["t"], "recorded_duty": sample.get("duty"),
                            "draft_target": None if target is None else round(target, 2)})
        return {"applied": False, "points": curve, "source": source, "samples": samples,
                "note": "Draft targets only; no ramp, temperature response, fan ownership or output is changed."}

    def export(self, session_id, format="json"):
        if format == "timeline-csv":
            with self._lock:
                header = {k: v for k, v in self._find(session_id, self._sessions, self._active).items() if k != "samples"}
            return {"filename": f"airglow-cooling-{header['id'][:12]}-timeline.csv", "mime": "text/csv",
                    "content": sessionnotes.timeline_csv(header)}
        session = self.get(session_id)
        filename = f"airglow-cooling-{session['id'][:12]}"
        if format == "json":
            return {"filename": filename + ".json", "mime": "application/json",
                    "content": json.dumps({"format": "airglow-cooling", "version": 1, "note": self.NOTE,
                                           **session}, ensure_ascii=False, indent=2, allow_nan=False)}
        if format != "csv":
            raise ValueError("Export format must be json, csv or timeline-csv")
        output = io.StringIO(newline="")
        writer = csv.writer(output)
        fields = session.get("sensors", [])
        units = {"GB": "GiB", "MB": "MiB"}
        writer.writerow(["time_epoch_s", "elapsed_s", "cpu_c", "gpu_c", "liquid_c", "source_c", "curve_commanded_duty_pct",
                         "measured_rpm", "curve", "temperature_source", "channel_id", "channel_name", "channel_mode",
                         "channel_target_pct", "channel_measured_duty_pct", "channel_rpm", "workload"]
                        + [f"sensor {f['id']} ({units.get(f['unit'], f['unit'])})" for f in fields])
        for sample in session["samples"]:
            curve = sample.get("curve") or {}
            for channel in sample.get("channels") or [{}]:
                row = [sample.get(k) for k in ("t", "elapsed", "cpu", "gpu", "liquid", "temp", "duty", "rpm")]
                row += [curve.get("name"), curve.get("source")]
                row += [channel.get(k) for k in ("id", "name", "mode", "target", "duty", "rpm")]
                writer.writerow([sessionnotes.csv_safe(v) for v in row + [session["annotation"]] + sample.get("readings", [])])
        return {"filename": filename + ".csv", "mime": "text/csv", "content": output.getvalue()}
