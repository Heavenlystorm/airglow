"""Screen previews using built-in renderers or a separate memory-watched WebView2 process."""

import base64
import json
import math
import threading
import time
from types import SimpleNamespace

import numpy as np

from . import fxformat, lightsetup
from .lighting import effects, geometry, studio
from .lighting.engine import Engine

MAX_POINTS = 4096
LEASE_SECONDS = 10


def _bounded(value, minimum, maximum):
    return type(value) in (int, float) and minimum <= value <= maximum


def _points(raw):
    if not isinstance(raw, list) or len(raw) > MAX_POINTS:
        raise ValueError("preview needs at most 4096 LED positions")
    for point in raw:
        if not isinstance(point, list) or not 2 <= len(point) <= 7:
            raise ValueError("invalid preview LED position")
        if any(not _bounded(v, -100000, 100000) for v in point[:2]):
            raise ValueError("preview LED positions must be finite and within the preview range")
        if len(point) > 2 and point[2] is not None and (not isinstance(point[2], str) or len(point[2]) > 80):
            raise ValueError("invalid preview LED area")
        if len(point) > 3 and point[3] is not None and not _bounded(point[3], 0, 1):
            raise ValueError("invalid local preview LED position")
        if len(point) > 4 and point[4] is not None and (type(point[4]) is not int or not 0 <= point[4] < MAX_POINTS):
            raise ValueError("invalid preview LED fan index")
        if len(point) == 7:
            pair = point[6]
            if (not isinstance(pair, list) or len(pair) != 2
                    or any(type(n) is not int or not -100000 <= n <= 100000 for n in pair)):
                raise ValueError("invalid painted LED position")
    return raw


def _builtin_zone(points):
    canvas = np.asarray([point[:2] for point in points], dtype=float).reshape(-1, 2) / studio.UNIT
    n = len(points)
    local = [point[3] if len(point) > 3 and point[3] is not None else i / max(n - 1, 1)
             for i, point in enumerate(points)]
    fan = [point[4] if len(point) > 4 and point[4] is not None else 0 for point in points]
    return SimpleNamespace(pos=geometry.normalised(canvas), canvas=canvas, n=n,
                           local=np.asarray(local), fan=np.asarray(fan, dtype=int))


def _preview_audio(raw):
    if raw is None:
        return None
    if not isinstance(raw, dict):
        raise ValueError("preview audio must be an object")
    audio = {key: raw.get(key, 0) for key in ("bass", "level")}
    if any(not _bounded(value, 0, 1) for value in audio.values()):
        raise ValueError("preview audio levels must be between 0 and 1")
    audio["beat"] = raw.get("beat", False)
    if type(audio["beat"]) is not bool:
        raise ValueError("preview audio beat must be true or false")
    for key, values in (("bands", raw.get("bands", [0] * 64)), ("fine", raw.get("fine"))):
        if key == "fine" and values is None:
            continue
        if (not isinstance(values, list) or not 1 <= len(values) <= 256
                or any(not _bounded(value, 0, 1) for value in values)):
            raise ValueError("preview audio bands must contain 1 to 256 numbers between 0 and 1")
        audio[key] = values
    return audio


def _sample_audio(t):
    phase = t % .5
    bass = math.exp(-9 * phase)
    sweep = (t * .35) % 1
    bands = [min(1, .85 * bass * max(0, 1 - (i / 63) * 2.2) + .9 * math.exp(-((i / 63 - sweep) ** 2) / .004))
             for i in range(64)]
    return {"level": .25 + .5 * bass, "bass": bass, "beat": phase < 1 / 30, "bands": bands}


def _sample_screen(pos):
    return np.column_stack((pos[:, 0], pos[:, 1], 1 - pos[:, 0]))


def _render_builtin(effect, zone, params, brightness, t, audio):
    frame = None
    if "audio" in effect.needs:
        sample = audio if audio is not None else _sample_audio(t)
        frame = SimpleNamespace(**{**sample, "bands": np.asarray(sample["bands"])})
    ctx = SimpleNamespace(cpu_temp=60, gpu_temp=60, screen=SimpleNamespace(sample=_sample_screen), audio=frame)
    rgb = effect.render(zone, t, params, ctx)
    if np.asarray(rgb).shape != (len(zone.pos), 3) or not np.isfinite(rgb).all():
        raise ValueError("the preview effect did not produce a valid frame")
    return Engine._to_bytes(rgb, brightness).tobytes()


def _preview_params(renderer, params):
    try:
        return effects.clean_params(renderer.id, params)
    except OverflowError:
        raise ValueError("preview parameters must be finite numbers") from None


