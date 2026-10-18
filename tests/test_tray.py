import unittest

from airglow.tray import curve_label


class CurveLabelTests(unittest.TestCase):
    PRESETS = [{"id": "neon-01234", "name": "Neon"}]

    def test_named_curves(self):
        self.assertEqual(curve_label("balanced", self.PRESETS), "Balanced")

    def test_a_profiles_own_curve_uses_its_name(self):
        self.assertEqual(curve_label("profile:neon-01234", self.PRESETS), "Neon")
        self.assertEqual(curve_label("profile:gone", self.PRESETS), "Profile")

    def test_the_curve_being_tried(self):
        self.assertEqual(curve_label("trial", self.PRESETS, {"name": "Aurora"}), "Aurora's")
        self.assertEqual(curve_label("trial", self.PRESETS), "Trial")


if __name__ == "__main__":
    unittest.main()
