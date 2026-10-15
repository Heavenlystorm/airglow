import hashlib
import io
import json
import re
import tempfile
import time
import unittest
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

from airglow import config, sensors, tools
from tests.fakes import FakeManager


def make_zip(path, entries):
    with zipfile.ZipFile(path, "w") as z:
        for name, data in entries.items():
            z.writestr(name, data)
    return path


def checked(path):
    data = Path(path).read_bytes()
    return len(data), hashlib.sha256(data).hexdigest()


class FakeResponse(io.BytesIO):
    def __init__(self, data):
        super().__init__(data)
        self.headers = {"Content-Length": str(len(data))}

    def __enter__(self):
        return self

    def __exit__(self, *a):
        self.close()


class Commands:
    """Stands in for subprocess.run and records what would have run."""

    def __init__(self, code=0):
        self.calls, self.code = [], code

    def __call__(self, cmd, **kw):
        self.calls.append(cmd)
        return SimpleNamespace(returncode=self.code, stdout="", stderr="")

    def named(self, *words):
        return [c for c in self.calls if all(w in c for w in words)]


class SpecTests(unittest.TestCase):
    def test_every_download_is_pinned(self):
        s = tools.spec()
        for item in [*s["tools"].values(), s["pawnio"]]:
            self.assertTrue(item["url"].startswith("https://"), item["url"])
            self.assertRegex(item["sha256"], r"^[0-9a-f]{64}$")
            self.assertIsInstance(item["size"], int)
        for t in s["tools"].values():
            self.assertTrue(re.fullmatch(r"[\w .\-]+", t["task"]), t["task"])
            t["args"].format(openrgb_port=6742)                         # every placeholder is one we fill in
        self.assertIn("--server-port 6742", s["tools"]["openrgb"]["args"].format(openrgb_port=6742))

    def test_lhm_address_is_the_ip_it_listens_on(self):
        # LHM's listener only answers requests whose Host matches its prefix, so this must not be "localhost"
        self.assertEqual(tools.LHM_URL, "http://127.0.0.1:8085")
        config.validate_settings({"lhm_url": tools.LHM_URL})


class DownloadTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        self.data = b"x" * 200_000

    def tearDown(self):
        self.tmp.cleanup()

    def test_good_file_is_kept_and_progress_reported(self):
        seen = []
        out = tools.download("https://example.invalid/a.zip", self.dir / "a.zip", len(self.data),
                             hashlib.sha256(self.data).hexdigest(), seen.append, opener=lambda *a, **k: FakeResponse(self.data))
        self.assertEqual(out.read_bytes(), self.data)
        self.assertEqual(seen[-1], 1.0)
        self.assertFalse(list(self.dir.glob("*.part")))

    def test_wrong_checksum_leaves_nothing_behind(self):
        with self.assertRaisesRegex(tools.SetupError, "checksum"):
            tools.download("https://example.invalid/a.zip", self.dir / "a.zip", len(self.data), "0" * 64,
                           opener=lambda *a, **k: FakeResponse(self.data))
        self.assertEqual(list(self.dir.iterdir()), [])

    def test_oversized_download_is_stopped(self):
        with self.assertRaisesRegex(tools.SetupError, "larger"):
            tools.download("https://example.invalid/a.zip", self.dir / "a.zip", 1000, "0" * 64,
                           opener=lambda *a, **k: FakeResponse(self.data))
        self.assertEqual(list(self.dir.iterdir()), [])

    def test_network_error_is_a_setup_error(self):
        def broken(*a, **k):
            raise OSError("no route")
        with self.assertRaisesRegex(tools.SetupError, "couldn't download"):
            tools.download("https://example.invalid/a.zip", self.dir / "a.zip", opener=broken)


class FileTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_extract_strips_the_top_folder_and_skips_the_rest(self):
        z = make_zip(self.dir / "o.zip", {"OpenRGB Windows 64-bit/OpenRGB.exe": "exe",
                                          "OpenRGB Windows 64-bit/platforms/qwindows.dll": "dll",
                                          "elsewhere/readme.txt": "no"})
        tools.extract(z, self.dir / "out", "OpenRGB Windows 64-bit/")
        self.assertEqual((self.dir / "out/OpenRGB.exe").read_text(), "exe")
        self.assertTrue((self.dir / "out/platforms/qwindows.dll").is_file())
        self.assertFalse((self.dir / "out/elsewhere").exists())

    def test_extract_refuses_paths_outside_the_folder(self):
        z = make_zip(self.dir / "bad.zip", {"../escaped.txt": "x"})
        with self.assertRaisesRegex(tools.SetupError, "unsafe"):
            tools.extract(z, self.dir / "out")
        self.assertFalse((self.dir / "escaped.txt").exists())

    def test_lhm_config_turns_the_server_on_for_this_pc_only(self):
        root = ET.fromstring(tools.lhm_config("ab" * 32))
        v = {a.get("key"): a.get("value") for a in root.iter("add")}
        self.assertEqual(v["runWebServerMenuItem"], "true")          # LHM reads booleans as exactly "true"
        self.assertEqual(v["listenerIp"], "127.0.0.1")
        self.assertEqual(v["listenerPort"], "8085")
        self.assertEqual(v["authenticationEnabled"], "true")
        self.assertEqual(v["authenticationPassword"], "ab" * 32)
        self.assertEqual(v["startMinMenuItem"], "true")

    def test_task_runs_as_the_user_with_their_highest_rights(self):
        xml = tools.task_xml("S-1-5-21-1", r"C:\Program Files\Airglow Tools\OpenRGB\OpenRGB.exe",
                             '--server --name "a<b&c"', r"C:\Program Files\Airglow Tools\OpenRGB", "test")
        root = ET.fromstring(xml.split("\n", 1)[1])                   # ElementTree can't take the UTF-16 declaration
        ns = {"t": "http://schemas.microsoft.com/windows/2004/02/mit/task"}
        self.assertEqual(root.find(".//t:LogonTrigger/t:UserId", ns).text, "S-1-5-21-1")
        self.assertEqual(root.find(".//t:Principal/t:RunLevel", ns).text, "HighestAvailable")
        self.assertEqual(root.find(".//t:Exec/t:Arguments", ns).text, '--server --name "a<b&c"')
        self.assertEqual(root.find(".//t:ExecutionTimeLimit", ns).text, "PT0S")


class InstallTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        self.root = self.dir / "Program Files" / "Airglow Tools"
        lhm = make_zip(self.dir / "lhm.zip", {"LibreHardwareMonitor.exe": "lhm", "LibreHardwareMonitor.exe.config": "c"})
        orgb = make_zip(self.dir / "orgb.zip", {"OpenRGB Windows 64-bit/OpenRGB.exe": "orgb"})
        self.spec = tools.spec()
        for tid, z in (("lhm", lhm), ("openrgb", orgb)):
            self.spec["tools"][tid]["size"], self.spec["tools"][tid]["sha256"] = checked(z)
        self.job = {"tools": ["lhm", "openrgb"], "files": {"lhm": str(lhm), "openrgb": str(orgb)},
                    "user_sid": "S-1-5-21-7", "openrgb_port": 6750, "lhm_password_sha256": "cd" * 32}
        patches = [mock.patch.object(tools, "spec", return_value=self.spec),
                   mock.patch.object(tools, "_processes", return_value=[]),
                   mock.patch.object(tools, "pawnio_needed", return_value=False)]
        for p in patches:
            p.start()
            self.addCleanup(p.stop)

    def tearDown(self):
        self.tmp.cleanup()

    def test_installs_configures_and_registers_both(self):
        run = Commands()
        done = tools.install(self.job, self.root, run)
        self.assertEqual(done, ["lhm", "openrgb"])
        self.assertTrue((self.root / "LibreHardwareMonitor/LibreHardwareMonitor.exe").is_file())
        self.assertTrue((self.root / "OpenRGB/OpenRGB.exe").is_file())
        cfg = (self.root / "LibreHardwareMonitor/LibreHardwareMonitor.config").read_text(encoding="utf-8")
        self.assertIn('value="' + "cd" * 32 + '"', cfg)
        self.assertEqual(len(run.named("schtasks", "/Create")), 2)
        self.assertEqual(len(run.named("schtasks", "/Run")), 2)
        self.assertFalse((self.root / ".staging").exists())

    def test_openrgb_task_uses_the_configured_port(self):
        captured = {}
        real = tools.task_xml
        with mock.patch.object(tools, "task_xml", side_effect=lambda *a: captured.setdefault(Path(a[1]).name, a) and real(*a)):
            tools.install(self.job, self.root, Commands())
        self.assertIn("--server-port 6750", captured["OpenRGB.exe"][2])
        self.assertEqual(captured["OpenRGB.exe"][0], "S-1-5-21-7")

    def test_a_tampered_download_is_not_installed(self):
        Path(self.job["files"]["openrgb"]).write_bytes(b"something else")
        with self.assertRaisesRegex(tools.SetupError, "bytes|checksum"):
            tools.install(self.job, self.root, Commands())
        self.assertFalse((self.root / "OpenRGB").exists())
        self.assertFalse((self.root / ".staging").exists())

    def test_failed_task_registration_is_reported(self):
        with self.assertRaisesRegex(tools.SetupError, "start at sign-in"):
            tools.install(self.job, self.root, Commands(code=1))

    def test_vc_runtime_must_be_signed_by_microsoft(self):
        fake = self.dir / "vc_redist.x64.exe"
        fake.write_bytes(b"not really")
        run = Commands(code=1)                                         # the signature check says no
        with self.assertRaisesRegex(tools.SetupError, "signed by Microsoft"):
            tools.install({**self.job, "vcredist": str(fake)}, self.root, run)
        self.assertFalse(run.named("/install", "/quiet"))

    def test_remove_deletes_tasks_and_folder(self):
        tools.install(self.job, self.root, Commands())
        run = Commands()
        tools.remove(self.root, run)
        self.assertFalse(self.root.exists())
        self.assertEqual(len(run.named("schtasks", "/Delete")), 2)


class StatusTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name) / "Airglow Tools"
        p = mock.patch.object(tools, "tools_dir", return_value=self.root)
        p.start()
        self.addCleanup(p.stop)
        self.ctrl = SimpleNamespace(sensors={"online": False}, manager=FakeManager())

    def tearDown(self):
        self.tmp.cleanup()

    def by_id(self, procs=()):
        with mock.patch.object(tools, "_processes", side_effect=lambda exe: [(1, path) for name, path in procs if name == exe]):
            st = tools.status(self.ctrl)
        return st, {t["id"]: t for t in st["tools"]}

    def test_nothing_installed_offers_setup(self):
        st, t = self.by_id()
        self.assertTrue(st["setup_available"])
        self.assertFalse(t["lhm"]["working"] or t["lhm"]["other_copy"])

    def test_a_working_tool_needs_nothing(self):
        self.ctrl.sensors["online"] = True
        self.ctrl.manager.bridge.client.connected = True
        st, t = self.by_id()
        self.assertTrue(t["lhm"]["working"] and t["openrgb"]["working"])
        self.assertFalse(st["setup_available"])

    def test_someone_elses_unreachable_copy_is_left_alone(self):
        st, t = self.by_id([("LibreHardwareMonitor.exe", r"D:\tools\LHM\LibreHardwareMonitor.exe")])
        self.assertTrue(t["lhm"]["other_copy"])
        self.assertFalse(t["openrgb"]["other_copy"])

    def test_our_own_copy_is_not_someone_elses(self):
        exe = self.root / "LibreHardwareMonitor" / "LibreHardwareMonitor.exe"
        exe.parent.mkdir(parents=True)
        exe.write_text("x")
        st, t = self.by_id([("LibreHardwareMonitor.exe", None)])       # elevated: path can't be read
        self.assertFalse(t["lhm"]["other_copy"])
        self.assertTrue(st["installed_any"])


class AuthTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        p = mock.patch.object(tools.paths, "data_dir", return_value=Path(self.tmp.name))
        p.start()
        self.addCleanup(p.stop)
        tools._auth_cache = (None, {})

    def tearDown(self):
        self.tmp.cleanup()

    def test_header_only_when_airglow_set_lhm_up(self):
        self.assertEqual(tools.lhm_headers(), {})
        tools.auth_file().write_text(json.dumps({"user": "airglow", "password": "pw"}), encoding="utf-8")
        self.assertEqual(tools.lhm_headers(), {"Authorization": "Basic YWlyZ2xvdzpwdw=="})

    def test_sensor_reads_send_the_header(self):
        seen = {}

        def fake(req, timeout):
            seen.update(req.headers)
            raise OSError("offline")
        with mock.patch.object(sensors.urllib.request, "urlopen", side_effect=fake):
            self.assertIsNone(sensors.read_tree("http://127.0.0.1:8085", headers={"Authorization": "Basic x"}))
        self.assertEqual(seen.get("Authorization"), "Basic x")


class SetupFlowTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.data = Path(self.tmp.name) / "data"
        self.store = config.Store(self.data)
        self.ctrl = SimpleNamespace(sensors={"online": False}, manager=FakeManager())
        patches = [mock.patch.object(tools.paths, "data_dir", return_value=self.data),
                   mock.patch.object(tools, "tools_dir", return_value=Path(self.tmp.name) / "pf"),
                   mock.patch.object(tools, "_processes", return_value=[]),
                   mock.patch.object(tools, "pawnio_needed", return_value=False),
                   mock.patch.object(tools, "vcredist_needed", return_value=False),
                   mock.patch.object(tools, "current_user_sid", return_value="S-1-5-21-9"),
                   mock.patch.object(tools, "download", side_effect=lambda url, dest, *a, **k: Path(dest).write_bytes(b"z") and Path(dest))]
        for p in patches:
            p.start()
            self.addCleanup(p.stop)
        tools.JOB = tools.Job()
        tools._auth_cache = (None, {})

    def tearDown(self):
        self.tmp.cleanup()

    def wait(self):
        for _ in range(200):
            if not tools.JOB.busy():
                return tools.JOB.snapshot()
            time.sleep(0.02)
        self.fail("setup didn't finish")

    def elevated_ok(self, args):
        job = json.loads(Path(args[1]).read_text(encoding="utf-8"))
        self.job = job
        Path(job["result"]).write_text(json.dumps({"ok": True, "done": job["tools"]}), encoding="utf-8")
        return 0

    def test_success_stores_the_password_and_points_airglow_at_lhm(self):
        with mock.patch.object(tools, "run_elevated", side_effect=self.elevated_ok):
            tools.start_setup(self.ctrl, self.store)
            snap = self.wait()
        self.assertEqual(snap["state"], "done", snap)
        self.assertEqual(self.job["tools"], ["lhm", "openrgb"])
        self.assertEqual(self.job["user_sid"], "S-1-5-21-9")
        auth = json.loads(tools.auth_file().read_text(encoding="utf-8"))
        self.assertEqual(hashlib.sha256(auth["password"].encode()).hexdigest(), self.job["lhm_password_sha256"])
        self.assertNotIn(auth["password"], json.dumps(self.store.settings()))  # never in settings or bug reports
        self.assertEqual(self.store.settings()["lhm_url"], tools.LHM_URL)
        self.assertFalse((self.data / "downloads").exists())

    def test_saying_no_at_the_prompt_changes_nothing(self):
        with mock.patch.object(tools, "run_elevated", return_value=None):
            tools.start_setup(self.ctrl, self.store)
            snap = self.wait()
        self.assertEqual(snap["state"], "cancelled")
        self.assertFalse(tools.auth_file().exists())
        self.assertEqual(self.store.settings()["lhm_url"], "http://localhost:8085")

    def test_a_failed_elevated_step_is_reported(self):
        def failing(args):
            job = json.loads(Path(args[1]).read_text(encoding="utf-8"))
            Path(job["result"]).write_text(json.dumps({"ok": False, "error": "disk full"}), encoding="utf-8")
            return 1
        with mock.patch.object(tools, "run_elevated", side_effect=failing):
            tools.start_setup(self.ctrl, self.store)
            snap = self.wait()
        self.assertEqual((snap["state"], snap["error"]), ("failed", "disk full"))
        self.assertFalse(tools.auth_file().exists())

    def test_only_missing_tools_are_set_up(self):
        self.ctrl.manager.bridge.client.connected = True
        with mock.patch.object(tools, "run_elevated", side_effect=self.elevated_ok):
            tools.start_setup(self.ctrl, self.store)
            self.wait()
        self.assertEqual(self.job["tools"], ["lhm"])

    def test_nothing_to_do_is_refused(self):
        self.ctrl.sensors["online"] = True
        self.ctrl.manager.bridge.client.connected = True
        with self.assertRaises(tools.SetupError):
            tools.start_setup(self.ctrl, self.store)


if __name__ == "__main__":
    unittest.main()
