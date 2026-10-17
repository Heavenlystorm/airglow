import tempfile
import unittest
from pathlib import Path
from unittest import mock

from airglow import autostart, install


class LegacyInstallTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = Path(self.tmp.name)
        self.old = root / "Programs" / "Airglow"
        self.old.mkdir(parents=True)
        (self.old / "Airglow.exe").write_bytes(b"MZ")
        (self.old / "_internal").mkdir()
        self.links = root / "links"
        self.links.mkdir()
        patcher = mock.patch.object(install, "_stop_running_from")
        self.stop = patcher.start()
        self.addCleanup(patcher.stop)

    def lnk(self, name, target):
        # what matters to points_into: the target path stored as UTF-16, like the shell writes it
        path = self.links / name
        path.mkdir(exist_ok=True)
        f = path / "Airglow.lnk"
        f.write_bytes(b"L\x00\x00\x00" + str(target).encode("utf-16-le") + b"\x00\x00")
        return f

    def test_removes_folder_and_only_its_own_shortcuts(self):
        ours = self.lnk("start", self.old / "Airglow.exe")
        new = self.lnk("desktop", Path(self.tmp.name) / "Airglow" / "Airglow.exe")
        self.assertTrue(install.remove_legacy_install(self.old, shortcut_dirs=[ours.parent, new.parent]))
        self.assertFalse(self.old.exists())
        self.assertFalse(ours.exists())
        self.assertTrue(new.exists())
        self.stop.assert_called_once()

    def test_points_into_is_case_insensitive_and_needs_the_whole_folder(self):
        f = self.lnk("x", str(self.old / "Airglow.exe").upper())
        self.assertTrue(install.points_into(f, self.old))
        self.assertFalse(install.points_into(f, Path(str(self.old)[:-1])))

    def test_nothing_to_do_without_an_old_install(self):
        self.assertFalse(install.remove_legacy_install(Path(self.tmp.name) / "nope", shortcut_dirs=[]))
        self.stop.assert_not_called()


class AfterInstallTests(unittest.TestCase):
    def run_hook(self, existing_user, had_startup):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        if existing_user:
            (Path(tmp.name) / "config.json").write_text("{}")
        enable = mock.Mock()
        with mock.patch.object(install.paths, "data_dir", return_value=Path(tmp.name)), \
             mock.patch.object(install.paths, "log_dir", return_value=Path(tmp.name)), \
             mock.patch.object(install, "_hook_logging"), \
             mock.patch.object(install, "remove_legacy_install", return_value=existing_user), \
             mock.patch.object(autostart, "is_enabled", return_value=had_startup), \
             mock.patch.object(autostart, "enable", enable):
            install.after_install("1.6.0")
        return enable.called

    def test_fresh_install_starts_with_windows(self):
        self.assertTrue(self.run_hook(existing_user=False, had_startup=False))

    def test_moved_install_keeps_the_choice(self):
        self.assertTrue(self.run_hook(existing_user=True, had_startup=True))     # re-pointed at the new copy
        self.assertFalse(self.run_hook(existing_user=True, had_startup=False))

    def test_a_failed_move_doesnt_fail_the_install(self):
        with tempfile.TemporaryDirectory() as tmp, \
             mock.patch.object(install.paths, "data_dir", return_value=Path(tmp)), \
             mock.patch.object(install, "_hook_logging"), \
             mock.patch.object(install, "remove_legacy_install", side_effect=PermissionError("in use")), \
             mock.patch.object(autostart, "is_enabled", return_value=True), \
             mock.patch.object(autostart, "enable") as enable, \
             self.assertLogs("airglow.install", "ERROR"):
            install.after_install("1.6.0")
        enable.assert_called_once()


class UninstallTests(unittest.TestCase):
    def test_removal_scripts_cover_every_helper(self):
        ask, remove = install.removal_scripts(root=r"C:\Program Files\Airglow Tools")
        self.assertIn("'Airglow - LibreHardwareMonitor'", remove)
        self.assertIn("'Airglow - OpenRGB'", remove)
        self.assertIn(r"Remove-Item -LiteralPath 'C:\Program Files\Airglow Tools' -Recurse -Force", remove)
        self.assertIn("-Verb RunAs", ask)
        self.assertNotIn("}}", remove)                       # f-string braces must come out single

    def test_quotes_are_escaped(self):
        self.assertEqual(install._ps("it's"), "'it''s'")

    def test_uninstall_turns_off_start_with_windows(self):
        with mock.patch.object(install, "_hook_logging"), \
             mock.patch.object(autostart, "disable") as disable, \
             mock.patch("airglow.tools.tools_dir", return_value=Path("Z:/does/not/exist")), \
             mock.patch.object(install, "offer_tool_removal") as offer:
            install.before_uninstall("1.6.0")
        disable.assert_called_once()
        offer.assert_not_called()


class ScoopPathTests(unittest.TestCase):
    def test_start_up_follows_scoops_current_link(self):
        exe = r"C:\Users\me\scoop\apps\airglow\1.6.0\current\Airglow.exe"
        self.assertEqual(autostart.stable_exe(exe), r"C:\Users\me\scoop\apps\airglow\current\current\Airglow.exe")

    def test_other_paths_are_left_alone(self):
        exe = r"C:\Users\me\AppData\Local\Airglow\current\Airglow.exe"
        self.assertEqual(autostart.stable_exe(exe), exe)


if __name__ == "__main__":
    unittest.main()
