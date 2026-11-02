"""Orchestrates everything: devices, the lighting engine, automations, sensors and the fans (native fan
controllers, and the motherboard headers and coolers LibreHardwareMonitor drives).

"Base" is what the user set by hand; automations, sleep and lock lay overrides on top. The effective scene
and fan curve are recomputed whenever any of those change.
"""

import collections
import logging
import threading
import time
from urllib.parse import quote

from . import automation, devices, fancontrol, fans, lhmfans, sensors, system, tools
from .config import SAFETY_TEMP, validate_scene
from .controlstatus import ControlStatus
from .cooling import Recorder
from .hw import DeviceManager
from .history import SensorHistory
from .lighting import geometry
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
        self.manager = manager or DeviceManager(store=store)
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
        self._sensor_ready = threading.Event()
        self.lhm = sensors.LhmPoller(lambda: store.settings()["lhm_url"], tools.lhm_headers,
                                     on_change=self._sensor_ready.set)
        self.lhm_fans = lhm_fans or lhmfans.LhmFans(lambda: store.settings()["lhm_url"], tools.lhm_headers)
        self.fan_channels = []                   # what LHM can drive, with what Airglow does with each
        self._system = sensors.SystemSensors()
        self.sensors = dict(sensors.OFFLINE)
        self.readings = {}                        # every sensor by id: {name, group, kind, unit, value}
        self.catalog_rev = 0
        self._catalog_key = ()
        self.readings_history = SensorHistory(self.HISTORY)
        self.control = ControlStatus()
        self.control.observe("controller", "Controller cycles", kind="component", stale_after=5.0)
        self.control.observe("sensors", "Temperature samples", kind="component", online=False, stale_after=6.0)
        self.cooling = Recorder(store.dir)
        self._lhm_sample_seen = None
        self.engine = Engine(self.manager, store, sensors=lambda: self.sensors, async_writes=async_writes,
                             control=self.control)
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
            scene = validate_scene(merged)
            preset = self.active_preset
            if any(k in patch for k in ("effect", "params", "brightness")):
                preset = None
            self._commit_base(scene, self.base_curve, preset)

    def set_fan_curve(self, name):
        if name not in self.store.curves():
            raise ValueError(f"unknown fan curve '{name}'")
        with self._lock:
            self._commit_base(self.base_scene, name, None)

    def _commit_base(self, scene, curve, preset, *, end_trial=False):
        """Persist a candidate before publishing it to loops, snapshots or the lighting engine."""
        if self.trial is None or end_trial:
            self.store.save_state(scene=scene, fan_curve=curve, active_preset=preset)
        if end_trial and self.trial is not None:
            self.trial = None
            self.engine.set_trial_zones(())
        self.base_scene, self.base_curve, self.active_preset = scene, curve, preset
        self.apply()

    def apply_preset(self, preset):
        steps = []

        def step(label, fn):
            try:
                fn()
                steps.append({"step": label, "ok": True})
            except Exception as e:
                log.warning("preset %s: %s failed: %s", preset.get("name"), label, e)
                steps.append({"step": label, "ok": False, "error": str(e)})

        with self._lock:
            scene = validate_scene({"effect": preset["effect"], "params": preset["params"],
                                    "brightness": preset["brightness"], "areas": preset.get("areas"), "on": True})
            previous_curve = self.trial["before"]["curve"] if self.trial else self.base_curve
            curve = self.preset_curve(preset) or previous_curve
            # An external FanControl action is reported separately; do not claim its success in advance.
            selected = None if preset.get("fan_profile") else preset["id"]
            self._commit_base(scene, curve, selected, end_trial=True)
            steps.append({"step": "lighting", "ok": True})
            if self.preset_curve(preset):
                steps.append({"step": "fans", "ok": True})
            if preset.get("fan_profile"):
                step("FanControl", lambda: fancontrol.apply_profile(self.store.settings(), preset["fan_profile"]))
                if all(s["ok"] for s in steps):
                    self.mark_preset(preset["id"])
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
        clear_zones: zones whose own effect would hide the profile's per-area effects; ignored only while trying.
        kind "studio": the Studio playing the effect being edited ("Also on my lights")."""
        with self._lock:
            if self.trial is None:
                self.trial = {"before": {"scene": dict(self.base_scene), "curve": self.base_curve,
                                         "preset": self.active_preset}, "clear_zones": {}}
            self.trial.update(name=name, fan_points=fan_points, kind=kind)
            zones = self.store.zones()
            for key in clear_zones:
                if key not in zones:
                    continue
                dev = self.manager.get(key.rpartition("/")[0])
                zone = next((z for z in (dev.zones if dev else []) if f"{dev.id}/{z.id}" == key), None)
                self.trial["clear_zones"][key] = zone.max_leds if zone else geometry.total_leds(zones[key]["layout"])
            self.engine.set_trial_zones(self.trial["clear_zones"])
            self.base_scene = validate_scene({**scene, "on": True})
            self.base_curve = "trial" if fan_points else self.trial["before"]["curve"]
            self.active_preset = None
            self.apply()                           # not saved: a restart goes back to what you had

    def keep_profile(self, preset, rules=()):
        """Save a tried profile and its selection before ending the transient preview."""
        with self._lock:
            state = None
            clear = ()
            if self.trial is not None:
                state = {"scene": validate_scene({"effect": preset["effect"], "params": preset["params"],
                                                 "brightness": preset["brightness"], "areas": preset.get("areas"),
                                                 "on": True}),
                         "fan_curve": self.preset_curve(preset) or self.trial["before"]["curve"],
                         "active_preset": preset["id"]}
                clear = self.trial["clear_zones"]
            presets = self.store.install_profile(preset, rules, state=state, clear_zones=clear)
            if state is not None:
                self.base_scene, self.base_curve = state["scene"], state["fan_curve"]
                self.active_preset, self.trial = preset["id"], None
                self.engine.set_trial_zones(())
            self.presets_changed()
            return presets

    def end_trial(self, keep_preset=None):
        """Discard the profile being tried, or keep it: keep_preset is the profile it was installed as."""
        with self._lock:
            if self.trial is None:
                return
            if keep_preset:
                scene = validate_scene({"effect": keep_preset["effect"], "params": keep_preset["params"],
                                        "brightness": keep_preset["brightness"], "areas": keep_preset.get("areas"),
                                        "on": True})
                curve = self.preset_curve(keep_preset) or self.trial["before"]["curve"]
                selected = None if keep_preset.get("fan_profile") else keep_preset["id"]
                self.store.save_trial_state({"scene": scene, "fan_curve": curve, "active_preset": selected},
                                             clear_zones=self.trial["clear_zones"])
                self.base_scene, self.base_curve, self.active_preset = scene, curve, selected
                self.trial = None
                self.engine.set_trial_zones(())
                self.apply()
                if keep_preset.get("fan_profile"):
                    try:
                        fancontrol.apply_profile(self.store.settings(), keep_preset["fan_profile"])
                    except Exception as e:
                        log.warning("preset %s: FanControl failed: %s", keep_preset.get("name"), e)
                    else:
                        self.mark_preset(keep_preset["id"])
                return
            before = self.trial["before"]
            curve = before["curve"] if self.curve_points(before["curve"]) else "balanced"
            self._commit_base(before["scene"], curve, before["preset"], end_trial=True)

    def mark_preset(self, preset_id):
        """The current settings were just saved as this preset."""
        with self._lock:
            self.store.save_state(active_preset=preset_id)
            self.active_preset = preset_id

    def presets_changed(self):
        """Presets were edited or removed: rules and the effective scene may point at them."""
        with self._lock:
            if self.active_preset and self.active_preset not in {p["id"] for p in self.store.get_presets()}:
                self.store.save_state(active_preset=None)
                self.active_preset = None
            try:
                self.automations.evaluate()
            except Exception:
                log.exception("automations failed")
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
            owner = ("System power / lock" if self.sleeping or self.shutting_down or
                     (self.locked and settings["lights_off_on_lock"]) else
                     f"Automation: {lr.get('name', lr['id'])}" if lr else
                     "Live draft / trial" if self.trial else "Manual settings")
            self.control.observe("lighting", "Lighting frames", kind="component", owner=owner, stale_after=5.0)
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
        return bool(self.hubs) or any(c["mode"] in ("curve", "steady") for c in self.fan_channels)

    @staticmethod
    def _hub_ports(hub):
        # Every port, not only fans reporting RPM: a fan that stalled at a low speed reads 0 and would otherwise
        # never be sent a faster one, however hot it gets. An empty port ignores the command.
        return list(range(hub.FAN_PORTS))

    def _run_fans(self):
        with self._lock:
            if self._stop.is_set():          # quitting: shutdown() hands the fans back, don't take them again
                return
            # A profile update may detach an owned curve. Resolve its identity and points in one snapshot,
            # with the same controller -> Store lock order as profile transactions, before any hardware I/O.
            cfg = self.store.settings()
            temp = fans.source_temp(self.sensors, cfg["temp_source"])
            curve = self.curve_points(self.effective_curve)
            hottest = fans.source_temp(self.sensors, "max")
        waited = time.monotonic() - self._started
        if temp is not None and not self._temp_seen:
            self._temp_seen = True
            log.info("first temperature %.0f °C, %.0f s after start; fans follow the curve", temp, waited)
        if temp is None and not self._temp_seen and waited < self.STARTUP_GRACE:
            if self.fan_duty is None:
                log.info("no temperatures yet (LibreHardwareMonitor starting?); fans at the curve's lowest speed "
                         "for up to %.0f s", self.STARTUP_GRACE)
            target = fans.lowest_duty(curve)
            why = "lowest speed, no reading yet"
        else:
            target = fans.target_duty(curve, temp, cfg["fan_failsafe_duty"])
            why = "the curve" if temp is not None else f"failsafe {cfg['fan_failsafe_duty']}%, no reading"
        # "full speed at 90 °C" holds for the hottest part, not only the one the curve follows (the coolant stays
        # cool while a processor whose pump has stopped overheats)
        if hottest is not None and hottest >= SAFETY_TEMP:
            target = 100
            why = f"full speed, {hottest:.0f} °C on the hottest part"
        duty = fans.step_toward(self.fan_duty, target, cfg["fan_max_step"])
        self._fan_reason(duty, why, temp)
        self.fan_temp, self.fan_target, self.fan_duty = temp, round(target), duty
        for dev, hub in self._fan_devices():
            key = "fan:" + dev.id
            rule = self.automations.fan_rule
            owner = f"Automation: {rule.get('name', rule['id'])}" if rule else "Airglow curve"
            self.control.observe(key, dev.name + " fans", kind="fans", owner=owner, stale_after=6.0)
            started = time.monotonic()
            self.control.queued(key)
            try:
                hub.poll()
                hub.set_fan_duty({p: duty for p in self._hub_ports(hub)})
                self.control.succeeded(key, duration=time.monotonic() - started)
            except (devices.DeviceError, OSError) as e:
                self.control.failed(key, e)
                log.warning("%s fans: %s", dev.name, e)
                dev.error = str(e)
                dev.close()
        for dev in self.manager.natives.values():
            hub = getattr(dev, "hub", None)
            if hub is not None and dev.online and not getattr(hub, "FAN_PORTS", 0):
                # lighting-only controllers (HUE 2, HUE 2 Ambient, N7 boards) have no fan ports, so the loop above
                # skips them: without this their lost LED-info report is never re-asked and strips stay at 40 LEDs
                try:
                    hub.poll()
                except (devices.DeviceError, OSError) as e:
                    log.warning("%s: %s", dev.name, e)
                    dev.error = str(e)
                    dev.close()
        self._run_lhm_fans(duty, temp, cfg["fan_failsafe_duty"], grace=temp is None and not self._temp_seen
                           and time.monotonic() - self._started < self.STARTUP_GRACE, hottest=hottest)

    def _run_lhm_fans(self, duty, temp, failsafe, grace=False, hottest=None):
        """Motherboard headers, pumps and cooler fans through LibreHardwareMonitor: the ones you chose follow the
        curve's speed or hold their own. While LHM isn't answering nothing is sent (each try would stall this
        loop), and everything is sent again once it's back."""
        if self._stop.is_set():              # quitting: release_fans() already handed these back
            return
        found = lhmfans.channels(self._flat)
        adoption_errors = {}
        try:
            chosen = self.store.adopt_fan_channels(found)
        except (OSError, ValueError) as error:
            chosen = self.store.fan_channels()
            adoption_errors = {ch["id"]: f"Fan setup could not be saved: {error}" for ch in found
                               if ch["id"] not in chosen and not ch["pump"] and ch["rpm"] and ch["rpm"] > 0}
        targets = {}
        for ch in found:
            c = chosen.get(ch["id"])
            if not c or c["mode"] == "bios":
                continue
            if c["mode"] == "curve":
                targets[ch["id"]] = duty
            else:
                # Startup grace waives the missing-reading floor, never the full-speed thermal override.
                minimum = c["duty"] if grace else failsafe
                targets[ch["id"]] = fans.steady_duty(c["duty"], temp, minimum, hottest)
        if self._flat:
            before = dict(getattr(self.lhm_fans, "sent", {}))
            started = time.monotonic()
            self.lhm_fans.apply(targets)
            after = dict(getattr(self.lhm_fans, "sent", {}))
        else:
            self.lhm_fans.resend_all()
            before = after = {}
        errors = {**self.lhm_fans.errors, **adoption_errors}
        self.fan_channels = [{**ch, "mode": (chosen.get(ch["id"]) or {}).get("mode"),
                              "steady": (chosen.get(ch["id"]) or {}).get("duty"), "target": targets.get(ch["id"]),
                              "error": errors.get(ch["id"])} for ch in found]
        present = set()
        for ch in self.fan_channels:
            sid, key = ch["id"], "lhm:" + ch["id"]
            present.add(key)
            held = sid in after
            managed = ch["mode"] in ("curve", "steady")
            owner = ("Airglow" if held and after[sid][0] is not None else "Airglow requested") if managed else (
                "Release pending" if held else "BIOS / device")
            detail = ("No RPM reading; connection is unverified" if ch["rpm"] is None else
                      "0 RPM; may be empty, stopped, or have no tach signal" if ch["rpm"] == 0 else
                      f"{ch['rpm']:,} RPM detected")
            if ch["target"] is not None:
                detail += f"; requested {ch['target']:g}%"
            if ch["duty"] is not None:
                detail += f"; reported {ch['duty']:g}%"
            self.control.observe(key, ch["name"], kind="lhm-fan", owner=owner,
                                 online=True, expected=bool(managed or held), stale_after=45.0,
                                 detail=detail, action="#fans/" + quote(sid, safe=""))
            if errors.get(sid):
                self.control.failed(key, errors[sid])
            elif sid in after and after[sid][0] is not None and before.get(sid) != after[sid]:
                self.control.succeeded(key, duration=time.monotonic() - started)
            elif sid in targets and (sid not in after or after[sid][0] != targets[sid]):
                self.control.queued(key)
            elif sid in before and sid not in after:
                self.control.succeeded(key, duration=time.monotonic() - started)
            elif not managed and not held:
                self.control.clear_failure(key)
        self.control.reconcile("lhm-fan", present)
        self.control.reconcile("fans", {"fan:" + d.id for d, _ in self._fan_devices()})

    def _fan_reason(self, duty, why, temp):
        """Say why the fans are where they are whenever it changes materially, so "they run faster than I set"
        can be answered from the log: which input asked for it."""
        last = getattr(self, "_fan_reason_last", None)
        if last and last[1] == why and abs(duty - last[0]) < 5:
            return
        self._fan_reason_last = (duty, why)
        log.info("fans at %d%% (%s), temperature %s", duty, why,
                 "unknown" if temp is None else f"{temp:.0f} °C")

    def failsafe(self, force=False):
        # Windows' shutdown notification, the shutdown itself and quitting all reach this within a second or two;
        # sending the same speeds again only costs USB traffic while the session is going down.
        now = time.monotonic()
        if not force and now - self._failsafe_at < 2.0:
            return
        self._failsafe_at = now
        duty = max(self.store.settings()["fan_failsafe_duty"], self.fan_duty or 0)
        hottest = fans.source_temp(self.sensors, "max")
        if hottest is not None and hottest >= SAFETY_TEMP:
            duty = 100
        for dev, hub in self._fan_devices():
            try:
                hub.set_fan_duty({p: duty for p in self._hub_ports(hub)})
                log.info("%s fans set to failsafe %d%%", dev.name, duty)
            except (devices.DeviceError, OSError) as e:
                log.error("could not set failsafe fan speed on %s: %s", dev.name, e)
                dev.error = str(e)
                dev.close()
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
        started = time.monotonic()
        if self._read_sensors:
            summary, flat = self._read_sensors(), (self._read_flat() if self._read_flat else [])
        else:
            tree = self.lhm.tree                       # read on the poller's thread, never here
            flat = sensors.flatten(tree) if tree else []
            summary = sensors.summary(flat) if tree else dict(sensors.OFFLINE)
        self.sensors = summary
        self._flat = flat
        online = bool(summary.get("online"))
        self.control.observe("sensors", "Temperature samples", kind="component", online=online, stale_after=6.0)
        if online and self._read_sensors:
            self.control.succeeded("sensors", duration=time.monotonic() - started)
        elif online and self.lhm.last_success is not None and self.lhm.last_success != self._lhm_sample_seen:
            self.control.succeeded("sensors", at=self.lhm.last_success_at, mono=self.lhm.last_success,
                                   duration=self.lhm.last_duration)
            self._lhm_sample_seen = self.lhm.last_success
        return flat

    def _app_readings(self):
        native = [{"id": f"{dev.id}/{i}", **h.fans[i]} for dev, h in self._fan_devices() for i in h.connected_fans()]
        channels = native + [c for c in self.fan_channels
                             if c["mode"] in ("curve", "steady") and not c.get("pump")]
        fan_readings = []
        for key, name, kind, unit, short in (("duty", "Managed fan speed", "control", "%", "Fans"),
                                             ("rpm", "Managed fan RPM", "fan", "RPM", "Fan RPM")):
            measured = [c for c in channels if c.get(key) is not None]
            value = round(sum(c[key] for c in measured) / len(measured)) if measured else None
            reading = sensors._meta("app/fan_" + key, name, "Fans", kind, unit, value, short=short)
            reading["source"] = "Airglow-controlled fans"
            reading["source_ids"] = [(c.get("fan_id") or c["id"]) if key == "rpm" else c["id"] for c in measured]
            fan_readings.append(reading)
        # What an hour at the current power draw costs. total_power is CPU + graphics; the rest of the PC, the PSU's
        # losses and any energy accumulated over time are deliberately not estimated here.
        watts = self.sensors.get("total_power")
        if watts is None:                       # some LHM builds only report the two halves
            parts = [self.sensors.get("cpu_power"), self.sensors.get("gpu_power")]
            watts = round(sum(parts), 1) if all(p is not None for p in parts) else None
        price = self.store.settings().get("energy_price") or 0.0
        cost = round(watts / 1000 * price, 3) if watts is not None else None
        cost_meta = sensors._meta("app/energy_cost", "Cost per hour", "Quick picks", "cost", "€/h", cost, short="Cost")
        cost_meta["source"] = "CPU + GPU"
        cost_meta["needs_lhm"] = True
        return [*fan_readings, cost_meta]

    def _update_readings(self, flat):
        self.readings = sensors.build(flat, self._system.read(), self._app_readings())
        key = tuple((k, m["name"], m["unit"], m.get("source"), m.get("source_id"), tuple(m.get("source_ids") or []),
                     m.get("sensor_name"), m.get("max")) for k, m in self.readings.items())
        if key != self._catalog_key:
            self._catalog_key = key
            self.catalog_rev += 1
        self.readings_history.add(time.time(), {k: m["value"] for k, m in self.readings.items()})

    def readings_for(self, ids):
        r = self.readings
        return {i: (r[i]["value"] if i in r else None) for i in ids}

    # ------------------------------------------------------------ loop
    def _record(self):
        s = self.sensors
        self.history.append({"t": round(time.time()), "cpu": s.get("cpu_temp"), "gpu": s.get("gpu_temp"),
                             "cpu_w": s.get("cpu_power"), "gpu_w": s.get("gpu_power"), "duty": self.fan_duty})
        if not self.cooling.active_summary():
            return
        try:
            self._record_cooling()
            self.cooling.sample_error = None
        except Exception as e:
            message = str(e) or type(e).__name__
            if self.cooling.sample_error != message:
                log.exception("cooling sample failed; fan control continues")
                self.control.event("cooling", "failure", message)
            self.cooling.sample_error = message

    def _record_cooling(self):
        s = self.sensors
        channels = [{"id": f"{dev.id}/{i}", "name": f"{dev.name} · Fan {i + 1}", "mode": "curve",
                     "rpm": hub.fans[i].get("rpm"), "duty": hub.fans[i].get("duty"), "target": self.fan_duty}
                    for dev, hub in self._fan_devices() for i in hub.connected_fans()]
        channels += self.fan_channels
        rpms = [c["rpm"] for c in channels if c.get("rpm") is not None]
        with self._lock:
            cfg = self.store.settings()
            curve = {"name": self.effective_curve, "points": self.curve_points(self.effective_curve),
                     "source": cfg["temp_source"], "failsafe": cfg["fan_failsafe_duty"]}
        self.cooling.sample({"cpu": s.get("cpu_temp"), "gpu": s.get("gpu_temp"), "liquid": s.get("liquid_temp"),
                             "temp": self.fan_temp, "duty": self.fan_duty if self.hubs or
                             any(c.get("mode") == "curve" for c in self.fan_channels) else None,
                             "rpm": sum(rpms) / len(rpms) if rpms else None, "channels": channels, "curve": curve})

    def _apply_device_params(self):
        saved = self.store.device_params()
        for d in self.manager.all().values():
            if d.parameters:
                d.set_params(saved.get(d.id, {}))

    def tick(self):
        started = time.monotonic()
        try:
            self._tick()
        except Exception as e:
            self.control.failed("controller", e)
            raise
        # The light tick between sensor/fan cycles cannot prove a failed output cycle recovered.
        if self._ticks % 2 == 1:
            self.control.succeeded("controller", duration=time.monotonic() - started)

    def _tick(self):
        now = time.time()
        if self._last_tick is not None and now - self._last_tick > 10 and self.sleeping:
            self._on_resume()                         # missed the resume broadcast
        self._last_tick = now
        self._ticks += 1
        with self._lock:
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
                    # Even ticks skip sensors and fans, so they cannot prove a failed fan cycle recovered.
                    if self._ticks % 2 == 1:
                        failures = 0
                except Exception:
                    failures += 1
                    log.exception("controller tick failed (%d in a row)", failures)
                    if failures >= 5:
                        self.failsafe()
                deadline = time.monotonic() + self.TICK
                while not self._stop.is_set():
                    remaining = deadline - time.monotonic()
                    if remaining <= 0:
                        break
                    if self._sensor_ready.wait(remaining):
                        self._sensor_ready.clear()
                        if self._stop.is_set():
                            break
                        try:
                            # Publish first/recovered identity now, without changing fan ramp or automation cadence.
                            self._update_readings(self._poll_sensors())
                        except Exception:
                            log.exception("publishing sensor availability failed")
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
        self.store.ensure_zones([d.info() for d in self.manager.all().values()])
        self._apply_device_params()
        if not self._read_sensors:
            self.lhm.start()
        self.manager.start(self.store.openrgb_enabled)
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
        self._sensor_ready.set()
        try:
            self.end_trial()
        except Exception:
            log.exception("ending the trial failed")
        if self._thread:
            self._thread.join(timeout=self.TICK + 1)
        self.lhm.stop()
        if self.cooling.active_summary():
            try:
                self.cooling.stop(reason="interrupted")
            except Exception:
                log.exception("saving the interrupted recording failed; shutdown continues")
        self.engine.stop()
        self.failsafe()
        self.release_fans()
        self.manager.close()
        if not self.cooling.wait_for_save(1.0):
            log.warning("cooling save is still pending after shutdown; the interrupted marker is retained")
        log.info("controller stopped")

    # ------------------------------------------------------------ views
    def snapshot(self, devs=None):
        fan_devs = self._fan_devices()
        a = self.automations
        rules = {r["id"]: r for r in self.store.get_rules()}
        light_rule, fan_rule = a.light_rule, a.fan_rule   # once each: evaluate() can clear them between readings
        if devs is None:                          # the API passes the list it already built for the same event
            devs = [d.info() for d in self.manager.all().values()]
        trial = self.trial                        # read once: the API thread can end the trial meanwhile
        return {
            "scene": self.base_scene,
            "effective": self.effective_scene,
            "active_preset": self.active_preset,
            "trial": {"name": trial.get("name", ""), "fans": bool(trial.get("fan_points")),
                      "kind": trial.get("kind", "profile")} if trial else None,
            "override": {
                "lighting": light_rule["id"] if light_rule else None,
                "fans": fan_rule["id"] if fan_rule else None,
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
            "catalog_rev": self.catalog_rev,
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
            "control": {**self.control.snapshot(compact=True), "recording": self.cooling.active_summary()},
        }