def _effect(raw):
    if not isinstance(raw, dict) or raw.get("kind") not in ("builtin", "code", "layers"):
        raise ValueError("unknown preview effect")
    if raw["kind"] == "builtin":
        effect_id = raw.get("id")
        renderer = dict.get(effects.EFFECTS, effect_id) if isinstance(effect_id, str) else None
        if renderer is None:
            raise ValueError("unknown built-in preview effect")
        brightness = raw.get("brightness", 100)
        if not _bounded(brightness, 0, 100):
            raise ValueError("preview brightness must be between 0 and 100")
        return {"kind": "builtin", "renderer": renderer, "brightness": brightness}
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
        self._builtins = {}
        self._scenes = {}
        self._profile = None

    def _watch(self):
        while not self._stop.wait(0.5):
            self.maintain()

    def maintain(self):
        host = self.host
        with host._lock:
            now = time.monotonic()
            self._expire(now)
            host._watch(now)

    def _expire(self, now):
        if self._profile is not None and self._profile.deadline <= now:
            self._stop_profile()
        for iid, scene in list(self._scenes.items()):
            if now - scene.last_used > LEASE_SECONDS:
                self._unload(iid)
        for inst in list(self._builtins.values()) + list(self.host.instances.values()):
            if now - inst.last_used > LEASE_SECONDS:
                self._unload(inst.id)

    def _unload(self, iid):
        scene = self._scenes.pop(iid, None)
        if scene is not None:
            self._unload_groups(scene)
        self._builtins.pop(iid, None)
        self.host.problems.pop(f"preview:{iid}", None)
        if self.host.instances.pop(iid, None) is not None:
            self.host._emit({"type": "unload", "id": iid})

    def _watch_leases(self):
        if self._thread is None:
            self._thread = threading.Thread(target=self._watch, name="preview-watch", daemon=True)
            self._thread.start()

    def _slots(self):
        instances = self.host.instances
        return len(instances) + len(self._builtins) + sum(
            not any(child in instances for child in scene.studio) for scene in self._scenes.values())

    def _load(self, points, effect, params, now):
        host = self.host
        if self._slots() >= studio.MAX_INSTANCES:
            raise ValueError("Too many screen previews are running. Close one and try again.")
        host._seq += 1
        iid = f"p{host._seq}"
        if effect["kind"] == "builtin":
            renderer = effect["renderer"]
            self._builtins[iid] = SimpleNamespace(id=iid, renderer=renderer, zone=_builtin_zone(points),
                                                  brightness=effect["brightness"], last_used=now,
                                                  params=_preview_params(renderer, params))
        else:
            eff = SimpleNamespace(**{**effect, "fx_id": f"preview:{iid}"})
            inst = studio.Instance(iid, eff, params, now)
            inst.points = points
            inst.details = {}
            host.instances[iid] = inst
            try:
                host._ensure_running()
                host._emit(host._load_event(inst))
            except Exception:
                self._unload(iid)
                raise
            inst.loaded = True
        self._watch_leases()
        return iid

    def _tick_host(self, inst, now, t, dt, audio, backdrop=False):
        inst.last_used = self.host._last_active = now
        self.host._emit({"type": "tick", "id": inst.id, "time": t, "dt": dt, "audio": audio, "backdrop": backdrop})

    def _unload_groups(self, view):
        for iid in view.studio:
            self._unload(iid)

    def _stop_profile(self):
        if self._profile is not None:
            self._unload_groups(self._profile)
            self._profile = None

    def _render_groups(self, zones, brightness):
        builtins, studio_groups = {}, {}
        for key, zone, effect_id, params in zones:
            renderer = effects.EFFECTS.get(effect_id)
            if renderer is None:
                renderer, params = effects.EFFECTS["solid"], {}
            params = _preview_params(renderer, params)
            if isinstance(renderer, studio.JsEffect):
                group = studio_groups.setdefault((renderer.id, json.dumps(params, sort_keys=True)),
                                                 (renderer, params, [], {}))
                _, _, points, slices = group
                slices[key] = slice(len(points), len(points) + zone.n)
                points.extend(studio._points(zone))
                if len(points) > MAX_POINTS:
                    raise ValueError("There are too many LEDs for one Studio effect preview.")
            else:
                builtins[key] = (renderer, zone, params)
        return SimpleNamespace(brightness=brightness, builtins=builtins, studio={}, groups=studio_groups)

    def _start_groups(self, view, now):
        try:
            for renderer, params, points, slices in view.groups.values():
                effect = {"kind": renderer.kind, "name": renderer.name, "code": renderer.code,
                          "layers": renderer.layers, "permissions": renderer.permissions}
                view.studio[self._load(points, effect, params, now)] = slices
        except Exception:
            self._unload_groups(view)
            raise
        view.groups.clear()
        self._watch_leases()

    def _group_frames(self, view, now, t, dt, audio):
        out = {key: _render_builtin(renderer, zone, params, view.brightness, t, audio)
               for key, (renderer, zone, params) in view.builtins.items()}
        problem = None
        for iid, slices in view.studio.items():
            failure = self.host.problems.get(f"preview:{iid}")
            if failure:
                problem = failure
                continue
            inst = self.host.instances.get(iid)
            if inst is None:
                problem = {"reason": "stopped", "message": "The preview ended."}
                continue
            self._tick_host(inst, now, t, dt, audio)
            if inst.colors is not None:
                rgb = np.frombuffer(inst.colors, dtype=np.uint8).reshape(-1, 3) / 255
                rgb = Engine._to_bytes(rgb, view.brightness)
                out.update({key: rgb[part].tobytes() for key, part in slices.items()})
        return out, problem

    def _load_scene(self, effect, points, now):
        """Lease a composed scene in preview-only slots in the same point order as the UI."""
        effect_id, raw_zones = effect.get("id"), effect.get("zones")
        if not isinstance(effect_id, str) or effect_id not in effects.EFFECTS:
            raise ValueError("The effect is no longer available for preview.")
        brightness = effect.get("brightness", 100)
        if not _bounded(brightness, 0, 100):
            raise ValueError("preview brightness must be between 0 and 100")
        if not isinstance(raw_zones, list) or not 1 <= len(raw_zones) <= min(256, len(points)):
            raise ValueError("invalid preview LED zones")
        start, zones, keys = 0, [], {}
        for row in raw_zones:
            if not isinstance(row, dict):
                raise ValueError("invalid preview LED zone")
            key, count, renderer, params = row.get("key"), row.get("count"), row.get("id"), row.get("params", {})
            if (not isinstance(key, str) or not 1 <= len(key) <= 256 or key in keys
                    or type(count) is not int or not 1 <= count <= len(points) - start
                    or not isinstance(renderer, str) or not isinstance(params, dict)):
                raise ValueError("invalid preview LED zone")
            segment = points[start:start + count]
            if row.get("layout") is None:
                zone = _builtin_zone(segment)
            else:
                try:
                    layout = geometry.validate_layout(row["layout"], count)
                except (OverflowError, TypeError):
                    raise ValueError("invalid preview LED layout") from None
                if geometry.total_leds(layout) != count:
                    raise ValueError("preview LED layout does not match the zone")
                canvas, local, fan = geometry.positions(layout)
                zone = SimpleNamespace(pos=geometry.normalised(canvas), canvas=canvas, n=count, local=local, fan=fan)
            zone.key = key
            zone.spot = segment[0][2] if len(segment[0]) > 2 else None
            zones.append((key, zone, renderer, params))
            keys[key] = None
            start += count
        if start != len(points):
            raise ValueError("preview LED zones must cover every position")
        view = self._render_groups(zones, brightness)
        if self._slots() + max(1, len(view.groups)) > studio.MAX_INSTANCES:
            raise ValueError("Too many screen previews are running. Close one and try again.")
        self._start_groups(view, now)
        self.host._seq += 1
        iid = f"p{self.host._seq}"
        view.keys, view.last_used = tuple(keys), now
        self._scenes[iid] = view
        return iid

    def set_profile(self, scene, zones=None):
        """Set the gallery's single screen-only profile preview for the configured rig."""
        with self.host._lock:
            now = time.monotonic()
            self._expire(now)
            if scene is None:
                self._stop_profile()
                return
            if self._stop.is_set():
                raise ValueError("The screen preview service has stopped.")
            old = self._profile
            if (old and old.scene == scene and old.zones == zones
                    and all(iid in self.host.instances for iid in old.studio)):
                old.deadline = now + LEASE_SECONDS
                return
            if sum(geometry.total_leds(z["layout"]) for z in zones.values()) > MAX_POINTS * studio.MAX_INSTANCES:
                raise ValueError("There are too many LEDs to preview this profile.")
            entries = []
            for key, cfg in zones.items():
                canvas, local, fan = geometry.positions(cfg["layout"])
                spot = lightsetup.zone_spot(cfg)
                zone = SimpleNamespace(pos=geometry.normalised(canvas), canvas=canvas, local=local, fan=fan,
                                       n=len(canvas), key=key, spot=spot)
                chosen = scene.get("areas", {}).get(spot, scene) if scene.get("on", True) else {"effect": "off"}
                entries.append((key, zone, chosen["effect"], chosen.get("params")))
            view = self._render_groups(entries, scene.get("brightness", 100))
            held = sum(iid in self.host.instances for iid in old.studio) if old else 0
            if len(view.groups) + self._slots() - held > studio.MAX_INSTANCES:
                raise ValueError("Too many screen previews are running. Close one and try again.")
            self._stop_profile()
            self._start_groups(view, now)
            view.scene, view.zones = scene, zones
            view.started, view.deadline, view.last = now, now + LEASE_SECONDS, None
            self._profile = view

    def profile_frames(self):
        with self.host._lock:
            now = time.monotonic()
            self._expire(now)
            profile = self._profile
            if profile is None:
                return {"zones": {}}
            t = now - profile.started
            dt = 1 / 30 if profile.last is None else min(.25, max(0, now - profile.last))
            profile.last = now
            out, problem = self._group_frames(profile, now, t, dt, _sample_audio(t))
            return {"zones": {key: rgb.hex() for key, rgb in out.items()}, **({"problem": problem} if problem else {})}

    def route(self, action, body):
        host = self.host
        with host._lock:
            if self._stop.is_set():
                raise ValueError("The screen preview service has stopped.")
            now = time.monotonic()
            self._expire(now)
            if action == "load":
                points, raw = _points(body.get("points")), body.get("effect")
                if isinstance(raw, dict) and raw.get("kind") == "scene":
                    return {"id": self._load_scene(raw, points, now)}
                effect = _effect(raw)
                params = raw.get("params", {})
                if not isinstance(params, dict):
                    raise ValueError("preview parameters must be an object")
                return {"id": self._load(points, effect, params, now)}
            iid = str(body.get("id") or "")
            scene = self._scenes.get(iid)
            builtin = self._builtins.get(iid)
            inst = host.instances.get(iid)
            if action == "stop":
                self._unload(iid)
                return {"ok": True}
            if action != "tick":
                raise ValueError("unknown preview request")
            if inst is None and builtin is None and scene is None:
                return {"problem": host.problems.get(f"preview:{iid}") or {"reason": "stopped", "message": "The preview ended."}}
            updates = body.get("updates") or {}
            if not isinstance(updates, dict):
                raise ValueError("invalid preview update")
            try:
                t, dt = float(body.get("time", 0)), float(body.get("dt", 1 / 30))
            except (TypeError, ValueError, OverflowError):
                raise ValueError("preview time must be a number") from None
            if not math.isfinite(t) or not math.isfinite(dt) or abs(t) > 1e9:
                raise ValueError("preview time must be finite and within the preview range")
            audio = _preview_audio(body.get("audio"))
            dt = max(0, min(dt, 0.25))
            if scene is not None:
                if updates:
                    raise ValueError("Reload the preview to change a scene.")
                scene.last_used = now
                out, problem = self._group_frames(scene, now, t, dt, audio or _sample_audio(t))
                if problem:
                    return {"problem": problem}
                colors = base64.b64encode(b"".join(out[key] for key in scene.keys)).decode("ascii") if (
                    len(out) == len(scene.keys)) else None
                return {"colors": colors, "logs": []}
            if "params" in updates and not isinstance(updates["params"], dict):
                raise ValueError("preview parameters must be an object")
            if builtin is not None:
                zone = _builtin_zone(_points(updates["points"])) if "points" in updates else builtin.zone
                params = (_preview_params(builtin.renderer, {**builtin.params, **updates["params"]})
                          if "params" in updates else builtin.params)
                frame = _render_builtin(builtin.renderer, zone, params, builtin.brightness, t, audio)
                builtin.zone, builtin.params, builtin.last_used = zone, params, now
                return {"colors": base64.b64encode(frame).decode("ascii"), "logs": []}
            if "points" in updates:
                inst.points = _points(updates["points"])
                host._emit({"type": "points", "id": iid, "points": inst.points})
            if "params" in updates:
                inst.params.update(updates["params"])
                host._emit({"type": "params", "id": iid, "params": inst.params})
            if "layers" in updates:
                clean = _effect({"kind": "layers", "layers": updates["layers"]})
                inst.eff.layers = clean["layers"]
                host._emit({"type": "layers", "id": iid, "layers": clean["layers"]})
            self._tick_host(inst, now, t, dt, audio, bool(body.get("backdrop")))
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
            self._builtins.clear()
            self._scenes.clear()
            self._profile = None
            self.host.close()
            self.host._wake.notify_all()
