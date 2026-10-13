import unittest
from unittest import mock

from airglow import autostart


class AutostartTests(unittest.TestCase):
    def test_command_starts_in_background(self):
        self.assertTrue(autostart.command().endswith("--background"))

    def test_source_run_is_not_the_installed_copy(self):
        self.assertFalse(autostart.is_installed_copy())

    def test_repair_never_touches_registry_from_a_non_installed_copy(self):
        with mock.patch.object(autostart, "is_enabled", return_value=True), \
             mock.patch.object(autostart, "current", return_value='"C:\\elsewhere\\Airglow.exe" --background'), \
             mock.patch.object(autostart, "enable", side_effect=AssertionError("must not write")):
            self.assertFalse(autostart.repair())


if __name__ == "__main__":
    unittest.main()
