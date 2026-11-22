"""The lighting engine: renders effects for every enabled zone ~30 times a second and streams the result to
the devices. Only changed frames are written (plus a keep-alive), and while nothing on screen moves (Solid, Off)
it renders only IDLE_FPS frames a second, so static scenes cost almost nothing."""

import logging
import threading
import time
from types import SimpleNamespace

import numpy as np

from .. import lightsetup
from ..controlstatus import ControlStatus
from ..devices import DeviceError, ORDERS
from ..openrgb import OpenRGBError
from . import effects, geometry, studio
from .inputs import AudioAnalyzer, ScreenSampler
from .winddown import WindDown

# a channel order's index list, built once instead of per zone per frame
ORDER_INDEX = {name: list(idx) for name, idx in ORDERS.items()}

log = logging.getLogger(__name__)


class DeviceWriter:
    """One per device: keeps only the newest frame for each zone and writes it on its own thread, so slow USB
    devices update in parallel (not one after another) and a slow device never holds up rendering."""

    def __init__(self, dev, on_complete=None):
        self.dev = dev
        self._pending = {}
        self._lock = threading.Lock()
        self._wake = threading.Event()
        self._idle = threading.Event()
        self._idle.set()
        self._stopped = False
        self._on_complete = on_complete
        self.thread = threading.Thread(target=self._run, name=f"write-{dev.id}", daemon=True)
        self.thread.start()

    def submit(self, zone_id, colors):
        with self._lock:
            if self._stopped:
                return False
            self._pending[zone_id] = colors            # replaces an older frame nobody has written yet
            self._idle.clear()
        self._wake.set()
        return True

    def wait_idle(self, timeout):
        return self._idle.wait(timeout)

    def stop(self):
        with self._lock:
            self._stopped = True
            self._pending.clear()
        self._wake.set()

    def _run(self):
        while not self._stopped:
            self._wake.wait(0.5)
            self._wake.clear()
            with self._lock:
                if self._stopped:
                    self._idle.set()
                    return
                batch, self._pending = self._pending, {}
            if batch and self.dev.online:
                started, error = time.monotonic(), None
                try:
                    for zone_id, colors in batch.items():
                        if self._stopped:
                            break
                        self.dev.write_zone(zone_id, colors)
                    if not self._stopped:
                        self.dev.flush()
                        self.dev.error = None
                except (DeviceError, OpenRGBError, OSError) as e:
                    error = str(e)
                    log.warning("%s: %s", self.dev.name, e)
                    self.dev.error = str(e)
                    self.dev.close()                   # the device manager reconnects it in the background
                except Exception as e:                 # a bug in a device's code must not end this thread for good
                    error = str(e) or type(e).__name__
                    log.exception("%s: writing failed", self.dev.name)
                    self.dev.error = str(e) or type(e).__name__
                with self._lock:
                    if not self._stopped and self._on_complete:
                        self._on_complete(error, time.monotonic() - started, bool(self._pending))
            with self._lock:
                if not self._pending:
                    self._idle.set()


