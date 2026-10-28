"""The lighting engine: renders effects for every enabled zone ~30 times a second and streams the result to
the devices. Only changed frames are written (plus a keep-alive), and while nothing on screen moves (Solid, Off)
it renders only IDLE_FPS frames a second, so static scenes cost almost nothing."""

import logging
import threading
import time
from types import SimpleNamespace

import numpy as np

from .. import lightsetup
from ..devices import DeviceError, ORDERS
from ..openrgb import OpenRGBError
from . import effects, geometry, studio
from .inputs import AudioAnalyzer, ScreenSampler

# a channel order's index list, built once instead of per zone per frame
ORDER_INDEX = {name: list(idx) for name, idx in ORDERS.items()}

log = logging.getLogger(__name__)


class DeviceWriter:
    """One per device: keeps only the newest frame for each zone and writes it on its own thread, so slow USB
    devices update in parallel (not one after another) and a slow device never holds up rendering."""

    def __init__(self, dev):
        self.dev = dev
        self._pending = {}
        self._lock = threading.Lock()
        self._wake = threading.Event()
        self._idle = threading.Event()
        self._idle.set()
        self._stopped = False
        self.thread = threading.Thread(target=self._run, name=f"write-{dev.id}", daemon=True)
        self.thread.start()

    def submit(self, zone_id, colors):
        with self._lock:
            self._pending[zone_id] = colors            # replaces an older frame nobody has written yet
            self._idle.clear()
        self._wake.set()

    def wait_idle(self, timeout):
        return self._idle.wait(timeout)

    def stop(self):
        self._stopped = True
        self._wake.set()

    def _run(self):
        while not self._stopped:
            self._wake.wait(0.5)
            self._wake.clear()
            with self._lock:
                batch, self._pending = self._pending, {}
            if batch and self.dev.online:
                try:
                    for zone_id, colors in batch.items():
                        self.dev.write_zone(zone_id, colors)
                    self.dev.flush()
                except (DeviceError, OpenRGBError, OSError) as e:
                    log.warning("%s: %s", self.dev.name, e)
                    self.dev.error = str(e)
                    self.dev.close()                   # the device manager reconnects it in the background
                except Exception as e:                 # a bug in a device's code must not end this thread for good
                    log.exception("%s: writing failed", self.dev.name)
                    self.dev.error = str(e) or type(e).__name__
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

    def __init__(self, manager, store, sensors=lambda: {}, async_writes=True):
        self.manager, self.store, self._sensors = manager, store, sensors
        self.async_writes = async_writes  # tests write synchronously so they can check results right away
        self._writers = {}               # device id -> DeviceWriter
        self._online = {}                # device id -> the device object if it was online last frame (re-send on change)
        self.scene = dict(store.state["scene"])
        self._trial_zones = frozenset()  # zone overrides suppressed during a trial, never saved to the store
        self.screen, self.audio = ScreenSampler(), AudioAnalyzer()
        self._frames = ({}, {})          # (zone key -> the last frame's bytes, the same as hex once someone asked)
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

    # ------------------------------------------------------------ commands
    def set_scene(self, scene):
        scene = dict(scene)
        try:                                         # never render with missing or bad parameters
            scene["params"] = effects.clean_params(scene.get("effect"), scene.get("params"))
        except ValueError as e:
            log.warning("bad scene (%s); falling back to solid", e)
            scene.update(effect="solid", params=effects.clean_params("solid", {}))
        with self._lock:
            self.scene = scene
            self._last.clear()                       # force a full re-send
        self._kick.set()

    def identify(self, zone_key):
        self._identify[zone_key] = time.monotonic() + self.IDENTIFY_SECONDS
        self._kick.set()

    def probe(self, zone_key, mode, n=0):
        """Light a zone for the setup guide, over every LED the header can drive, whatever is configured:
        "blink" flashes them all, "count" lights the first n (the last one amber) so you can see where the
        strip ends, "off" goes back to normal."""
        with self._lock:
            if mode == "off":
                if self._probe.pop(zone_key, None):
                    self._probe_done.add(zone_key)
            else:
                self._probe[zone_key] = (mode, int(n), time.monotonic() + self.PROBE_SECONDS)
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
            if zone_key:
                self._geom.pop(zone_key, None)
                self._last.pop(zone_key, None)
            else:
                self._geom.clear()
                self._last.clear()
        self._kick.set()                             # a zone's settings changed (an override, its layout)

    def set_trial_zones(self, keys):
        with self._lock:
            self._trial_zones = frozenset(keys)
            self.invalidate()

    # ------------------------------------------------------------ rendering
    def _geometry(self, key, layout, zcfg=None):
        cached = self._geom.get(key)
        if cached and cached[0] == layout:
            return cached[1]
        pos, local, fan = geometry.positions(layout)
        # key and spot: Studio effects draw for these LEDs, and their layers can be limited to an area
        z = SimpleNamespace(pos=geometry.normalised(pos), canvas=pos, local=local, fan=fan, n=len(pos), key=key,
                            spot=lightsetup.zone_spot(zcfg) if zcfg else None)
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
        deadline = self._identify.get(key) if own else None
        if deadline and deadline > now:
            return self._identify_colors(z, deadline, now), 100
        scene = scene or self.scene
        if not scene.get("on", True):
            return np.zeros((z.n, 3)), 0
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
        frames = {}
        self._moving = False
        seen = set()
        for dev in list(self.manager.all().values()):
            online = dev.online
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
            for zone in dev.zones:
                key = f"{dev.id}/{zone.id}"
                seen.add(key)
                zcfg = zones.get(key)
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
                    elif not zcfg.get("enabled") and key not in self._identify:
                        continue                  # identify works on a switched-off zone, like the probe does
                    else:
                        try:
                            if geometry.total_leds(zcfg["layout"]) == 0:
                                continue
                            rgb, bri = self.render_zone(key, zcfg, now, t)
                        except Exception:
                            # one unusable zone must not stop every other zone's frame (and the whole loop)
                            if key not in self._bad_zones:
                                self._bad_zones.add(key)
                                log.exception("zone %s can't be drawn; showing it dark", key)
                            rgb, bri = np.zeros((max(zcfg["layout"].get("leds", 1) if isinstance(zcfg.get("layout"), dict)
                                                     else 1, 1), 3)), 0
                rgb_ints = self._to_bytes(rgb, bri)
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
                        writer = self._writers[dev.id] = DeviceWriter(dev)
                    writer.submit(zone.id, [tuple(px) for px in ints.tolist()])
                    self._last[key] = (raw, now)
                    continue
                try:
                    dev.write_zone(zone.id, [tuple(px) for px in ints.tolist()])
                    self._last[key] = (raw, now)
                    wrote = True
                except (DeviceError, OpenRGBError, OSError) as e:
                    log.warning("%s: %s", dev.name, e)
                    dev.error = str(e)
                    dev.close()
                    break
            if wrote:
                try:
                    dev.flush()
                except (DeviceError, OpenRGBError, OSError) as e:
                    dev.error = str(e)
        for key, probe in list(self._probe.items()):
            if key not in seen and probe[2] <= now:
                # its zone went away (unplugged, or OpenRGB reloaded the device): a stale probe would count as
                # busy and hold the whole engine at 30 fps for the rest of the session
                del self._probe[key]
        for key in list(self._probe_done):
            if key not in seen:
                self._probe_done.discard(key)
        self._frames = (frames, None)
        studio.HOST.tick(t, self.audio.frame if self.audio.active else None)
        if self._preview and self._preview[1] <= now:
            self._preview, self.preview_frames = None, {}
        if self._preview:
            self._render_preview(zones, now, t)      # builds its own dict: no lock, no tearing for the reader
        self.frame_id += 1
        self.last_frame = time.monotonic()
        self._identify = {k: d for k, d in self._identify.items() if d > now}
        with self._lock:
            busy = self._probe or self._probe_done or self._preview
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
            except Exception:
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
                zcfg = self.store.zones().get(f"{dev.id}/{zone.id}")
                if zcfg and zcfg.get("enabled"):
                    black = [(0, 0, 0)] * geometry.total_leds(zcfg["layout"])
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
        if self._thread:
            self._thread.join(timeout=2)
        for w in self._writers.values():
            w.wait_idle(1.0)
            w.stop()
