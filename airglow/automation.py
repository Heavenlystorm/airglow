"""Automations: plain rules of the form "when <condition>, <action>".

Rules are state-based, not event-based: each second every enabled rule is checked, and while its condition
holds it is "active". The highest active rule in the list wins (one winner for lighting, one for fans). When
it stops being true, things go back to what you had set by hand. That makes "revert when the game closes"
automatic and avoids rules that fire once and leave things in a strange state.
"""

import math
import re
import time

WHEN_TYPES = ("app_running", "app_focused", "idle", "time", "locked", "temp_above")
THEN_TYPES = ("preset", "lights_off", "dim", "fan_curve")
_EXE = re.compile(r"[\w .\-()]{1,80}\.exe", re.I)
_HHMM = re.compile(r"([01]\d|2[0-3]):[0-5]\d")
TEMP_HYSTERESIS = 3        # °C: a "temperature above" rule stays on until it has cooled this far below the mark


def whole_number(value, message):
    """An int from a number or numeric text; ValueError (not TypeError) for anything else, so the API answers 400."""
    if isinstance(value, bool):
        raise ValueError(message)
    try:
        return int(value)
    except (TypeError, ValueError, OverflowError):
        raise ValueError(message) from None


def validate_rule(r, preset_ids=None, curve_names=None):
    if not isinstance(r, dict):
        raise ValueError("an automation must be an object")
    rid = str(r.get("id") or "")
    if not re.fullmatch(r"[a-z0-9\-]{1,64}", rid):
        raise ValueError("automation id must be lowercase letters, digits and dashes")
    w, t = r.get("when") or {}, r.get("then") or {}
    if not isinstance(w, dict) or not isinstance(t, dict):
        raise ValueError("an automation needs a when and a then")
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
        m = whole_number(w.get("minutes", 10), "idle time must be 1 to 240 minutes")
        if not 1 <= m <= 240:
            raise ValueError("idle time must be 1 to 240 minutes")
        when["minutes"] = m
    elif wt == "time":
        start, end = str(w.get("start", "")), str(w.get("end", ""))
        if not _HHMM.fullmatch(start) or not _HHMM.fullmatch(end) or start == end:
            raise ValueError("enter a start and end time like 23:00 and 07:00")
        days = w.get("days", list(range(7)))
        if not isinstance(days, list) or not days or not all(isinstance(d, int) and 0 <= d <= 6 for d in days):
            raise ValueError("pick at least one day")
        when.update(start=start, end=end, days=sorted(set(days)))
    elif wt == "temp_above":
        temp = whole_number(w.get("temp", 80), "temperature must be between 30 and 100 °C")
        if not 30 <= temp <= 100:
            raise ValueError("temperature must be between 30 and 100 °C")
        src = w.get("source", "max")
        if src not in ("max", "cpu", "gpu", "liquid"):
            raise ValueError("temperature source must be max, cpu, gpu or liquid")
        when.update(temp=temp, source=src)
    then = {"type": tt}
    if tt == "preset":
        pid = t.get("preset")
        if not isinstance(pid, str) or preset_ids is not None and pid not in preset_ids:
            raise ValueError("pick a preset that exists")
        then["preset"] = pid
    elif tt == "dim":
        b = whole_number(t.get("brightness", 20), "brightness must be between 0 and 100")
        if not 0 <= b <= 100:
            raise ValueError("brightness must be between 0 and 100")
        then["brightness"] = b
    elif tt == "fan_curve":
        c = t.get("curve")
        if not isinstance(c, str) or curve_names is not None and c not in curve_names:
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
        temp = trigger_temperature(w["source"], sensors)
        limit = w["temp"] - TEMP_HYSTERESIS if was_active else w["temp"]
        return temp is not None and temp >= limit
    return False


def trigger_temperature(source, sensors):
    if source == "max":
        known = [sensors[k] for k in ("cpu_temp", "gpu_temp") if sensors.get(k) is not None]
        return max(known, default=None)
    return sensors.get(f"{source}_temp")


def targets(rule, presets):
    action = rule["then"]
    if action["type"] == "fan_curve":
        return ("fans",)
    if action["type"] == "preset":
        preset = presets.get(action["preset"])
        if not preset:
            return ()
        return ("lighting", "fans") if preset.get("fan_points") or preset.get("fan_curve") else ("lighting",)
    return ("lighting",) if action["type"] in ("lights_off", "dim") else ()


def resolve(rules, active_ids, presets):
    """Pick the winning rule for lighting and for fans (first active rule in list order wins each).
    presets: {id: preset}. A profile only claims the fans if it sets a fan curve (a named one or its own)."""
    light = fans = None
    for r in rules:
        if not r["enabled"] or r["id"] not in active_ids:
            continue
        domains = targets(r, presets)
        if light is None and "lighting" in domains:
            light = r
        if fans is None and "fans" in domains:
            fans = r
    return light, fans


