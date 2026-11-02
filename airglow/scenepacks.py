"""Portable Studio scenes. Validation and installation never receive a controller.

Packs contain effect sources and named areas, not paths, device identities or ownership.
Preview tokens bind the exact sources and installed library versions to the user's choices.
"""
import hashlib
import json
import math
import re

from . import fxformat, fxgallery
from .lighting import effects

FORMAT = "airglow-scene-pack"
MAX_BYTES = 240 * 1024  # leaves room for choices and the preview token inside a 256 KiB API request
MAX_EFFECTS = 12
AREAS = {"all", "front", "top", "rear", "bottom", "side", "cooler", "ram", "board", "gpu", "desk"}
KEY = re.compile(r"[a-z0-9][a-z0-9-]{0,47}\Z")


class PackError(ValueError):
    def __init__(self, message, status=400):
        super().__init__(message)
        self.status = status


def _keys(value, allowed, label):
    if not isinstance(value, dict) or set(value) - set(allowed):
        raise PackError(f"{label} has unsupported fields.")


def _json(value):
    try:
        return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False)
    except (TypeError, ValueError):
        raise PackError("The scene pack must contain finite JSON values.") from None


def _values(raw, schema):
    if not isinstance(raw, dict) or set(raw) - {p["id"] for p in schema}:
        raise PackError("An area has unknown effect controls.")
    for spec in schema:
        if spec["id"] not in raw:
            continue
        value = raw[spec["id"]]
        if spec["type"] == "range" and (isinstance(value, bool) or not isinstance(value, (int, float))
                or not math.isfinite(value) or not spec["min"] <= value <= spec["max"]):
            raise PackError(f"{spec['label']} is outside its allowed range.")
        if spec["type"] == "toggle" and not isinstance(value, bool):
            raise PackError(f"{spec['label']} must be true or false.")
    try:
        clean = effects.clean_values(schema, raw)
    except ValueError as e:
        raise PackError(str(e)) from None
    return {key: clean[key] for key in raw}  # omitted controls remain the effect's defaults


def validate(pack):
    _json(pack)  # reject non-finite/non-JSON values before sizing the portable file
    if len(json.dumps(pack, ensure_ascii=False, indent=2, allow_nan=False).encode("utf-8")) > MAX_BYTES:
        raise PackError("A scene pack can be at most 240 KiB.", 413)
    _keys(pack, ("format", "version", "name", "effects", "areas"), "The scene pack")
    if pack.get("format") != FORMAT or type(pack.get("version")) is not int or pack["version"] != 1:
        raise PackError("This is not a supported Airglow scene pack (version 1).")
    if not isinstance(pack.get("name"), str) or not 1 <= len(pack["name"].strip()) <= 80:
        raise PackError("Give the scene pack a name of 1 to 80 characters.")
    entries = pack.get("effects")
    if not isinstance(entries, list) or not 1 <= len(entries) <= MAX_EFFECTS:
        raise PackError("A scene pack needs 1 to 12 effects.")
    checked, by_key = [], {}
    for item in entries:
        _keys(item, ("key", "source", "origin", "sha256"), "An effect")
        key = item.get("key")
        if not isinstance(key, str) or not KEY.fullmatch(key) or key in by_key:
            raise PackError("Each effect needs a unique portable key.")
        if item.get("origin") not in ("mine", "community") or not isinstance(item.get("source"), str):
            raise PackError("Each effect needs its source and origin.")
        try:
            eff = fxformat.validate(item["source"])
        except fxformat.FormatError as e:
            raise PackError(str(e)) from None
        if item.get("sha256") != eff["sha256"]:
            raise PackError(f"The checksum for {eff['name']} does not match its source.")
        if item["origin"] == "community" and (not eff["author"] or eff["author"] == "me" or not eff["license"]):
            raise PackError("Community effects need their original author and open-source licence.")
        checked.append({**eff, "key": key, "origin": item["origin"]})
        by_key[key] = checked[-1]
    areas = pack.get("areas", [])
    if not isinstance(areas, list) or len(areas) > len(AREAS):
        raise PackError("The scene pack has too many area assignments.")
    assigned, clean_areas = set(), []
    for area in areas:
        _keys(area, ("area", "effect", "values"), "An area assignment")
        name, key = area.get("area"), area.get("effect")
        if not isinstance(name, str) or name not in AREAS or name in assigned:
            raise PackError("Each named area may be assigned only once.")
        if not isinstance(key, str) or key not in by_key:
            raise PackError("An area refers to an effect that is not in this pack.")
        assigned.add(name)
        clean_areas.append({"area": name, "effect": key, "values": _values(area.get("values", {}), by_key[key]["schema"])})
    return checked, clean_areas


def _provenance(checked):
    # Even a community file relabelled 'mine' must not bypass a known revocation.
    needs_gallery = any(e["origin"] == "community" or e["author"] not in ("", "me")
                        or "." in (e.get("remix_of") or "") for e in checked)
    if not needs_gallery:
        return
    gallery = fxgallery.listing()  # fail closed if provenance cannot be checked
    for eff in checked:
        for revoked in gallery["revoked"]:
            if revoked["id"] == eff["id"] and (not revoked["sha256"] or eff["sha256"] in revoked["sha256"]):
                raise PackError(f"{eff['name']} was revoked: {revoked['reason']}", 409)
            if revoked["id"] == eff.get("remix_of") and not revoked["sha256"]:
                raise PackError(f"The original of {eff['name']} was revoked: {revoked['reason']}", 409)
        verified = any(e["id"] == eff["id"] and e["sha256"] == eff["sha256"] for e in gallery["effects"])
        if eff["origin"] == "community" and not verified:
            raise PackError(f"{eff['name']} does not match a current community gallery version.", 409)
        if verified:
            eff["origin"] = "community"  # a relabelled exact copy retains durable revocation tracking


