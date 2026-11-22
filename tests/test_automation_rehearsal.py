import copy
import unittest
from threading import RLock
from types import SimpleNamespace
from unittest.mock import Mock

from airglow import api, automation as A


def rule(rid, when, then, enabled=True):
    return A.validate_rule({"id": rid, "when": when, "then": then, "enabled": enabled})


SCENE = {"on": True, "effect": "solid", "brightness": 36, "params": {"color": "#772233"}}
RULES = [
    rule("night", {"type": "time", "start": "23:00", "end": "07:00", "days": [6]}, {"type": "lights_off"}),
    rule("game", {"type": "app_focused", "exe": "Game.exe"}, {"type": "preset", "preset": "gaming"}),
    rule("hot", {"type": "temp_above", "source": "max", "temp": 80}, {"type": "fan_curve", "curve": "performance"}),
    rule("idle", {"type": "idle", "minutes": 3}, {"type": "dim", "brightness": 80}),
    rule("bg", {"type": "app_running", "exe": "game.exe"}, {"type": "lights_off"}),
    rule("locked", {"type": "locked"}, {"type": "fan_curve", "curve": "silent"}, False),
]
PRESETS = {"gaming": {"id": "gaming", "name": "Game profile", "effect": "comet", "brightness": 89, "params": {"speed": 20},
                      "areas": {"front": {"on": False}}, "fan_points": [[30, 40], [80, 90]]}}


def simulate(**inputs):
    return A.rehearse({"weekday": 0, "clock": "00:15", **inputs}, RULES, PRESETS, SCENE, "balanced", lambda c: True)


class RehearsalTests(unittest.TestCase):
    def test_overnight_boundary_per_domain_partial_profile_win_and_return(self):
        events = dict(foreground="GAME.exe", cpu=83, locked=True)
        first = simulate(**events)
        self.assertEqual(first["active"], ["bg", "game", "hot", "night"])
        self.assertEqual(first["winners"], {"lighting": "night", "fans": "game"})
        self.assertFalse(first["scene"]["on"])
        self.assertEqual(first["curve"], "profile:gaming")
        self.assertEqual(first["rows"][1]["targets"], ("lighting", "fans"))
        self.assertFalse(first["rows"][-1]["active"])
        end = simulate(clock="07:00", **events)
        self.assertEqual(end["winners"], {"lighting": "game", "fans": "game"})
        self.assertEqual(end["scene"], {"effect": "comet", "brightness": 89, "params": {"speed": 20}, "on": True, "areas": PRESETS["gaming"]["areas"]})
        idle = simulate(clock="08:00", idle=3)
        self.assertEqual(idle["winners"]["lighting"], "idle")
        self.assertEqual(idle["scene"]["brightness"], 36)
        self.assertEqual(simulate(clock="08:00")["scene"], SCENE)
        self.assertEqual(simulate(clock="08:00")["curve"], "balanced")

    def test_temperature_data_and_history_are_exclusively_from_the_hypothetical_sequence(self):
        self.assertEqual(simulate(cpu=None, gpu=None, liquid=95)["rows"][2]["temperature"], None)
        self.assertNotIn("hot", simulate(cpu=79)["active"])
        hot = simulate(cpu=80)
        self.assertIn("hot", hot["active"])
        kept = simulate(cpu=77, previous=hot["active"])
        self.assertEqual(kept["rows"][2]["threshold"], 77)
        self.assertIn("hot", kept["active"])
        self.assertNotIn("hot", simulate(cpu=76.9, previous=kept["active"])["active"])
        self.assertNotIn("hot", simulate(cpu=None, previous=hot["active"])["active"])
        running = simulate(clock="07:01", running=["gAmE.exe"])
        self.assertIn("bg", running["active"])
        self.assertNotIn("game", running["active"])

    def test_route_reads_configured_state_without_reading_signals_or_controlling_hardware(self):
        store = Mock(spec=["get_rules", "get_presets"])
        store.get_rules.return_value = copy.deepcopy(RULES)
        store.get_presets.return_value = copy.deepcopy(list(PRESETS.values()))
        ctrl = SimpleNamespace(_lock=RLock(), base_scene=copy.deepcopy(SCENE), base_curve="silent", curve_points=lambda c: True)
        server = SimpleNamespace(ctrl=ctrl, store=store, hooks={})
        before = copy.deepcopy((ctrl.base_scene, store.get_rules.return_value, store.get_presets.return_value))
        result = api.route(server, "POST", "/api/automations/rehearse", {"weekday": 0, "clock": "01:00"})
        self.assertEqual(result["base"], {"scene": SCENE, "curve": "silent"})
        self.assertEqual(result["winners"], {"lighting": "night", "fans": None})
        self.assertEqual(before, (ctrl.base_scene, store.get_rules.return_value, store.get_presets.return_value))
        self.assertEqual(ctrl.base_curve, "silent")

    def test_malformed_signals_and_nonfinite_or_boolean_readings_are_refused(self):
        for change in [{"weekday": False}, {"weekday": 7}, {"clock": "24:01"}, {"cpu": float("nan")},
                       {"gpu": True}, {"liquid": -300}, {"locked": 1}, {"idle": -1},
                       {"running": "game.exe"}, {"foreground": "folder/game.exe"}, {"previous": [42]}]:
            with self.subTest(change=change), self.assertRaises(ValueError):
                simulate(**change)
