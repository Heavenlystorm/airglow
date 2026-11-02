"""Read-only control evidence and opt-in cooling workbench endpoints."""


def route(server, method, path, body, query=None):
    from .api import ApiError  # API dispatch imports this module at call time.
    ctrl = server.ctrl
    body, query = body or {}, query or {}
    def param(key, default=""):
        value = query.get(key, [default])
        return value[0] if isinstance(value, list) and value else value
    try:
        if method == "GET":
            if path == "/api/control/status":
                return {**ctrl.control.snapshot(), "events": ctrl.control.events(),
                        "recording": ctrl.cooling.active_summary()}
            if path == "/api/control/events":
                return {"events": ctrl.control.events(after=max(0, int(param("after", "0"))))}
            if path == "/api/cooling/sessions":
                return ctrl.cooling.status()
            if path == "/api/cooling/session":
                return ctrl.cooling.get(param("id"))
            if path == "/api/cooling/export":
                return ctrl.cooling.export(param("id"), param("format", "json"))
        elif method == "POST":
            if path == "/api/control/recheck":
                target = body.get("target", "all")
                if target not in ("all", "sensors", "devices", "lighting"):
                    raise ValueError("Choose sensors, devices, lighting or all")
                if target in ("all", "sensors"):
                    ctrl.lhm.wake()
                if target in ("all", "devices"):
                    ctrl.manager.request_reset()
                if target in ("all", "lighting"):
                    ctrl.engine.invalidate()
                ctrl.control.event(target, "request", f"Recheck requested: {target}; waiting for completed work")
                return {"requested": True, "completed": False, "target": target}
            if path == "/api/cooling/start":
                return ctrl.cooling.start(body.get("annotation", ""))
            if path == "/api/cooling/stop":
                return ctrl.cooling.stop()
            if path == "/api/cooling/retry-save":
                return ctrl.cooling.retry_save()
            if path == "/api/cooling/delete":
                return ctrl.cooling.delete(body.get("id"))
            if path == "/api/cooling/compare":
                return ctrl.cooling.compare(body.get("first"), body.get("second"))
            if path == "/api/cooling/replay":
                return ctrl.cooling.replay(body.get("id"), body.get("points"), body.get("source", "max"))
    except (ValueError, TypeError) as e:
        raise ApiError(400, str(e)) from None
    raise ApiError(404, "Unknown control or cooling endpoint")
