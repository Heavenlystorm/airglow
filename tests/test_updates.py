import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from airglow import __version__, config, updates

ROOT = Path(__file__).resolve().parent.parent


class FakeAsset:
    def __init__(self, version):
        self.Version, self.NotesMarkdown = version, f"notes for {version}"


class FakeInfo:
    def __init__(self, version, deltas=()):
        self.TargetFullRelease, self.DeltasToTarget = FakeAsset(version), list(deltas)


class FakeManager:
    def __init__(self, found=None, fail=None):
        self.found, self.fail = found, fail
        self.downloaded = self.applied = None

    def check_for_updates(self):
        if self.fail:
            raise RuntimeError(self.fail)
        return FakeInfo(self.found) if self.found else None

    def download_updates(self, info, progress=None):
        progress and progress(50)
        self.downloaded = info

    def wait_exit_then_apply_updates(self, info, silent=False, restart=True, restart_args=None):
        self.applied = (info, silent, restart, restart_args)


class VersionTests(unittest.TestCase):
    def test_semver_order(self):
        order = ["1.5.0", "1.6.0-beta.1", "1.6.0-beta.2", "1.6.0-beta.10", "1.6.0-rc.1", "1.6.0", "1.6.1", "1.10.0"]
        self.assertEqual(sorted(order, key=updates.parse_version), order)

    def test_newer_ignores_the_tag_prefix(self):
        self.assertTrue(updates.newer("v1.6.0", "1.5.0"))
        self.assertFalse(updates.newer("1.5.0", "v1.5.0"))


class MetaTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        cache = Path(self.tmp.name) / "cache" / "app-meta.json"
        patcher = mock.patch.object(updates, "_meta_cache", return_value=cache)
        patcher.start()
        self.addCleanup(patcher.stop)
        self.addCleanup(self.tmp.cleanup)

    def test_the_published_file_is_valid(self):
        meta = updates.clean_meta(json.loads((ROOT / "meta" / "app-meta.json").read_text(encoding="utf-8")))
        self.assertIn(1, meta["effectApi"])
        self.assertFalse(updates.newer(meta["minSupported"], __version__),
                         "app-meta.json would tell this very version it's unsupported")

    def test_rejects_bad_shapes(self):
        good = {"schema": 1, "minSupported": "1.6.0", "effectApi": [1], "notice": None}
        for bad in ({**good, "schema": 2}, {**good, "minSupported": "soon"}, {**good, "effectApi": []},
                    {**good, "effectApi": ["1"]}, {**good, "notice": "x" * 301}, [], None):
            with self.assertRaises(ValueError):
                updates.clean_meta(bad)

    def opener(self, payload):
        class Resp:
            def __enter__(self):
                return self

            def __exit__(self, *a):
                return False

            def read(self, n=-1):
                return payload

        return lambda req, timeout: Resp()

    def test_fetch_caches_and_falls_back_to_the_cache(self):
        body = json.dumps({"schema": 1, "minSupported": "1.0.0", "effectApi": [1], "notice": "Maintenance"}).encode()
        self.assertEqual(updates.fetch_meta(opener=self.opener(body))["notice"], "Maintenance")

        def offline(req, timeout):
            raise OSError("no network")
        self.assertEqual(updates.fetch_meta(opener=offline)["notice"], "Maintenance")
        self.assertEqual(updates.fetch_meta(opener=self.opener(b"{not json"))["notice"], "Maintenance")


class UpdaterTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        patcher = mock.patch.object(updates, "_meta_cache", return_value=Path(self.tmp.name) / "cache" / "m.json")
        patcher.start()
        self.addCleanup(patcher.stop)
        self.store = config.Store(self.tmp.name)
        self.now = 1000.0
        self.meta = None

    def updater(self, kind="velopack", managers=None, **kw):
        self.made = managers or {}
        return updates.Updater(self.store, kind=kind, managers=lambda ch: self.made.get(ch, self.made.get("any", [])),
                               fetch_meta=lambda: self.meta, clock=lambda: self.now, **kw)

    def test_nothing_new(self):
        u = self.updater(managers={"stable": [FakeManager()]})
        self.assertEqual(u.check_now()["state"], "latest")

    def test_downloads_and_restarts(self):
        mgr = FakeManager("9.0.0")
        u = self.updater(managers={"stable": [mgr]})
        quit_app = mock.Mock()
        u.quit_app = quit_app
        s = u.check_now()
        self.assertEqual((s["state"], s["latest"], s["progress"]), ("ready", "9.0.0", 100))
        self.assertIs(mgr.downloaded.TargetFullRelease.Version, "9.0.0")
        u.restart_to_update()
        self.assertEqual(mgr.applied[1:], (True, True, []))
        quit_app.assert_called_once()

    def test_restart_without_a_download_is_refused(self):
        with self.assertRaises(RuntimeError):
            self.updater(managers={"stable": [FakeManager()]}).restart_to_update()

    def test_beta_takes_the_newer_of_both_feeds(self):
        self.store.update_settings({"update_channel": "beta"})
        stable, beta = FakeManager("9.0.0"), FakeManager("9.1.0-beta.1")
        u = self.updater(managers={"beta": [stable, beta]})
        self.assertEqual(u.check_now()["latest"], "9.1.0-beta.1")
        self.assertIsNone(stable.downloaded)
        self.assertIsNotNone(beta.downloaded)

    def test_failures_are_reported_not_raised(self):
        u = self.updater(managers={"stable": [FakeManager(fail="GitHub answered 503")]})
        s = u.check_now()
        self.assertEqual((s["state"], s["error"]), ("error", "GitHub answered 503"))
        self.assertEqual(self.updater(managers={"stable": [FakeManager()]}).check_now()["state"], "latest")

    def test_scoop_installs_never_check(self):
        u = self.updater(kind="scoop", managers={"stable": [FakeManager("9.0.0")]})
        self.assertEqual(u.check_now()["state"], "idle")

    def test_manual_installs_only_report(self):
        manual = mock.Mock(return_value={"newer": True, "latest": "9.0.0", "url": "https://x", "notes": ""})
        u = self.updater(kind="manual", manual_check=manual)
        self.assertEqual(u.check_now()["state"], "available")

    def test_schedule_waits_for_the_opt_in(self):
        u = self.updater(managers={"stable": [FakeManager()]})
        self.assertFalse(u.due())                         # off by default: never goes online on its own
        self.store.update_settings({"online_checks": True})
        self.assertTrue(u.due())
        u.check_now()
        self.now += updates.CHECK_EVERY - 1
        self.assertFalse(u.due())
        self.now += 1
        self.assertTrue(u.due())

    def test_a_ready_update_is_not_checked_again(self):
        self.store.update_settings({"online_checks": True})
        mgr = FakeManager("9.0.0")
        u = self.updater(managers={"stable": [mgr]})
        u.check_now()
        self.now += updates.CHECK_EVERY * 2
        self.assertFalse(u.due())
        mgr.downloaded = None
        u.check_now()
        self.assertIsNone(mgr.downloaded)

    def test_meta_marks_old_versions_unsupported(self):
        self.meta = {"schema": 1, "minSupported": "99.0.0", "effectApi": [1], "notice": "Please update"}
        s = self.updater(managers={"stable": [FakeManager()]}).check_now()
        self.assertTrue(s["unsupported"])
        self.assertEqual(s["notice"], "Please update")


if __name__ == "__main__":
    unittest.main()
