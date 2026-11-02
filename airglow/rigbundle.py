"""Bounded, local rig migration: validate, map, preview, then commit without taking over cooling."""

import copy
import hashlib
import json
import re
from datetime import datetime, timezone

from . import config, lightsetup
from .automation import validate_rule
from .lighting import effects, geometry

FORMAT = "airglow-rig"
VERSION = 1
MAX_BUNDLE_BYTES = 192 * 1024       # leaves room for explicit mappings inside the API's 256 KiB body limit
MAX_ZONES = 128
MAX_CURVES = 32
SETTINGS = ("design", "temp_unit", "energy_price")
ZONE_FIELDS = {"name", "order", "override", "layout", "component"}
PROFILE_FIELDS = {"id", "name", "effect", "params", "brightness", "areas", "description", "fan_curve", "fan_points"}
FILES = ("config.json", "presets.json", "automations.json")


def _json(value, *, pretty=False):
    try:
        return json.dumps(value, ensure_ascii=False, allow_nan=False, sort_keys=True,
                          indent=2 if pretty else None, separators=None if pretty else (",", ":"))
    except (ValueError, TypeError, RecursionError):
        raise ValueError("the rig bundle must contain finite JSON values") from None


def _fields(value, allowed, what):
    if not isinstance(value, dict):
        raise ValueError(f"{what} must be an object")
    if set(value) - set(allowed):
        raise ValueError(f"{what} contains unsupported fields: {', '.join(sorted(set(value) - set(allowed)))}")


def _list(value, maximum, what):
    if not isinstance(value, list) or len(value) > maximum:
        raise ValueError(f"{what} must be a list of up to {maximum} entries")
    return value


def _readable(store):
    protected = set(FILES) & store._protected_files
    if protected:
        raise ValueError(f"{', '.join(sorted(protected))} could not be read at startup. Restart Airglow before migrating; "
                         "the original files have been kept.")


def inventory(devices):
    """Only lighting capabilities and display labels, never device parameters or channel ownership."""
    out = {}
    for device in devices:
        for zone in device.get("zones", []):
            key = f"{device['id']}/{zone['id']}"
            out[key] = {"key": key, "name": zone["name"], "device": device["name"],
                        "online": bool(device.get("online")), "max_leds": zone["max_leds"],
                        "default_leds": zone["default_leds"], "shape": zone.get("shape", "strip"),
                        "resizable": bool(zone.get("resizable")), "matrix": zone.get("matrix"),
                        "provisional": bool(zone.get("provisional"))}
    return out


def validate(bundle):
    if len(_json(bundle).encode("utf-8")) > MAX_BUNDLE_BYTES:
        raise ValueError("rig bundles may be at most 192 KiB; export fewer profiles or zones")
    _fields(bundle, {"format", "version", "created_at", "settings", "curves", "profiles", "automations", "zones",
                     "warnings"}, "rig bundle")
    if bundle.get("format") != FORMAT or type(bundle.get("version")) is not int or bundle["version"] != VERSION:
        raise ValueError("unsupported rig bundle format or version; this Airglow reads version 1")
    settings = bundle.get("settings", {})
    _fields(settings, SETTINGS, "portable settings")
    settings = config.validate_settings(settings)
    curves = bundle.get("curves", {})
    if not isinstance(curves, dict) or len(curves) > MAX_CURVES:
        raise ValueError("a rig bundle holds up to 32 curves")
    if any(not isinstance(name, str) or not re.fullmatch(r"[a-z0-9][a-z0-9-]{0,63}", name) for name in curves):
        raise ValueError("curve ids must be short lowercase letters, digits and dashes")
    curves = {name: config.validate_curve(points) for name, points in curves.items()}
    presets = _list(bundle.get("profiles", []), 24, "profiles")
    for preset in presets:
        _fields(preset, PROFILE_FIELDS, "portable profile")
    presets = config.validate_presets(presets, curves)
    rules = _list(bundle.get("automations", []), 50, "automations")
    rules = config.validate_rules(rules, lambda rule: validate_rule(rule, {p["id"] for p in presets}, set(curves)))
    zones, seen = [], set()
    for zone in _list(bundle.get("zones", []), MAX_ZONES, "zones"):
        _fields(zone, {"id", "device", "config"}, "portable zone")
        zid = zone.get("id")
        if not isinstance(zid, str) or not re.fullmatch(r"[\x21-\x7e]{1,160}", zid) or zid in seen:
            raise ValueError("source zone ids must be unique short identifiers")
        seen.add(zid)
        device = zone.get("device", "Saved device")
        if not isinstance(device, str) or not device.strip() or len(device) > 160:
            raise ValueError("a source device needs a short display name")
        zone_cfg = zone.get("config")
        _fields(zone_cfg, ZONE_FIELDS, "portable zone settings")
        clean = config.validate_zone(zone_cfg, geometry.MAX_LEDS)
        clean.pop("enabled", None)
        zones.append({"id": zid, "device": device, "config": clean})
    warnings = _list(bundle.get("warnings", []), 100, "warnings")
    if any(not isinstance(w, str) or len(w) > 300 for w in warnings):
        raise ValueError("bundle warnings must be short text")
    return {"format": FORMAT, "version": VERSION, "settings": settings, "curves": curves,
            "profiles": presets, "automations": rules, "zones": zones, "warnings": list(warnings)}


