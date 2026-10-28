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
import threading
import time
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from . import (__version__, autostart, conflicts, coolers, diagnostics, fancontrol, gallery, lightsetup, paths, profiles,
               system, tools, updates)
from .automation import validate_rule, whole_number
from .config import DEFAULT_CURVES, MIN_FAN_DUTY, SAFETY_TEMP
from .lighting import effects, geometry, studio

log = logging.getLogger(__name__)
CSRF_HEADER = "X-Airglow"
MAX_BODY = 256 * 1024
STREAM_STATE_EVERY = 2.0      # seconds between state events on /api/stream
STREAM_FRAME_EVERY = 1 / 15   # LED frames for the live preview, when the page asks for them
CSP = "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; object-src 'none'; frame-ancestors 'none'"
# The page effects run in (web/runtime/frame.html, in a sandboxed iframe): it may start a worker from its own blob and
# load its own script, nothing else. No connections, so an effect can't reach Airglow's API or the internet.
EFFECT_FRAME_CSP = ("default-src 'none'; script-src 'self' http://127.0.0.1:* http://localhost:* blob:; worker-src blob:; "
                    "connect-src 'none'; img-src 'none'; style-src 'none'; form-action 'none'; frame-ancestors 'self'")
# Windows' registry can map .js to text/plain, which browsers refuse to run as a module.
TYPES = {".html": "text/html", ".js": "text/javascript", ".css": "text/css",
         ".svg": "image/svg+xml", ".ico": "image/x-icon", ".png": "image/png", ".json": "application/json",
         ".woff2": "font/woff2"}


class ApiError(Exception):
    def __init__(self, status, message):
        super().__init__(message)
        self.status = status


