"""Local HTTP API + static UI. Bound to 127.0.0.1 only.

Every write needs the `X-Airglow: 1` header, which a web page on another origin can't send without a
CORS preflight we never approve, and the Host header must be this server, which blocks DNS rebinding.
"""

import json
import logging
import mimetypes
import os
import re
import socket
import time
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from . import __version__, autostart, conflicts, fancontrol, paths, system, updates
from .automation import validate_rule
from .config import DEFAULT_CURVES, MIN_FAN_DUTY, SAFETY_TEMP
from .lighting import effects, geometry

log = logging.getLogger(__name__)
CSRF_HEADER = "X-Airglow"
MAX_BODY = 256 * 1024
# Windows' registry can map .js to text/plain, which browsers refuse to run as a module.
TYPES = {".html": "text/html", ".js": "text/javascript", ".css": "text/css",
         ".svg": "image/svg+xml", ".ico": "image/x-icon", ".png": "image/png", ".json": "application/json"}


class ApiError(Exception):
    def __init__(self, status, message):
        super().__init__(message)
        self.status = status


class Server(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = False   # on Windows SO_REUSEADDR would let a second process share the port

    def __init__(self, port, ctrl, store, hooks=None):
        self.ctrl, self.store, self.hooks = ctrl, store, hooks or {}
        super().__init__(("127.0.0.1", port), Handler)
        bound = self.server_address[1]          # differs from `port` when port=0 (tests)
        self.allowed_hosts = {f"127.0.0.1:{bound}", f"localhost:{bound}"}

    def server_bind(self):
        if hasattr(socket, "SO_EXCLUSIVEADDRUSE"):
            self.socket.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
        super().server_bind()


class Handler(BaseHTTPRequestHandler):
    server_version = f"Airglow/{__version__}"
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):
        log.debug("%s %s", self.address_string(), fmt % args)

    def _send(self, status, payload, ctype="application/json; charset=utf-8"):
        body = payload if isinstance(payload, bytes) else json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Content-Security-Policy",
                         "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; object-src 'none'; frame-ancestors 'none'")
        self.end_headers()
        self.wfile.write(body)

    def _body(self):
        n = int(self.headers.get("Content-Length") or 0)
        if n > MAX_BODY:
            raise ApiError(413, "request too large")
        try:
            body = json.loads(self.rfile.read(n) or b"{}")
        except (json.JSONDecodeError, UnicodeDecodeError):
            raise ApiError(400, "request body must be JSON") from None
        if not isinstance(body, dict):
            raise ApiError(400, "request body must be a JSON object")
        return body

    def _dispatch(self, method):
        try:
            if self.headers.get("Host") not in self.server.allowed_hosts:
                raise ApiError(403, "forbidden host")
            parsed = urllib.parse.urlparse(self.path)
            path = parsed.path
            if method == "POST" and self.headers.get(CSRF_HEADER) != "1":
                raise ApiError(403, "missing request header")
            if path.startswith("/api/"):
                body = self._body() if method == "POST" else None
                return self._send(200, route(self.server, method, path, body, urllib.parse.parse_qs(parsed.query)))
            if method == "GET":
                return self._static(path)
            raise ApiError(404, "not found")
        except ApiError as e:
            self._send(e.status, {"error": str(e)})
        except ValueError as e:
            self._send(400, {"error": str(e)})
        except Exception:
            log.exception("API error on %s %s", method, self.path)
            self._send(500, {"error": "Airglow hit an internal error. Details are in the log folder (Settings, About)."})

    def _static(self, path):
        rel = "index.html" if path in ("", "/") else urllib.parse.unquote(path).lstrip("/")
        target = (paths.WEB_DIR / rel).resolve()
        if paths.WEB_DIR.resolve() not in target.parents or not target.is_file():
            raise ApiError(404, "not found")
        ctype = TYPES.get(target.suffix.lower()) or mimetypes.guess_type(target.name)[0] or "application/octet-stream"
        if ctype.startswith("text/") or ctype.endswith("javascript"):
            ctype += "; charset=utf-8"
        self._send(200, target.read_bytes(), ctype)

    def do_GET(self):
        self._dispatch("GET")

    def do_POST(self):
        self._dispatch("POST")


# ---------------------------------------------------------------- views

def _settings_view(store):
    s = store.settings()
    return {"settings": s, "start_with_windows": autostart.is_enabled(), "fan_profiles": fancontrol.profiles(s),
            "fancontrol_installed": fancontrol.exe_path(s) is not None, "version": __version__,
            "updates_available": bool(updates.UPDATE_REPO),
            "data_dir": str(paths.data_dir())}


def _zones_view(ctrl, store):
    cfgs = store.zones()
    out = []
    for dev in ctrl.manager.all().values():
        for z in dev.zones:
            key = f"{dev.id}/{z.id}"
            cfg = cfgs.get(key)
            if not cfg:
                continue
            pos, local, fan = geometry.positions(cfg["layout"])
            out.append({"key": key, "device": {"id": dev.id, "name": dev.name, "vendor": dev.vendor, "kind": dev.kind,
                                               "type": dev.type_name, "online": dev.online, "error": dev.error},
                        "zone": z.info(), "config": cfg,
                        "positions": [[round(float(x), 4), round(float(y), 4)] for x, y in pos],
                        "fan_index": fan.tolist()})
    return {"canvas": {"w": geometry.CANVAS_W, "h": geometry.CANVAS_H}, "zones": out}


def _find_zone(ctrl, key):
    dev_id, _, zone_id = key.rpartition("/")
    dev = ctrl.manager.get(dev_id)
    zone = next((z for z in dev.zones if z.id == zone_id), None) if dev else None
    if not zone:
        raise ApiError(404, "That device or zone isn't connected right now.")
    return dev, zone


