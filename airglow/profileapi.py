"""Profiles and current-look saving, with stable identities for automation references."""

import re

from . import gallery, lightsetup, profiles
from .api import ApiError


def _preset(store, pid):
    p = next((p for p in store.get_presets() if p["id"] == pid), None)
    if not p:
        raise ApiError(404, "That profile no longer exists.")
    return p


def _current_areas(ctrl, store):
    """Per-area effects for "save current": the scene's own, then each area's first zone you gave its own effect."""
    areas = dict(ctrl.base_scene.get("areas") or {})
    mine = {}
    for cfg in store.zones().values():
        if cfg.get("enabled") and cfg.get("override"):
            mine.setdefault(lightsetup.zone_spot(cfg), {"effect": cfg["override"]["effect"],
                                                        "params": cfg["override"]["params"]})
    return {**areas, **mine}


def route(server, method, path, body, query=None):
    ctrl, store, hooks = server.ctrl, server.store, server.hooks
    if method == "GET":
        if path == "/api/presets":
            return store.get_presets()
        raise ApiError(404, "not found")
    if method != "POST":
        raise ApiError(404, "not found")
    if path == "/api/presets":
        presets = store.set_presets(body.get("presets"))
        ctrl.presets_changed()
        return presets
    if path == "/api/presets/from-current":
        # The controller lock freezes this scene while the store commits the profile and selection together.
        with ctrl._lock, store._lock:
            name = str(body.get("name") or "").strip()
            scene = ctrl.base_scene
            new = {"name": name, "effect": scene["effect"], "params": scene["params"],
                   "brightness": scene["brightness"], "fan_curve": None, "fan_profile": None,
                   "description": body.get("description") or ""}
            previous = (next((p for p in store.get_presets() if p["id"] == body["id"]), None)
                        if body.get("id") is not None else None)
            if body.get("include_areas", True):
                new["areas"] = _current_areas(ctrl, store)
            if body.get("include_fans", True):
                if previous:
                    new["fan_profile"] = previous.get("fan_profile")
                if ctrl.base_curve in store.curves():
                    new["fan_curve"] = ctrl.base_curve
                else:
                    new["fan_points"] = ctrl.curve_points(ctrl.base_curve)
            fan_rule = ctrl.automations.fan_rule
            automated = previous and fan_rule and fan_rule["then"].get("preset") == previous["id"]
            if automated:
                old_points = ctrl.curve_points(ctrl.preset_curve(previous))
                new_points = new.get("fan_points") or store.curves().get(new.get("fan_curve"))
                if old_points != new_points:
                    label = fan_rule.get("name") or fan_rule["id"]
                    raise ValueError(f"Automation {label} is currently using this profile's fan settings. "
                                     "Pause automations in Automations before changing or removing those settings; "
                                     "a lighting update with the same fan curve can be saved now.")
            saved_curve = store.state["fan_curve"]
            presets, pid = store.save_current_profile(new, body.get("id"))
            retained_curve = store.state["fan_curve"] if store.state["fan_curve"] != saved_curve else None
            if retained_curve:
                if ctrl.base_curve == saved_curve:
                    ctrl.base_curve = retained_curve
                if ctrl.effective_curve == saved_curve:
                    ctrl.effective_curve = retained_curve
                if ctrl.trial and ctrl.trial["before"]["curve"] == saved_curve:
                    ctrl.trial["before"]["curve"] = retained_curve
            if automated:
                # Equivalent points may move from an owned curve to a named one; retire the old lookup now,
                # even when the automation's identity stays unchanged and the next tick need not call apply().
                ctrl.effective_curve = ctrl.preset_curve(next(p for p in presets if p["id"] == pid))
            ctrl.active_preset = pid
        return {"presets": presets, "id": pid, "curves": store.curves(), "retained_curve":
                {"id": retained_curve, "points": store.curves()[retained_curve]} if retained_curve else None}
    if path == "/api/preview":
        if body.get("profile"):
            ctrl.engine.preview(profiles.scene_of(profiles.validate_shared(body["profile"])))
        elif body.get("preset"):
            p = _preset(store, body["preset"])
            ctrl.engine.preview(profiles.scene_of(profiles.to_shared(p, store.get_rules(), curves=store.curves())))
        else:
            ctrl.engine.preview(None)
        return {"ok": True}
    if path == "/api/profiles/try":
        shared = profiles.validate_shared(body.get("profile"))
        covered = set((shared.get("areas") or {}))
        clear = [k for k, c in store.zones().items() if c.get("override") and lightsetup.zone_spot(c) in covered]
        ctrl.try_profile(profiles.scene_of(shared), shared["name"],
                         fan_points=(shared.get("fans") or {}).get("curve") if body.get("include_fans") else None,
                         clear_zones=clear)
        return {"ok": True}
    if path == "/api/profiles/discard":
        ctrl.end_trial()
        return {"ok": True}
    if path == "/api/profiles/keep":
        shared = profiles.validate_shared(body.get("profile"))
        presets = store.get_presets()
        if len(presets) >= 24:
            raise ApiError(400, "You have 24 profiles already. Remove one first (Profiles, Edit).")
        pid = profiles.new_id(shared["name"], {p["id"] for p in presets})
        preset = profiles.to_preset(shared, pid, include_fans=bool(body.get("include_fans")))
        new = []
        if body.get("include_automations") and shared.get("automations"):
            rules = store.get_rules()
            new = profiles.rules_to_install(shared, pid, {r["id"] for r in rules})
        presets = ctrl.keep_profile(preset, new)
        return {"presets": presets, "id": pid}
    if path == "/api/profiles/share":
        p = _preset(store, body.get("id"))
        shared = profiles.to_shared(p, store.get_rules(), author=str(body.get("author") or "").strip(),
                                    description=str(body.get("description") or p.get("description") or "").strip(),
                                    include_fans=bool(body.get("include_fans")),
                                    include_automations=bool(body.get("include_automations")),
                                    curves=store.curves())
        if not shared["author"]:
            raise ApiError(400, "Add your GitHub name, so the gallery can credit you.")
        url, filled = gallery.share_url(shared)
        if body.get("open", True) and hooks.get("open_url") and gallery.is_share_url(url):
            hooks["open_url"](url)
        return {"url": url, "filled": filled, "source": gallery.share_file(shared),
                "path": f"{gallery.FOLDER}/{shared['id']}.json", "profile": shared}
    if path == "/api/profiles/open-share":          # after the page put a long profile on the clipboard
        url = str(body.get("url"))
        if not gallery.is_share_url(url):
            raise ApiError(400, "not a share address")
        if hooks.get("open_url"):
            hooks["open_url"](url)
        return {"ok": True}
    m = re.fullmatch(r"/api/presets/([a-z0-9\-]{1,64})/apply", path)
    if m:
        preset = next((p for p in store.get_presets() if p["id"] == m.group(1)), None)
        if not preset:
            raise ApiError(404, "That preset no longer exists.")
        steps = ctrl.apply_preset(preset)
        return {"ok": all(s["ok"] for s in steps), "steps": steps}
    raise ApiError(404, "not found")
