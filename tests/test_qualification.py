import hashlib
import json
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest.mock import patch
from zipfile import ZipFile

from tools import qualify


class QualificationTests(unittest.TestCase):
    def test_source_gate_invokes_the_isolated_units_and_every_browser_entry_point(self):
        with patch.object(qualify, "run_check") as run:
            qualify.source(Path("test-output"))
        commands = [tuple(call.args[2]) for call in run.call_args_list]
        self.assertTrue(any(command[-7:] == ("-m", "unittest", "discover", "-s", "tests", "-t", ".")
                            for command in commands))
        self.assertTrue(any("tests/test_effect_fidelity.mjs" in command for command in commands))
        for suite in ("tools/test-production-ui.mjs", "tools/test-scene-audition.mjs", "tools/run-ui-checks.py"):
            self.assertTrue(any(suite in command for command in commands), suite)
        self.assertFalse(any("tools/test-effect-host-native.py" in command for command in commands))

    def test_a_failing_check_stops_before_launching_browser_fixtures(self):
        with patch.object(qualify, "run_check", side_effect=qualify.CheckFailed("Python unit tests")) as run:
            with self.assertRaises(qualify.CheckFailed):
                qualify.source(Path("test-output"))
        self.assertEqual(run.call_count, 1)

    def test_package_gate_rejects_a_stale_bundle_before_native_launch(self):
        with TemporaryDirectory() as temp:
            base = Path(temp)
            (base / "web").mkdir()
            (base / "web/index.html").write_text("new", encoding="utf-8")
            exe = base / "dist/Airglow/Airglow.exe"
            web = exe.parent / "_internal/web"
            web.mkdir(parents=True)
            exe.write_bytes(b"MZ program")
            (web / "index.html").write_text("stale", encoding="utf-8")
            with patch.object(qualify, "ROOT", base), patch.object(qualify, "run_check") as run:
                with self.assertRaisesRegex(qualify.CheckFailed, "Packaged web asset differs"):
                    qualify.package(base / "output", base / "dist")
            run.assert_not_called()

    def test_generated_release_assets_match_the_versioned_manifest(self):
        with TemporaryDirectory() as temp:
            dist = Path(temp)
            release = dist / "releases"
            release.mkdir()
            exe = dist / "Airglow/Airglow.exe"
            web = exe.parent / "_internal/web"
            web.mkdir(parents=True)
            exe.write_bytes(b"MZ program")
            (web / "index.html").write_text("bundled web", encoding="utf-8")
            (release / "Airglow-beta-Setup.exe").write_bytes(b"MZ setup")
            full = release / "Airglow-1.16.0-beta.1-full.nupkg"
            for archive in (release / "Airglow-beta-Portable.zip", full):
                with ZipFile(archive, "w") as zip:
                    zip.write(exe, "app/Airglow.exe")
                    zip.write(web / "index.html", "app/_internal/web/index.html")
            feed = release / "releases.beta.json"
            payload = {"Assets": [{"Type": "Full", "Version": "1.16.0-beta.1", "PackageId": "Airglow",
                                   "FileName": full.name, "SHA256": hashlib.sha256(full.read_bytes()).hexdigest(),
                                   "Size": full.stat().st_size}]}
            feed.write_text(json.dumps(payload), encoding="utf-8")
            self.assertIn(full, qualify.release_assets(release, "1.16.0-beta.1"))
            payload["Assets"][0]["SHA256"] = "0" * 64
            feed.write_text(json.dumps(payload), encoding="utf-8")
            with self.assertRaisesRegex(qualify.CheckFailed, "SHA256"):
                qualify.release_assets(release, "1.16.0-beta.1")