class Server(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = False   # on Windows SO_REUSEADDR would let a second process share the port

    def __init__(self, port, ctrl, store, hooks=None, updater=None):
        self.ctrl, self.store, self.hooks, self.updater = ctrl, store, hooks or {}, updater
        self.stopping = threading.Event()        # ends open event streams
        super().__init__(("127.0.0.1", port), Handler)
        bound = self.server_address[1]          # differs from `port` when port=0 (tests)
        self.allowed_hosts = {f"127.0.0.1:{bound}", f"localhost:{bound}"}
        # The effect frame runs with an opaque (sandboxed) origin, so 'self' does not match its own script: name
        # this server's origin instead of the former http://127.0.0.1:* wildcard, which let an effect load code
        # from any other local listener.
        self.frame_csp = EFFECT_FRAME_CSP.replace("http://127.0.0.1:* http://localhost:*",
                                                  f"http://127.0.0.1:{bound} http://localhost:{bound}")
        self.frame_path = (paths.WEB_DIR / "runtime" / "frame.html").resolve()

    def handle_error(self, request, client_address):
        """A client that drops a keep-alive connection is normal; don't print a traceback for it."""
        log.debug("connection error from %s", client_address, exc_info=True)

    def shutdown(self):
        self.stopping.set()
        super().shutdown()

    def server_bind(self):
        if hasattr(socket, "SO_EXCLUSIVEADDRUSE"):
            self.socket.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
        super().server_bind()


class Handler(BaseHTTPRequestHandler):
    server_version = f"Airglow/{__version__}"
    protocol_version = "HTTP/1.1"
    timeout = 20                      # a stalled request or a half-sent body must not hold a thread forever

    def log_message(self, fmt, *args):
        log.debug("%s %s", self.address_string(), fmt % args)

    def _send(self, status, payload, ctype="application/json; charset=utf-8", csp=CSP, cache="no-store", extra=None):
        body = payload if isinstance(payload, bytes) else json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", cache)
        if self.close_connection:
            self.send_header("Connection", "close")
        for key, value in (extra or {}).items():
            self.send_header(key, value)
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Content-Security-Policy", csp)
        self.end_headers()
        self.wfile.write(body)

    def _body(self):
        try:
            n = int(self.headers.get("Content-Length") or 0)
        except ValueError:
            raise ApiError(400, "request body must be JSON") from None
        if n > MAX_BODY or n < 0:                 # -1 would make read() block until the peer hangs up
            raise ApiError(413, "request too large")
        try:
            body = json.loads(self.rfile.read(n) or b"{}")
        except (json.JSONDecodeError, UnicodeDecodeError):
            raise ApiError(400, "request body must be JSON") from None
        if not isinstance(body, dict):
            raise ApiError(400, "request body must be a JSON object")
        return body

    def _dispatch(self, method):
        # Only the routes below consume a POST body. Any answer sent before that (403, 413, a 404 for an unknown
        # POST path) must close the connection: on a keep-alive socket the unread body would otherwise be parsed
        # as the next request, which let a web page smuggle one carrying the X-Airglow header it may not send.
        body_read = method != "POST"
        try:
            if self.headers.get("Host") not in self.server.allowed_hosts:
                raise ApiError(403, "forbidden host")
            parsed = urllib.parse.urlparse(self.path)
            path = parsed.path
            if method == "POST" and self.headers.get(CSRF_HEADER) != "1":
                raise ApiError(403, "missing request header")
            if method == "GET" and path == "/api/stream":
                return self._stream(urllib.parse.parse_qs(parsed.query))
            if method == "GET" and path == "/api/fx/host/stream":
                return self._host_stream(urllib.parse.parse_qs(parsed.query))
            if (method == "POST" and path == "/api/fx/host/frames"
                    and self.headers.get("Content-Type") == "application/octet-stream"):
                frames = self._host_frames()
                body_read = True
                return self._send(200, frames)
            if path.startswith("/api/"):
                body = self._body() if method == "POST" else None
                body_read = True
                return self._send(200, route(self.server, method, path, body, urllib.parse.parse_qs(parsed.query)))
            if method == "GET":
                return self._static(path)
            raise ApiError(404, "not found")
        except ApiError as e:
            self._close_unread(body_read)
            self._send(e.status, {"error": str(e)})
        except ValueError as e:
            self._close_unread(body_read)
            self._send(400, {"error": str(e)})
        except Exception:
            log.exception("API error on %s %s", method, self.path)
            self._close_unread(body_read)
            self._send(500, {"error": "Airglow hit an internal error. Details are in the log folder (Settings, About)."})

    def _close_unread(self, body_read):
        """End the connection when a POST body was never consumed, so it can't be read as the next request."""
        if not body_read:
            self.close_connection = True

    def _stream(self, query):
        """Server-sent events: "state" every couple of seconds (what GET /api/state returns), and "frames" for the
        live preview when the page passes frames=1. Replaces polling; the page reconnects with other options
        when it needs frames or not, and closes the stream while the window is hidden."""
        want_frames = (query.get("frames") or ["0"])[0] == "1"
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream; charset=utf-8")
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Connection", "close")
        self.end_headers()
        self.close_connection = True
        server, ctrl = self.server, self.server.ctrl

        def send(event, data):
            self.wfile.write(f"event: {event}\ndata: {json.dumps(data, separators=(',', ':'))}\n\n".encode())
            self.wfile.flush()

        last_state, last_frame, last_zones = 0.0, None, None
        try:
            self.wfile.write(b"retry: 2000\n\n")
            while not server.stopping.is_set():
                now = time.monotonic()
                if now - last_state >= STREAM_STATE_EVERY:
                    try:
                        payload = route(server, "GET", "/api/state", None)
                    except Exception:              # a transient failure (a device going away) is not a disconnect
                        payload = None
                        log.exception("state event failed")
                    if payload is not None:
                        # Outside the try on purpose: writing to a client that went away raises OSError, which the
                        # handler below turns into the end of this stream. Catching it here logged a traceback and
                        # kept looping on the same dead socket, once per state tick, for every closed page.
                        send("state", payload)
                    last_state = now
                if want_frames and ctrl.engine.frame_id != last_frame:
                    last_frame, zones = ctrl.engine.frame_id, ctrl.engine.frames
                    # the engine makes a new frame every tick even when nothing moves (Solid, Off, a paused
                    # automation): sending only changes lets the page skip repainting the preview, its main cost
                    if zones != last_zones:
                        last_zones = zones
                        send("frames", {"id": last_frame, "fps": round(ctrl.engine.fps, 1), "zones": zones})
                server.stopping.wait(STREAM_FRAME_EVERY if want_frames else 0.25)
        except (OSError, ValueError):
            pass                                  # the page went away
        except Exception:                         # headers are out, so no error response is possible: just end
            log.exception("event stream failed")

    def _host_stream(self, query):
        """What the effect host page runs, and its frame clock (lighting/studio.py)."""
        if not studio.HOST.check_token((query.get("token") or [""])[0]):
            raise ApiError(403, "forbidden")
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream; charset=utf-8")
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Connection", "close")
        self.end_headers()
        self.close_connection = True

        def send(event, data):
            self.wfile.write(f"event: {event}\ndata: {json.dumps(data, separators=(',', ':'))}\n\n".encode())
            self.wfile.flush()

        try:
            self.wfile.write(b"retry: 1000\n\n")
            studio.HOST.stream(send, self.server.stopping)
        except (OSError, ValueError):
            pass                                  # the host page went away
        except Exception:
            log.exception("effect host stream failed")

    def _host_frames(self):
        """The effect host's colours for one frame, packed (web/js/fxhost.js): 30 of these a second, so no JSON."""
        n = int(self.headers.get("Content-Length") or 0)
        if n > MAX_BODY:
            raise ApiError(413, "request too large")
        body = self.rfile.read(n)
        if not studio.HOST.check_token(self.headers.get("X-Airglow-Token")):
            raise ApiError(403, "forbidden")
        return studio.HOST.packed_frames(body)

    def _static(self, path):
        rel = "index.html" if path in ("", "/") else urllib.parse.unquote(path).lstrip("/")
        target = (paths.WEB_DIR / rel).resolve()
        if paths.WEB_DIR.resolve() not in target.parents or not target.is_file():
            raise ApiError(404, "not found")
        ctype = TYPES.get(target.suffix.lower()) or mimetypes.guess_type(target.name)[0] or "application/octet-stream"
        if ctype.startswith("text/") or ctype.endswith("javascript"):
            ctype += "; charset=utf-8"
        csp = self.server.frame_csp if target == self.server.frame_path else CSP
        stat = target.stat()
        etag = f'"{stat.st_mtime_ns:x}-{stat.st_size:x}"'
        if self.headers.get("If-None-Match") == etag:
            self.send_response(304)
            self.send_header("ETag", etag)
            self.send_header("Cache-Control", "no-cache")
            self.end_headers()
            return
        self._send(200, target.read_bytes(), ctype, csp, cache="no-cache", extra={"ETag": etag})

    def do_GET(self):
        self._dispatch("GET")

    def do_POST(self):
        self._dispatch("POST")


# ---------------------------------------------------------------- views

def _settings_view(store, updater=None):
    s = store.settings()
    return {"settings": s, "start_with_windows": autostart.is_enabled(), "fan_profiles": fancontrol.profiles(s),
            "fancontrol_installed": fancontrol.exe_path(s) is not None, "version": __version__,
            "updates_available": bool(updates.UPDATE_REPO) and updater is not None,
            "update": updater.status() if updater else None,
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
                                               "type": dev.type_name, "online": dev.online, "error": dev.error,
                                               "experimental": dev.experimental, "note": dev.note,
                                               "plugin": dev.plugin, "parameters": list(dev.parameters),
                                               "params": dict(dev.params)},
                        "zone": z.info(), "config": cfg, "spot": lightsetup.zone_spot(cfg),
                        "positions": [[round(float(x), 4), round(float(y), 4)] for x, y in pos],
                        "local": [round(float(v), 4) for v in local],
                        "fan_index": fan.tolist()})
    return {"canvas": {"w": geometry.CANVAS_W, "h": geometry.CANVAS_H}, "zones": out}


