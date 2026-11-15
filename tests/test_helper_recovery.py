import base64
import hashlib
import json
import struct
import subprocess
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

from airglow import openrgb, sensors, tools
from tests.test_tools import checked, make_zip


class TaskScheduler:
    def __init__(self, name, folder):
        self.name, self.folder = name, folder
        self.xml = self.before = tools.task_xml("S-1-5-21-7", folder / "OpenRGB.exe", "--previous",
                                               folder, "Previous configuration")
        self.running = True
        self.mutations = []
        self.reject_create = False
        self.reject_restore = False
        self.reject_run = False

    def __call__(self, command, **kwargs):
        op = command[1]
        if op == "/Query":
            if "/XML" in command:
                return subprocess.CompletedProcess(command, 0, self.xml, "")
            csv = f'"\\{self.name}","N/A","Ready"\r\n' if self.xml else ""
            return subprocess.CompletedProcess(command, 0, csv, "")
        self.mutations.append(op)
        code = 0
        if op == "/End":
            self.running = False
        elif op == "/Create":
            candidate = Path(command[command.index("/XML") + 1]).read_text(encoding="utf-16")
            if candidate == self.before and self.reject_restore or candidate != self.before and self.reject_create:
                code = 1
            else:
                self.xml = candidate
        elif op == "/Run":
            if self.xml != self.before and self.reject_run:
                code = 1
            else:
                self.running = True
        elif op == "/Delete":
            self.xml = None
        return subprocess.CompletedProcess(command, code, "", "")

    def processes(self, name):
        if name == "OpenRGB.exe" and self.running:
            return [(4271, str(self.folder / "OpenRGB.exe"))]
        return []


class HelperRepairTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.work = Path(temp.name)
        self.root = self.work / "Airglow Tools"
        self.openrgb = self.root / "OpenRGB"
        self.openrgb.mkdir(parents=True)
        (self.openrgb / "OpenRGB.exe").write_bytes(b"old")
        (self.openrgb / "user-before.txt").write_text("preserved", encoding="utf-8")
        self.spec = tools.spec()
        self.job = {"tools": ["openrgb"], "files": {}, "user_sid": "S-1-5-21-7", "openrgb_port": 6750}
        self.archive("openrgb", {"OpenRGB Windows 64-bit/OpenRGB.exe": "verified replacement"})
        self.archive("lhm", {"LibreHardwareMonitor.exe": "verified replacement"})
        password = b"fixture-password"
        self.job["lhm_password_sha256"] = hashlib.sha256(password).hexdigest()
        self.job["lhm_authorization"] = "Basic " + base64.b64encode(b"airglow:" + password).decode()
        self.scheduler = TaskScheduler(self.spec["tools"]["openrgb"]["task"], self.openrgb)
        self.enterContext(mock.patch.object(tools, "spec", return_value=self.spec))
        self.enterContext(mock.patch.object(tools, "downloads_dir", return_value=self.work))
        self.enterContext(mock.patch.object(tools, "_processes", side_effect=self.scheduler.processes))
        self.healthy = self.enterContext(mock.patch.object(tools, "_helper_healthy", return_value=True, create=True))
        self.enterContext(mock.patch("psutil.Process", side_effect=AssertionError("fixture never opens a live PID")))

    def archive(self, tid, content):
        path = make_zip(self.work / f"{tid}.zip", content)
        t = self.spec["tools"][tid]
        t["size"], t["sha256"] = checked(path)
        self.job["files"][tid] = str(path)

    def repair(self):
        return tools.install(self.job, self.root, self.scheduler)

    def assert_old_helper_restored(self):
        self.assertEqual((self.openrgb / "OpenRGB.exe").read_bytes(), b"old")
        self.assertTrue((self.openrgb / "user-before.txt").is_file())
        self.assertEqual(self.scheduler.xml, self.scheduler.before)
        self.assertTrue(self.scheduler.running)

    def test_bad_archive_for_second_helper_leaves_first_helper_and_task_untouched(self):
        self.job["tools"].append("lhm")
        Path(self.job["files"]["lhm"]).write_bytes(b"corrupt")
        with self.assertRaisesRegex(tools.SetupError, "bytes|checksum"):
            self.repair()
        self.assertEqual(self.scheduler.mutations, [])
        self.assert_old_helper_restored()

    def test_pinned_archive_missing_its_executable_leaves_old_copy_untouched(self):
        self.archive("openrgb", {"OpenRGB Windows 64-bit/readme.txt": "no executable"})
        with self.assertRaisesRegex(tools.SetupError, "wasn't in its download"):
            self.repair()
        self.assertEqual(self.scheduler.mutations, [])
        self.assert_old_helper_restored()

    def test_failed_registration_restores_the_old_files_xml_and_running_task(self):
        self.scheduler.reject_create = True
        with self.assertRaisesRegex(tools.SetupError, "start at sign-in"):
            self.repair()
        self.assert_old_helper_restored()

    def test_failed_run_restores_the_old_files_xml_and_running_task(self):
        self.scheduler.reject_run = True
        with self.assertRaisesRegex(tools.SetupError, "start OpenRGB"):
            self.repair()
        self.assert_old_helper_restored()

    def test_failed_run_reports_a_clean_rollback_to_the_requesting_process(self):
        self.scheduler.reject_run = True
        job_path = self.work / "Airglow" / "downloads" / "setup-job.json"
        job_path.parent.mkdir(parents=True)
        job_path.write_text(json.dumps(self.job), encoding="utf-8")
        install = tools.install
        logger = tools.logging.getLogger()
        level = logger.level
        handlers = list(logger.handlers)
        try:
            with (mock.patch.object(tools.paths, "log_dir", return_value=self.work),
                  mock.patch.object(tools, "install", side_effect=lambda job, download_root:
                                    install(job, self.root, self.scheduler, download_root=self.work))):
                self.assertEqual(tools.elevated_main(["--setup-tools", str(job_path)]), 1)
        finally:
            logger.setLevel(level)
            for handler in set(logger.handlers) - set(handlers):
                logger.removeHandler(handler)
                handler.close()
        self.assert_old_helper_restored()
        result = json.loads(tools.result_file(job_path, "--setup-tools").read_text(encoding="utf-8"))
        self.assertIs(result.get("rolled_back"), True)
        self.assertIn("start OpenRGB", result["error"])

    def test_scheduler_timeout_still_restores_the_old_copy(self):
        def scheduler(command, **kwargs):
            if command[1] == "/Create" and "openrgb-new-task.xml" in command[-2]:
                self.assertEqual(kwargs["timeout"], tools.TASK_TIMEOUT)
                raise subprocess.TimeoutExpired(command, kwargs["timeout"])
            return self.scheduler(command, **kwargs)

        with self.assertRaisesRegex(tools.SetupError, "timed out"):
            tools.install(self.job, self.root, scheduler)
        self.assert_old_helper_restored()

    def test_a_queued_task_without_health_times_out_and_restores_the_old_copy(self):
        self.healthy.return_value = False
        clock = SimpleNamespace(now=0.0)
        with mock.patch.object(tools.time, "monotonic", side_effect=lambda: clock.now), \
             mock.patch.object(tools.time, "sleep", side_effect=lambda t: setattr(clock, "now", clock.now + t)):
            with self.assertRaisesRegex(tools.SetupError, "health"):
                self.repair()
        self.assertGreater(clock.now, 0)
        self.assertLessEqual(clock.now, 60)
        self.assert_old_helper_restored()

    def test_success_only_retires_backup_after_health_proof(self):
        def prove(*args):
            self.assertEqual((self.openrgb / "OpenRGB.exe").read_bytes(), b"verified replacement")
            (backup,) = tuple(self.root.rglob("user-before.txt"))
            self.assertNotEqual(backup.parent, self.openrgb)
            self.assertEqual((backup.parent / "OpenRGB.exe").read_bytes(), b"old")
            return True

        self.healthy.side_effect = prove
        self.assertEqual(self.repair(), ["openrgb"])
        self.healthy.assert_called()
        self.assertFalse(list(self.root.rglob("user-before.txt")))
        self.assertNotEqual(self.scheduler.xml, self.scheduler.before)

    def test_a_failed_rollback_keeps_the_prior_task_xml_for_manual_recovery(self):
        self.scheduler.reject_run = True
        self.scheduler.reject_restore = True
        with self.assertRaisesRegex(tools.SetupError, "restore|rollback") as error:
            self.repair()
        self.assertEqual((self.openrgb / "OpenRGB.exe").read_bytes(), b"old")
        (saved_xml,) = [p for p in self.root.rglob("*.xml") if p.read_text(encoding="utf-16") == self.scheduler.before]
        self.assertIn(str(saved_xml.parent.resolve()), str(error.exception))

    def test_process_interruption_does_not_dispose_of_the_previous_helper(self):
        self.healthy.side_effect = KeyboardInterrupt
        with self.assertRaises(KeyboardInterrupt):
            self.repair()
        (old_file,) = self.root.rglob("user-before.txt")
        self.assertEqual((old_file.parent / "OpenRGB.exe").read_bytes(), b"old")
        self.assertTrue(any(p.read_text(encoding="utf-16") == self.scheduler.before for p in self.root.rglob("*.xml")))

    def test_mismatched_lhm_credential_is_refused_before_task_changes(self):
        self.job["tools"].append("lhm")
        self.job["lhm_password_sha256"] = "a" * 64
        with self.assertRaisesRegex(tools.SetupError, "credentials"):
            self.repair()
        self.assertEqual(self.scheduler.mutations, [])
        self.assert_old_helper_restored()

    def test_only_the_expected_executable_can_satisfy_startup(self):
        def probe(*args):
            self.scheduler.running = False
            return True

        self.healthy.side_effect = probe
        clock = SimpleNamespace(now=0.0)
        with mock.patch.object(tools.time, "monotonic", side_effect=lambda: clock.now), \
             mock.patch.object(tools.time, "sleep", side_effect=lambda t: setattr(clock, "now", clock.now + t)):
            with self.assertRaisesRegex(tools.SetupError, "health"):
                self.repair()
        self.assert_old_helper_restored()


