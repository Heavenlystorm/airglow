"""Orchestrates everything: devices, the lighting engine, automations, sensors and the fans (native fan
controllers, and the motherboard headers and coolers LibreHardwareMonitor drives).

"Base" is what the user set by hand; automations, sleep and lock lay overrides on top. The effective scene
and fan curve are recomputed whenever any of those change.
"""

import collections
import logging
import threading
import time

from . import automation, devices, fancontrol, fans, lhmfans, sensors, system, tools
from .config import SAFETY_TEMP, validate_scene
from .hw import DeviceManager
from .history import SensorHistory
from .lighting.engine import Engine

log = logging.getLogger(__name__)


class Controller:
    TICK = 1.0                 # automations; sensors and fans run every other tick
    HISTORY = 300              # samples kept (10 minutes at 2 s)
    # At sign-in Airglow often starts before LibreHardwareMonitor answers. Until the first temperature arrives (for at
    # most this long) the fans run at the bottom of the curve, which is where a freshly started PC sits, instead of
    # jumping to the failsafe speed and ramping back down over the next half minute.
    STARTUP_GRACE = 45.0

    def __init__(self, store, manager=None, read_sensors=None, signals=None, start_power_events=False,
                 async_writes=True, read_flat=None, lhm_fans=None):
        self.store = store
        self.manager = manager or DeviceManager()
        if hasattr(self.manager, "dynamic_enabled"):
            self.manager.dynamic_enabled = lambda: store.settings()["dynamic_lighting_enabled"]
        s = store.state
        self.base_scene = dict(s["scene"])
        self.trial = None                          # a profile being tried: {"name", "before": what to go back to}
        self.base_curve = s["fan_curve"] if self.curve_points(s["fan_curve"]) else "balanced"
        self.active_preset = s["active_preset"]
        self._read_sensors = read_sensors        # tests pass a summary; normally the full LHM tree is read
        self._read_flat = read_flat              # tests: LHM's sensors as sensors.flatten() gives them
        self._flat = []                          # the last LHM sensor list (fan channels come from it)
        self.lhm = sensors.LhmPoller(lambda: store.settings()["lhm_url"], tools.lhm_headers)
        self.lhm_fans = lhm_fans or lhmfans.LhmFans(lambda: store.settings()["lhm_url"], tools.lhm_headers)
        self.fan_channels = []                   # what LHM can drive, with what Airglow does with each
        self._system = sensors.SystemSensors()
        self.sensors = dict(sensors.OFFLINE)
        self.readings = {}                        # every sensor by id: {name, group, kind, unit, value}
        self.readings_history = SensorHistory(self.HISTORY)
        self.engine = Engine(self.manager, store, sensors=lambda: self.sensors, async_writes=async_writes)
        self.automations = automation.Automations(store, signals or system.snapshot, lambda: self.sensors)
        self.sleeping = self.locked = self.shutting_down = False
        self.effective_scene = dict(self.base_scene)
        self.effective_curve = self.base_curve
        self.fan_temp = self.fan_target = self.fan_duty = None
        self._started = time.monotonic()
        self._temp_seen = False
        self._failsafe_at = 0.0            # shutdown, quitting and a failing tick can all ask within a moment
        self.history = collections.deque(maxlen=self.HISTORY)
        self.rev = 0
        self._ticks = 0
        self._engine_stall_logged = False
        self._last_tick = None
        self._lock = threading.RLock()
        self._stop = threading.Event()
        self._thread = None
        self._power = None
        if start_power_events:
            self._power = system.PowerEvents({"suspend": self._on_suspend, "resume": self._on_resume,
                                              "shutdown": self._on_shutdown,
                                              "shutdown_cancelled": self._on_shutdown_cancelled, "lock": lambda: self._on_lock(True),
                                              "unlock": lambda: self._on_lock(False)})
        self.apply()

    # ------------------------------------------------------------ manual commands
    def set_scene(self, patch):
        with self._lock:
            merged = {**self.base_scene, **{k: v for k, v in patch.items() if k in ("effect", "params", "brightness", "on")}}
            if "effect" in patch and patch["effect"] != self.base_scene["effect"] and "params" not in patch:
                merged["params"] = {}                     # new effect starts from its defaults
            self.base_scene = validate_scene(merged)
            if any(k in patch for k in ("effect", "params", "brightness")):
                self.active_preset = None
            self._changed()

    def set_fan_curve(self, name):
        if name not in self.store.curves():
            raise ValueError(f"unknown fan curve '{name}'")
        with self._lock:
            self.base_curve = name
            self.active_preset = None
            self._changed()

    def apply_preset(self, preset):
        steps = []

        def step(label, fn):
            try:
                fn()
                steps.append({"step": label, "ok": True})
            except Exception as e:
                log.warning("preset %s: %s failed: %s", preset.get("name"), label, e)
                steps.append({"step": label, "ok": False, "error": str(e)})

        def lighting():
            self.base_scene = validate_scene({"effect": preset["effect"], "params": preset["params"],
                                              "brightness": preset["brightness"], "areas": preset.get("areas"),
                                              "on": True})

        with self._lock:
            if self.trial is not None:
                self._leave_trial()                # picking a profile ends a trial: Discard won't take it back
            step("lighting", lighting)
            curve = self.preset_curve(preset)
            if curve:
                step("fans", lambda: setattr(self, "base_curve", curve))
            if preset.get("fan_profile"):
                step("FanControl", lambda: fancontrol.apply_profile(self.store.settings(), preset["fan_profile"]))
            self.active_preset = preset["id"] if all(s["ok"] for s in steps) else None
            self._changed()
        return steps

    # ------------------------------------------------------------ fan curves, including a profile's own
    @staticmethod
    def preset_curve(preset):
        """The curve a profile switches to: its own ("profile:<id>"), a named one, or None to leave fans alone."""
        if preset.get("fan_points"):
            return f"profile:{preset['id']}"
        return preset.get("fan_curve")

    def curve_points(self, name):
        """Points of a curve by name: a named curve, a profile's own, or the one being tried. None if unknown."""
        if not name:
            return None
        if name == "trial":
            return (self.trial or {}).get("fan_points")
        if name.startswith("profile:"):
            p = next((p for p in self.store.get_presets() if p["id"] == name[8:]), None)
            return p.get("fan_points") if p else None
        return self.store.curves().get(name)

    # ------------------------------------------------------------ trying a profile before keeping it
    def try_profile(self, scene, name, fan_points=None, clear_zones=(), kind="profile"):
        """Play a profile on the real lights (and its fan curve, when given) until end_trial().
        clear_zones: zones whose own effect would hide the profile's per-area effects; put back on discard.
        kind "studio": the Studio playing the effect being edited ("Also on my lights")."""
        with self._lock:
            if self.trial is None:
                zones = self.store.zones()
                self.trial = {"before": {"scene": dict(self.base_scene), "curve": self.base_curve,
                                         "preset": self.active_preset,
                                         "overrides": {k: zones[k].get("override") for k in clear_zones if k in zones}}}
            self.trial.update(name=name, fan_points=fan_points, kind=kind)
            for key in clear_zones:
                self._set_override(key, None)
            self.base_scene = validate_scene({**scene, "on": True})
            self.base_curve = "trial" if fan_points else self.trial["before"]["curve"]
            self.active_preset = None
            self.apply()                           # not saved: a restart goes back to what you had

    def end_trial(self, keep_preset=None):
        """Discard the profile being tried, or keep it: keep_preset is the profile it was installed as."""
        with self._lock:
            if self.trial is None:
                return
            if keep_preset:
                self.trial = None
                self.apply_preset(keep_preset)
                return
            before = self._leave_trial()
            self.base_scene, self.active_preset = before["scene"], before["preset"]
            self._changed()

    def _leave_trial(self):
        """End the trial: zones get their own effects back and the fans their curve. Returns what it replaced."""
        before, self.trial = self.trial["before"], None
        for key, override in before["overrides"].items():
            self._set_override(key, override)
        self.base_curve = before["curve"] if self.curve_points(before["curve"]) else "balanced"
        return before

    def _set_override(self, key, override):
        cfg = self.store.zones().get(key)
        dev = self.manager.get(key.rpartition("/")[0])
        zone = next((z for z in (dev.zones if dev else []) if f"{dev.id}/{z.id}" == key), None)
        if cfg is None or zone is None or cfg.get("override") == override:
            return
        self.store.set_zone(key, {**cfg, "override": override}, zone.max_leds)
        self.engine.invalidate(key)

    def mark_preset(self, preset_id):
        """The current settings were just saved as this preset."""
        with self._lock:
            self.active_preset = preset_id
            self.store.save_state(active_preset=preset_id)

    def presets_changed(self):
        """Presets were edited or removed: rules and the effective scene may point at them."""
        with self._lock:
            if self.active_preset and self.active_preset not in {p["id"] for p in self.store.get_presets()}:
                self.active_preset = None
                self.store.save_state(active_preset=None)
            try:
                self.automations.evaluate()
            except Exception:
                log.exception("automations failed")
            self.apply()

    def _changed(self):
        if self.trial is not None:
            # changes while trying a profile adjust what's being tried and are never saved: a restart (or Discard)
            # goes back to what you had, and "trial" is never written as your fan curve
            self.apply()
            return
        self.store.save_state(scene=self.base_scene, fan_curve=self.base_curve, active_preset=self.active_preset)
        self.apply()

    # ------------------------------------------------------------ overrides
    def apply(self):
        """Recompute the effective scene and fan curve from base + automations + sleep/lock."""
        with self._lock:
            scene, curve = dict(self.base_scene), self.base_curve
            presets = {p["id"]: p for p in self.store.get_presets()}
            lr, fr = self.automations.light_rule, self.automations.fan_rule
            if lr:
                t = lr["then"]
                if t["type"] == "preset" and t["preset"] in presets:
                    p = presets[t["preset"]]
                    scene = {"effect": p["effect"], "params": p["params"], "brightness": p["brightness"], "on": True}
                    if p.get("areas"):
                        scene["areas"] = p["areas"]
                elif t["type"] == "lights_off":
                    scene["on"] = False
                elif t["type"] == "dim":
                    scene["brightness"] = min(scene["brightness"], t["brightness"])
            if fr:
                t = fr["then"]
                if t["type"] == "fan_curve":
                    curve = t["curve"]
                elif t["type"] == "preset" and t["preset"] in presets and self.preset_curve(presets[t["preset"]]):
                    curve = self.preset_curve(presets[t["preset"]])
            settings = self.store.settings()
            if self.sleeping or self.shutting_down or (self.locked and settings["lights_off_on_lock"]):
                scene["on"] = False
            if not self.curve_points(curve):
                curve = "balanced"
            if scene != self.effective_scene:
                self.effective_scene = scene
                self.engine.set_scene(scene)
            self.effective_curve = curve
            self.rev += 1

    # ------------------------------------------------------------ power events
    def _on_suspend(self):
        log.info("system is going to sleep")
        if self.store.settings()["lights_off_on_sleep"]:
            self.sleeping = True
            self.apply()
            self.engine.blackout()

    def _on_resume(self):
        log.info("system woke up")
        self.sleeping = False
        self.lhm_fans.resend_all()                # firmware may have taken the headers back while asleep
        self.manager.reset(self.store.openrgb_enabled())
        self.engine.invalidate()
        self.apply()

    def _on_shutdown(self):
        log.info("Windows is shutting down")
        if self.store.settings()["lights_off_on_sleep"]:
            # keep them dark: the engine runs until the process ends, and its next frame would light them again
            self.shutting_down = True
            self.apply()
            self.engine.blackout()
        self.failsafe()

    def _on_shutdown_cancelled(self):
        log.info("Windows isn't shutting down after all")
        if self.shutting_down:
            self.shutting_down = False
            self.apply()

    def _on_lock(self, locked):
        self.locked = locked
        self.apply()

    # ------------------------------------------------------------ fans
    def _fan_devices(self):
        """Connected native controllers with fan ports, as (device, hub) pairs."""
        return [(d, d.hub) for d in self.manager.natives.values()
                if getattr(d, "hub", None) is not None and d.online and getattr(d.hub, "FAN_PORTS", 0)]

    @property
    def hubs(self):
        return [h for _, h in self._fan_devices()]

    @property
    def hub(self):
        """The first connected fan controller (most rigs have one)."""
        hubs = self.hubs
        return hubs[0] if hubs else None

    @property
    def drives_fans(self):
        """Some fan follows Airglow: a native fan controller, or a channel driven through LibreHardwareMonitor."""
        return bool(self.hubs) or any(c["mode"] for c in self.fan_channels)

    @staticmethod
    def _hub_ports(hub):
        # Every port, not only fans reporting RPM: a fan that stalled at a low speed reads 0 and would otherwise
        # never be sent a faster one, however hot it gets. An empty port ignores the command.
        return list(range(hub.FAN_PORTS))

    def _run_fans(self):
        cfg = self.store.settings()
        temp = fans.source_temp(self.sensors, cfg["temp_source"])
        curve = self.curve_points(self.effective_curve)
        waited = time.monotonic() - self._started
        if temp is not None and not self._temp_seen:
            self._temp_seen = True
            log.info("first temperature %.0f °C, %.0f s after start; fans follow the curve", temp, waited)
        if temp is None and not self._temp_seen and waited < self.STARTUP_GRACE:
            if self.fan_duty is None:
                log.info("no temperatures yet (LibreHardwareMonitor starting?); fans at the curve's lowest speed "
                         "for up to %.0f s", self.STARTUP_GRACE)
            target = fans.lowest_duty(curve)
        else:
            target = fans.target_duty(curve, temp, cfg["fan_failsafe_duty"])
        # "full speed at 90 °C" holds for the hottest part, not only the one the curve follows (the coolant stays
        # cool while a processor whose pump has stopped overheats)
        hottest = fans.source_temp(self.sensors, "max")
        if hottest is not None and hottest >= SAFETY_TEMP:
            target = 100
        duty = fans.step_toward(self.fan_duty, target, cfg["fan_max_step"])
        self.fan_temp, self.fan_target, self.fan_duty = temp, round(target), duty
        for dev, hub in self._fan_devices():
            try:
                hub.poll()
                hub.set_fan_duty({p: duty for p in self._hub_ports(hub)})
            except (devices.DeviceError, OSError) as e:
                log.warning("%s fans: %s", dev.name, e)
        self._run_lhm_fans(duty, temp, cfg["fan_failsafe_duty"], grace=temp is None and not self._temp_seen
                           and time.monotonic() - self._started < self.STARTUP_GRACE, hottest=hottest)

    def _run_lhm_fans(self, duty, temp, failsafe, grace=False, hottest=None):
        """Motherboard headers, pumps and cooler fans through LibreHardwareMonitor: the ones you chose follow the
        curve's speed or hold their own. While LHM isn't answering nothing is sent (each try would stall this
        loop), and everything is sent again once it's back."""
        chosen = self.store.fan_channels()
        found = lhmfans.channels(self._flat)
        targets = {}
        for ch in found:
            c = chosen.get(ch["id"])
            if not c:
                continue
            if c["mode"] == "curve":
                targets[ch["id"]] = duty
            else:
                targets[ch["id"]] = c["duty"] if grace else fans.steady_duty(c["duty"], temp, failsafe, hottest)
        if self._flat:
            self.lhm_fans.apply(targets)
        else:
            self.lhm_fans.resend_all()
        errors = self.lhm_fans.errors
        self.fan_channels = [{**ch, "mode": (chosen.get(ch["id"]) or {}).get("mode"),
                              "steady": (chosen.get(ch["id"]) or {}).get("duty"), "target": targets.get(ch["id"]),
                              "error": errors.get(ch["id"])} for ch in found]

    def failsafe(self, force=False):
        # Windows' shutdown notification, the shutdown itself and quitting all reach this within a second or two;
        # sending the same speeds again only costs USB traffic while the session is going down.
        now = time.monotonic()
        if not force and now - self._failsafe_at < 2.0:
            return
        self._failsafe_at = now
        duty = self.store.settings()["fan_failsafe_duty"]
        for dev, hub in self._fan_devices():
            try:
                hub.set_fan_duty({p: duty for p in self._hub_ports(hub)})
                log.info("%s fans set to failsafe %d%%", dev.name, duty)
            except (devices.DeviceError, OSError) as e:
                log.error("could not set failsafe fan speed on %s: %s", dev.name, e)
        # LHM channels Airglow drives: at least the failsafe speed (a pump held faster keeps its speed)
        n = self.lhm_fans.failsafe(duty)
        if n:
            log.info("%d fan%s through LibreHardwareMonitor set to at least %d%%", n, "" if n == 1 else "s", duty)

    def fan_channels_changed(self):
        """A channel's setting changed: show it right away; the next fan tick sends it."""
        chosen = self.store.fan_channels()
        self.fan_channels = [{**c, "mode": (chosen.get(c["id"]) or {}).get("mode"),
                              "steady": (chosen.get(c["id"]) or {}).get("duty")} for c in self.fan_channels]

    def release_fans(self):
        """Quitting: fans driven through LHM go back to the BIOS or the cooler's own curve."""
        try:
            self.lhm_fans.release_all()
        except Exception:
            log.exception("could not hand fans back to their own control")

    # ------------------------------------------------------------ sensors
    def _poll_sensors(self):
        if self._read_sensors:
            summary, flat = self._read_sensors(), (self._read_flat() if self._read_flat else [])
        else:
            tree = self.lhm.tree                       # read on the poller's thread, never here
            flat = sensors.flatten(tree) if tree else []
            summary = sensors.summary(flat) if tree else dict(sensors.OFFLINE)
        self.sensors = summary
        self._flat = flat
        return flat

    def _app_readings(self):
        hubs = self.hubs
        rpms = [h.fans[i]["rpm"] for h in hubs for i in h.connected_fans()]
        rpms += [c["rpm"] for c in self.fan_channels if c["mode"] == "curve" and c["rpm"]]
        # What an hour at the current power draw costs. total_power is CPU + graphics; the rest of the PC, the PSU's
        # losses and any energy accumulated over time are deliberately not estimated here.
        watts = self.sensors.get("total_power")
        if watts is None:                       # some LHM builds only report the two halves
            parts = [self.sensors.get("cpu_power"), self.sensors.get("gpu_power")]
            watts = round(sum(p for p in parts if p is not None), 1) if any(p is not None for p in parts) else None
        price = self.store.settings().get("energy_price") or 0.0
        cost = round(watts / 1000 * price, 3) if watts is not None else None
        cost_meta = sensors._meta("app/energy_cost", "Cost per hour", "Quick picks", "cost", "€/h", cost, short="Cost")
        cost_meta["source"] = "CPU + GPU"
        cost_meta["needs_lhm"] = True
        return [
            sensors._meta("app/fan_duty", "Case fan speed", "Fans", "control", "%",
                          self.fan_duty if self.drives_fans else None, short="Fans"),
            sensors._meta("app/fan_rpm", "Case fan RPM", "Fans", "fan", "RPM",
                          round(sum(rpms) / len(rpms)) if rpms else None, short="Fan RPM"),
            cost_meta,
        ]

    def _update_readings(self, flat):
        self.readings = sensors.build(flat, self._system.read(), self._app_readings())
        self.readings_history.add(time.time(), {k: m["value"] for k, m in self.readings.items()})

    def readings_for(self, ids):
        r = self.readings
        return {i: (r[i]["value"] if i in r else None) for i in ids}

    # ------------------------------------------------------------ loop
    def _record(self):
        s = self.sensors
        self.history.append({"t": round(time.time()), "cpu": s.get("cpu_temp"), "gpu": s.get("gpu_temp"),
                             "cpu_w": s.get("cpu_power"), "gpu_w": s.get("gpu_power"), "duty": self.fan_duty})

    def _apply_device_params(self):
        saved = self.store.device_params()
        for d in self.manager.all().values():
            if d.parameters:
                d.set_params(saved.get(d.id, {}))

    def tick(self):
        now = time.time()
        if self._last_tick is not None and now - self._last_tick > 10 and self.sleeping:
            self._on_resume()                         # missed the resume broadcast
        self._last_tick = now
        self._ticks += 1
        before = (self.automations.light_rule or {}).get("id"), (self.automations.fan_rule or {}).get("id")
        try:
            self.automations.evaluate()
        except Exception:
            log.exception("automations failed")
        after = (self.automations.light_rule or {}).get("id"), (self.automations.fan_rule or {}).get("id")
        if before != after:
            log.info("automation in control: lighting=%s fans=%s", *after)
            self.apply()
        self.store.ensure_zones([d.info() for d in self.manager.all().values()])
        self._apply_device_params()
        if not self.engine.healthy and self.engine.last_frame is not None and not self._engine_stall_logged:
            log.error("lighting engine stopped producing frames; restart Airglow to recover")
            self._engine_stall_logged = True
        if self._ticks % 2 == 1:
            flat = self._poll_sensors()
            self._run_fans()
            self._record()
            self._update_readings(flat)

    def _loop(self):
        failures = 0
        try:
            while not self._stop.is_set():
                try:
                    self.tick()
                    failures = 0
                except Exception:
                    failures += 1
                    log.exception("controller tick failed (%d in a row)", failures)
                    if failures >= 5:
                        self.failsafe()
                self._stop.wait(self.TICK)
        finally:
            if not self._stop.is_set():
                log.error("controller loop stopped unexpectedly; fans set to failsafe")
                self.failsafe()

    @staticmethod
    def _warm_up_inputs():
        """Import the screen/audio libraries early. Their first import (cffi parsing a C header for SoundCard)
        holds the interpreter for a second or two; doing it a few seconds after start means switching to a
        reactive effect later is instant instead of stalling the lights."""
        time.sleep(4)
        for mod in ("mss", "soundcard"):
            try:
                __import__(mod)
            except Exception as e:  # missing input libraries only disable those effects
                log.warning("could not preload %s: %s", mod, e)

    def start(self):
        threading.Thread(target=self._warm_up_inputs, name="warm-up", daemon=True).start()
        self.manager.maintain(self.store.openrgb_enabled(), force=True)
        self.manager.start(self.store.openrgb_enabled)
        self.store.ensure_zones([d.info() for d in self.manager.all().values()])
        self._apply_device_params()
        if not self._read_sensors:
            self.lhm.start()
        self.engine.start()
        if self._power:
            self._power.start()
        self._thread = threading.Thread(target=self._loop, name="controller", daemon=True)
        self._thread.start()

    @property
    def healthy(self):
        return self._thread is not None and self._thread.is_alive() and self.engine.healthy

    def shutdown(self):
        if self._stop.is_set():
            return
        self._stop.set()
        if self._thread:
            self._thread.join(timeout=self.TICK + 1)
        self.lhm.stop()
        self.engine.stop()
        self.failsafe()
        self.release_fans()
        self.manager.close()
        log.info("controller stopped")

    # ------------------------------------------------------------ views
    def snapshot(self):
        fan_devs = self._fan_devices()
        a = self.automations
        rules = {r["id"]: r for r in self.store.get_rules()}
        devs = [d.info() for d in self.manager.all().values()]
        trial = self.trial                        # read once: the API thread can end the trial meanwhile
        return {
            "scene": self.base_scene,
            "effective": self.effective_scene,
            "active_preset": self.active_preset,
            "trial": {"name": trial["name"], "fans": bool(trial.get("fan_points")),
                      "kind": trial.get("kind", "profile")} if trial else None,
            "override": {
                "lighting": a.light_rule["id"] if a.light_rule else None,
                "fans": a.fan_rule["id"] if a.fan_rule else None,
                "active": sorted(a.active & set(rules)),
                "paused_until": a.paused_until if a.paused else None,
                "locked": self.locked and self.store.settings()["lights_off_on_lock"],
            },
            "fans": {"curve": self.effective_curve, "base_curve": self.base_curve, "temp": self.fan_temp,
                     "target": self.fan_target, "duty": self.fan_duty,
                     # no reading at all after the start-up grace: these are the failsafe speeds, not the curve's
                     "failsafe": self.fan_temp is None and time.monotonic() - self._started >= self.STARTUP_GRACE,
                     "ports": [{"port": i + 1, "device": d.name, **h.fans[i]}
                               for d, h in fan_devs for i in h.connected_fans()],
                     "hub_online": bool(fan_devs),
                     "channels": self.fan_channels},
            "sensors": self.sensors,
            "devices": devs,
            "openrgb": {"enabled": self.store.openrgb_enabled(), "status": self.manager.bridge.status,
                        "connected": bool(self.manager.bridge.client.connected),
                        "installed": tools.installed("openrgb"),
                        # lights from OpenRGB are set up: worth a notice when it isn't there
                        "used": self.store.has_openrgb_zones()},
            "dynamic_lighting": {"enabled": self.store.settings()["dynamic_lighting_enabled"],
                                 "status": getattr(getattr(self.manager, "dynamic", None), "status", "Off")},
            "inputs": {"screen": {"active": self.engine.screen.active, "error": self.engine.screen.error},
                       "audio": {"active": self.engine.audio.active, "error": self.engine.audio.error}},
            "fps": round(self.engine.fps, 1),
            "healthy": self.healthy,
        }