def preset_curve(preset):
    return f"profile:{preset['id']}" if preset.get("fan_points") else preset.get("fan_curve")


def rule_outputs(base_scene, base_curve, light_rule, fan_rule, presets, curve_points):
    """Apply rule winners to the hand-set state. Device safety and power policies run afterwards."""
    scene, curve = dict(base_scene), base_curve
    if light_rule:
        action = light_rule["then"]
        if action["type"] == "preset" and action["preset"] in presets:
            preset = presets[action["preset"]]
            scene = {"effect": preset["effect"], "params": preset["params"], "brightness": preset["brightness"], "on": True}
            if preset.get("areas"):
                scene["areas"] = preset["areas"]
        elif action["type"] == "lights_off":
            scene["on"] = False
        elif action["type"] == "dim":
            scene["brightness"] = min(scene["brightness"], action["brightness"])
    if fan_rule:
        action = fan_rule["then"]
        if action["type"] == "fan_curve":
            curve = action["curve"]
        elif action["type"] == "preset" and action["preset"] in presets:
            curve = preset_curve(presets[action["preset"]]) or curve
    return scene, curve if curve_points(curve) else "balanced"


def _scenario(body):
    day, clock = body.get("weekday"), body.get("clock")
    if type(day) is not int or not 0 <= day <= 6 or not isinstance(clock, str) or not _HHMM.fullmatch(clock):
        raise ValueError("Choose a weekday and local time.")
    idle, locked = body.get("idle", 0), body.get("locked", False)
    if type(idle) not in (float, int) or not 0 <= idle <= 10080 or not math.isfinite(idle) or type(locked) is not bool:
        raise ValueError("Enter idle minutes from 0 to 10080 and choose a lock state.")
    running, front = body.get("running", []), body.get("foreground", "")
    if not isinstance(running, list) or len(running) > 64 or not isinstance(front, str):
        raise ValueError("Enter up to 64 program filenames, such as game.exe.")
    names = [*running, *([front] if front else [])]
    if any(not isinstance(name, str) or not _EXE.fullmatch(name) for name in names):
        raise ValueError("Use program filenames such as game.exe.")
    sensors = {}
    for key in ("cpu", "gpu", "liquid"):
        temp = body.get(key)
        if temp is not None and (type(temp) not in (float, int) or not -273.15 <= temp <= 1000 or not math.isfinite(temp)):
            raise ValueError("Use a finite temperature in °C, or leave the reading empty.")
        sensors[key + "_temp"] = temp
    previous = body.get("previous", [])
    if not isinstance(previous, list) or len(previous) > 50 or any(not isinstance(r, str) for r in previous):
        raise ValueError("Previous matches must be rule IDs from an earlier rehearsal.")
    # Only weekday/hour/minute participate in the production matcher. This is a synthetic local week.
    now = time.struct_time((2026, 1, 5 + day, int(clock[:2]), int(clock[3:]), 0, day, 5 + day, -1))
    return {"now": now, "running": {n.lower() for n in names}, "foreground": front.lower(),
            "idle": idle * 60, "locked": locked}, sensors, set(previous)


def matching(rules, signals, sensors, previous):
    return {r["id"] for r in rules if r["enabled"] and is_active(r, signals, sensors, r["id"] in previous)}


def rehearse(body, rules, presets, scene, curve, curve_points):
    """Stateless rule rehearsal. Previous matches belong to the caller's hypothetical sequence."""
    sig, sensors, previous = _scenario(body)
    active = matching(rules, sig, sensors, previous)
    light, fan = resolve(rules, active, presets)
    result_scene, result_curve = rule_outputs(scene, curve, light, fan, presets, curve_points)
    rows = []
    for rank, r in enumerate(rules, 1):
        trigger, rid = r["when"], r["id"]
        row = {"rank": rank, "rule": r, "active": rid in active, "targets": targets(r, presets)}
        if trigger["type"] == "temp_above":
            row["temperature"] = trigger_temperature(trigger["source"], sensors)
            row["threshold"] = trigger["temp"] - (TEMP_HYSTERESIS if rid in previous else 0)
        rows.append(row)
    return {"active": sorted(active), "rows": rows, "winners": {"lighting": light["id"] if light else None, "fans": fan["id"] if fan else None},
            "scene": result_scene, "curve": result_curve, "base": {"scene": scene, "curve": curve}}


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
        self.active = matching(rules, sig, sensors, self.active)
        presets = {p["id"]: p for p in self.store.get_presets()}
        self.light_rule, self.fan_rule = resolve(rules, self.active, presets)