class HelperProofTests(unittest.TestCase):
    def test_openrgb_proof_negotiates_the_protocol_on_the_expected_pids_listener(self):
        listener = SimpleNamespace(status="LISTEN", laddr=("127.0.0.1", 6750))
        process = mock.Mock()
        process.net_connections.return_value = [listener]
        connection = mock.MagicMock()
        packet = openrgb.header(0, openrgb.REQUEST_PROTOCOL_VERSION, 4)
        connection.__enter__.return_value.recv.side_effect = [packet, struct.pack("<I", 5)]
        with mock.patch("psutil.Process", return_value=process) as get_process, \
             mock.patch.object(tools.socket, "create_connection", return_value=connection) as connect:
            self.assertTrue(tools._helper_healthy("openrgb", 42, 6750, {}, 0.5))
            get_process.assert_called_once_with(42)
            self.assertEqual(connect.call_args.args, (("127.0.0.1", 6750),))
            connection.__enter__.return_value.sendall.assert_called_once_with(
                packet + struct.pack("<I", openrgb.CLIENT_PROTOCOL))

            connect.reset_mock()
            process.net_connections.return_value = [SimpleNamespace(status="LISTEN", laddr=("127.0.0.1", 6742))]
            self.assertFalse(tools._helper_healthy("openrgb", 42, 6750, {}, 0.5))
            connect.assert_not_called()

    def test_lhm_proof_reads_the_authenticated_sensor_tree(self):
        headers = {"Authorization": "Basic fixture"}
        with mock.patch.object(sensors, "read_tree", return_value={"Children": []}) as read:
            self.assertTrue(tools._helper_healthy("lhm", 42, 6750, headers, 0.5))
        read.assert_called_once_with(tools.LHM_URL, timeout=0.5, headers=headers)
        with mock.patch.object(sensors, "read_tree", return_value={"unrelated": []}):
            self.assertFalse(tools._helper_healthy("lhm", 42, 6750, headers, 0.5))

    def test_elevated_setup_consumes_the_requesting_users_job_before_install(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            downloads = root / "requesting user" / "Airglow" / "downloads"
            downloads.mkdir(parents=True)
            request = downloads / "setup-job.json"
            credentials = "fixture-only-password"
            request.write_text(json.dumps({"lhm_authorization": credentials}), encoding="utf-8")
            elevated_user = root / "elevated user"
            elevated_user.mkdir()

            def install(job, *, download_root):
                self.assertEqual(job["lhm_authorization"], credentials)
                self.assertFalse(request.exists())
                self.assertEqual(download_root, downloads.resolve())
                return ["lhm"]

            logger = tools.logging.getLogger()
            level = logger.level
            handlers = list(logger.handlers)
            try:
                with mock.patch.object(tools.paths, "log_dir", return_value=elevated_user), \
                     mock.patch.object(tools, "install", side_effect=install):
                    self.assertEqual(tools.elevated_main(["--setup-tools", str(request)]), 0)
            finally:
                logger.setLevel(level)
                for handler in set(logger.handlers) - set(handlers):
                    logger.removeHandler(handler)
                    handler.close()
            receipt = (downloads / "setup-result.json").read_text(encoding="utf-8")
            self.assertEqual(json.loads(receipt), {"ok": True, "done": ["lhm"], "error": None})
            self.assertNotIn(credentials, receipt + (elevated_user / "tools-setup.log").read_text(encoding="utf-8"))
