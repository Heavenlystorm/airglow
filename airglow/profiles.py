"""Shareable profiles: the file format the community gallery uses, and turning it into a local profile and back.

A shared profile is data only, never code:

    {"airglow_profile": 1, "id": "neon-dusk", "name": "Neon Dusk", "author": "someone", "description": "...",
     "lighting": {"effect": "gradient", "params": {...}, "brightness": 90},
     "areas": {"front": {"effect": "comet", "params": {...}}},          optional, keyed by where lights sit
     "fans": {"curve": [[40, 30], [70, 60], [85, 100]]},                 optional
     "automations": [{"when": {...}, "then": {"type": "preset", "preset": "self"}}]}   optional

"self" in an automation means this profile. Everything goes through the same validation as the app's own
settings, so a file from the gallery can't hold anything the app couldn't.
"""

import json
import re
import time

from . import automation, config
from .lighting import effects

FORMAT = 1
MAX_BYTES = 32_000
MAX_AUTOMATIONS = 10
SELF = "self"
_ID = re.compile(r"[a-z0-9][a-z0-9\-]{0,47}")


def slug(name):
    return re.sub(r"[^a-z0-9]+", "-", str(name).lower()).strip("-")[:40] or "profile"


def new_id(name, taken=()):
    """A local id for a profile: its name, plus a number when that's taken."""
    base = slug(name)
    pid, n = base, 2
    while pid in taken:
        pid, n = f"{base}-{n}", n + 1
    return pid


def _text(d, key, longest, required=False):
    v = d.get(key, "")
    if not isinstance(v, str) or len(v.strip()) > longest or (required and not v.strip()):
        raise ValueError(f"{key} must be text of up to {longest} characters")
    return v.strip()


def validate_shared(data):
    """Clean a shared profile (from the gallery, or about to be shared). Raises ValueError."""
    if not isinstance(data, dict) or data.get("airglow_profile") != FORMAT:
        raise ValueError("this isn't an Airglow profile (or it's from a newer version)")
    if len(json.dumps(data)) > MAX_BYTES:
        raise ValueError("this profile is too big")
    pid = data.get("id")
    if not isinstance(pid, str) or not _ID.fullmatch(pid):
        raise ValueError("a profile id is lowercase letters, digits and dashes")
    out = {"airglow_profile": FORMAT, "id": pid, "name": _text(data, "name", 40, required=True),
           "author": _text(data, "author", 40), "description": _text(data, "description", 200)}
    light = data.get("lighting")
    if not isinstance(light, dict):
        raise ValueError("a profile needs its lighting")
    scene = config.validate_scene({**light, "areas": data.get("areas"), "on": True})
    out["lighting"] = {"effect": scene["effect"], "params": scene["params"], "brightness": scene["brightness"]}
    if scene.get("areas"):
        out["areas"] = scene["areas"]
    fans = data.get("fans")
    if fans:
        if not isinstance(fans, dict):
            raise ValueError("fans must be an object")
        out["fans"] = {"curve": config.validate_curve(fans.get("curve"))}
    rules = data.get("automations") or []
    if not isinstance(rules, list) or len(rules) > MAX_AUTOMATIONS:
        raise ValueError(f"a profile can carry up to {MAX_AUTOMATIONS} automations")
    if rules:
        out["automations"] = [_clean_rule(r) for r in rules]
    return out


def _clean_rule(r):
    """An automation in a shared profile: no id, no enabled flag, and only "self" as a profile to switch to."""
    if not isinstance(r, dict):
        raise ValueError("an automation must be an object")
    clean = automation.validate_rule({"id": "shared", "when": r.get("when"), "then": r.get("then")},
                                     preset_ids={SELF}, curve_names=set(config.DEFAULT_CURVES))
    return {"when": clean["when"], "then": clean["then"]}


def to_shared(preset, rules=(), *, author="", description=None, include_fans=True, include_automations=True,
              curves=None):
    """A local profile as a shareable one. rules: your automations; those that switch to this profile come along
    (as "self") when include_automations is set. curves: your named curves, for a profile that uses one."""
    # the author's name in the id keeps two people's "Night Mode" apart in the gallery
    pid = f"{slug(preset['name'])}-{slug(author)}"[:48].strip("-") if author else slug(preset["name"])
    data = {"airglow_profile": FORMAT, "id": pid, "name": preset["name"], "author": author,
            "description": preset.get("description", "") if description is None else description,
            "lighting": {"effect": preset["effect"], "params": preset["params"], "brightness": preset["brightness"]}}
    if preset.get("areas"):
        data["areas"] = preset["areas"]
    points = preset.get("fan_points") or (curves or {}).get(preset.get("fan_curve"))
    if include_fans and points:
        data["fans"] = {"curve": points}
    if include_automations:
        mine = [r for r in rules if r["then"].get("type") == "preset" and r["then"].get("preset") == preset["id"]]
        if mine:
            data["automations"] = [{"when": r["when"], "then": {"type": "preset", "preset": SELF}}
                                   for r in mine[:MAX_AUTOMATIONS]]
    return validate_shared(data)


def to_preset(shared, local_id, include_fans=True):
    """A shared profile as one of your profiles (validated by the store when saved)."""
    p = {"id": local_id, "name": shared["name"], "effect": shared["lighting"]["effect"],
         "params": shared["lighting"]["params"], "brightness": shared["lighting"]["brightness"],
         "fan_curve": None, "fan_profile": None}
    if shared.get("description"):
        p["description"] = shared["description"]
    if shared.get("areas"):
        p["areas"] = shared["areas"]
    if include_fans and shared.get("fans"):
        p["fan_points"] = shared["fans"]["curve"]
    return p


def rules_to_install(shared, local_id, taken_ids):
    """The profile's automations as your own, switching to the installed profile, with fresh ids."""
    out, taken = [], set(taken_ids)
    stamp = int(time.time()) % 100000
    for i, r in enumerate(shared.get("automations") or []):
        then = dict(r["then"])
        if then.get("type") == "preset":
            then["preset"] = local_id
        rid = f"{local_id}-{stamp}-{i}"[:64]
        while rid in taken:
            rid = f"{rid[:60]}-{len(taken)}"
        taken.add(rid)
        out.append({"id": rid, "enabled": True, "when": r["when"], "then": then})
    return out


def badges(shared):
    """What a profile needs or carries, for the gallery's filters."""
    used = [shared["lighting"]["effect"]] + [a["effect"] for a in (shared.get("areas") or {}).values()]
    needs = {n for e in used for n in effects.EFFECTS[e].needs}
    out = []
    if "audio" in needs:
        out.append("music")
    if "screen" in needs:
        out.append("screen")
    if shared.get("fans"):
        out.append("fans")
    if shared.get("automations"):
        out.append("automations")
    if shared.get("areas"):
        out.append("areas")
    return out


def scene_of(shared):
    """The scene a shared profile plays (for previews and trying it)."""
    return config.validate_scene({**shared["lighting"], "areas": shared.get("areas"), "on": True})
