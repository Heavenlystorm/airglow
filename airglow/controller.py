"""Orchestrates everything: devices, the lighting engine, automations, sensors and the fans (native fan
controllers, and the motherboard headers and coolers LibreHardwareMonitor drives).

"Base" is what the user set by hand; automations, sleep and lock lay overrides on top. The effective scene
and fan curve are recomputed whenever any of those change.
"""

import collections
import copy
import logging
import secrets
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
    SAMPLE_STALE_AFTER = 5.0

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
        self._fan_channels_lock = threading.Lock()
        self.fan_channels = []                   # what LHM can drive, with what Airglow does with each
        self._system = sensors.SystemSensors()
        self.sensors = dict(sensors.OFFLINE)
        self._readings = {}                       # every sensor by id: {name, group, kind, unit, value}
        self._sampled_at = None
        self.catalog_rev = 0
        self._catalog_key = ()
        self.readings_history = SensorHistory(self.HISTORY)
        self.control = ControlStatus()
        self.control.observe("controller", "Controller cycles", kind="component", stale_after=self.SAMPLE_STALE_AFTER)
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
        self._scene_revision = self._active_scene_revision = 0
        self._scene_session = secrets.token_hex(8)
        self._scene_pending = None
        self._scene_error = None
        self._scene_undo = None
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
    def set_scene(self, patch, *, end_trial_kind=None):
        with self._lock:
            off = patch.get("on") is False and not {"effect", "params", "brightness"}.intersection(patch)
            if not off:
                self._refresh_scene()
                if "expected_revision" in patch and (type(patch["expected_revision"]) is not int or patch["expected_revision"] < 0):
                    raise ValueError("expected_revision must be a non-negative integer")
                if patch.get("expected_session", self._scene_session) != self._scene_session:
                    raise ValueError("Airglow restarted. Refresh before editing the lights.")
                if patch.get("expected_revision", self._scene_revision) != self._scene_revision:
                    raise ValueError("The lighting scene changed. Refresh before editing it again.")
                if patch.get("expected_effect", self.base_scene["effect"]) != self.base_scene["effect"]:
                    raise ValueError("Those parameters belong to a different effect. Refresh before editing.")
                if patch.get("retry"):
                    from .lighting import studio
                    studio.HOST.retry()
            end_trial = bool(self.trial and end_trial_kind and self.trial.get("kind") == end_trial_kind)
            base = self.trial["before"]["scene"] if end_trial else self.base_scene
            curve = self.trial["before"]["curve"] if end_trial else self.base_curve
            if off:
                if self._scene_pending:
                    base = self._scene_pending["before"][0]
                scene = {**base, "on": False}
            else:
                # "areas" is only sent to put back an earlier look exactly (Undo after a profile); null clears them.
                merged = {**base, **{k: v for k, v in patch.items() if k in ("effect", "params", "brightness", "on", "areas")}}
                if "effect" in patch and patch["effect"] != base["effect"] and "params" not in patch:
                    merged["params"] = {}                 # new effect starts from its defaults
                scene = validate_scene(merged)
            preset = self.active_preset
            if any(k in patch for k in ("effect", "params", "brightness")):
                preset = None
            self._commit_base(scene, curve, preset, end_trial=end_trial, force=not off and bool(patch.get("retry")))

    def set_fan_curve(self, name):
        if name not in self.store.curves():
            raise ValueError(f"unknown fan curve '{name}'")
        with self._lock:
            if self.trial is None:
                self.store.save_state(fan_curve=name, active_preset=None)
            self.base_curve, self.active_preset, self._scene_undo = name, None, None
            if self._scene_pending:
                pending = self._scene_pending
                pending["curve"], pending["preset"], pending["allow_undo"] = name, None, False
                pending["before"] = (pending["before"][0], name, None)
                pending.pop("fan_profile", None)
            self.apply()

    def _commit_base(self, scene, curve, preset, *, end_trial=False, force=False):
        """Persist a candidate before publishing it to loops, snapshots or the lighting engine."""
        self.engine.validate_scene_requirements(scene, trial_zones=() if end_trial else None)
        self._scene_revision += 1
        self._scene_error, self._scene_undo = None, None
        previous_pending = self._scene_pending
        before = previous_pending["before"] if previous_pending else (self.base_scene, self.base_curve, self.active_preset)
        self._scene_pending = None
        overridden = self._scene_overridden()
        if (force or scene != before[0]) and scene.get("on", True) and not overridden and (self.trial is None or end_trial):
            before_zones = previous_pending["before_zones"] if previous_pending else self.engine._trial_zones
            if end_trial:
                self.engine.set_trial_zones(())
            self.engine.set_scene(scene, revision=self._scene_revision,
                                  transition_ms=self.store.settings().get("lighting_transition_ms", 350), commit_required=True)
            self.base_scene, self.effective_scene = scene, scene
            self._scene_pending = {"revision": self._scene_revision, "before": before, "scene": scene,
                                   "curve": curve, "preset": preset, "end_trial": end_trial,
                                   "before_zones": before_zones, "config": copy.deepcopy(self.store.config),
                                   "rules": copy.deepcopy(self.store.rules)}
            self.engine.prepare_scene()
            self._refresh_scene(raise_save_error=True)
            self.rev += 1
            return
        # Off is a safety action: cancel the renderer even if its preference cannot be saved.
        if not scene.get("on", True) and not overridden:
            self.engine.set_scene(scene, revision=self._scene_revision)
            self.effective_scene = scene
        if self.trial is None or end_trial:
            try:
                self.store.save_state(scene=scene, fan_curve=curve, active_preset=preset)
            except Exception as exc:
                self._scene_error = f"Could not save the scene: {exc}"
                self.base_scene, self.base_curve, self.active_preset = before
                if not scene.get("on", True) and not overridden:
                    self.base_scene = scene
                    self._scene_error = f"Lights are off, but that preference could not be saved: {exc}"
                elif previous_pending:
                    self.engine.set_trial_zones(previous_pending["before_zones"])
                    self.engine.set_scene(before[0], revision=self._active_scene_revision,
                                          transition_ms=self.store.settings().get("lighting_transition_ms", 350))
                    self.engine.prepare_scene()
                    self.effective_scene = before[0]
                self.rev += 1
                raise
        if end_trial and self.trial is not None:
            self.trial = None
            self.engine.set_trial_zones(())
        self.base_scene, self.base_curve, self.active_preset = scene, curve, preset
        if not overridden:
            self._active_scene_revision = self._scene_revision
        self.apply()

    def _scene_overridden(self):
        return bool(self.automations.light_rule or self._power_holds_lights_off())

    def _power_holds_lights_off(self):
        return self.sleeping or self.shutting_down or (self.locked and self.store.settings()["lights_off_on_lock"])

    def _refresh_scene(self, raise_save_error=False):
        """Only the owning command may persist or roll back its prepared candidate."""
        with self._lock:
            pending = self._scene_pending
            if not pending or pending["revision"] != self._scene_revision:
                return
            if self._power_holds_lights_off():
                self.apply()
                return
            if self.engine.scene_revision != pending["revision"]:
                self._scene_pending = None
                return
            if self.engine.scene_state == "pending":
                return
            error, save_error = self.engine.scene_error, None
            if self.engine.scene_state == "ready":
                try:
                    self.store.save_state(scene=pending["scene"], fan_curve=pending["curve"], active_preset=pending["preset"])
                except Exception as exc:
                    error = f"Could not save the scene: {exc}"
                    save_error = exc
                else:
                    self.base_scene, self.base_curve, self.active_preset = pending["scene"], pending["curve"], pending["preset"]
                    if pending["end_trial"]:
                        self.trial = None
                        self.engine.set_trial_zones(())
                    self._active_scene_revision = self._scene_revision
                    self.engine.commit_scene(self._scene_revision)
                    self._scene_undo = {"scene": pending["before"][0], "revision": self._scene_revision,
                                        "curve": self.base_curve, "zones": self.store.zones_rev,
                                        "config": copy.deepcopy(self.store.config), "rules": copy.deepcopy(self.store.rules)}
                    if pending.get("allow_undo") is False or pending["config"] != self.store.config or pending["rules"] != self.store.rules:
                        self._scene_undo = None
                    self._scene_pending = None
                    self.apply()
                    if pending.get("fan_profile"):
                        try:
                            fancontrol.apply_profile(self.store.settings(), pending["fan_profile"])
                            self.mark_preset(pending["profile_id"])
                        except Exception as exc:
                            self._scene_error = f"Lighting applied, but FanControl failed: {exc}"
                    return
            self._scene_pending = None
            self._scene_error = error or "The scene could not start"
            self.base_scene, self.base_curve, self.active_preset = pending["before"]
            self.effective_scene = dict(self.base_scene)
            self.engine.set_trial_zones(pending["before_zones"])
            self.engine.set_scene(self.base_scene, revision=self._active_scene_revision,
                                  transition_ms=self.store.settings().get("lighting_transition_ms", 350))
            self.engine.prepare_scene()
            self.rev += 1
            if save_error is not None and raise_save_error:
                raise save_error

    def scene_status(self):
        self._refresh_scene()
        undo = self._scene_undo
        error = self._scene_error
        if error and self.engine.scene_state == "failed":
            error += "; the previous scene also could not start. Last valid lights are held. Retry when ready."
        overridden = self._scene_overridden()
        waiting = self._scene_pending or (self.engine.scene_state == "pending" and not overridden)
        if not error and self.engine.scene_state == "failed" and not overridden:
            error = self.engine.scene_error
        if not error:
            error = self.engine.scene_capacity_error
        if not waiting and not error and not overridden:
            self._active_scene_revision = self._scene_revision
        return {"session": self._scene_session, "desired_revision": self._scene_revision, "active_revision": self._active_scene_revision,
                "renderer_revision": self.engine.scene_generation,
                "status": "failed" if error else "pending" if waiting else "ready",
                "error": error, "overridden": overridden,
                "undo_available": bool(undo and undo["revision"] == self._scene_revision and
                                       undo["curve"] == self.base_curve and undo["zones"] == self.store.zones_rev and
                                       undo["config"] == self.store.config and undo["rules"] == self.store.rules and
                                       not self._scene_overridden() and not self.trial)}

    def undo_scene(self, expected_revision=None, expected_session=None):
        with self._lock:
            if expected_session is not None and expected_session != self._scene_session:
                raise ValueError("Airglow restarted. This lighting change can no longer be undone")
            if expected_revision is not None and (type(expected_revision) is not int or expected_revision != self._scene_revision):
                raise ValueError("A newer lighting change replaced this one. It cannot be undone here")
            if not self.scene_status()["undo_available"]:
                raise ValueError("This lighting change can no longer be undone")
            scene = self._scene_undo["scene"]
            self._commit_base(scene, self.base_curve, None)

    def scene_response(self):
        with self._lock:
            status = self.scene_status()
            effective, wind_down = self._visible_scene()
            return {"scene": copy.deepcopy(self.base_scene), "effective": copy.deepcopy(effective),
                    "scene_status": status, "wind_down": wind_down}

    def _visible_scene(self):
        wind = self.engine.wind_down()
        scene = self.effective_scene
        if wind["phase"] != "idle":
            scene = {**scene, "brightness": min(scene["brightness"], wind["cap"])}
        return scene, wind

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
            pending = self._scene_pending
            steps.append({"step": "lighting", "ok": not bool(self._scene_error), "pending": bool(pending)})
            if self.preset_curve(preset):
                steps.append({"step": "fans", "ok": True, "pending": bool(pending)})
            if preset.get("fan_profile"):
                if pending:
                    pending.update(fan_profile=preset["fan_profile"], profile_id=preset["id"])
                    steps.append({"step": "FanControl", "ok": True, "pending": True})
                else:
                    step("FanControl", lambda: fancontrol.apply_profile(self.store.settings(), preset["fan_profile"]))
                    if all(s["ok"] for s in steps):
                        self.mark_preset(preset["id"])
        return steps

    # ------------------------------------------------------------ fan curves, including a profile's own
    @staticmethod
    def preset_curve(preset):
        """The curve a profile switches to: its own ("profile:<id>"), a named one, or None to leave fans alone."""
        return automation.preset_curve(preset)

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
            if self._scene_pending:
                self.base_scene, self.base_curve, self.active_preset = self._scene_pending["before"]
                self._scene_pending = None
            self._scene_revision += 1
            self._scene_undo, self._scene_error = None, None
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
            self._scene_undo = None

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
            safety_off = self._power_holds_lights_off()
            overridden = safety_off or bool(self.automations.light_rule)
            if overridden or self.automations.fan_rule:
                self._scene_undo = None
            pending = self._scene_pending if overridden else None
            if pending:
                self._scene_pending = None
                if safety_off:
                    # Safety cancels renderer preparation, including its unsaved settings.
                    self.base_scene, self.base_curve, self.active_preset = pending["before"]
                    self.engine.set_trial_zones(pending["before_zones"])
                    self._scene_revision += 1
                else:
                    try:
                        self.store.save_state(scene=pending["scene"], fan_curve=pending["curve"], active_preset=pending["preset"])
                    except Exception as exc:
                        self.base_scene, self.base_curve, self.active_preset = pending["before"]
                        self.engine.set_trial_zones(pending["before_zones"])
                        self._scene_error = f"Could not save the inactive lighting selection: {exc}"
                    else:
                        self.base_curve, self.active_preset = pending["curve"], pending["preset"]
                        if pending["end_trial"]:
                            self.trial = None
                            self.engine.set_trial_zones(())
            presets = {p["id"]: p for p in self.store.get_presets()}
            lr, fr = self.automations.light_rule, self.automations.fan_rule
            scene, curve = automation.rule_outputs(self.base_scene, self.base_curve, lr, fr, presets, self.curve_points)
            if safety_off:
                scene["on"] = False
            if scene != self.effective_scene or pending:
                # The override owns its frames even when its output is identical to the pending scene.
                self.engine.set_scene(scene, transition_ms=self.store.settings().get("lighting_transition_ms", 350))
                self.effective_scene = scene
            self.effective_curve = curve
            owner = ("System power / lock" if safety_off else
                     f"Automation: {lr.get('name', lr['id'])}" if lr else
                     "Live draft / trial" if self.trial else "Manual settings")
            self.control.observe("lighting", "Lighting frames", kind="component", owner=owner, stale_after=5.0)
            self.rev += 1

    # ------------------------------------------------------------ power events
    def _on_suspend(self):
        log.info("system is going to sleep")
        if self.store.settings()["lights_off_on_sleep"]:
            self.sleeping = True
            try:
                self.apply()
            finally:
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
        try:
            if self.store.settings()["lights_off_on_sleep"]:
                # keep them dark: the engine runs until the process ends, and its next frame would light them again
                self.shutting_down = True
                try:
                    self.apply()
                finally:
                    self.engine.blackout()
        finally:
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
            sample = self.sensors if self._read_sensors or self.lhm.fresh_tree is not None else sensors.OFFLINE
            temp = fans.source_temp(sample, cfg["temp_source"])
            curve = self.curve_points(self.effective_curve)
            hottest = fans.source_temp(sample, "max")
        waited = time.monotonic() - self._started
        if temp is not None and not self._temp_seen:
            self._temp_seen = True
            log.info("first temperature %.0f °C, %.0f s after start; fans follow the curve", temp, waited)
        target = fans.target_duty(curve, temp, cfg["fan_failsafe_duty"])
        if temp is None:
            target = max(target, self.fan_duty or 0)  # a warm restart or missing sample cannot prove it is safe to slow down
        why = "the curve" if temp is not None else f"failsafe {target}%, no reading"
        # "full speed at 90 °C" holds for the hottest part, not only the one the curve follows (the coolant stays
        # cool while a processor whose pump has stopped overheats)
        if hottest is not None and hottest >= SAFETY_TEMP:
            target = 100
            why = f"full speed, {hottest:.0f} °C on the hottest part"
        duty = round(target) if temp is None else fans.step_toward(self.fan_duty, target, cfg["fan_max_step"])
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
        # An ownership save must follow any writes and status publication based on the previous choice.
        # Slow LHM requests must not take the controller lock used by lighting and state reads.
        with self._fan_channels_lock:
            self._run_lhm_fans(duty, temp, cfg["fan_failsafe_duty"], hottest=hottest)

    def _run_lhm_fans(self, duty, temp, failsafe, hottest=None):
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
                targets[ch["id"]] = fans.steady_duty(c["duty"], temp, failsafe, hottest)
        if self._flat:
            before = dict(getattr(self.lhm_fans, "sent", {}))
            started = time.monotonic()
            self.lhm_fans.apply(targets)
            after = dict(getattr(self.lhm_fans, "sent", {}))
        else:
            self.lhm_fans.resend_all()
            if not self._read_sensors and self.lhm.tree is not None:
                self.lhm_fans.failsafe(max(duty, failsafe))
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

    def set_fan_channel(self, sid, cfg):
        """Publish the saved choice now. The next hardware tick applies it or hands the channel back."""
        with self._fan_channels_lock:
            choice = self.store.set_fan_channel(sid, cfg)
            self.fan_channels = [
                {**channel, "mode": choice["mode"], "steady": choice.get("duty"), "target": None}
                if channel["id"] == sid else channel for channel in self.fan_channels
            ]
            return self.fan_channels

    def release_fans(self):
        """Quitting: fans driven through LHM go back to the BIOS or the cooler's own curve."""
        try:
            with self._fan_channels_lock:
                self.lhm_fans.release_all()
        except Exception:
            log.exception("could not hand fans back to their own control")

    # ------------------------------------------------------------ sensors
    def _poll_sensors(self):
        started = time.monotonic()
        if self._read_sensors:
            summary, flat = self._read_sensors(), (self._read_flat() if self._read_flat else [])
        else:
            tree = self.lhm.fresh_tree                 # an old snapshot cannot prove a live temperature
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

    def _update_readings(self, flat, sampled_at=None):
        if sampled_at is None:
            sampled_at = time.monotonic()
        readings = sensors.build(flat, self._system.read(), self._app_readings())
        key = tuple((k, m["name"], m["unit"], m.get("source"), m.get("source_id"), tuple(m.get("source_ids") or []),
                     m.get("sensor_name"), m.get("max")) for k, m in readings.items())
        with self._lock:
            if key != self._catalog_key:
                self._catalog_key = key
                self.catalog_rev += 1
            self._readings, self._sampled_at = readings, sampled_at
        stale = self._sample_stale(sampled_at)
        self.readings_history.add(time.time(), {k: None if stale else m["value"] for k, m in readings.items()})

    def _sample_stale(self, sampled_at):
        return sampled_at is not None and time.monotonic() - sampled_at > self.SAMPLE_STALE_AFTER

    def _current_readings(self):
        with self._lock:
            readings, revision = self._readings, self.catalog_rev
            stale = self._sample_stale(self._sampled_at)
            online = self.lhm.fresh_tree is not None if stale else bool(self.sensors.get("online"))
        return stale, readings, revision, online

    @property
    def readings(self):
        stale, readings, _, _ = self._current_readings()
        return {i: {**m, "value": None} for i, m in readings.items()} if stale else readings

    def sensor_catalog(self):
        stale, readings, revision, online = self._current_readings()
        return {"sensors": [{**m, "value": None} if stale else m for m in readings.values()],
                "lhm_online": online, "revision": revision, "telemetry_stale": stale}

    # ------------------------------------------------------------ loop
    def _record(self, sampled_at=None):
        stale = self._sample_stale(sampled_at)
        s = {} if stale else self.sensors
        self.history.append({"t": round(time.time()), "cpu": s.get("cpu_temp"), "gpu": s.get("gpu_temp"),
                             "cpu_w": s.get("cpu_power"), "gpu_w": s.get("gpu_power"),
                             "duty": None if stale else self.fan_duty})
        if not self.cooling.active_summary():
            return
        try:
            self._record_cooling(stale=stale)
            self.cooling.sample_error = None
        except Exception as e:
            message = str(e) or type(e).__name__
            if self.cooling.sample_error != message:
                log.exception("cooling sample failed; fan control continues")
                self.control.event("cooling", "failure", message)
            self.cooling.sample_error = message

    def _record_cooling(self, stale=False):
        s = {} if stale else self.sensors
        channels = [{"id": f"{dev.id}/{i}", "name": f"{dev.name} · Fan {i + 1}", "mode": "curve",
                     "rpm": hub.fans[i].get("rpm"), "duty": hub.fans[i].get("duty"), "target": self.fan_duty}
                    for dev, hub in self._fan_devices() for i in hub.connected_fans()]
        channels += self.fan_channels
        if stale:
            channels = [{**channel, "rpm": None, "duty": None, "target": None} for channel in channels]
        rpms = [c["rpm"] for c in channels if c.get("rpm") is not None]
        with self._lock:
            cfg = self.store.settings()
            curve = {"name": self.effective_curve, "points": self.curve_points(self.effective_curve),
                     "source": cfg["temp_source"], "failsafe": cfg["fan_failsafe_duty"]}
        self.cooling.sample({"cpu": s.get("cpu_temp"), "gpu": s.get("gpu_temp"), "liquid": s.get("liquid_temp"),
                             "temp": None if stale else self.fan_temp, "duty": self.fan_duty if not stale and
                             (self.hubs or any(c.get("mode") == "curve" for c in self.fan_channels)) else None,
                             "rpm": sum(rpms) / len(rpms) if rpms else None, "channels": channels, "curve": curve,
                             "readings": {} if stale else self.readings})

    def _apply_device_params(self):
        saved = self.store.device_params()
        for d in self.manager.all().values():
            if d.parameters:
                d.set_params(saved.get(d.id, {}))

    def tick(self):
        started = time.monotonic()
        self._refresh_scene()
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
            sampled_at = time.monotonic()  # slow fan I/O cannot renew an old sensor sample
            self._run_fans()
            self._update_readings(flat, sampled_at)
            self._record(sampled_at)

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
        steps = []
        if self._power:
            steps.append(("stopping power events", self._power.stop))
        steps.append(("ending the trial", self.end_trial))
        thread = self._thread
        if thread and thread.ident is not None and thread is not threading.current_thread():
            steps.append(("joining the controller loop", lambda: thread.join(timeout=self.TICK + 1)))
        steps.extend((
            ("stopping the sensor poller", self.lhm.stop),
            ("saving the interrupted recording", self._stop_recording),
            ("stopping the lighting engine", self.engine.stop),
            ("applying fan failsafe", self.failsafe),
            ("releasing fans", self.release_fans),
            ("closing devices", self.manager.close),
            ("waiting for the cooling recording", self._wait_for_recording),
        ))
        for description, stop in steps:
            try:
                stop()
            except Exception:
                log.exception("%s failed during shutdown", description)
        log.info("controller stopped")

    def _stop_recording(self):
        if self.cooling.active_summary():
            self.cooling.stop(reason="interrupted")

    def _wait_for_recording(self):
        if not self.cooling.wait_for_save(1.0):
            log.warning("cooling save is still pending after shutdown; the interrupted marker is retained")

    # ------------------------------------------------------------ views
    def snapshot(self, devs=None, reading_ids=None):
        with self._lock:
            scene_status = self.scene_status()
            scene, active_preset = self.base_scene, self.active_preset
            effective, wind_down = self._visible_scene()
        fan_devs = self._fan_devices()
        a = self.automations
        rules = {r["id"]: r for r in self.store.get_rules()}
        light_rule, fan_rule = a.light_rule, a.fan_rule   # once each: evaluate() can clear them between readings
        if devs is None:                          # the API passes the list it already built for the same event
            devs = [d.info() for d in self.manager.all().values()]
        trial = self.trial                        # read once: the API thread can end the trial meanwhile
        stale, readings, revision, online = self._current_readings()
        summary, channels = self.sensors, self.fan_channels
        if stale:
            summary = {**sensors.OFFLINE, "online": online,
                       "cpu_name": summary.get("cpu_name"), "gpu_name": summary.get("gpu_name")}
            channels = [{**c, "rpm": None, "duty": None, "target": None} for c in channels]
        return {
            "scene": scene,
            "scene_status": scene_status,
            "effective": effective,
            "wind_down": wind_down,
            "active_preset": active_preset,
            "trial": {"name": trial.get("name", ""), "fans": bool(trial.get("fan_points")),
                      "kind": trial.get("kind", "profile")} if trial else None,
            "override": {
                "lighting": light_rule["id"] if light_rule else None,
                "fans": fan_rule["id"] if fan_rule else None,
                "active": sorted(a.active & set(rules)),
                "paused_until": a.paused_until if a.paused else None,
                "locked": self.locked and self.store.settings()["lights_off_on_lock"],
                "power": self.sleeping or self.shutting_down,
            },
            "fans": {"curve": self.effective_curve, "base_curve": self.base_curve, "temp": None if stale else self.fan_temp,
                     "target": None if stale else self.fan_target, "duty": None if stale else self.fan_duty,
                     "failsafe": None if stale else self.fan_temp is None,
                     "ports": [] if stale else [{"port": i + 1, "device": d.name, **h.fans[i]}
                               for d, h in fan_devs for i in h.connected_fans()],
                     "hub_online": bool(fan_devs),
                     "channels": channels},
            "sensors": summary,
            "telemetry_stale": stale,
            "catalog_rev": revision,
            **({"readings": {i: readings[i]["value"] if not stale and i in readings else None for i in reading_ids}}
               if reading_ids is not None else {}),
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
