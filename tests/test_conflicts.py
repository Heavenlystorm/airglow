import unittest
from unittest import mock

from airglow import conflicts


class ConflictTests(unittest.TestCase):
    def detect(self, running, lit=None):
        with mock.patch.object(conflicts.system, "running_exes", return_value=frozenset(running)):
            return {c["name"]: c for c in conflicts.detect(lit)}

    def test_lighting_software_always_counts(self):
        found = self.detect({"signalrgb.exe", "l-connect 3.exe", "ledkeeper2.exe"}, lit=set())
        self.assertEqual(set(found), {"SignalRGB", "Lian Li L-Connect", "MSI Mystic Light"})

    def test_peripheral_software_only_counts_for_brands_airglow_lights(self):
        running = {"lghub.exe", "lghub_agent.exe", "steelseriesgg.exe"}
        self.assertEqual(self.detect(running, lit={"nzxt", "asus"}), {})        # mouse settings stay yours
        found = self.detect(running, lit={"logitech"})
        self.assertEqual(found["Logitech G HUB"]["exes"], ["lghub.exe", "lghub_agent.exe"])
        self.assertNotIn("SteelSeries GG", found)
        self.assertEqual(set(self.detect(running)), {"Logitech G HUB", "SteelSeries GG"})     # diagnostics: all

    def test_every_known_exe_is_lower_case(self):
        self.assertTrue(all(exe == exe.lower() and exe.endswith(".exe") for exe in conflicts.KNOWN))
        names = {n for n, _ in conflicts.KNOWN.values()}
        self.assertLessEqual(set(conflicts.ONLY_FOR_VENDORS), names)


if __name__ == "__main__":
    unittest.main()