class Engine:
    FPS = 30
    IDLE_FPS = 4             # while every zone shows a still effect; any command brings back FPS at once
    STILL_AFTER = 0.5        # an animated effect whose colours haven't changed for this long is still, for now
    KEEPALIVE = 5.0          # re-send unchanged frames this often (some controllers forget)
    RESUME_GAP = 10.0        # a frame gap this long means the PC slept
    IDENTIFY_SECONDS = 6.0
    PROBE_SECONDS = 15.0     # the setup guide keeps renewing its probe; if it goes away, the zone goes back on its own
    PREVIEW_SECONDS = 10.0   # the gallery keeps asking while a profile's preview is on screen
    PROBE_LEVEL = 0.25       # white at a quarter: a long strip at full white can draw more than a header supplies

    def __init__(self, manager, store, sensors=lambda: {}, async_writes=True, control=None):
        self.manager, self.store, self._sensors = manager, store, sensors
        self.async_writes = async_writes  # tests write synchronously so they can check results right away
        self.control = control or ControlStatus()
        self.control.observe("lighting", "Lighting frames", kind="component", stale_after=5.0)
        self._writers = {}               # device id -> DeviceWriter
        self._online = {}                # device id -> the device object if it was online last frame (re-send on change)
        self.scene = dict(store.state["scene"])
        self._trial_zones = frozenset()  # zone overrides suppressed during a trial, never saved to the store
        self.screen, self.audio = ScreenSampler(), AudioAnalyzer()
        self._frames = ({}, {})          # (zone key -> the last frame's bytes, the same as hex once someone asked)
        self._source_frames, self._source_levels = {}, {}
        self._prepared_levels = {}
        self._wind_down = WindDown()
        self.frame_id = 0
        self.fps = 0.0
        self._identify = {}              # zone key -> deadline
        self._probe = {}                 # zone key -> (mode, LEDs to light, deadline), from the setup guide
        self._probe_done = set()         # zones whose probe just ended: sent dark once, past the configured LEDs
        self._preview = None             # (scene, deadline): a profile drawn for the gallery, never sent to devices
        self._preview_read = 0.0         # when the page last asked for preview frames (none: don't render them)
        self.preview_frames = {}
        self._bad_zones = set()          # zones whose layout can't be drawn (logged once each)
        self._last = {}                  # zone key -> (bytes, time sent)
        self._driven = {}                # zone key -> LED count sent; survives invalidation so Off can send black
        self._dev_sent = {}              # device id -> time a frame was last sent to it (for Device.max_fps)
        self._geom = {}                  # zone key -> (layout snapshot, namespace)
        self._missing = set()            # effects not on this PC (drawn as Solid), logged once each
        self._failing = set()            # effects whose render raised, logged once until they work again
        self._solid = effects.clean_params("solid", {})
        self._lock = threading.RLock()
        self._stop = threading.Event()
        self._thread = None
        self._t0 = time.monotonic()
        self._last_tick = None
        self.last_frame = None           # monotonic time of the last finished frame (for the health check)
        self.idle = False                # the last frame had nothing moving: render at IDLE_FPS
        self._moving = False             # set by render_zone when an animated effect was drawn this frame
        self._changed_at = 0.0           # when a zone's colours last changed (an animated effect can be still too)
        self._kick = threading.Event()   # a command arrived: render the next frame now
        self.scene_revision = 0
        self.scene_generation = 0
        self.scene_state = "ready"
        self.scene_error = None
        self.scene_capacity_error = None
        self._studio_groups = []
        self._studio_zones = set()
        self._transition = None
        self._prepared = {}

    @property
    def frames(self):
        """zone key -> "rrggbb..." for the live preview and the tray icon. Made into hex only when read: nobody
        reads it while the window is closed, and the engine has better things to do 30 times a second."""
        raw, hexed = self._frames
        if hexed is None:
            hexed = {k: v.tobytes().hex() for k, v in raw.items()}
            if self._frames[0] is raw:               # still the newest frame
                self._frames = (raw, hexed)
        return hexed

    @frames.setter
    def frames(self, value):
        self._frames = ({}, dict(value))
        self._source_frames, self._source_levels = {}, {}

    # ------------------------------------------------------------ commands
    def wind_down(self, action=None, *, minutes=None, token=None, session=None, version=None):
        with self._lock:
            now = time.monotonic()
            level = self.scene["brightness"] if self.scene.get("on") else 0
            if action == "start":
                if session is not None and (session != self._wind_down.session or version != self._wind_down.version):
                    raise ValueError("The wind-down status changed. Review it before starting a timer.")
                initial = level
                if level and self._transition:
                    # A crossfade may still be displaying its old level. Ignore identify/setup flashes.
                    initial = max((self._source_levels[k] for k in self._prepared if k in self._source_levels
                                   and k not in self._probe and self._identify.get(k, 0) <= now), default=level)
                self._wind_down.start(minutes, initial, now)
            elif action == "cancel":
                self._wind_down.cancel(token)
            elif action is not None:
                raise ValueError("Choose Start or Cancel for the wind-down timer.")
            if action:
                self._kick.set()
            return self._wind_down.status(level, now)

    def _transition_source(self):
        # Previously displayed frames may have been dimmed by a timer. Always blend the original colours
        # and their brightness levels, then cap the finished frame exactly once at the output boundary.
        return {"from": {k: v.copy() for k, v in self._source_frames.items()}, "levels": self._source_levels.copy()}

    def set_scene(self, scene, *, revision=None, transition_ms=0, commit_required=False):
        scene = dict(scene)
        try:                                         # never render with missing or bad parameters
            scene["params"] = effects.clean_params(scene.get("effect"), scene.get("params"))
        except ValueError as e:
            log.warning("bad scene (%s); falling back to solid", e)
            scene.update(effect="solid", params=effects.clean_params("solid", {}))
        with self._lock:
            self.scene_revision = self.scene_revision + 1 if revision is None else revision
            self.scene_generation += 1
            self.scene_state, self.scene_error = "pending", None
            self.scene_capacity_error = None
            self._transition = {**self._transition_source(),
                                "started": time.monotonic(), "ready_at": None,
                                "duration": transition_ms / 1000, "committed": not commit_required}
            if not scene.get("on", True):
                self._transition = None
                self.scene_state = "ready"
                studio.HOST.reconcile([])
                self._studio_groups = []
                self._studio_zones.clear()
                self._identify.clear()
                self._probe_done.update(self._probe)
                self._probe.clear()
                self._dev_sent.clear()
            self.scene = scene
            self._last.clear()                       # force a full re-send
        self._kick.set()

    def _requirements(self, scene, strict=False, zones=None, trial_zones=None, active=None, resolve_points=True):
        out = []
        if not scene.get("on", True):
            return out
        if active is None:
            active = {f"{dev.id}/{zone.id}" for dev in self.manager.all().values() if dev.online for zone in dev.zones}
        for key, cfg in (self.store.zones() if zones is None else zones).items():
            if key not in active or not cfg.get("enabled") or not geometry.total_leds(cfg["layout"]):
                continue
            z = self._geometry(key, cfg["layout"], cfg) if resolve_points else None
            spot = z.spot if z is not None else lightsetup.zone_spot(cfg)
            chosen = scene
            suppressed = self._trial_zones if trial_zones is None else trial_zones
            if key not in suppressed and cfg.get("override"):
                chosen = cfg["override"]
            elif scene.get("areas", {}).get(spot):
                chosen = scene["areas"][spot]
            eff = effects.EFFECTS.get(chosen["effect"])
            if eff is None:
                if strict:
                    raise ValueError(f"Effect {chosen['effect']} is no longer available")
                eff, params = effects.EFFECTS["solid"], self._solid
            else:
                params = effects.clean_params(chosen["effect"], chosen.get("params"))
            out.append((key, cfg, eff, params, z))
        return out

    @staticmethod
    def _effect_groups(requirements):
        groups = []
        for key, cfg, eff, params, z in requirements:
            if not isinstance(eff, studio.JsEffect):
                continue
            signature = (eff.fx_id, eff.sha256, params)
            zones = next((zones for ident, zones in groups if ident == signature), None)
            if zones is None:
                zones = []
                groups.append((signature, zones))
            zones.append((key, z.n if z is not None else geometry.total_leds(cfg["layout"])))
        return groups

    def validate_scene_requirements(self, scene, strict=True, zones=None, trial_zones=None):
        with self._lock:
            requirements = self._requirements(scene, strict=strict, zones=zones, trial_zones=trial_zones, resolve_points=False)
            groups = self._effect_groups(requirements)
            if len(groups) > studio.MAX_INSTANCES:
                raise ValueError(f"Only {studio.MAX_INSTANCES} distinct Studio effects can run at once")
            if any(sum(count for _, count in members) > studio.MAX_INSTANCE_LEDS for _, members in groups):
                raise ValueError(f"One Studio effect can drive at most {studio.MAX_INSTANCE_LEDS:,} LEDs")

    def _admit_studio_zones(self, groups):
        # A reconnect can add a requirement after validation. Keep working effect groups and zones first.
        capacity_error = None
        if len(groups) > studio.MAX_INSTANCES:
            groups = sorted(groups, key=lambda group: group[0] not in self._studio_groups)[:studio.MAX_INSTANCES]
            capacity_error = ("Only four distinct Studio effects can run at once. "
                              "Extra zones stay dark until a Studio effect slot is free.")
        accepted = set()
        for _, members in groups:
            if sum(count for _, count in members) <= studio.MAX_INSTANCE_LEDS:
                accepted.update(key for key, _ in members)
                continue
            remaining = studio.MAX_INSTANCE_LEDS
            for key, count in sorted(members, key=lambda item: item[0] not in self._studio_zones):
                if count <= remaining:
                    accepted.add(key)
                    remaining -= count
            if not capacity_error:
                capacity_error = (f"One Studio effect can drive at most {studio.MAX_INSTANCE_LEDS:,} LEDs. "
                                  "Extra zones stay dark until its LED count is reduced.")
        if capacity_error != self.scene_capacity_error:
            self.scene_generation += 1
        self.scene_capacity_error = capacity_error
        self._studio_groups = [signature for signature, _ in groups]
        self._studio_zones = accepted
        return accepted

    def validate_zone_changes(self, changes):
        with self._lock:
            zones = {**self.store.zones(), **changes}
            self.validate_scene_requirements(self.scene, strict=False, zones=zones)

    def prepare_scene(self):
        with self._lock:
            now = time.monotonic()
            self._prepare_scene_frame(self.store.zones(), now, now - self._t0)

    def commit_scene(self, revision):
        with self._lock:
            if revision == self.scene_revision and self._transition:
                self._transition["committed"] = True
                self._transition["ready_at"] = None
        self._kick.set()

    def hold_scene_failure(self, message):
        with self._lock:
            self.scene_state, self.scene_error = "failed", message
            if self._transition:
                self._transition.update(self._transition_source())

    def _prepare_scene_frame(self, zones, now, t, active=None):
        """Prepare all renderers before any device receives the candidate frame."""
        if self.scene_state == "failed" and self._transition:
            self._prepared = self._transition["from"]
            self._prepared_levels = self._transition["levels"]
            return
        requirements = self._requirements(self.scene, zones=zones, active=active)
        accepted = self._admit_studio_zones(self._effect_groups(requirements))
        required = studio.HOST.reconcile([(e, p, z) for key, _, e, p, z in requirements if key in accepted])
        candidate, levels, error = {}, {}, None
        for key, cfg, eff, params, z in requirements:
            if isinstance(eff, studio.JsEffect) and key not in accepted:
                candidate[key] = np.zeros((z.n, 3), dtype=np.uint8)
                continue
            rgb, bri = self.render_zone(key, cfg, now, t)
            if np.asarray(rgb).shape != (z.n, 3) or not np.isfinite(rgb).all() or eff.id in self._failing:
                error = f"{eff.name} did not produce a valid frame"
                continue
            candidate[key] = self._to_bytes(rgb, bri)
            levels[key] = bri
            if isinstance(eff, studio.JsEffect) and eff.fx_id in studio.HOST.problems:
                error = studio.HOST.problems[eff.fx_id]["text"]
        ready, problems = studio.HOST.readiness(required)
        if problems:
            error = problems[0]["text"]
        transition = self._transition
        if transition and not transition["committed"] and self.scene_capacity_error:
            error = self.scene_capacity_error
        if error and transition is None:
            transition = self._transition = {**self._transition_source(),
                                            "started": now, "ready_at": None, "duration": 0, "committed": True}
        if transition:
            if error or (not ready and now - transition["started"] >= studio.START_GRACE):
                self.scene_state, self.scene_error = "failed", error or "The effect did not supply its first frame in time"
                studio.HOST.reconcile([])
            if self.scene_state == "failed" or not ready:
                if self.scene_state != "failed":
                    self.scene_state = "pending"
                self._prepared = {k: transition["from"].get(k, np.zeros((z.n, 3), dtype=np.uint8))
                                  for k, _, _, _, z in requirements}
                self._prepared_levels = transition["levels"]
                return
            if transition["ready_at"] is None:
                self.scene_state = "ready"
                if not transition["committed"]:
                    self._prepared = {k: transition["from"].get(k, np.zeros_like(v)) for k, v in candidate.items()}
                    self._prepared_levels = transition["levels"]
                    return
                transition["ready_at"] = now
            duration = transition["duration"]
            alpha = min(1.0, (now - transition["ready_at"]) / duration) if duration else 1.0
            for key, value in candidate.items():
                old = transition["from"].get(key)
                if old is not None and old.shape == value.shape:
                    candidate[key] = np.floor(old * (1 - alpha) + value * alpha + .5).astype(np.uint8)
                    levels[key] = transition["levels"].get(key, 100) * (1 - alpha) + levels.get(key, 100) * alpha
            if alpha >= 1:
                self._transition = None
        self._prepared = candidate
        self._prepared_levels = levels

    def identify(self, zone_key):
        with self._lock:
            now = time.monotonic()
            self._check_flash_allowed(now)
            self._identify[zone_key] = now + self.IDENTIFY_SECONDS
        self._kick.set()

    def _check_flash_allowed(self, now):
        if not self.scene.get("on", True):
            raise ValueError("The lights are off. Turn lighting on or wait for the power hold to end before testing a zone.")
        if self._wind_down.cap(now) == 0:
            raise ValueError("Wind down is holding the lights at zero. Release the timer before testing a zone.")

    def probe(self, zone_key, mode, n=0):
        """Light a zone for the setup guide, over every LED the header can drive, whatever is configured:
        "blink" flashes them all, "count" lights the first n (the last one amber) so you can see where the
        strip ends, "off" goes back to normal."""
        with self._lock:
            if mode == "off":
                if self._probe.pop(zone_key, None):
                    self._probe_done.add(zone_key)
            else:
                now = time.monotonic()
                self._check_flash_allowed(now)
                self._probe[zone_key] = (mode, int(n), now + self.PROBE_SECONDS)
            self._last.pop(zone_key, None)
        self._kick.set()

    def preview(self, scene):
        """Render a profile for the gallery's mini stage alongside the real frames, for a few seconds. None stops."""
        with self._lock:
            self._preview = (scene, time.monotonic() + self.PREVIEW_SECONDS) if scene else None
            self._preview_read = time.monotonic() if scene else 0.0    # someone asked, so render at least once
            if not scene:
                self.preview_frames = {}
        self._kick.set()

    def mark_preview_read(self):
        """The page asked for preview frames: it is worth rendering (and waking the loop for) them."""
        self._preview_read = time.monotonic()
        self._kick.set()

    def _render_preview(self, zones, now, t):
        scene, deadline = self._preview
        if deadline <= now or now - self._preview_read > 0.5:
            return                       # nobody is looking: don't render a second copy of the whole rig
        out = {}
        for dev in list(self.manager.all().values()):
            if not dev.online:
                continue
            for zone in dev.zones:
                key = f"{dev.id}/{zone.id}"
                zcfg = zones.get(key)
                if not zcfg or not zcfg.get("enabled") or geometry.total_leds(zcfg["layout"]) == 0:
                    continue
                rgb, bri = self.render_zone(f"preview:{key}", zcfg, now, t, scene=scene, own=False)
                out[key] = self._to_bytes(rgb, bri).tobytes().hex()
        self.preview_frames = out

    def _probe_colors(self, mode, n, count, now):
        out = np.zeros((count, 3))
        if mode == "blink":
            if int(now * 3) % 2 == 0:
                out[:] = self.PROBE_LEVEL
        else:
            n = min(max(n, 0), count)
            out[:n] = self.PROBE_LEVEL
            if n:
                out[n - 1] = (1.0, 0.55, 0.1)          # the last lit LED: where you stop
        return out

    def invalidate(self, zone_key=None):
        with self._lock:
            if self._transition and not self._transition["committed"] and self.scene_state == "ready":
                self.scene_state = "pending"
            if zone_key:
                self._geom.pop(zone_key, None)
                self._last.pop(zone_key, None)
                if self._transition:
                    self._transition["from"].pop(zone_key, None)
            else:
                self._geom.clear()
                self._last.clear()
                if self._transition:
                    self._transition["from"].clear()
        self._kick.set()                             # a zone's settings changed (an override, its layout)

    def set_trial_zones(self, keys):
        with self._lock:
            if self._trial_zones == frozenset(keys):
                return
            self._trial_zones = frozenset(keys)
            self.invalidate()

    # ------------------------------------------------------------ rendering
    def _geometry(self, key, layout, zcfg=None):
        cached = self._geom.get(key)
        spot = lightsetup.zone_spot(zcfg) if zcfg else None
        if cached and cached[0] == layout and cached[1].spot == spot:
            return cached[1]
        pos, local, fan = geometry.positions(layout)
        # key and spot: Studio effects draw for these LEDs, and their layers can be limited to an area
        z = SimpleNamespace(pos=geometry.normalised(pos), canvas=pos, local=local, fan=fan, n=len(pos), key=key,
                            spot=spot)
        self._geom[key] = (dict(layout), z)
        return z

    def _context(self, needs, params):
        s = self._sensors() or {}
        if "screen" in needs:
            self.screen.touch(display=int(params.get("display", 1)))
        if "audio" in needs:
            self.audio.touch()
        return SimpleNamespace(cpu_temp=s.get("cpu_temp"), gpu_temp=s.get("gpu_temp"),
                               screen=self.screen.frame if "screen" in needs else None,
                               audio=self.audio.frame if "audio" in needs else None)

    def _identify_colors(self, z, deadline, now):
        """First LED flashes green, then a white dot walks the LEDs in order so you can count them."""
        elapsed = self.IDENTIFY_SECONDS - (deadline - now)
        out = np.full((z.n, 3), 0.04)
        if elapsed < 1.0:
            if int(elapsed * 6) % 2 == 0:
                out[0] = (0, 1, 0)
        else:
            i = int((elapsed - 1.0) / (self.IDENTIFY_SECONDS - 1.0) * z.n) % max(z.n, 1)
            out[: i] = 0.18
            out[i] = 1.0
        return out

    def render_zone(self, key, zcfg, now, t, scene=None, own=True):
        """One zone's colours. scene/own=False: how a profile would look, without this rig's identify flashes or
        the effects you gave single zones (the gallery's preview)."""
        layout = zcfg["layout"]
        z = self._geometry(key, layout, zcfg)
        scene = scene or self.scene
        if not scene.get("on", True):
            return np.zeros((z.n, 3)), 0
        deadline = self._identify.get(key) if own else None
        if deadline and deadline > now:
            return self._identify_colors(z, deadline, now), 100
        eff_id, params = scene["effect"], scene["params"]
        if own and key not in self._trial_zones and zcfg.get("override"):
            eff_id, params = zcfg["override"]["effect"], zcfg["override"]["params"]
        elif scene.get("areas"):                       # a profile's effect for the area this zone sits in
            area = scene["areas"].get(z.spot)      # cached per zone by _geometry, not rescanned every frame
            if area:
                eff_id, params = area["effect"], area["params"]
        eff = effects.EFFECTS.get(eff_id)
        if eff is None:                                # a Studio effect that isn't on this PC any more
            if eff_id not in self._missing:
                self._missing.add(eff_id)
                log.warning("effect %s isn't on this PC any more; showing Solid instead", eff_id)
            eff, params = effects.EFFECTS["solid"], self._solid
        else:
            self._missing.discard(eff_id)
        if eff.animated:
            self._moving = True
        ctx = self._context(eff.needs, params)
        try:
            rgb = eff.render(z, t, params, ctx)
            self._failing.discard(eff_id)
        except Exception:
            if eff_id not in self._failing:            # once, not 30 times a second per zone
                self._failing.add(eff_id)
                log.exception("effect %s failed (logged again only after it has worked)", eff_id)
            rgb = np.zeros((z.n, 3))
        return rgb, scene.get("brightness", 100)

    @staticmethod
    def _to_bytes(rgb, brightness):
        vals = np.clip(rgb * (brightness / 100.0), 0, 1)
        return (vals * 255 + 0.5).astype(np.uint8)

    @staticmethod
    def _small_change(old, new, limit):
        """True when frame `new` is within `limit` of the last one sent on every channel. A channel going dark
        always counts, so a fade never stops a few steps short of off."""
        if len(old) != len(new):
            return False
        a = np.frombuffer(old, np.uint8).astype(np.int16)
        b = np.frombuffer(new, np.uint8).astype(np.int16)
        return int(np.abs(a - b).max(initial=0)) <= limit and not np.any((b == 0) & (a != 0))

    def tick(self):
        # A command cannot replace the prepared scene halfway through publishing its zone batch.
        with self._lock:
            self._tick_frame()

    def _tick_frame(self):
        now = time.monotonic()
        wall = time.time()
        if self._last_tick is not None and wall - self._last_tick > self.RESUME_GAP:
            log.info("woke up after %.0f s; reconnecting lighting devices", wall - self._last_tick)
            self.manager.request_reset()              # on the device thread: reconnecting can take seconds
            self._last.clear()
        self._last_tick = wall
        # Reconnecting and OpenRGB housekeeping run on the device manager's own thread; this loop only
        # writes to devices that are already connected, so it never waits on the network or USB enumeration.

        t = now - self._t0
        zones = self.store.zones()
        frames, source_levels = {}, {}
        cap = self._wind_down.cap(now)
        source_frames = {} if cap is not None else frames
        self._moving = False
        seen = set()
        current = self.manager.all()
        online_ids = {dev.id for dev in current.values() if dev.online}
        active = {f"{dev.id}/{zone.id}" for dev in current.values() if dev.id in online_ids for zone in dev.zones}
        self._prepare_scene_frame(zones, now, t, active)
        for key, writer in list(self._writers.items()):
            if current.get(key) is not writer.dev or not writer.dev.online:
                writer.stop()
                self._writers.pop(key, None)
        present = {"light:" + key for key in current}
        self.control.reconcile("lighting", present)
        for key in set(self._online) - set(current):
            self._online.pop(key, None)
            self._dev_sent.pop(key, None)
            for zone_key in [k for k in self._driven if k.startswith(key + "/")]:
                self._driven.pop(zone_key, None)
                self._last.pop(zone_key, None)
                self._geom.pop(zone_key, None)
        for dev in list(current.values()):
            online = dev.id in online_ids
            status_key = "light:" + dev.id
            enabled = any((zones.get(f"{dev.id}/{z.id}") or {}).get("enabled") for z in dev.zones)
            self.control.observe(status_key, dev.name, kind="lighting", owner="Airglow" if enabled else "Device",
                                 online=online, expected=enabled, stale_after=15 if getattr(dev, "keepalive", True) else None)
            # (re)connected, or reloaded as a new object (OpenRGB's device list changed): send every zone again
            if online and self._online.get(dev.id) is not dev:
                for k in [k for k in self._last if k.startswith(dev.id + "/")]:
                    del self._last[k]
            self._online[dev.id] = dev if online else None
            if not online:
                continue
            # a slow device is sent at most max_fps frames a second (half an engine frame of slack, so timer
            # jitter doesn't turn 10 fps into 7.5); its zones are still rendered for the live preview
            max_fps = getattr(dev, "max_fps", None)
            sent = self._dev_sent.get(dev.id)
            throttled = bool(max_fps) and sent is not None and now - sent < 1.0 / max_fps - 0.5 / self.FPS
            min_change = getattr(dev, "min_change", 0)
            keepalive = getattr(dev, "keepalive", True)
            wrote = False
            write_started = time.monotonic()
            for zone in dev.zones:
                key = f"{dev.id}/{zone.id}"
                seen.add(key)
                zcfg = zones.get(key)
                level = None
                with self._lock:
                    probe = self._probe.get(key)
                    if probe and probe[2] <= now:
                        del self._probe[key]
                        self._probe_done.add(key)
                        probe = None
                    ended = not probe and key in self._probe_done
                    self._probe_done.discard(key)
                    if probe:
                        rgb, bri = self._probe_colors(probe[0], probe[1], zone.max_leds, now), 100
                    elif ended:                           # dark past the configured LEDs, then back to normal
                        rgb, bri = np.zeros((zone.max_leds, 3)), 100
                    elif not zcfg:
                        continue
                    elif not zcfg.get("enabled") and self._identify.get(key, 0) <= now:
                        count = self._driven.get(key, 0)
                        if not count:
                            continue              # leave never-enabled peripherals under their own control
                        rgb, bri = np.zeros((count, 3)), 100
                    else:
                        try:
                            if geometry.total_leds(zcfg["layout"]) == 0:
                                continue
                            prepared = self._prepared.get(key)
                            if prepared is not None:
                                rgb, bri = prepared / 255.0, 100
                                level = self._prepared_levels.get(key, 100)
                            else:
                                rgb, bri = self.render_zone(key, zcfg, now, t)
                        except Exception:
                            # one unusable zone must not stop every other zone's frame (and the whole loop)
                            if key not in self._bad_zones:
                                self._bad_zones.add(key)
                                log.exception("zone %s can't be drawn; showing it dark", key)
                            rgb, bri = np.zeros((max(zcfg["layout"].get("leds", 1) if isinstance(zcfg.get("layout"), dict)
                                                     else 1, 1), 3)), 0
                rgb_ints = self._to_bytes(rgb, bri)
                level = bri if level is None else level
                source_levels[key] = level
                if cap is not None:
                    source_frames[key] = rgb_ints
                    if cap < level:
                        rgb_ints = np.floor(rgb_ints * (cap / level) + .5).astype(np.uint8)
                frames[key] = rgb_ints                                      # preview shows true colours
                order = zcfg.get("order", "RGB") if zcfg else "RGB"
                ints = rgb_ints[:, ORDER_INDEX[order]] if order != "RGB" else rgb_ints
                raw = ints.tobytes()
                prev = self._last.get(key)
                if prev is None or prev[0] != raw:
                    self._changed_at = now              # colours moved: even a static-looking scene isn't still
                if throttled:
                    if ended:                          # the dark frame after a probe goes out next time
                        with self._lock:
                            self._probe_done.add(key)
                    continue
                if prev and (not keepalive or now - prev[1] < self.KEEPALIVE) and (
                        prev[0] == raw or (min_change and self._small_change(prev[0], raw, min_change))):
                    continue
                self._dev_sent[dev.id] = now
                if self.async_writes:
                    writer = self._writers.get(dev.id)
                    if writer is None or writer.dev is not dev:
                        if writer:                     # OpenRGB reloaded the device as a new object
                            writer.stop()
                        def completed(error, duration, pending, key=status_key):
                            if error:
                                self.control.failed(key, error)
                            else:
                                self.control.succeeded(key, duration=duration, pending=pending)
                        writer = self._writers[dev.id] = DeviceWriter(dev, completed)
                    self.control.queued(status_key)
                    writer.submit(zone.id, [tuple(px) for px in ints.tolist()])
                    self._last[key] = (raw, now)
                    self._driven[key] = len(ints)
                    continue
                try:
                    self.control.queued(status_key)
                    dev.write_zone(zone.id, [tuple(px) for px in ints.tolist()])
                    self._last[key] = (raw, now)
                    self._driven[key] = len(ints)
                    wrote = True
                except (DeviceError, OpenRGBError, OSError) as e:
                    self.control.failed(status_key, e)
                    log.warning("%s: %s", dev.name, e)
                    dev.error = str(e)
                    dev.close()
                    break
            if wrote:
                try:
                    dev.flush()
                    dev.error = None
                    self.control.succeeded(status_key, duration=time.monotonic() - write_started)
                except (DeviceError, OpenRGBError, OSError) as e:
                    dev.error = str(e)
                    self.control.failed(status_key, e)
        for key, probe in list(self._probe.items()):
            if key not in seen and probe[2] <= now:
                # its zone went away (unplugged, or OpenRGB reloaded the device): a stale probe would count as
                # busy and hold the whole engine at 30 fps for the rest of the session
                del self._probe[key]
        for key in list(self._probe_done):
            if key not in seen:
                self._probe_done.discard(key)
        self._source_frames, self._source_levels = source_frames, source_levels
        self._frames = (frames, None)
        studio.HOST.tick(t, self.audio.frame if self.audio.active else None)
        if self._preview and self._preview[1] <= now:
            self._preview, self.preview_frames = None, {}
        if self._preview:
            self._render_preview(zones, now, t)      # builds its own dict: no lock, no tearing for the reader
        self.frame_id += 1
        self.last_frame = time.monotonic()
        self.control.succeeded("lighting", duration=self.last_frame - now)
        self._identify = {k: d for k, d in self._identify.items() if d > now}
        with self._lock:
            busy = self._probe or self._probe_done or self._preview or self._transition
        # An animated effect only needs full rate while it's actually changing: "Temperature" is animated but still
        # whenever the PC's temperature is, so it drops to IDLE_FPS like a static colour does.
        moving = self._moving and now - self._changed_at < self.STILL_AFTER
        self.idle = not (moving or self._identify or busy or self.screen.active or self.audio.active)

    # ------------------------------------------------------------ lifecycle
    def _loop(self):
        period = 1.0 / self.FPS
        count, window = 0, time.monotonic()
        while not self._stop.is_set():
            start = time.monotonic()
            self._kick.clear()
            try:
                self.tick()
            except Exception as e:
                self.control.failed("lighting", e)
                log.exception("lighting frame failed")
                time.sleep(0.5)
            count += 1
            if start - window >= 2.0:
                self.fps, count, window = count / (start - window), 0, start
            if self.idle:                    # nothing moves: wait longer, but wake for a command right away
                self._kick.wait(max(0.0, 1.0 / self.IDLE_FPS - (time.monotonic() - start)))
                continue
            # time.sleep uses a high-resolution timer on Windows (Python 3.11+); Event.wait rounds up to ~15.6 ms
            # steps, which turned a 33 ms frame into 47 ms (21 fps).
            time.sleep(max(0.0, period - (time.monotonic() - start)))

    def start(self):
        self._thread = threading.Thread(target=self._loop, name="lighting", daemon=True)
        self._thread.start()

    @property
    def healthy(self):
        """Alive and actually producing frames (a stuck thread is still 'alive')."""
        if self._thread is None or not self._thread.is_alive():
            return False
        return self.last_frame is None or time.monotonic() - self.last_frame < 5.0

    def blackout(self):
        """Send all zones dark once and wait for it to reach the hardware (used for sleep/shutdown lights-off)."""
        for dev in list(self.manager.all().values()):
            if not dev.online:
                continue
            writer = self._writers.get(dev.id)
            if writer and writer.dev is not dev:       # a replaced device's writer: write this one directly
                writer = None
            for zone in dev.zones:
                key = f"{dev.id}/{zone.id}"
                zcfg = self.store.zones().get(key)
                count = self._driven.get(key, 0)
                if zcfg and zcfg.get("enabled"):
                    count = max(count, geometry.total_leds(zcfg["layout"]))
                if count:
                    black = [(0, 0, 0)] * count
                    if writer:
                        writer.submit(zone.id, black)
                    else:
                        try:
                            dev.write_zone(zone.id, black)
                        except (DeviceError, OpenRGBError, OSError):
                            pass
            if writer:
                writer.wait_idle(1.5)
            else:
                try:
                    dev.flush()
                except (DeviceError, OpenRGBError, OSError):
                    pass

    def stop(self):
        self._stop.set()
        if self._thread and self._thread.ident is not None:
            self._thread.join(timeout=2)
        for w in self._writers.values():
            w.wait_idle(1.0)
            w.stop()