def export_bundle(store, devices):
    with store._lock:
        _readable(store)
        available = inventory(devices)
        presets = [{k: v for k, v in p.items() if k in PROFILE_FIELDS} for p in store.get_presets()]
        curves = store.curves()
        rules, warnings = [], []
        for rule in store.get_rules():
            try:
                rules.append(validate_rule(rule, {p["id"] for p in presets}, set(curves)))
            except ValueError as error:
                warnings.append(f"Skipped automation {rule.get('id', '?')}: {error}")
        zones = [{"id": key, "device": available.get(key, {}).get("device", "Saved device"),
                  "config": {k: v for k, v in zone.items() if k in ZONE_FIELDS}}
                 for key, zone in store.zones().items()]
        bundle = {"format": FORMAT, "version": VERSION,
                  "created_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
                  "settings": {key: store.config[key] for key in SETTINGS}, "curves": curves,
                  "profiles": presets, "automations": rules, "zones": zones, "warnings": warnings}
        validate(bundle)
        if len(_json(bundle, pretty=True).encode("utf-8")) > MAX_BUNDLE_BYTES:
            raise ValueError("this rig exceeds the 192 KiB portable bundle limit")
        return copy.deepcopy(bundle)


def _unique(name, taken):
    candidate, number = name, 2
    while candidate in taken:
        suffix = f"-{number}"
        candidate, number = name[:64 - len(suffix)] + suffix, number + 1
    taken.add(candidate)
    return candidate


def _mapped_zone(source, target, previous):
    if target["provisional"]:
        raise ValueError(f"{target['name']} is still detecting its layout; recheck before mapping it")
    layout = source["config"]["layout"]
    count = geometry.total_leds(layout)
    if not target["resizable"] and (count != target["default_leds"] or layout["shape"] != target["shape"]):
        raise ValueError(f"{target['name']} has a fixed {target['default_leds']}-LED {target['shape']} layout")
    if target["shape"] == "matrix":
        matrix = target.get("matrix") or {}
        if layout["shape"] != "matrix" or any(layout.get(k) != matrix.get(k) for k in ("rows", "cols", "map")):
            raise ValueError(f"{target['name']} needs its own grid mapping")
    elif layout["shape"] == "matrix":
        raise ValueError(f"{target['name']} is not a matrix device")
    # A layout import cannot enable a previously disabled zone or acquire a device by accident.
    return config.validate_zone({**source["config"], "enabled": bool(previous.get("enabled"))}, target["max_leds"])


