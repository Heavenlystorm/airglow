"""Automations: plain rules of the form "when <condition>, <action>".

Rules are state-based, not event-based: each second every enabled rule is checked, and while its condition
holds it is "active". The highest active rule in the list wins (one winner for lighting, one for fans). When
it stops being true, things go back to what you had set by hand. That makes "revert when the game closes"
automatic and avoids rules that fire once and leave things in a strange state.
"""

import re
import time

WHEN_TYPES = ("app_running", "app_focused", "idle", "time", "locked", "temp_above")
THEN_TYPES = ("preset", "lights_off", "dim", "fan_curve")
_EXE = re.compile(r"[\w .\-()]{1,80}\.exe", re.I)
_HHMM = re.compile(r"([01]\d|2[0-3]):[0-5]\d")
TEMP_HYSTERESIS = 3        # °C: a "temperature above" rule stays on until it has cooled this far below the mark


def validate_rule(r, preset_ids=None, curve_names=None):
    if not isinstance(r, dict):
        raise ValueError("an automation must be an object")
    rid = str(r.get("id") or "")
    if not re.fullmatch(r"[a-z0-9\-]{1,64}", rid):
        raise ValueError("automation id must be lowercase letters, digits and dashes")
    w, t = r.get("when") or {}, r.get("then") or {}
    wt, tt = w.get("type"), t.get("type")
    if wt not in WHEN_TYPES:
        raise ValueError("pick what should start this automation")
    if tt not in THEN_TYPES:
        raise ValueError("pick what this automation should do")
    when = {"type": wt}
    if wt in ("app_running", "app_focused"):
        exe = str(w.get("exe") or "").strip()
        if not _EXE.fullmatch(exe):
            raise ValueError("enter the program's file name, like Cyberpunk2077.exe")
        when["exe"] = exe
    elif wt == "idle":
        m = int(w.get("minutes", 10))
        if not 1 <= m <= 240:
            raise ValueError("idle time must be 1 to 240 minutes")
        when["minutes"] = m
    elif wt == "time":
        start, end = str(w.get("start", "")), str(w.get("end", ""))
        if not _HHMM.fullmatch(start) or not _HHMM.fullmatch(end) or start == end:
            raise ValueError("enter a start and end time like 23:00 and 07:00")
        days = w.get("days") or list(range(7))
        if not isinstance(days, list) or not days or not all(isinstance(d, int) and 0 <= d <= 6 for d in days):
            raise ValueError("pick at least one day")
        when.update(start=start, end=end, days=sorted(set(days)))
    elif wt == "temp_above":
        temp = int(w.get("temp", 80))
        if not 30 <= temp <= 100:
            raise ValueError("temperature must be between 30 and 100 °C")
        src = w.get("source", "max")
        if src not in ("max", "cpu", "gpu"):
            raise ValueError("temperature source must be max, cpu or gpu")
        when.update(temp=temp, source=src)
    then = {"type": tt}
    if tt == "preset":
        pid = t.get("preset")
        if preset_ids is not None and pid not in preset_ids:
            raise ValueError("pick a preset that exists")
        then["preset"] = pid
    elif tt == "dim":
        b = int(t.get("brightness", 20))
        if not 0 <= b <= 100:
            raise ValueError("brightness must be between 0 and 100")
        then["brightness"] = b
    elif tt == "fan_curve":
        c = t.get("curve")
        if curve_names is not None and c not in curve_names:
            raise ValueError("pick a fan curve")
        then["curve"] = c
    return {"id": rid, "enabled": bool(r.get("enabled", True)), "when": when, "then": then}


def _in_window(now, start, end, days):
    mins = now.tm_hour * 60 + now.tm_min
    s = int(start[:2]) * 60 + int(start[3:])
    e = int(end[:2]) * 60 + int(end[3:])
    if s < e:
        return s <= mins < e and now.tm_wday in days
    # crosses midnight: the part after midnight belongs to the day the window started
    if mins >= s:
        return now.tm_wday in days
    return mins < e and (now.tm_wday - 1) % 7 in days


def is_active(rule, sig, sensors, was_active=False):
    w = rule["when"]
    t = w["type"]
    if t == "app_running":
        return w["exe"].lower() in sig["running"]
    if t == "app_focused":
        return (sig.get("foreground") or "") == w["exe"].lower()
    if t == "idle":
        return sig["idle"] >= w["minutes"] * 60
    if t == "locked":
        return bool(sig["locked"])
    if t == "time":
        return _in_window(sig["now"], w["start"], w["end"], w["days"])
    if t == "temp_above":
        temps = {"cpu": sensors.get("cpu_temp"), "gpu": sensors.get("gpu_temp")}
        known = [v for v in temps.values() if v is not None]
        temp = (max(known) if known else None) if w["source"] == "max" else temps.get(w["source"])
        limit = w["temp"] - TEMP_HYSTERESIS if was_active else w["temp"]
        return temp is not None and temp >= limit
    return False


def resolve(rules, active_ids, presets):
    """Pick the winning rule for lighting and for fans (first active rule in list order wins each).
    presets: {id: preset}. A profile only claims the fans if it sets a fan curve (a named one or its own)."""
    light = fans = None
    for r in rules:
        if not r["enabled"] or r["id"] not in active_ids:
            continue
        tt = r["then"]["type"]
        preset = presets.get(r["then"].get("preset")) if tt == "preset" else None
        if light is None and (tt in ("lights_off", "dim") or preset):
            light = r
        if fans is None and (tt == "fan_curve" or (preset and (preset.get("fan_curve") or preset.get("fan_points")))):
            fans = r
    return light, fans


def minutes_until(hour, now=None):
    """Minutes from now until the clock next shows hour:00 (for "pause until morning")."""
    t = time.localtime(now)
    mins = (hour * 60 - (t.tm_hour * 60 + t.tm_min)) % (24 * 60)
    return mins or 24 * 60


class Automations:
    """Evaluates rules once a second; tells the controller what overrides are in effect."""

    def __init__(self, store, signals, sensors):
        self.store, self._signals, self._sensors = store, signals, sensors
        self.active = set()
        self.paused_until = 0.0
        self.light_rule = self.fan_rule = None

    @property
    def paused(self):
        return time.time() < self.paused_until

    def pause(self, minutes):
        if not 0 <= minutes <= 24 * 60:
            raise ValueError("automations can be paused for up to a day")
        self.paused_until = time.time() + minutes * 60 if minutes else 0.0

    def evaluate(self):
        rules = self.store.get_rules()
        if self.paused or not any(r["enabled"] for r in rules):
            self.active, self.light_rule, self.fan_rule = set(), None, None
            return
        sig = self._signals()
        sensors = self._sensors() or {}
        self.active = {r["id"] for r in rules if r["enabled"] and is_active(r, sig, sensors, r["id"] in self.active)}
        presets = {p["id"]: p for p in self.store.get_presets()}
        self.light_rule, self.fan_rule = resolve(rules, self.active, presets)
