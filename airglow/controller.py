"""Orchestrates everything: devices, the lighting engine, automations, sensors and the NZXT hub fans.

"Base" is what the user set by hand; automations, sleep and lock lay overrides on top. The effective scene
and fan curve are recomputed whenever any of those change.
"""

import collections
import logging
import threading
import time

from . import automation, devices, fancontrol, fans, sensors, system
from .config import validate_scene
from .hw import DeviceManager
from .history import SensorHistory
from .lighting.engine import Engine

log = logging.getLogger(__name__)


class Controller:
    TICK = 1.0                 # automations; sensors and fans run every other tick
    HISTORY = 300              # samples kept (10 minutes at 2 s)

    def __init__(self, store, manager=None, read_sensors=None, signals=None, start_power_events=False,
                 async_writes=True):
        self.store = store
        self.manager = manager or DeviceManager()
        s = store.state
        self.base_scene = dict(s["scene"])
        self.base_curve = s["fan_curve"] if s["fan_curve"] in store.curves() else "balanced"
        self.active_preset = s["active_preset"]
        self._read_sensors = read_sensors        # tests pass a summary; normally the full LHM tree is read
        self._system = sensors.SystemSensors()
        self.sensors = dict(sensors.OFFLINE)
        self.readings = {}                        # every sensor by id: {name, group, kind, unit, value}
        self.readings_history = SensorHistory(self.HISTORY)
        self.engine = Engine(self.manager, store, sensors=lambda: self.sensors, async_writes=async_writes)
        self.automations = automation.Automations(store, signals or system.snapshot, lambda: self.sensors)
        self.sleeping = self.locked = False
        self.effective_scene = dict(self.base_scene)
        self.effective_curve = self.base_curve
        self.fan_temp = self.fan_target = self.fan_duty = None
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
                                              "shutdown": self._on_shutdown, "lock": lambda: self._on_lock(True),
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
                                              "brightness": preset["brightness"], "on": True})

        with self._lock:
            step("lighting", lighting)
            if preset.get("fan_curve"):
                step("fans", lambda: setattr(self, "base_curve", preset["fan_curve"]))
            if preset.get("fan_profile"):
                step("FanControl", lambda: fancontrol.apply_profile(self.store.settings(), preset["fan_profile"]))
            self.active_preset = preset["id"] if all(s["ok"] for s in steps) else None
            self._changed()
        return steps

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
                elif t["type"] == "lights_off":
                    scene["on"] = False
                elif t["type"] == "dim":
                    scene["brightness"] = min(scene["brightness"], t["brightness"])
            if fr:
                t = fr["then"]
                if t["type"] == "fan_curve":
                    curve = t["curve"]
                elif t["type"] == "preset" and presets.get(t["preset"], {}).get("fan_curve"):
                    curve = presets[t["preset"]]["fan_curve"]
            settings = self.store.settings()
            if self.sleeping or (self.locked and settings["lights_off_on_lock"]):
                scene["on"] = False
            if curve not in self.store.curves():
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
        self.manager.reset(self.store.openrgb_enabled())
        self.engine.invalidate()
        self.apply()

    def _on_shutdown(self):
        log.info("Windows is shutting down")
        if self.store.settings()["lights_off_on_sleep"]:
            self.engine.blackout()
        self.failsafe()

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

    def _hub_ports(self, hub):
        return hub.connected_fans() or list(range(hub.FAN_PORTS))

    def _run_fans(self):
        cfg = self.store.settings()
        temp = fans.source_temp(self.sensors, cfg["temp_source"])
        target = fans.target_duty(self.store.curves()[self.effective_curve], temp, cfg["fan_failsafe_duty"])
        duty = fans.step_toward(self.fan_duty, target, cfg["fan_max_step"])
        self.fan_temp, self.fan_target, self.fan_duty = temp, round(target), duty
        for dev, hub in self._fan_devices():
            try:
                hub.poll()
                hub.set_fan_duty({p: duty for p in self._hub_ports(hub)})
            except (devices.DeviceError, OSError) as e:
                log.warning("%s fans: %s", dev.name, e)

    def failsafe(self):
        duty = self.store.settings()["fan_failsafe_duty"]
        for dev, hub in self._fan_devices():
            try:
                hub.set_fan_duty({p: duty for p in self._hub_ports(hub)})
                log.info("%s fans set to failsafe %d%%", dev.name, duty)
            except (devices.DeviceError, OSError) as e:
                log.error("could not set failsafe fan speed on %s: %s", dev.name, e)

    # ------------------------------------------------------------ sensors
    def _poll_sensors(self):
        if self._read_sensors:
            summary, flat = self._read_sensors(), []
        else:
            tree = sensors.read_tree(self.store.settings()["lhm_url"])
            flat = sensors.flatten(tree) if tree else []
            summary = sensors.summary(flat) if tree else dict(sensors.OFFLINE)
        self.sensors = summary
        return flat

    def _app_readings(self):
        hubs = self.hubs
        hub = bool(hubs)
        rpms = [h.fans[i]["rpm"] for h in hubs for i in h.connected_fans()]
        return [
            sensors._meta("app/fan_duty", "Case fan speed", "Fans", "control", "%",
                          self.fan_duty if hub else None, short="Fans"),
            sensors._meta("app/fan_rpm", "Case fan RPM", "Fans", "fan", "RPM",
                          round(sum(rpms) / len(rpms)) if rpms else None, short="Fan RPM"),
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
        self.engine.stop()
        self.failsafe()
        self.manager.close()
        log.info("controller stopped")

    # ------------------------------------------------------------ views
    def snapshot(self):
        fan_devs = self._fan_devices()
        a = self.automations
        rules = {r["id"]: r for r in self.store.get_rules()}
        devs = [d.info() for d in self.manager.all().values()]
        return {
            "scene": self.base_scene,
            "effective": self.effective_scene,
            "active_preset": self.active_preset,
            "override": {
                "lighting": a.light_rule["id"] if a.light_rule else None,
                "fans": a.fan_rule["id"] if a.fan_rule else None,
                "active": sorted(a.active & set(rules)),
                "paused_until": a.paused_until if a.paused else None,
                "locked": self.locked and self.store.settings()["lights_off_on_lock"],
            },
            "fans": {"curve": self.effective_curve, "base_curve": self.base_curve, "temp": self.fan_temp,
                     "target": self.fan_target, "duty": self.fan_duty,
                     "ports": [{"port": i + 1, "device": d.name, **h.fans[i]}
                               for d, h in fan_devs for i in h.connected_fans()],
                     "hub_online": bool(fan_devs)},
            "sensors": self.sensors,
            "devices": devs,
            "openrgb": {"enabled": self.store.openrgb_enabled(), "status": self.manager.bridge.status},
            "inputs": {"screen": {"active": self.engine.screen.active, "error": self.engine.screen.error},
                       "audio": {"active": self.engine.audio.active, "error": self.engine.audio.error}},
            "fps": round(self.engine.fps, 1),
            "healthy": self.healthy,
        }
