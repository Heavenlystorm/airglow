import time
import unittest

from airglow import automation as A
from tests.fakes import signals


def at(hour, minute, wday):
    return time.struct_time((2026, 10, 12, hour, minute, 0, wday, 285, -1))


def rule(rid, when, then, enabled=True):
    return {"id": rid, "enabled": enabled, "when": when, "then": then}


class ValidationTests(unittest.TestCase):
    def test_valid_rules(self):
        r = A.validate_rule({"id": "a", "when": {"type": "app_running", "exe": "Cyberpunk2077.exe"},
                             "then": {"type": "preset", "preset": "gaming"}}, {"gaming"})
        self.assertTrue(r["enabled"])
        r = A.validate_rule({"id": "b", "when": {"type": "time", "start": "23:00", "end": "07:00"},
                             "then": {"type": "lights_off"}})
        self.assertEqual(r["when"]["days"], list(range(7)))

    def test_invalid_rules_have_readable_messages(self):
        cases = [
            ({"id": "a", "when": {"type": "app_running", "exe": "notepad"}, "then": {"type": "lights_off"}}, "Cyberpunk2077.exe"),
            ({"id": "a", "when": {"type": "time", "start": "25:00", "end": "07:00"}, "then": {"type": "lights_off"}}, "23:00"),
            ({"id": "a", "when": {"type": "idle", "minutes": 0}, "then": {"type": "lights_off"}}, "1 to 240"),
            ({"id": "a", "when": {"type": "locked"}, "then": {"type": "preset", "preset": "x"}}, "preset that exists"),
            ({"id": "a", "when": {"type": "nope"}, "then": {"type": "lights_off"}}, "start this automation"),
        ]
        for r, msg in cases:
            with self.assertRaisesRegex(ValueError, msg):
                A.validate_rule(r, set(), {"silent"})


class ConditionTests(unittest.TestCase):
    def test_time_window_same_day(self):
        self.assertTrue(A._in_window(at(9, 30, 0), "09:00", "17:00", [0]))
        self.assertFalse(A._in_window(at(17, 0, 0), "09:00", "17:00", [0]))
        self.assertFalse(A._in_window(at(10, 0, 5), "09:00", "17:00", [0]))

    def test_time_window_across_midnight_belongs_to_start_day(self):
        friday_only = [4]
        self.assertTrue(A._in_window(at(23, 30, 4), "23:00", "07:00", friday_only))
        self.assertTrue(A._in_window(at(3, 0, 5), "23:00", "07:00", friday_only))      # Saturday 03:00
        self.assertFalse(A._in_window(at(3, 0, 4), "23:00", "07:00", friday_only))     # Friday 03:00 (Thursday's night)

    def test_each_condition(self):
        sig = signals(idle=700, foreground="game.exe", running=frozenset({"game.exe"}), locked=True)
        sensors = {"cpu_temp": 85, "gpu_temp": 60}
        self.assertTrue(A.is_active(rule("a", {"type": "app_running", "exe": "Game.exe"}, {}), sig, sensors))
        self.assertTrue(A.is_active(rule("a", {"type": "app_focused", "exe": "GAME.exe"}, {}), sig, sensors))
        self.assertTrue(A.is_active(rule("a", {"type": "idle", "minutes": 10}, {}), sig, sensors))
        self.assertFalse(A.is_active(rule("a", {"type": "idle", "minutes": 15}, {}), sig, sensors))
        self.assertTrue(A.is_active(rule("a", {"type": "locked"}, {}), sig, sensors))
        self.assertTrue(A.is_active(rule("a", {"type": "temp_above", "temp": 80, "source": "max"}, {}), sig, sensors))
        self.assertFalse(A.is_active(rule("a", {"type": "temp_above", "temp": 80, "source": "gpu"}, {}), sig, sensors))
        self.assertFalse(A.is_active(rule("a", {"type": "temp_above", "temp": 80, "source": "cpu"}, {}), sig, {}))


class ResolveTests(unittest.TestCase):
    PRESETS = {"gaming": {"fan_curve": "performance"}, "calm": {"fan_curve": None}}

    def test_first_active_wins_per_domain(self):
        rules = [rule("night", {}, {"type": "lights_off"}),
                 rule("game", {}, {"type": "preset", "preset": "gaming"}),
                 rule("quiet", {}, {"type": "fan_curve", "curve": "silent"})]
        light, fans = A.resolve(rules, {"night", "game", "quiet"}, self.PRESETS)
        self.assertEqual((light["id"], fans["id"]), ("night", "game"))

    def test_preset_without_fan_curve_leaves_fans_alone(self):
        rules = [rule("calm", {}, {"type": "preset", "preset": "calm"}),
                 rule("quiet", {}, {"type": "fan_curve", "curve": "silent"})]
        light, fans = A.resolve(rules, {"calm", "quiet"}, self.PRESETS)
        self.assertEqual((light["id"], fans["id"]), ("calm", "quiet"))

    def test_disabled_and_inactive_rules_are_ignored(self):
        rules = [rule("a", {}, {"type": "lights_off"}, enabled=False), rule("b", {}, {"type": "dim", "brightness": 5})]
        self.assertEqual(A.resolve(rules, {"a"}, {}), (None, None))
        self.assertEqual(A.resolve(rules, {"a", "b"}, {})[0]["id"], "b")


class QolTests(unittest.TestCase):
    RULE = {"id": "hot", "enabled": True, "when": {"type": "temp_above", "temp": 80, "source": "cpu"},
            "then": {"type": "fan_curve", "curve": "performance"}}

    def test_temp_rule_has_hysteresis(self):
        sig = signals()
        self.assertFalse(A.is_active(self.RULE, sig, {"cpu_temp": 79}))
        self.assertTrue(A.is_active(self.RULE, sig, {"cpu_temp": 80}))
        self.assertTrue(A.is_active(self.RULE, sig, {"cpu_temp": 78}, was_active=True))      # no flapping at 79-80
        self.assertFalse(A.is_active(self.RULE, sig, {"cpu_temp": 76}, was_active=True))

    def test_evaluate_remembers_active_rules_for_hysteresis(self):
        temps = {"cpu_temp": 81}
        store = type("S", (), {"get_rules": lambda s: [dict(self.RULE)], "get_presets": lambda s: []})()
        a = A.Automations(store, signals, lambda: temps)
        a.evaluate()
        temps["cpu_temp"] = 78
        a.evaluate()
        self.assertEqual(a.active, {"hot"})

    def test_pause_is_bounded(self):
        a = A.Automations(None, signals, dict)
        with self.assertRaises(ValueError):
            a.pause(-5)
        with self.assertRaises(ValueError):
            a.pause(24 * 60 + 1)

    def test_minutes_until(self):
        base = time.mktime((2026, 10, 13, 23, 30, 0, 0, 0, -1))
        self.assertEqual(A.minutes_until(6, base), 6 * 60 + 30)
        at_six = time.mktime((2026, 10, 13, 6, 0, 0, 0, 0, -1))
        self.assertEqual(A.minutes_until(6, at_six), 24 * 60)


if __name__ == "__main__":
    unittest.main()
