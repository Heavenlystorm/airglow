import os
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest import mock

from airglow import config, diagnostics
from airglow.controller import Controller
from tests.fakes import FakeLightDevice, FakeManager, FakeNzxt, signals


class ScrubTests(unittest.TestCase):
    def test_user_pc_and_serials_are_removed(self):
        text = ('C:\\Users\\jane\\AppData\\Roaming\\Airglow host JANE-PC '
                '"key": "openrgb:corsair-k70:sn123456/z0" and C:/Users/jane/x')
        out = diagnostics.scrub(text, ["C:\\Users\\jane", "C:/Users/jane", "JANE-PC", "jane"])
        for s in ("jane", "JANE-PC", "sn123456"):
            self.assertNotIn(s, out)
        self.assertIn("openrgb:corsair-k70:s", out)                      # ids still line up, just hashed
        self.assertEqual(diagnostics.scrub("openrgb:a:b", []), diagnostics.scrub("openrgb:a:b", []))


class ReportTests(unittest.TestCase):
    def test_zip_has_the_expected_files_and_no_secrets(self):
        with tempfile.TemporaryDirectory() as tmp:
            logs = Path(tmp, "logs")
            logs.mkdir()
            user = os.environ.get("USERNAME", "someone")
            (logs / "airglow.log").write_text(f"started from C:\\Users\\{user}\\Desktop\n", encoding="utf-8")
            store = config.Store(Path(tmp, "data"))
            ctrl = Controller(store, manager=FakeManager(FakeNzxt(), FakeLightDevice("openrgb:kb:serial99", "Kb")),
                              signals=signals, read_sensors=lambda: {"online": True, "cpu_temp": 40.0},
                              async_writes=False)
            with mock.patch.object(diagnostics.paths, "log_dir", return_value=logs), \
                 mock.patch.object(diagnostics.sensors, "read_tree", return_value={"Text": os.environ.get("COMPUTERNAME", "PC"), "Children": []}):
                path = diagnostics.write(ctrl, store, Path(tmp, "out"))
            with zipfile.ZipFile(path) as z:
                names = set(z.namelist())
                self.assertTrue({"about.json", "settings.json", "lhm_data.json", "logs/airglow.log"} <= names)
                blob = "\n".join(z.read(n).decode("utf-8") for n in names)
            if len(user) > 2:
                self.assertNotIn(f"\\{user}\\", blob)
            self.assertNotIn("serial99", blob)


if __name__ == "__main__":
    unittest.main()