def route(server, method, path, body, query=None):
    ctrl, store, hooks = server.ctrl, server.store, server.hooks

    if method == "GET":
        if path == "/api/state":
            return {**ctrl.snapshot(), "temp_unit": store.settings()["temp_unit"], "zones_rev": store.zones_rev,
                    "onboarded": store.state.get("onboarded", False), "conflicts": conflicts.detect(),
                    "readings": ctrl.readings_for(store.tracked_sensors())}
        if path == "/api/sensors":
            return {"sensors": list(ctrl.readings.values()), "lhm_online": bool(ctrl.sensors.get("online"))}
        if path == "/api/sensors/history":
            ids = [i for v in (query or {}).get("ids", []) for i in v.split(",") if i][:40]
            return ctrl.readings_history.get(ids)
        if path == "/api/overview":
            return store.get_overview()
        if path == "/api/history":
            return list(ctrl.history)
        if path == "/api/frames":
            return {"id": ctrl.engine.frame_id, "fps": round(ctrl.engine.fps, 1), "zones": ctrl.engine.frames}
        if path == "/api/presets":
            return store.get_presets()
        if path == "/api/effects":
            return effects.catalog()
        if path == "/api/zones":
            return _zones_view(ctrl, store)
        if path == "/api/curves":
            return {"curves": store.curves(), "defaults": DEFAULT_CURVES, "safety_temp": SAFETY_TEMP, "min_duty": MIN_FAN_DUTY}
        if path == "/api/settings":
            return _settings_view(store)
        if path == "/api/automations":
            return store.get_rules()
        if path == "/api/apps":
            return system.windowed_exes()
        raise ApiError(404, "not found")

    # ---------------------------------------------------------------- POST
    if path == "/api/scene":
        ctrl.set_scene(body)
        return {"scene": ctrl.base_scene}
    if path == "/api/overview":
        return store.set_overview(body)
    if path == "/api/overview/reset":
        return store.reset_overview()
    if path == "/api/fan_curve":
        ctrl.set_fan_curve(body.get("name"))
        return {"ok": True}
    if path == "/api/presets":
        presets = store.set_presets(body.get("presets"))
        ctrl.presets_changed()
        return presets
    if path == "/api/presets/from-current":
        name = str(body.get("name") or "").strip()
        pid = re.sub(r"[^a-z0-9]+", "-", name.lower()).strip("-")[:40] or "preset"
        pid = f"{pid}-{int(time.time()) % 100000:05d}"
        scene = ctrl.base_scene
        new = {"id": pid, "name": name, "effect": scene["effect"], "params": scene["params"],
               "brightness": scene["brightness"], "fan_curve": ctrl.base_curve, "fan_profile": None}
        presets = store.set_presets(store.get_presets() + [new])
        ctrl.mark_preset(pid)
        return {"presets": presets, "id": pid}
    m = re.fullmatch(r"/api/presets/([a-z0-9\-]{1,64})/apply", path)
    if m:
        preset = next((p for p in store.get_presets() if p["id"] == m.group(1)), None)
        if not preset:
            raise ApiError(404, "That preset no longer exists.")
        steps = ctrl.apply_preset(preset)
        return {"ok": all(s["ok"] for s in steps), "steps": steps}
    m = re.fullmatch(r"/api/curves/([a-z]+)(/reset)?", path)
    if m:
        points = store.reset_curve(m.group(1)) if m.group(2) else store.set_curve(m.group(1), body.get("points"))
        return {"points": points}
    m = re.fullmatch(r"/api/zones/([^/]+)(/identify)?", path)
    if m:
        key = urllib.parse.unquote(m.group(1))
        dev, zone = _find_zone(ctrl, key)
        if m.group(2):
            ctrl.engine.identify(key)
            return {"ok": True}
        cfg = store.set_zone(key, body, zone.max_leds)
        ctrl.engine.invalidate(key)
        return {"config": cfg}
    if path == "/api/settings":
        if "start_with_windows" in body:
            autostart.enable() if body.pop("start_with_windows") else autostart.disable()
        if body:
            store.update_settings(body)
            if {"openrgb_host", "openrgb_port", "openrgb_enabled"} & set(body):
                ctrl.manager.bridge.client.host = store.settings()["openrgb_host"]
                ctrl.manager.bridge.client.port = store.settings()["openrgb_port"]
                ctrl.manager.bridge.client.close()
                ctrl.manager.bridge._last_try = 0.0           # try the new address now, not after the retry wait
            ctrl.apply()
        return _settings_view(store)
    if path == "/api/automations":
        presets = {p["id"] for p in store.get_presets()}
        rules = store.set_rules(body.get("rules"), lambda r: validate_rule(r, presets, set(store.curves())))
        ctrl.automations.evaluate()
        ctrl.apply()
        return rules
    if path == "/api/automations/pause":
        ctrl.automations.pause(int(body.get("minutes", 0)))
        ctrl.automations.evaluate()
        ctrl.apply()
        return {"paused_until": ctrl.automations.paused_until or None}
    if path == "/api/conflicts/close":
        return {"failed": conflicts.close(str(body.get("name")))}
    if path == "/api/updates/check":
        try:
            return updates.check()
        except RuntimeError as e:
            raise ApiError(502, str(e)) from None
    if path == "/api/onboarded":
        store.save_state(onboarded=True)
        return {"ok": True}
    if path == "/api/window/show":
        if hooks.get("show_window"):
            hooks["show_window"]()
        return {"ok": True}
    if path == "/api/open/logs":
        os.startfile(paths.log_dir())  # noqa: S606 - opens our own log folder in Explorer
        return {"ok": True}
    raise ApiError(404, "not found")
