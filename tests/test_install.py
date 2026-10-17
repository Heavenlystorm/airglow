import os
import subprocess
import sys
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


class StartAfterSetupTests(unittest.TestCase):
    """Setup ends a running Airglow to replace it; after a silent install (winget) the hook starts it again."""

    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.data = Path(tmp.name)
        for name in ("data_dir", "log_dir"):
            patcher = mock.patch.object(install.paths, name, return_value=self.data)
            patcher.start()
            self.addCleanup(patcher.stop)

    def after_install(self, popen=None):
        with mock.patch.object(install, "_hook_logging"),              mock.patch.object(install, "remove_legacy_install", return_value=False),              mock.patch.object(autostart, "is_enabled", return_value=True),              mock.patch.object(autostart, "enable"),              mock.patch.object(install.subprocess, "Popen", popen or mock.Mock()) as started:
            install.after_install("1.6.0")
        return started

    def test_marker_is_written_and_cleared(self):
        install.mark_running()
        self.assertEqual((self.data / "running").read_text(), str(os.getpid()))
        install.clear_running()
        self.assertFalse((self.data / "running").exists())
        install.clear_running()                                   # twice is fine

    def test_starts_again_when_setup_ended_a_running_copy(self):
        install.mark_running()
        started = self.after_install()
        started.assert_called_once()
        cmd = started.call_args.args[0]
        self.assertEqual(cmd, [sys.executable, "--after-setup", str(os.getppid())])
        self.assertEqual(started.call_args.kwargs["cwd"], Path(sys.executable).parent)

    def test_doesnt_start_when_airglow_had_quit(self):
        install.mark_running()
        install.clear_running()
        self.after_install().assert_not_called()

    def test_a_failed_start_doesnt_fail_the_install(self):
        install.mark_running()
        with self.assertLogs("airglow.install", "ERROR"):
            self.after_install(popen=mock.Mock(side_effect=OSError("no")))

    def test_breakaway_refused_falls_back(self):
        popen = mock.Mock(side_effect=[OSError("access denied"), None])
        with mock.patch.object(install.subprocess, "Popen", popen):
            install.start_after_setup(1234)
        self.assertEqual(popen.call_count, 2)
        self.assertNotIn("creationflags", popen.call_args.kwargs)

    def test_uninstall_clears_the_marker(self):
        install.mark_running()
        with mock.patch.object(install, "_hook_logging"), mock.patch.object(autostart, "disable"),              mock.patch("airglow.tools.tools_dir", return_value=Path("Z:/does/not/exist")):
            install.before_uninstall("1.6.0")
        self.assertFalse((self.data / "running").exists())

    def test_waits_for_setup_then_starts_unless_setup_did(self):
        setup = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(0.5)"])
        self.addCleanup(setup.wait)
        with mock.patch.object(install, "other_copies_running", return_value=False):
            self.assertTrue(install.wait_for_setup(setup.pid, timeout=10))
        self.assertIsNotNone(setup.poll())                        # it really waited
        with mock.patch.object(install, "other_copies_running", return_value=True):
            self.assertFalse(install.wait_for_setup(setup.pid))

    def test_a_missing_or_bad_setup_pid_doesnt_wait(self):
        with mock.patch.object(install, "other_copies_running", return_value=False):
            self.assertTrue(install.wait_for_setup("not a pid"))
            self.assertTrue(install.wait_for_setup(2 ** 31 - 1))

    def test_other_copies_are_found_by_exe_but_not_this_process(self):
        self.assertFalse(install.other_copies_running(sys.executable + ".nope"))
        other = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(5)"])
        self.addCleanup(other.wait)
        self.addCleanup(other.kill)
        self.assertTrue(install.other_copies_running(sys.executable))


class AfterSetupStartTests(unittest.TestCase):
    def main(self, setup_done_it):
        from airglow import app
        with mock.patch.object(install, "run_hooks"),              mock.patch.object(install, "wait_for_setup", return_value=not setup_done_it) as wait,              mock.patch.object(app, "setup_logging") as started, mock.patch.object(app, "Store"),              mock.patch.object(app, "single_instance", return_value=False),              mock.patch.object(app, "ask_running_instance_to_show") as show:
            self.assertEqual(app.main(["--after-setup", "1234"]), 0)
        wait.assert_called_once_with("1234")
        return started, show

    def test_starts_in_the_tray(self):
        # stops at the single-instance check here; a start in the tray doesn't ask a running copy to show its window
        started, show = self.main(setup_done_it=False)
        started.assert_called_once()
        show.assert_not_called()

    def test_nothing_to_do_when_setup_started_it(self):
        started, _ = self.main(setup_done_it=True)
        started.assert_not_called()


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
