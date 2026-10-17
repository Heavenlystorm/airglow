import unittest

from airglow import fans
from airglow.config import DEFAULT_CURVES, SAFETY_TEMP

CURVE = [[40, 30], [60, 50], [80, 100]]


class CurveTests(unittest.TestCase):
    def test_flat_below_first_point(self):
        self.assertEqual(fans.curve_duty(CURVE, 20), 30)

    def test_flat_above_last_point(self):
        self.assertEqual(fans.curve_duty(CURVE, 95), 100)

    def test_linear_between_points(self):
        self.assertAlmostEqual(fans.curve_duty(CURVE, 50), 40)
        self.assertAlmostEqual(fans.curve_duty(CURVE, 70), 75)

    def test_unsorted_points_are_sorted(self):
        self.assertAlmostEqual(fans.curve_duty(list(reversed(CURVE)), 50), 40)

    def test_default_curves_never_slow_down_as_it_gets_hotter(self):
        for name, pts in DEFAULT_CURVES.items():
            duties = [fans.curve_duty(pts, t) for t in range(20, 100)]
            self.assertEqual(duties, sorted(duties), name)


class TargetTests(unittest.TestCase):
    def test_no_temperature_uses_failsafe(self):
        self.assertEqual(fans.target_duty(CURVE, None, 60), 60)

    def test_lowest_duty_is_the_coolest_point(self):
        self.assertEqual(fans.lowest_duty([[70, 80], [40, 30], [55, 45]]), 30)

    def test_safety_override(self):
        flat_quiet = [[20, 20], [100, 20]]
        self.assertEqual(fans.target_duty(flat_quiet, SAFETY_TEMP, 60), 100)
        self.assertEqual(fans.target_duty(flat_quiet, SAFETY_TEMP - 1, 60), 20)

    def test_source_temp(self):
        s = {"cpu_temp": 70, "gpu_temp": 55}
        self.assertEqual(fans.source_temp(s, "max"), 70)
        self.assertEqual(fans.source_temp(s, "cpu"), 70)
        self.assertEqual(fans.source_temp(s, "gpu"), 55)
        self.assertEqual(fans.source_temp({"cpu_temp": None, "gpu_temp": 50}, "max"), 50)
        self.assertIsNone(fans.source_temp({}, "max"))


class StepTests(unittest.TestCase):
    def test_first_value_jumps(self):
        self.assertEqual(fans.step_toward(None, 47, 5), 47)

    def test_ramp_is_limited(self):
        self.assertEqual(fans.step_toward(30, 60, 5), 35)
        self.assertEqual(fans.step_toward(60, 30, 5), 55)

    def test_small_change_lands_exactly(self):
        self.assertEqual(fans.step_toward(30, 32, 5), 32)

    def test_full_speed_is_immediate(self):
        self.assertEqual(fans.step_toward(30, 100, 5), 100)


if __name__ == "__main__":
    unittest.main()
