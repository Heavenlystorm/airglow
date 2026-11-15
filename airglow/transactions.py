"""Recoverable groups of application-owned settings files, under the Store lock."""

import hashlib
import json
import logging
from pathlib import Path

from .fileio import atomic_write_text

log = logging.getLogger(__name__)
NAMES = frozenset(("config.json", "presets.json", "state.json", "automations.json", "overview.json"))
JOURNAL = ".transaction.json"
MAX_JOURNAL = 8 * 1024 * 1024


class RecoveryError(ValueError):
    pass


def _digest(text):
    return None if text is None else hashlib.sha256(text.encode("utf-8")).hexdigest()


def _read(path):
    try:
        return path.read_bytes().decode("utf-8")
    except FileNotFoundError:
        return None


def _path(root, name):
    if name not in NAMES or (root / name).resolve().parent != root.resolve():
        raise RecoveryError("Settings recovery contains an unexpected path; the original files were kept.")
    return root / name


def recover(directory):
    """Rollback an unfinished group or finish a committed group before loading any settings."""
    root = Path(directory)
    journal = root / JOURNAL
    if not journal.exists():
        return
    try:
        if journal.stat().st_size > MAX_JOURNAL:
            raise ValueError("recovery record is too large")
        record = json.loads(journal.read_text(encoding="utf-8"))
        if (not isinstance(record, dict) or type(record.get("version")) is not int or record["version"] != 1
                or type(record.get("committed")) is not bool or not isinstance(record.get("files"), dict)
                or not 1 <= len(record["files"]) <= len(NAMES)):
            raise ValueError("invalid recovery record")
        targets = []
        for name, row in record["files"].items():
            path = _path(root, name)
            if not isinstance(row, dict) or set(row) != {"before", "after", "before_sha256", "after_sha256"}:
                raise ValueError("invalid recovery entry")
            for key in ("before", "after"):
                value = row[key]
                if not (isinstance(value, str) or key == "before" and value is None):
                    raise ValueError("invalid recovery contents")
                if row[key + "_sha256"] != _digest(value):
                    raise ValueError("recovery checksum mismatch")
            current = _read(path)
            if current not in (row["before"], row["after"]):
                raise ValueError(f"{name} changed outside the interrupted save")
            targets.append((path, row["after"] if record["committed"] else row["before"], row))
        # Validate every entry before touching any file, including entries already restored on an earlier attempt.
        for path, text, row in targets:
            current = _read(_path(root, path.name))
            if current not in (row["before"], row["after"]):
                raise ValueError(f"{path.name} changed during recovery")
            if current == text:
                continue
            if text is None:
                path.unlink(missing_ok=True)
            else:
                atomic_write_text(path, text)
        journal.unlink()
    except (OSError, ValueError, TypeError, KeyError) as error:
        raise RecoveryError(f"Settings recovery needs attention: {error}. "
                            f"Keep the files in {root} and the recovery record; no new settings were loaded.") from error


def commit(directory, writes, write):
    """Persist the recovery record before replacing files; a durable commit marker selects forward recovery."""
    root = Path(directory)
    recover(root)
    rows = {}
    for name, value in writes:
        path = _path(root, name)
        if name in rows:
            raise ValueError("a settings transaction cannot write the same file twice")
        before, after = _read(path), json.dumps(value, indent=2, ensure_ascii=False)
        rows[name] = {"before": before, "after": after, "before_sha256": _digest(before), "after_sha256": _digest(after)}
    if not rows:
        return
    record = {"version": 1, "committed": False, "files": rows}
    encoded = json.dumps(record, ensure_ascii=False)
    if len(encoded.encode("utf-8")) > MAX_JOURNAL:
        raise ValueError("the settings transaction is too large")
    journal = root / JOURNAL
    atomic_write_text(journal, encoded)
    try:
        for name, value in writes:
            write(name, value)
        record["committed"] = True
        atomic_write_text(journal, json.dumps(record, ensure_ascii=False))
    except Exception:
        recover(root)
        raise
    # A committed save remains successful if only cleanup is locked. Store protects later writes until restart.
    try:
        journal.unlink()
    except OSError:
        log.exception("settings were saved; recovery-record cleanup will retry on restart")
