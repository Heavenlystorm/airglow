"""Local rig bundle endpoints. Preview never mutates stores, ownership or active controller state."""

from . import rigbundle
from .api import ApiError, MAX_BODY


def route(server, method, path, body, query=None):
    ctrl, store = server.ctrl, server.store
    if method == "GET" and path == "/api/rig/export":
        return rigbundle.export_bundle(store, [d.info() for d in ctrl.manager.all().values()])
    if method != "POST" or path not in ("/api/rig/preview", "/api/rig/apply"):
        raise ApiError(404, "not found")
    if not isinstance(body, dict):
        raise ValueError("the import request must be an object")
    if len(rigbundle._json(body).encode("utf-8")) > MAX_BODY:
        raise ApiError(413, "the import and mappings exceed the 256 KiB request limit")
    with ctrl._lock:
        devices = [d.info() for d in ctrl.manager.all().values()]
        if path == "/api/rig/preview":
            return rigbundle.preview_bundle(store, devices, body.get("bundle"), body.get("mapping"))
        result = rigbundle.apply_bundle(store, devices, body.get("bundle"), body.get("mapping"), body.get("preview_token"))
        for zone in result["zones"]:
            if zone["status"] == "mapped" and zone["changes"]:
                ctrl.engine.invalidate(zone["target"])
        if any(change["key"] == "design" for change in result["changes"]["settings"]) and server.hooks.get("design_changed"):
            server.hooks["design_changed"](store.settings()["design"])
        return result
