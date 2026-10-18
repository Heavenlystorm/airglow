"""Studio effects on the real lights.

They run in the effect host: a separate Airglow process (airglow/effecthost.py) with its own hidden WebView2. Its page
(web/runtime/host.html) runs each effect in a sandboxed frame and worker, exactly like the Studio's preview, and
talks to this process over the local API: it listens on GET /api/fx/host/stream for what to run and the frame clock,
and answers with POST /api/fx/host/frames.

Here, a JsEffect is one more effect for the engine. Each tick it hands the host the LED positions it was asked about
and returns the newest colours the host sent back (so the lights trail the effect by one frame). The host process is
started when the first Studio effect is on and stopped a while after the last one goes off; if it hangs or uses too
much memory it's ended and started again, and an effect that misbehaves is turned off with the reason.
"""

import base64
import logging
import secrets
import subprocess
import threading
import time

import numpy as np

from .. import fxlibrary
from . import effects

log = logging.getLogger(__name__)

MAX_INSTANCES = 4
UNUSED_SECONDS = 3.0          # an effect nobody rendered for this long is unloaded
IDLE_STOP_SECONDS = 30.0      # the host process quits this long after the last effect went off
FRAME_TIMEOUT = 4.0           # no frames for this long while effects run: the host is stuck
START_GRACE = 15.0            # WebView2 can take a while to start
MEMORY_LIMIT = 512 * 1024 * 1024          # for any one WebView2 process (each effect's page runs in one)
RUNAWAY_SECONDS = 120.0                    # over the limit this soon after a fresh start: an effect is eating memory
UNIT = 200                    # stage canvas units -> effect canvas pixels (1.6 x 1.0 -> 320 x 200)

REASONS = {
    "error": "it stopped with an error",
    "hung": "it stopped answering (an endless loop?)",
    "slow": "it took too long to draw each frame",
    "memory": "the effects used too much memory",
    "crashed": "the effect host stopped",
    "busy": f"only {MAX_INSTANCES} Studio effects can run at once",
}


class Instance:
    """One effect running in the host, with the LED positions it draws for."""

    def __init__(self, iid, eff, params, now):
        self.id, self.eff, self.params = iid, eff, dict(params)
        self.slices = {}               # zone key -> (start, count, z) in the point list
        self.points = []
        self.colors = None             # bytes, 3 per point, from the host
        self.used_tick = 0
        self.last_used = now
        self.failed = None             # {"reason", "message", "line"}
        # sent to the host page yet? A new or edited effect is (re)loaded once per frame, after every zone has added
        # its LEDs: a host page that's still starting it would drop LED positions sent separately before it's ready.
        self.loaded = False

    def use(self, key, z, tick, now):
        """Make sure this zone's LEDs are in the point list. True when the list changed."""
        self.used_tick, self.last_used = tick, now
        hit = self.slices.get(key)
        if hit and hit[2] is z:
            return False
        start = len(self.points) if not hit else None
        if hit:                                   # the zone moved: rebuild the whole list
            self.slices.pop(key)
            self.points, rebuilt = [], {}
            for k, (_, _, zz) in self.slices.items():
                rebuilt[k] = (len(self.points), zz.n, zz)
                self.points += _points(zz)
            self.slices = rebuilt
            start = len(self.points)
        self.slices[key] = (start, z.n, z)
        self.points += _points(z)
        return True

    def colours(self, key):
        start, n, _ = self.slices[key]
        c = self.colors
        if c is None or len(c) < (start + n) * 3:
            return np.zeros((n, 3))
        return np.frombuffer(c, np.uint8, n * 3, start * 3).reshape(n, 3) / 255.0


def _points(z):
    spot = getattr(z, "spot", None)
    return [[round(float(x) * UNIT * 1.6, 2), round(float(y) * UNIT, 2), spot] for x, y in z.pos]