def _find_zone(ctrl, key):
    dev_id, _, zone_id = key.rpartition("/")
    dev = ctrl.manager.get(dev_id)
    zone = next((z for z in dev.zones if z.id == zone_id), None) if dev else None
    if not zone:
        raise ApiError(404, "That device or zone isn't connected right now.")
    return dev, zone


_CATALOG = []


def _catalog():
    """The product catalog, read once (it ships with the app)."""
    if not _CATALOG:
        _CATALOG.extend(lightsetup.load_catalog())
    return _CATALOG


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


_COOLERS = {"at": -1e9, "found": []}
_COOLERS_LOCK = threading.Lock()


def _coolers():
    """USB-connected AIO coolers, rescanned at most every 30 s (the setup guide asks on every open)."""
    with _COOLERS_LOCK:                       # two requests must not scan the USB bus at the same time
        if time.monotonic() - _COOLERS["at"] > 30:
            found = coolers.scan()
            _COOLERS.update(at=time.monotonic(), found=found)
        return _COOLERS["found"]


def _lit_vendors(store, devices):
    """Lower-case vendors of the devices with a zone switched on."""
    zones = store.zones()
    return {(d.vendor or "").lower() for d in devices
            if any((zones.get(f"{d.id}/{z.id}") or {}).get("enabled") for z in d.zones)}


