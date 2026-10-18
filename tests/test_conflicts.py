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

    def test_gcc_counts_only_from_a_gigabyte_folder(self):
        def procs(*items):
            return [mock.Mock(info={"name": n, "exe": e}) for n, e in items]
        compiler = procs(("gcc.exe", r"C:\msys64\ucrt64\bin\gcc.exe"), ("gcc.exe", None))
        with mock.patch("psutil.process_iter", return_value=compiler):
            self.assertEqual(self.detect({"gcc.exe"}), {})
        gigabyte = procs(("GCC.exe", r"C:\Program Files\GIGABYTE\Control Center\GCC.exe"))
        with mock.patch("psutil.process_iter", return_value=gigabyte):
            self.assertIn("Gigabyte Control Center", self.detect({"gcc.exe"}))

    def test_closing_never_kills_a_compiler_and_leaves_services_alone(self):
        gcc = mock.Mock(info={"name": "gcc.exe", "exe": r"C:\mingw\bin\gcc.exe"})
        ctrl = mock.Mock(info={"name": "GCC.exe", "exe": r"C:\Program Files\GIGABYTE\Control Center\GCC.exe"})
        with mock.patch("psutil.process_iter", return_value=[gcc, ctrl]):
            self.assertEqual(conflicts.close("Gigabyte Control Center"), {"failed": [], "services": []})
        gcc.terminate.assert_not_called()
        ctrl.terminate.assert_called_once()
        svc = mock.Mock(info={"name": "LightingService.exe", "exe": r"C:\Program Files (x86)\LightingService\LightingService.exe"})
        with mock.patch("psutil.process_iter", return_value=[svc]):
            self.assertEqual(conflicts.close("ASUS Aura service"), {"failed": [], "services": ["LightingService.exe"]})
        svc.terminate.assert_not_called()
        self.assertTrue(self.detect({"lightingservice.exe"})["ASUS Aura service"]["service"])
        self.assertFalse(self.detect({"signalrgb.exe"})["SignalRGB"]["service"])

    def test_every_known_exe_is_lower_case(self):
        self.assertTrue(all(exe == exe.lower() and exe.endswith(".exe") for exe in conflicts.KNOWN))
        names = {n for n, _ in conflicts.KNOWN.values()}
        self.assertLessEqual(set(conflicts.ONLY_FOR_VENDORS), names)
        self.assertLessEqual(set(conflicts.PATH_MUST_HAVE) | conflicts.SERVICES, set(conflicts.KNOWN))


if __name__ == "__main__":
    unittest.main()