def _plan(store, devices, raw, mapping):
    _readable(store)
    bundle = validate(raw)
    targets = inventory(devices)
    mapping = {} if mapping is None else mapping
    if not isinstance(mapping, dict) or set(mapping) - {z["id"] for z in bundle["zones"]}:
        raise ValueError("mapping must name only source zones from this bundle")
    mapped = [value for value in mapping.values() if value is not None]
    if any(not isinstance(value, str) for value in mapped) or len(set(mapped)) != len(mapped):
        raise ValueError("each target zone may be mapped only once")
    candidate = copy.deepcopy(store.config)
    candidate.update(bundle["settings"])
    ids = {"curves": {}, "profiles": {}, "automations": {}}
    changes = {"settings": [{"key": key, "before": store.config[key], "after": value}
                            for key, value in bundle["settings"].items() if value != store.config[key]],
               "curves": [], "profiles": [], "automations": []}
    # Reserve even dangling local references: creating their missing identity could otherwise activate an old rule.
    existing_rules = store.get_rules()
    taken = set(candidate["fan_curves"]) | {r["then"]["curve"] for r in existing_rules if r["then"]["type"] == "fan_curve"}
    if store.state.get("fan_curve"):
        taken.add(store.state["fan_curve"])
    for name, points in bundle["curves"].items():
        same = candidate["fan_curves"].get(name) == points
        target = name if same else _unique(name, taken)
        ids["curves"][name] = target
        if not same:
            candidate["fan_curves"][target] = points
        changes["curves"].append({"from": name, "to": target, "created": not same})
    if len(candidate["fan_curves"]) > MAX_CURVES:
        raise ValueError("this import would exceed 32 saved curves")
    presets = store.get_presets()
    taken = {p["id"] for p in presets} | {r["then"]["preset"] for r in existing_rules if r["then"]["type"] == "preset"}
    if store.state.get("active_preset"):
        taken.add(store.state["active_preset"])
    if str(store.state.get("fan_curve", "")).startswith("profile:"):
        taken.add(store.state["fan_curve"][8:])
    for preset in bundle["profiles"]:
        pid = _unique(preset["id"], taken)
        ids["profiles"][preset["id"]] = pid
        imported = {**preset, "id": pid, "fan_profile": None}
        if imported.get("fan_curve"):
            imported["fan_curve"] = ids["curves"][imported["fan_curve"]]
        presets.append(imported)
        changes["profiles"].append({"from": preset["id"], "to": pid, "name": preset["name"]})
    presets = config.validate_presets(presets, candidate["fan_curves"])
    rules, taken = existing_rules, {r["id"] for r in existing_rules}
    for rule in bundle["automations"]:
        rid = _unique(rule["id"], taken)
        ids["automations"][rule["id"]] = rid
        then = dict(rule["then"])
        if then["type"] == "preset":
            then["preset"] = ids["profiles"][then["preset"]]
        elif then["type"] == "fan_curve":
            then["curve"] = ids["curves"][then["curve"]]
        rules.append({**rule, "id": rid, "then": then, "enabled": False})
        changes["automations"].append({"from": rule["id"], "to": rid, "enabled": False})
    # Existing rules can reference an intentionally removed profile; do not alter those during migration.
    rules = config.validate_rules(rules, validate_rule)
    unresolved, skipped, zones = [], [], []
    for source in bundle["zones"]:
        zid, cfg = source["id"], source["config"]
        entry = {"id": zid, "name": cfg["name"], "device": source["device"],
                 "area": lightsetup.zone_spot(cfg), "leds": geometry.total_leds(cfg["layout"]),
                 "target": mapping.get(zid), "suggested": zid if zid in targets else None,
                 "status": "unresolved", "changes": []}
        if zid not in mapping:
            unresolved.append(zid)
        elif mapping[zid] is None:
            skipped.append(zid)
            entry["status"] = "skipped"
        else:
            target = targets.get(mapping[zid])
            previous = store.zones().get(mapping[zid])
            if target is None or previous is None:
                raise ValueError("a mapped target zone is no longer available; preview again")
            clean = _mapped_zone(source, target, previous)
            candidate["zones"][mapping[zid]] = clean
            entry.update(status="mapped", target_name=f"{target['device']} / {previous['name']}",
                         changes=[key for key in sorted(ZONE_FIELDS) if clean.get(key) != previous.get(key)])
        zones.append(entry)
    warnings = list(bundle["warnings"])
    if bundle["automations"]:
        warnings.append("Imported automations are disabled. Review and enable them in Automations when ready.")
    missing = set()
    for preset in bundle["profiles"]:
        missing.update([preset["effect"]] + [a["effect"] for a in preset.get("areas", {}).values()])
    for source in bundle["zones"]:
        if source["config"].get("override"):
            missing.add(source["config"]["override"]["effect"])
    for effect in sorted(missing):
        if effects.is_missing_studio(effect):
            warnings.append(f"Studio effect {effect} is unavailable here; install its scene pack before using it.")
    ready = not unresolved
    fingerprint = {"bundle": bundle, "mapping": mapping, "config": store.config, "presets": store.presets,
                   "rules": store.rules, "targets": targets}
    token = hashlib.sha256(_json(fingerprint).encode("utf-8")).hexdigest() if ready else None
    preview = {"format": FORMAT, "version": VERSION, "ready": ready, "preview_token": token,
               "zones": zones, "targets": list(targets.values()), "unresolved": unresolved, "skipped": skipped,
               "changes": changes, "id_map": ids, "warnings": warnings,
               "limits": {"bundle_bytes": MAX_BUNDLE_BYTES, "request_bytes": 256 * 1024},
               "preserved": "Fan and pump ownership, active curves, safety limits, helper paths and startup settings.",
               "persistence": "Individual files are atomic. Ordinary write failures roll back; power loss can interrupt a multi-file import."}
    return preview, candidate, presets, rules


def preview_bundle(store, devices, bundle, mapping=None):
    with store._lock:
        return _plan(store, devices, bundle, mapping)[0]


def apply_bundle(store, devices, bundle, mapping, preview_token):
    with store._lock:
        preview, candidate, presets, rules = _plan(store, devices, bundle, mapping)
        if not preview["ready"]:
            raise ValueError("finish the explicit zone mapping, then preview this import")
        if not preview_token or preview_token != preview["preview_token"]:
            raise ValueError("the rig or mapping changed; preview this import again before applying it")
        store._commit_files([("config.json", candidate), ("presets.json", presets), ("automations.json", rules)])
        changed_zones = candidate["zones"] != store.config["zones"]
        store.config, store.presets, store.rules = candidate, presets, rules
        store.zones_rev += changed_zones
        return {**preview, "applied": True, "preview_token": None}