def _setup_pending(ctrl, store):
    return lightsetup.pending(store, [d.info() for d in ctrl.manager.all().values()])


def route(server, method, path, body, query=None):
    ctrl, store, hooks = server.ctrl, server.store, server.hooks
    if path.startswith("/api/studio/"):
        from . import studioapi
        try:
            return studioapi.route(server, method, path, body, query)
        except studioapi.StudioError as e:
            raise ApiError(e.status, str(e)) from None

    if method == "GET":
        if path == "/api/state":
            devices = list(ctrl.manager.all().values())     # once: merging the three device sources is not free
            devs = [d.info() for d in devices]
            return {**ctrl.snapshot(devs), "temp_unit": store.settings()["temp_unit"], "zones_rev": store.zones_rev,
                    "onboarded": store.state.get("onboarded", False),
                    "online_asked": store.state.get("online_asked", False),
                    "update": server.updater.status() if server.updater else None,
                    "conflicts": conflicts.detect(_lit_vendors(store, devices)),
                    "studio": studio.HOST.status(),
                    "setup_pending": lightsetup.pending_count(store, devs),
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
            return _settings_view(store, server.updater)
        if path == "/api/automations":
            return store.get_rules()
        if path == "/api/apps":
            return system.windowed_exes()
        if path == "/api/tools":
            return tools.status(ctrl)
        if path == "/api/gallery":
            try:
                return gallery.listing()
            except RuntimeError as e:
                raise ApiError(502, str(e)) from None
        if path == "/api/gallery/profile":
            try:
                return gallery.fetch((query or {}).get("id", [""])[0])
            except RuntimeError as e:
                raise ApiError(502, str(e)) from None
        if path == "/api/preview/frames":
            ctrl.engine.mark_preview_read()       # only render the preview while someone is looking at it
            return {"zones": ctrl.engine.preview_frames}
        if path == "/api/setup":
            return {"pending": _setup_pending(ctrl, store), "catalog": _catalog(), "spots": lightsetup.spots(),
                    "coolers": _coolers()}
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
    if path == "/api/fans/channel":
        sid, mode = body.get("id"), body.get("mode")
        cfg = None if mode in (None, "", "off") else {"mode": mode, "duty": body.get("duty", 60)}
        if cfg and sid not in {c["id"] for c in ctrl.fan_channels}:
            raise ApiError(404, "LibreHardwareMonitor doesn't list that fan right now.")
        store.set_fan_channel(sid, cfg)
        ctrl.fan_channels_changed()
        return {"channels": ctrl.fan_channels}
    if path == "/api/presets":
        presets = store.set_presets(body.get("presets"))
        ctrl.presets_changed()
        return presets
    if path == "/api/presets/from-current":
        name = str(body.get("name") or "").strip()
        presets = store.get_presets()
        if len(presets) >= 24:
            raise ApiError(400, "You have 24 profiles already. Remove one first (Profiles, Edit).")
        pid = profiles.new_id(name, {p["id"] for p in presets})
        scene = ctrl.base_scene
        new = {"id": pid, "name": name, "effect": scene["effect"], "params": scene["params"],
               "brightness": scene["brightness"], "areas": _current_areas(ctrl, store), "fan_curve": None,
               "fan_profile": None, "description": body.get("description") or ""}
        if body.get("include_fans", True):
            if ctrl.base_curve in store.curves():
                new["fan_curve"] = ctrl.base_curve
            else:                                   # a profile's own curve, or one being tried: keep its points
                new["fan_points"] = ctrl.curve_points(ctrl.base_curve)
        presets = store.set_presets(presets + [new])
        ctrl.mark_preset(pid)
        return {"presets": presets, "id": pid}
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
    if path == "/api/setup/probe":
        key = str(body.get("zone") or "")
        dev, zone = _find_zone(ctrl, key)
        mode = body.get("mode")
        if mode not in ("blink", "count", "off"):
            raise ApiError(400, "mode must be blink, count or off")
        n = body.get("n", 0)
        if not isinstance(n, int) or isinstance(n, bool) or not 0 <= n <= zone.max_leds:
            raise ApiError(400, f"n must be 0 to {zone.max_leds}")
        ctrl.engine.probe(key, mode, n)
        return {"ok": True, "max_leds": zone.max_leds}
    if path == "/api/setup/apply":
        key = str(body.get("zone") or "")
        dev, zone = _find_zone(ctrl, key)
        if not zone.resizable:
            raise ApiError(400, "This zone's LEDs are built in; only where it sits can change.")
        cfg = store.set_zone(key, lightsetup.plan_header(store, key, zone, body, _catalog()), zone.max_leds)
        ctrl.engine.probe(key, "off")
        ctrl.engine.invalidate(key)
        return {"config": cfg}
    if path == "/api/setup/place":
        dev = ctrl.manager.get(str(body.get("device") or ""))
        if not dev:
            raise ApiError(404, "That device isn't connected right now.")
        if body.get("spot") not in lightsetup.SPOTS:
            raise ApiError(400, "pick where it sits")
        out = {}
        for key, cfg in lightsetup.plan_fixed(store, dev, body["spot"]).items():
            zone = next(z for z in dev.zones if f"{dev.id}/{z.id}" == key)
            out[key] = store.set_zone(key, cfg, zone.max_leds)
            ctrl.engine.invalidate(key)
        return {"configs": out}
    m = re.fullmatch(r"/api/devices/([^/]+)/params", path)
    if m:
        dev = ctrl.manager.get(urllib.parse.unquote(m.group(1)))
        if not dev or not dev.parameters:
            raise ApiError(404, "That device isn't connected, or has no settings.")
        try:
            params = store.set_device_params(dev.id, dev.parameters, body)
        except ValueError as e:
            raise ApiError(400, str(e)) from None
        dev.set_params(params)
        return {"params": params}
    if path == "/api/settings":
        if "start_with_windows" in body:
            autostart.enable() if body.pop("start_with_windows") else autostart.disable()
        if body:
            clean = store.update_settings(body)
            if "design" in clean:
                server.hooks.get("design_changed", lambda d: None)(clean["design"])
            if "online_checks" in body:
                store.save_state(online_asked=True)     # answered, whether from the question or from Settings
            if {"online_checks", "update_channel"} & set(body) and server.updater:
                server.updater.settings_changed()
            if "lhm_url" in body:
                ctrl.lhm.wake()                               # try the new address now
            if {"openrgb_host", "openrgb_port", "openrgb_enabled"} & set(body):
                ctrl.manager.bridge.client.host = store.settings()["openrgb_host"]
                ctrl.manager.bridge.client.port = store.settings()["openrgb_port"]
                ctrl.manager.bridge.client.close()
                ctrl.manager.bridge._last_try = 0.0           # try the new address now, not after the retry wait
            ctrl.apply()
        return _settings_view(store, server.updater)
    if path == "/api/settings/reset":
        store.reset_settings()
        server.hooks.get("design_changed", lambda d: None)(store.settings()["design"])
        bridge = ctrl.manager.bridge
        bridge.client.host = store.settings()["openrgb_host"]
        bridge.client.port = store.settings()["openrgb_port"]
        bridge.client.close()
        bridge._last_try = 0.0
        if server.updater:
            server.updater.settings_changed()
        ctrl.apply()
        return _settings_view(store, server.updater)
    if path == "/api/openrgb/start":
        try:
            tools.launch("openrgb", openrgb_port=store.settings()["openrgb_port"])
        except tools.SetupError as e:
            raise ApiError(409, str(e)) from None
        ctrl.manager.bridge._last_try = 0.0         # connect on the next housekeeping pass, not after the retry wait
        return {"ok": True}
    if path == "/api/automations":
        presets = {p["id"] for p in store.get_presets()}
        rules = store.set_rules(body.get("rules"), lambda r: validate_rule(r, presets, set(store.curves())))
        ctrl.automations.evaluate()
        ctrl.apply()
        return rules
    if path == "/api/automations/pause":
        ctrl.automations.pause(whole_number(body.get("minutes", 0), "minutes must be a whole number"))
        ctrl.automations.evaluate()
        ctrl.apply()
        return {"paused_until": ctrl.automations.paused_until or None}
    if path == "/api/conflicts/close":
        return conflicts.close(str(body.get("name")))
    if path in ("/api/tools/setup", "/api/tools/remove"):
        try:
            if path.endswith("setup"):
                ids = body.get("ids")
                return tools.start_setup(ctrl, store, [str(i) for i in ids] if isinstance(ids, list) else None)
            return tools.start_remove(ctrl)
        except tools.SetupError as e:
            raise ApiError(409, str(e)) from None
    if path == "/api/updates/check":
        if not server.updater:
            raise ApiError(404, "Update checks aren't set up for this build.")
        return server.updater.check_in_background()
    if path == "/api/updates/restart":
        if not server.updater:
            raise ApiError(404, "Update checks aren't set up for this build.")
        try:
            server.updater.restart_to_update()
        except RuntimeError as e:
            raise ApiError(409, str(e)) from None
        return {"ok": True}
    if path == "/api/fx/host/frames":
        if not studio.HOST.check_token(body.get("token")):
            raise ApiError(403, "forbidden")
        return studio.HOST.frames(body)
    if path == "/api/onboarded":
        store.save_state(onboarded=True)
        return {"ok": True}
    if path == "/api/window/show":
        if hooks.get("show_window"):
            hooks["show_window"]()
        return {"ok": True}
    if path == "/api/diagnostics":
        report = diagnostics.write(ctrl, store)
        if body.get("show", True) and hooks.get("reveal"):
            hooks["reveal"](report)
        return {"path": str(report), "name": report.name}
    if path == "/api/open/logs":
        os.startfile(paths.log_dir())  # noqa: S606 - opens our own log folder in Explorer
        return {"ok": True}
    raise ApiError(404, "not found")