class EffectHost:
    def __init__(self, library=None, spawn=None, clock=time.monotonic):
        self._library = library
        self._spawn = spawn or _spawn_host
        self._clock = clock
        self._lock = threading.RLock()
        self._wake = threading.Condition(self._lock)
        self.url = None                # http://127.0.0.1:<port>, set once the API is listening
        self.token = secrets.token_urlsafe(24)
        self.instances = {}            # iid -> Instance
        self.problems = {}             # fx_id -> {"name", "reason", "text", "message", "line"}
        self._events = []              # control events for the host page, in order
        self._tick_event = None        # the newest frame clock event (older ones are dropped)
        self._seq = 0
        self._tick = 0
        self._t_last = None
        self.proc = None
        self._started = 0.0
        self._last_frame = 0.0
        self._last_active = 0.0
        self._last_mem_check = 0.0
        self._restarts = []
        self.connected = False

    @property
    def library(self):
        return self._library or fxlibrary.library()

    # ------------------------------------------------------------ the engine's side
    def render(self, eff, z, t, params):
        """Colours for one zone (called by JsEffect.render on the lighting thread)."""
        key = getattr(z, "key", None) or str(id(z))
        with self._lock:
            if eff.fx_id in self.problems:
                return np.zeros((z.n, 3))
            inst = self._instance(eff, params)
            if inst is None:
                return np.zeros((z.n, 3))
            self._last_active = self._clock()
            if inst.use(key, z, self._tick, self._last_active) and inst.loaded:
                self._emit({"type": "points", "id": inst.id, "points": list(inst.points)})
            return inst.colours(key)

    def _instance(self, eff, params):
        for inst in self.instances.values():
            if inst.eff.fx_id == eff.fx_id and inst.params == params:
                if inst.eff.sha256 != eff.sha256:                  # edited (the Studio's draft): reload it
                    inst.eff, inst.loaded = eff, False
                return inst
        # same effect, new parameters, and the old ones weren't drawn this tick: a slider moved, keep the instance
        for inst in self.instances.values():
            if inst.eff.fx_id == eff.fx_id and inst.used_tick < self._tick:
                inst.params = dict(params)
                if inst.eff.sha256 != eff.sha256:
                    inst.eff, inst.loaded = eff, False
                elif inst.loaded:
                    self._emit({"type": "params", "id": inst.id, "params": inst.params})
                return inst
        if len(self.instances) >= MAX_INSTANCES:
            self._fail(eff, "busy")
            return None
        self._seq += 1
        inst = Instance(f"i{self._seq}", eff, params, self._clock())
        self.instances[inst.id] = inst
        self._ensure_running()
        return inst

    def _load_event(self, inst):
        e = inst.eff
        return {"type": "load", "id": inst.id, "kind": e.kind, "code": e.code, "layers": e.layers,
                "params": dict(inst.params), "points": list(inst.points)}

    def tick(self, t, audio=None):
        """Once per engine frame: send the frame clock, unload unused effects, watch the host."""
        now = self._clock()
        with self._lock:
            self._tick += 1
            for iid in [i for i, inst in self.instances.items() if now - inst.last_used > UNUSED_SECONDS]:
                del self.instances[iid]
                self._emit({"type": "unload", "id": iid})
            for inst in self.instances.values():                     # new or edited this frame: send it whole
                if not inst.loaded:
                    inst.loaded = True
                    self._emit(self._load_event(inst))
            if self.instances:
                self._last_active = now
                dt = 1 / 30 if self._t_last is None else max(0.0, min(0.25, t - self._t_last))
                wants_audio = any("audio" in i.eff.permissions for i in self.instances.values())
                self._tick_event = {"type": "tick", "time": round(t, 4), "dt": round(dt, 4),
                                    "audio": _audio(audio) if wants_audio else None}
                self._wake.notify_all()
            self._t_last = t
            self._watch(now)

    # ------------------------------------------------------------ the host page's side
    def _emit(self, event):
        self._events.append(event)
        self._wake.notify_all()

    def check_token(self, token):
        return isinstance(token, str) and secrets.compare_digest(token, self.token)

    def stream(self, send, stopping):
        """Feed the host page over SSE until it disconnects. send(event, data) raises OSError when it's gone."""
        with self._lock:
            self.connected = True
            self._last_frame = self._clock()
            self._events = [self._load_event(i) for i in self.instances.values()]   # a fresh page: load everything
            for inst in self.instances.values():
                inst.loaded = True
        try:
            while not stopping.is_set():
                with self._lock:
                    if not self._events and self._tick_event is None:
                        self._wake.wait(1.0)
                    events, self._events = self._events, []
                    tick, self._tick_event = self._tick_event, None
                for e in events:
                    send(e["type"], e)
                if tick:
                    send("tick", tick)
                elif not events:
                    send("ping", {})
        finally:
            with self._lock:
                self.connected = False

    def frames(self, body):
        """POST /api/fx/host/frames: {frames: {iid: base64 colours}, problems: {iid: {reason, message, line}},
        logs: [...]}"""
        with self._lock:
            self._last_frame = self._clock()
            for iid, b64 in (body.get("frames") or {}).items():
                inst = self.instances.get(iid)
                if inst and isinstance(b64, str):
                    try:
                        inst.colors = base64.b64decode(b64, validate=True)
                    except ValueError:
                        pass
            for iid, p in (body.get("problems") or {}).items():
                inst = self.instances.pop(iid, None)
                if inst and isinstance(p, dict):
                    self._fail(inst.eff, str(p.get("reason", "error")), str(p.get("message") or "")[:300],
                               p.get("line") if isinstance(p.get("line"), int) else None)
                    self._emit({"type": "unload", "id": iid})
        for line in (body.get("logs") or [])[:20]:
            log.info("effect: %s", str(line)[:300])
        return {"ok": True}

    def packed_frames(self, data):
        """POST /api/fx/host/frames as application/octet-stream: for each instance, the id's length (1 byte), the id,
        the colours' length (2 bytes, little endian) and the colours. Empty: nothing changed, the host is alive."""
        frames, o = {}, 0
        while o + 3 <= len(data):
            n = data[o]
            iid = data[o + 1:o + 1 + n].decode("ascii", "replace")
            size = int.from_bytes(data[o + 1 + n:o + 3 + n], "little")
            o += 3 + n
            if o + size > len(data):
                break                                    # cut short: keep what arrived whole
            frames[iid] = data[o:o + size]
            o += size
        with self._lock:
            self._last_frame = self._clock()
            for iid, colours in frames.items():
                inst = self.instances.get(iid)
                if inst:
                    inst.colors = colours
        return {"ok": True}

    # ------------------------------------------------------------ problems
    def _fail(self, eff, reason, message="", line=None):
        if eff.fx_id not in self.problems:
            log.warning("Studio effect %s turned off: %s %s", eff.fx_id, reason, message)
        self.problems[eff.fx_id] = {"effect": eff.fx_id, "name": eff.name, "reason": reason,
                                    "text": f"{eff.name} is off: {REASONS.get(reason, reason)}.",
                                    "message": message, "line": line}

    def retry(self, fx_id=None):
        """Let a turned-off effect run again (the user changed it, or pressed Try again)."""
        with self._lock:
            if fx_id:
                self.problems.pop(fx_id, None)
            else:
                self.problems.clear()

    def status(self):
        with self._lock:
            return {"running": self.proc is not None and self.proc.poll() is None, "connected": self.connected,
                    "effects": len(self.instances), "problems": list(self.problems.values())}

    # ------------------------------------------------------------ the process
    def _ensure_running(self):
        if self.url is None or (self.proc is not None and self.proc.poll() is None):
            return
        self._restarts = [r for r in self._restarts if self._clock() - r < 60]
        if len(self._restarts) >= 3:
            for inst in list(self.instances.values()):
                self._fail(inst.eff, "crashed")
            self.instances.clear()
            return
        self._restarts.append(self._clock())
        self.token = secrets.token_urlsafe(24)
        self._started = self._last_frame = self._clock()
        try:
            self.proc = self._spawn(f"{self.url}/runtime/host.html#token={self.token}")
            log.info("effect host started (pid %s)", getattr(self.proc, "pid", "?"))
        except OSError as e:
            log.error("couldn't start the effect host: %s", e)
            self.proc = None

    def _watch(self, now):
        p = self.proc
        if p is None:
            if self.instances:
                self._ensure_running()
            return
        if p.poll() is not None:
            log.warning("effect host exited (%s)", p.returncode)
            self.proc = None
            if self.instances:
                self._ensure_running()
            return
        if not self.instances:
            if now - self._last_active > IDLE_STOP_SECONDS:
                log.info("no Studio effects on; stopping the effect host")
                self.stop()
            return
        if now - self._started > START_GRACE and now - self._last_frame > FRAME_TIMEOUT:
            log.warning("effect host stopped answering; starting it again")
            self.stop()
            self._ensure_running()
            return
        if now - self._last_mem_check > 2.0:
            self._last_mem_check = now
            used = _tree_memory(p.pid)
            if used > MEMORY_LIMIT:
                if now - self._started < RUNAWAY_SECONDS:            # filled up fast: turn the effects off
                    for inst in list(self.instances.values()):
                        self._fail(inst.eff, "memory")
                    self.instances.clear()
                    self.stop()
                    return
                # grew slowly over a long time (WebView2 creeps up): start it fresh, the effects carry on
                log.info("effect host reached %d MB after %d minutes; starting it fresh", used // 2**20,
                         (now - self._started) // 60)
                self.stop()
                self._ensure_running()

    def stop(self):
        p, self.proc = self.proc, None
        if p is None:
            return
        try:
            import psutil
            parent = psutil.Process(p.pid)
            for child in parent.children(recursive=True):        # WebView2's own processes
                child.kill()
            parent.kill()
        except Exception:  # already gone
            pass


def _audio(frame):
    if frame is None:
        return None
    bands = getattr(frame, "bands64", None)
    if bands is None:
        bands = np.repeat(frame.bands, 4)
    return {"level": round(float(frame.level), 3), "bass": round(float(frame.bass), 3),
            "beat": bool(getattr(frame, "beat", False)), "bands": [round(float(b), 3) for b in bands]}


def _tree_memory(pid):
    """The biggest WebView2 process under the host. An effect that runs away with memory grows its own renderer
    process; adding all of them up would mostly count WebView2's shared memory again and again."""
    try:
        import psutil
        kids = psutil.Process(pid).children(recursive=True)
        return max((k.memory_info().rss for k in kids if k.name().lower().startswith("msedgewebview2")), default=0)
    except Exception:
        return 0


def _spawn_host(url):
    from ..tools import self_command
    exe, args = self_command(["--effect-host", url, "--parent", str(__import__("os").getpid())])
    return subprocess.Popen([exe, *args], creationflags=0x08000000)       # CREATE_NO_WINDOW


# ---------------------------------------------------------------- the engine's view of a Studio effect

class JsEffect(effects.Effect):
    animated = True

    def __init__(self, eff):
        self.fx_id, self.id = eff["fx_id"], eff["fx_id"]
        self.name, self.description = eff["name"], eff["description"]
        self.params = eff["schema"]
        self.needs = ("audio",) if "audio" in eff["permissions"] else ()
        self.kind, self.code, self.layers = eff["kind"], eff["code"], eff["layers"]
        self.permissions, self.sha256 = eff["permissions"], eff["sha256"]
        self.studio_info = {"studio": {"origin": eff["origin"], "author": eff["author"], "kind": eff["kind"],
                                       "tags": eff["tags"], "remix_of": eff["remix_of"],
                                       "permissions": eff["permissions"]}}

    def render(self, z, t, p, ctx):
        return HOST.render(self, z, t, p)


HOST = EffectHost()
_effects = {}                  # (fx_id, sha256) -> JsEffect
_looked_up = {}                # fx_id -> (library, its rev, when, JsEffect or None)
RECHECK_SECONDS = 1.0          # the engine asks for every zone on every frame; the file is looked at this often


def _provide(fx_id):
    # Looking an effect up checks its file, and the engine asks for every zone 30 times a second: that was most of
    # the lighting thread's time. Changes made in Airglow bump the library's rev and show at once; an effect file
    # replaced by hand shows within RECHECK_SECONDS. The Studio's draft is in memory, so it's never cached.
    lib, now = HOST.library, time.monotonic()
    hit = _looked_up.get(fx_id)
    if hit and hit[0] is lib and hit[1] == lib.rev and now - hit[2] < RECHECK_SECONDS:
        return hit[3]
    eff = lib.get(fx_id)
    found = None
    if eff:
        key = (fx_id, eff["sha256"])
        found = _effects.get(key)
        if found is None:
            found = _effects[key] = JsEffect(eff)
            if len(_effects) > 64:
                _effects.pop(next(iter(_effects)))
    if fx_id != fxlibrary.DRAFT:
        _looked_up[fx_id] = (lib, lib.rev, now, found)
        if len(_looked_up) > 64:
            _looked_up.pop(next(iter(_looked_up)))
    return found


def _list():
    return [_provide(e["fx_id"]) for e in HOST.library.all()]


effects.EFFECTS.provider = _provide
effects.EFFECTS.lister = _list
