"""System tray icon: open, presets, fan curve, lights toggle, quit. The icon takes on the colour of your lights and
the tooltip shows the readings pinned on the Overview."""

import logging
import threading

import pystray

from . import APP_NAME, automation, brand, sensors

log = logging.getLogger(__name__)


def tray_tint(rgb):
    """Dim colours are unreadable in the tray: scale so the brightest channel is at least 170."""
    peak = max(rgb) or 1
    return [round(c * 170 / peak) for c in rgb] if peak < 170 else list(rgb)


def curve_label(name, presets, trial=None):
    """A fan curve as people know it: "Silent", a profile's own curve by the profile's name, or the one being tried."""
    if name == "trial":
        return f"{trial['name']}'s" if trial and trial.get("name") else "Trial"
    if name.startswith("profile:"):
        return next((p["name"] for p in presets if p["id"] == name[8:]), "Profile")
    return name.capitalize()


class Tray:
    def __init__(self, ctrl, store, show_window, quit_app, updater=None):
        self.ctrl, self.store, self.updater = ctrl, store, updater
        self.show_window, self.quit_app = show_window, quit_app
        self._seen_rev = None
        self._icon_key = None
        self._stop = threading.Event()
        self.icon = pystray.Icon("Airglow", self._image(), "Airglow", self._menu())

    def _current_rgb(self):
        """Average colour of what the LEDs show right now (so a rainbow tints the icon as it moves)."""
        if not self.ctrl.effective_scene.get("on", True):
            return None
        total, n = [0, 0, 0], 0
        for hexs in list(self.ctrl.engine.frames.values())[:6]:
            for i in range(0, min(len(hexs), 6 * 24), 6):
                total[0] += int(hexs[i:i + 2], 16)
                total[1] += int(hexs[i + 2:i + 4], 16)
                total[2] += int(hexs[i + 4:i + 6], 16)
                n += 1
        return [round(v / n) for v in total] if n else None

    def _image(self):
        rgb = self._current_rgb()
        rgb = tray_tint(rgb) if rgb and max(rgb) > 8 else [120, 124, 130]
        self._icon_key = tuple(v // 12 for v in rgb)
        return brand.mark(rgb)

    def _tooltip(self):
        """App name, the readings pinned on the Overview, and the fans."""
        c = self.ctrl
        f = self.store.settings()["temp_unit"] == "F"
        lines = [APP_NAME]
        readings = c.readings
        pins = [readings[p] for p in self.store.get_overview()["pins"] if p in readings]
        if pins:
            lines.append("   ".join(f"{m.get('short', m['name'])} {sensors.fmt(m, m['value'], f)}" for m in pins[:2]))
            if len(pins) > 2:
                lines.append("   ".join(f"{m.get('short', m['name'])} {sensors.fmt(m, m['value'], f)}" for m in pins[2:4]))
        if c.fan_duty is not None and c.drives_fans:
            curve = curve_label(c.effective_curve, self.store.get_presets(), c.trial)
            lines.append(f"{curve} fans at {c.fan_duty}%")
        if self.updater and self.updater.state == "ready":
            lines.insert(1, f"Update {self.updater.latest} ready, applied on restart")   # before the cut at 127
        return "\n".join(lines)[:127]   # Windows tooltip limit

    def _menu(self):
        def preset_action(pid):
            def act(icon, item):
                preset = next((p for p in self.store.get_presets() if p["id"] == pid), None)
                if preset:
                    self.ctrl.apply_preset(preset)
            return act

        def curve_action(name):
            def act(icon, item):
                self.ctrl.set_fan_curve(name)
            return act

        def preset_items():
            presets = self.store.get_presets()
            if not presets:
                return [pystray.MenuItem("No presets yet", None, enabled=False)]
            return [pystray.MenuItem(p["name"], preset_action(p["id"]),
                                     checked=lambda item, pid=p["id"]: self.ctrl.active_preset == pid, radio=True)
                    for p in presets]

        def curve_items():
            return [pystray.MenuItem(name.capitalize(), curve_action(name),
                                     checked=lambda item, n=name: self.ctrl.base_curve == n, radio=True)
                    for name in self.store.curves()]

        def toggle_lights(icon, item):
            self.ctrl.set_scene({"on": not self.ctrl.base_scene.get("on", True)})

        def lights_text(item):
            return "Lights (held off by an automation)" if self.held_off() else "Lights"

        def pause_for(minutes):
            def act(icon, item):
                a = self.ctrl.automations
                a.pause(minutes() if callable(minutes) else minutes)
                a.evaluate()
                self.ctrl.apply()
            return act

        paused = lambda item: self.ctrl.automations.paused       # noqa: E731
        pause_menu = pystray.Menu(
            pystray.MenuItem("Resume automations", pause_for(0), visible=paused),
            pystray.MenuItem("For 15 minutes", pause_for(15)),
            pystray.MenuItem("For an hour", pause_for(60)),
            pystray.MenuItem("Until 06:00", pause_for(lambda: automation.minutes_until(6))),
        )

        return pystray.Menu(
            pystray.MenuItem("Open Airglow", lambda icon, item: self.show_window(), default=True),
            pystray.Menu.SEPARATOR,
            pystray.MenuItem("Presets", pystray.Menu(preset_items)),
            pystray.MenuItem("Fan curve", pystray.Menu(curve_items)),
            pystray.MenuItem(lights_text, toggle_lights, checked=lambda item: self.ctrl.base_scene.get("on", True)),
            pystray.MenuItem(lambda item: "Automations paused" if self.ctrl.automations.paused else "Pause automations",
                             pause_menu, visible=lambda item: bool(self.store.get_rules())),
            pystray.Menu.SEPARATOR,
            pystray.MenuItem("Quit Airglow", lambda icon, item: self.quit_app()),
        )

    def held_off(self):
        """Lights are on by your choice, but an automation, sleep or the lock is keeping them dark."""
        c = self.ctrl
        return c.base_scene.get("on", True) and not c.effective_scene.get("on", True)

    def refresh(self):
        c = self.ctrl
        rgb = self._current_rgb()
        key = tuple(v // 12 for v in tray_tint(rgb)) if rgb and max(rgb) > 8 else (10, 10, 10)
        if key != self._icon_key:
            self.icon.icon = self._image()
        self.icon.title = self._tooltip()
        rev = (c.rev, len(self.store.get_presets()), c.automations.paused)
        if rev != self._seen_rev:
            self._seen_rev = rev
            self.icon.update_menu()

    def notify(self, message):
        try:
            self.icon.notify(message, "Airglow")
        except Exception as e:  # notifications are a nicety
            log.debug("tray notify failed: %s", e)

    def _loop(self):
        while not self._stop.wait(2.0):
            try:
                self.refresh()
            except Exception:
                log.exception("tray refresh failed")

    def start(self):
        def setup(icon):
            icon.visible = True
            threading.Thread(target=self._loop, name="tray-refresh", daemon=True).start()
        threading.Thread(target=lambda: self.icon.run(setup=setup), name="tray", daemon=True).start()

    def stop(self):
        self._stop.set()
        try:
            self.icon.stop()
        except Exception:
            pass
