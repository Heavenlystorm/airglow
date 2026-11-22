"""Local user annotations for the cooling recorder. Sensor rows are always left intact."""

import csv
import io
import math
import re

MAX_MARKERS = 64


class NotesConflict(ValueError):
    """Another editor changed the saved text after it was opened."""


def _text(value, limit):
    return isinstance(value, str) and len(value) <= limit


def _time(value):
    return type(value) in (int, float) and -1e14 < value < 1e14 and math.isfinite(value)


def _marker_id(value):
    return isinstance(value, str) and re.fullmatch(r"[a-f0-9]{32}", value) is not None


def validate(session):
    revision = session.get("metadata_revision", 0)
    markers = session.get("markers", [])
    if (not _text(session.get("name", ""), 80) or not _text(session.get("notes", ""), 4000)
            or type(revision) is not int or revision < 0 or not isinstance(markers, list) or len(markers) > MAX_MARKERS):
        raise ValueError("invalid recording notes")
    ids = set()
    for m in markers:
        if (not isinstance(m, dict) or not _marker_id(m.get("id")) or m["id"] in ids
                or not _text(m.get("label"), 120) or not m["label"].strip()
                or not _time(m.get("created_at")) or not _time(m.get("elapsed")) or m["elapsed"] < 0
                or m.get("kind") not in ("live", "placed")
                or "duration_seconds" in session and m["elapsed"] > session["duration_seconds"]):
            raise ValueError("invalid recording event marker")
        ids.add(m["id"])


def duration(session):
    samples = session["samples"]
    return max(session.get("duration_seconds", 0), samples[-1]["elapsed"] if samples else 0,
               max((m["elapsed"] for m in session.get("markers", [])), default=0))


def metadata(session):
    return {"id": session["id"], "name": session.get("name", ""), "notes": session.get("notes", ""),
            "markers": session.get("markers", []), "metadata_revision": session.get("metadata_revision", 0)}


def text_patch(session, name, notes, expected):
    if not session["ended_reason"]:
        raise ValueError("Stop the recording before changing its name or notes")
    if not _text(name, 80) or not _text(notes, 4000):
        raise ValueError("Use up to 80 characters for a name and 4,000 for notes")
    if not isinstance(expected, dict) or not _text(expected.get("name"), 80) or not _text(expected.get("notes"), 4000):
        raise ValueError("The original recording name and notes are required")
    saved = {key: session.get(key, "") for key in ("name", "notes")}
    next_text = {"name": name.strip(), "notes": notes}
    if next_text == saved:
        return {}  # An acknowledged retry is harmless.
    if saved != expected:
        raise NotesConflict("The saved name or notes changed while you were editing.")
    return next_text


def new_marker(marker_id, label, elapsed, kind, created_at):
    if not _marker_id(marker_id):
        raise ValueError("Invalid event marker identity")
    if not _text(label, 120) or not label.strip():
        raise ValueError("Give the event a label of 1 to 120 characters")
    if not _time(elapsed) or elapsed < 0:
        raise ValueError("Choose a time in the recording")
    return {"id": marker_id, "label": label.strip(), "elapsed": round(elapsed, 3), "kind": kind, "created_at": created_at}


def add_patch(session, marker):
    markers = session.get("markers", [])
    existing = next((m for m in markers if m["id"] == marker["id"]), None)
    if existing:
        if (existing["label"] == marker["label"] and existing["kind"] == marker["kind"]
                and (marker["kind"] == "live" or existing["elapsed"] == marker["elapsed"])):
            return {}
        raise NotesConflict("This marker identity is already used for a different event")
    if len(markers) >= MAX_MARKERS:
        raise ValueError(f"A recording holds {MAX_MARKERS} markers. Remove one before adding another.")
    if marker["kind"] == "placed" and (not session["ended_reason"] or marker["elapsed"] > duration(session)):
        raise ValueError("Choose a time in the saved recording")
    return {"markers": sorted([*markers, marker], key=lambda m: (m["elapsed"], m["created_at"], m["id"]))}


def remove_patch(session, marker_id):
    if not _marker_id(marker_id):
        raise ValueError("Invalid event marker identity")
    markers = session.get("markers", [])
    remaining = [m for m in markers if m["id"] != marker_id]
    return {"markers": remaining} if len(remaining) != len(markers) else {}


def csv_safe(value):
    return "'" + value if isinstance(value, str) and value.lstrip().startswith(("=", "+", "-", "@")) else value


def timeline_csv(session):
    output = io.StringIO(newline="")
    writer = csv.writer(output)
    writer.writerow(["row_type", "recording_id", "recording_name", "original_workload", "notes", "started_epoch_s",
                     "marker_id", "marker_kind", "elapsed_s", "annotation_created_epoch_s", "label"])
    writer.writerow(["recording", csv_safe(session["id"]), csv_safe(session.get("name", "")), csv_safe(session["annotation"]),
                     csv_safe(session.get("notes", "")), session["started_at"], "", "", "", "", ""])
    for m in session.get("markers", []):
        writer.writerow(["marker", csv_safe(session["id"]), "", "", "", "", m["id"], m["kind"], m["elapsed"], m["created_at"], csv_safe(m["label"])])
    return output.getvalue()