def preview(lib, pack):
    checked, areas = validate(pack)
    _provenance(checked)
    with lib.transaction():
        return _preview(lib, pack, checked, areas)


def _preview(lib, pack, checked, areas):
    installed = lib.all()
    contents = []
    for eff in checked:
        matches = [old for old in installed if old["id"] == eff["id"]]
        old = next((old for old in matches if old["sha256"] == eff["sha256"]), matches[0] if matches else None)
        conflict = "none" if old is None else "same" if old["sha256"] == eff["sha256"] else "different"
        contents.append({"key": eff["key"], "name": eff["name"], "author": eff["author"], "license": eff["license"],
                         "remix_of": eff["remix_of"], "kind": eff["kind"], "origin": eff["origin"], "bytes": eff["bytes"],
                         "conflict": conflict, "existing_fx_id": old["fx_id"] if old else None,
                         "choices": ["keep", "copy"] if old else ["install"], "flags": fxformat.scan(eff["source"])})
    fingerprint = {"pack": pack, "library": sorted((e["fx_id"], e["sha256"]) for e in installed)}
    token = hashlib.sha256(_json(fingerprint).encode("utf-8")).hexdigest()
    return {"name": pack["name"], "effects": contents, "areas": areas, "token": token,
            "note": "Installation adds effects to Studio. Named area assignments are included for setup; your lights are unchanged."}


def install(lib, body):
    if body.get("confirm") is not True:
        raise PackError("Preview the scene pack and confirm installation first.", 409)
    pack = body.get("pack")
    checked, areas = validate(pack)
    _provenance(checked)  # network validation stays outside the library lock
    with lib.transaction():
        return _install(lib, body, checked, areas)


def _install(lib, body, checked, areas):
    summary = _preview(lib, body["pack"], checked, areas)
    if body.get("token") != summary["token"]:
        raise PackError("The pack or effect library changed. Preview it again before importing.", 409)
    choices = body.get("choices", {})
    if not isinstance(choices, dict) or set(choices) - {e["key"] for e in summary["effects"]}:
        raise PackError("The import choices do not match this scene pack.")
    batch, installing, mapping = [], [], {}
    for eff, item in zip(checked, summary["effects"]):
        choice = choices.get(eff["key"], "install" if item["conflict"] == "none" else "keep" if item["conflict"] == "same" else None)
        if choice not in item["choices"]:
            raise PackError(f"Choose Keep existing or Import a copy for {eff['name']}.", 409)
        if choice == "keep":
            mapping[eff["key"]] = item["existing_fx_id"]
            # A kept different version must still accept the pack's values.
            kept = lib.get(item["existing_fx_id"])
            for area in areas:
                if area["effect"] == eff["key"]:
                    area["values"] = _values(area["values"], kept["schema"])
        else:
            batch.append({"source": eff["source"], "origin": eff["origin"], "copy": choice == "copy", "sha256": eff["sha256"]})
            installing.append(eff["key"])
    added = lib.install_pack(batch)
    mapping.update((key, eff["fx_id"]) for key, eff in zip(installing, added))
    return {"name": summary["name"], "installed": [{"fx_id": e["fx_id"], "name": e["name"]} for e in added],
            "effects": [{"key": key, "fx_id": value} for key, value in mapping.items()],
            "areas": [{"area": a["area"], "fx_id": mapping[a["effect"]], "values": a["values"]} for a in areas],
            "applied": False}


def export(lib, body):
    ids = body.get("fx_ids")
    if not isinstance(ids, list) or not 1 <= len(ids) <= MAX_EFFECTS or any(not isinstance(x, str) for x in ids) or len(set(ids)) != len(ids):
        raise PackError("Select 1 to 12 saved effects to export.")
    entries, keys = [], {}
    for i, fx_id in enumerate(ids):
        eff = lib.get(fx_id)
        if not eff or eff["origin"] not in ("mine", "community"):
            raise PackError("Save each effect before exporting a scene pack.")
        key = f"effect-{i + 1}"
        keys[fx_id] = key
        entries.append({"key": key, "source": eff["source"], "origin": eff["origin"], "sha256": eff["sha256"]})
    areas = body.get("areas", [])
    if not isinstance(areas, list):
        raise PackError("Area assignments must be a list.")
    assignments = []
    for area in areas:
        _keys(area, ("area", "fx_id", "values"), "An area assignment")
        if not isinstance(area.get("fx_id"), str) or area["fx_id"] not in keys:
            raise PackError("Each area must use an effect included in this pack.")
        assignments.append({"area": area.get("area"), "effect": keys[area["fx_id"]], "values": area.get("values", {})})
    result = {"format": FORMAT, "version": 1, "name": body.get("name", "My scene pack"), "effects": entries, "areas": assignments}
    checked, _ = validate(result)
    _provenance(checked)
    return {"pack": result}


def route(lib, method, path, body):
    if method != "POST":
        raise PackError("Scene pack actions use POST.", 405)
    if path == "/api/studio/packs/preview":
        return preview(lib, body.get("pack"))
    if path == "/api/studio/packs/import":
        return install(lib, body)
    if path == "/api/studio/packs/export":
        return export(lib, body)
    raise PackError("Scene pack action not found.", 404)
