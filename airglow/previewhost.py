"""Studio screen previews in their own memory-watched WebView2 process, apart from real lighting."""

import base64
import math
import threading
import time
from types import SimpleNamespace

from . import fxformat
from .lighting import studio


def _points(raw):
    if not isinstance(raw, list) or len(raw) > 4096:
        raise ValueError("preview needs at most 4096 LED positions")
    for point in raw:
        if not isinstance(point, list) or not 2 <= len(point) <= 7:
            raise ValueError("invalid preview LED position")
        if any(not isinstance(v, (int, float)) or not math.isfinite(v) for v in point[:2]):
            raise ValueError("preview LED positions must be finite numbers")
        if len(point) == 7:
            pair = point[6]
            if (not isinstance(pair, list) or len(pair) != 2
                    or any(type(n) is not int or not -100000 <= n <= 100000 for n in pair)):
                raise ValueError("invalid painted LED position")
    return raw


def _effect(raw):
    if not isinstance(raw, dict) or raw.get("kind") not in ("code", "layers"):
        raise ValueError("unknown preview effect")
    parts = {"name": "Studio preview"}
    if raw["kind"] == "layers":
        layers = raw.get("layers")
        if not isinstance(layers, dict):
            raise ValueError("invalid preview layers")
        parts.update(layers=layers.get("layers"), master=layers.get("master"), expose=layers.get("expose"))
    else:
        code = raw.get("code") or ""
        if not isinstance(code, str) or len(code.encode("utf-8")) > fxformat.MAX_BYTES:
            raise ValueError("preview code must be text of at most 32 KB")
        parts["code"] = code
    clean = fxformat.validate(fxformat.to_html(parts))
    if clean["kind"] != raw["kind"]:
        raise ValueError("invalid preview layers")
    if clean["kind"] == "layers":
        clean["layers"] = {"v": 2, "layers": clean["layers"], "master": clean["master"], "expose": clean["expose"]}
    return clean


class PreviewHost:
    def __init__(self, url, spawn=None):
        launch = spawn or studio._spawn_host
        self.host = studio.EffectHost(spawn=lambda address: launch(address + "&preview=1"))
        self.host.url = url
        self._stop = threading.Event()
        self._thread = None

    def _watch(self):
        while not self._stop.wait(0.5):
            self.maintain()

    def maintain(self):
        host = self.host
        with host._lock:
            now = time.monotonic()
            for iid, inst in list(host.instances.items()):
                if now - inst.last_used > 10:
                    del host.instances[iid]
                    host._emit({"type": "unload", "id": iid})
            host._watch(now)

    def route(self, action, body):
        host = self.host
        with host._lock:
            if action == "load":
                points, effect = _points(body.get("points")), _effect(body.get("effect"))
                params = body["effect"].get("params", {})
                if not isinstance(params, dict):
                    raise ValueError("preview parameters must be an object")
                if len(host.instances) >= studio.MAX_INSTANCES:
                    raise ValueError("Too many screen previews are running. Close one and try again.")
                host._seq += 1
                iid = f"p{host._seq}"
                eff = SimpleNamespace(**{**effect, "fx_id": f"preview:{iid}"})
                inst = studio.Instance(iid, eff, params, time.monotonic())
                inst.points = points
                inst.details = {}
                host.instances[iid] = inst
                host._ensure_running()
                host._emit(host._load_event(inst))
                inst.loaded = True
                if self._thread is None:
                    self._thread = threading.Thread(target=self._watch, name="preview-watch", daemon=True)
                    self._thread.start()
                return {"id": iid}
            iid = str(body.get("id") or "")
            inst = host.instances.get(iid)
            if action == "stop":
                host.instances.pop(iid, None)
                host.problems.pop(f"preview:{iid}", None)
                host._emit({"type": "unload", "id": iid})
                return {"ok": True}
            if action != "tick":
                raise ValueError("unknown preview request")
            if inst is None:
                return {"problem": host.problems.get(f"preview:{iid}") or {"reason": "stopped", "message": "The preview ended."}}
            updates = body.get("updates") or {}
            if not isinstance(updates, dict):
                raise ValueError("invalid preview update")
            if "points" in updates:
                inst.points = _points(updates["points"])
                host._emit({"type": "points", "id": iid, "points": inst.points})
            if "params" in updates:
                if not isinstance(updates["params"], dict):
                    raise ValueError("preview parameters must be an object")
                inst.params.update(updates["params"])
                host._emit({"type": "params", "id": iid, "params": inst.params})
            if "layers" in updates:
                clean = _effect({"kind": "layers", "layers": updates["layers"]})
                inst.eff.layers = clean["layers"]
                host._emit({"type": "layers", "id": iid, "layers": clean["layers"]})
            now = time.monotonic()
            inst.last_used = host._last_active = now
            try:
                t, dt = float(body.get("time", 0)), float(body.get("dt", 1 / 30))
            except (TypeError, ValueError):
                raise ValueError("preview time must be a number") from None
            if not math.isfinite(t) or not math.isfinite(dt):
                raise ValueError("preview time must be finite")
            host._emit({"type": "tick", "id": iid, "time": t, "dt": max(0, min(dt, 0.25)),
                        "audio": body.get("audio"), "backdrop": bool(body.get("backdrop"))})
            # Wait only for the first frame. Later ticks return the newest completed colours without blocking.
            if inst.colors is None:
                host._wake.wait(0.25)
            problem = host.problems.get(inst.eff.fx_id)
            if problem:
                return {"problem": problem}
            details, inst.details = inst.details, {}
            return {"colors": base64.b64encode(inst.colors).decode() if inst.colors is not None else None,
                    "logs": [], **details}

    def frames(self, body):
        host = self.host
        with host._lock:
            for iid, details in (body.get("details") or {}).items():
                inst = host.instances.get(iid)
                if inst is not None and isinstance(details, dict):
                    inst.details.update({key: details[key] for key in ("ms", "backdrop", "logs") if key in details})
            result = host.frames(body)
            host._wake.notify_all()
            return result

    def packed_frames(self, data):
        with self.host._lock:
            result = self.host.packed_frames(data)
            self.host._wake.notify_all()
            return result

    def check_token(self, token):
        return self.host.check_token(token)

    def stream(self, send, stopping):
        return self.host.stream(send, stopping)

    def close(self):
        self._stop.set()
        with self.host._lock:
            self.host.close()
            self.host._wake.notify_all()
