"""Regressions for the November 4 audit; all files and servers are isolated fixtures."""

import copy
import json
import os
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from unittest import mock

from airglow import api, config, fans, fileio, lhmfans, lhmhttp, sensors, updates
from airglow.automation import validate_rule
from airglow.controller import Controller
from tests.fakes import FakeManager, FakeNzxt, signals


class SettingsAuditTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def test_wrong_top_level_shapes_are_quarantined_with_exact_original_bytes(self):
        for name, wrong in (("config.json", []), ("state.json", []), ("overview.json", []),
                            ("presets.json", {"format": 2, "presets": [{"name": "Neon"}]}),
                            ("automations.json", {"rules": []})):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                path = Path(directory, name)
                original = json.dumps(wrong).encode()
                path.write_bytes(original)
                store = config.Store(directory)
                self.assertEqual(path.with_name(name + ".corrupt").read_bytes(), original)
                self.assertTrue(store._load_failed)
                self.assertFalse(path.exists(), "startup must not replace a quarantined file")

    def test_future_schema_blocks_every_settings_write_even_after_another_load_error(self):
        for broken in (False, True):
            with self.subTest(broken=broken), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                original = json.dumps({"schema": config.SCHEMA_VERSION + 1, "future_only_field": {"x": 1}})
                (root / "config.json").write_text(original, encoding="utf-8")
                if broken:
                    (root / "presets.json").write_text('{"future": []}', encoding="utf-8")
                store = config.Store(root)
                before = copy.deepcopy(store.config)
                for name in ("config.json", "presets.json", "state.json", "automations.json", "overview.json"):
                    with self.subTest(name=name), self.assertRaises(ValueError):
                        store._write(name, {})
                with self.assertRaises(ValueError):
                    store.update_settings({"temp_unit": "F"})
                self.assertEqual(store.config, before)
                self.assertEqual((root / "config.json").read_text(encoding="utf-8"), original)

    def test_unchanged_dangling_rule_survives_save_and_profile_install(self):
        store = config.Store(self.root)
        rule = validate_rule({"id": "nightly", "when": {"type": "locked"},
                              "then": {"type": "preset", "preset": "silent"}})
        store.set_rules([rule], validate_rule)
        store.set_presets([p for p in store.get_presets() if p["id"] != "silent"])
        strict = lambda r: validate_rule(r, {p["id"] for p in store.get_presets()}, set(store.curves()))
        store.set_rules([rule, {"id": "other", "when": {"type": "locked"},
                                "then": {"type": "lights_off"}}], strict)
        self.assertEqual(store.get_rules()[0], rule)
        incoming = {"id": "installed", "when": {"type": "locked"},
                    "then": {"type": "preset", "preset": "new-profile"}}
        preset = {**store.get_presets()[0], "id": "new-profile", "name": "New"}
        store.install_profile(preset, [incoming])
        self.assertEqual(store.get_rules()[0], rule)
        for changed in ({**rule, "id": "new-invalid"}, {**rule, "enabled": False}):
            with self.assertRaises(ValueError):
                store.set_rules([changed], strict)


class FanSafetyAuditTests(unittest.TestCase):
    def test_https_sensor_addresses_keep_using_verified_tls(self):
        connection = mock.MagicMock()
        response = connection.getresponse.return_value.__enter__.return_value
        response.status = 200
        response.read.return_value = b'{"Children": []}'
        with mock.patch.object(lhmhttp.http.client, "HTTPSConnection", return_value=connection) as https:
            self.assertEqual(sensors.read_tree("https://sensors.example", headers={"Authorization": "Basic x"}), {"Children": []})
        https.assert_called_once_with("sensors.example", None, timeout=1.5)
        connection.request.assert_called_once_with("GET", "/data.json", body=None, headers={"Authorization": "Basic x"})

    def test_slow_dns_lookup_cannot_overrun_the_total_sensor_deadline(self):
        finished = threading.Event()
        def resolve(*args, **kwargs):
            try:
                time.sleep(0.2)
                return [(socket.AF_INET, socket.SOCK_STREAM, 6, "", ("127.0.0.1", 1))]
            finally:
                finished.set()
        with mock.patch.object(lhmhttp.socket, "getaddrinfo", side_effect=resolve):
            started = time.monotonic()
            result = sensors.read_tree("http://audit-slow-dns.invalid", timeout=0.05)
            elapsed = time.monotonic() - started
            finished.wait(1)
        self.assertIsNone(result)
        self.assertLess(elapsed, 0.15)

    def test_hotspot_is_in_max_but_explicit_gpu_still_means_edge(self):
        reading = {"cpu_temp": 62, "gpu_temp": 85, "gpu_hotspot": 105}
        self.assertEqual(fans.source_temp(reading, "max"), 105)
        self.assertEqual(fans.source_temp(reading, "gpu"), 85)
        self.assertEqual(fans.target_duty(config.DEFAULT_CURVES["performance"],
                                          fans.source_temp(reading, "max"), 30), 100)

    def test_stale_tree_cannot_drive_native_fans_from_cached_temperature(self):
        with tempfile.TemporaryDirectory() as directory:
            store = config.Store(directory)
            store.update_settings({"fan_failsafe_duty": 70})
            hub = FakeNzxt()
            manager = FakeManager(hub)
            manager.maintain(True)
            ctrl = Controller(store, manager=manager, signals=signals, async_writes=False)
            ctrl._temp_seen = True
            ctrl.sensors = {"online": True, "cpu_temp": 40, "gpu_temp": 35}
            ctrl.lhm.tree = {"Children": []}
            ctrl.lhm.last_success = time.monotonic() - 20
            ctrl._run_fans()
            self.assertEqual(ctrl.fan_target, 70)

    def test_shutdown_forced_pass_has_a_wall_clock_budget(self):
        clock, attempts = [100.0], []
        def send(_url, sid, duty, _headers):
            attempts.append((sid, duty))
            clock[0] += 0.6
            raise OSError("silent LHM")
        driver = lhmfans.LhmFans(lambda: "fixture", send=send)
        driver.sent = {f"/control/{i}": (30, 0) for i in range(32)}
        with mock.patch.object(lhmfans.time, "monotonic", side_effect=lambda: clock[0]):
            driver.failsafe(70)
        self.assertLessEqual(clock[0] - 100, 3.0)
        self.assertGreater(len(attempts), 0)
        self.assertEqual(len(driver.sent), 32, "unattempted channels must remain owned for release/retry")

    def test_stale_tree_is_not_published_as_a_new_sample(self):
        poller = sensors.LhmPoller(lambda: "fixture")
        poller.tree = {"Children": []}
        poller.last_success = time.monotonic() - 20
        self.assertIsNone(poller.fresh_tree)
        poller.last_success = time.monotonic()
        self.assertIs(poller.fresh_tree, poller.tree)

    def test_trickling_sensor_response_has_a_total_deadline_including_headers(self):
        for partial in (b"HTTP/1.1 200 OK\r\nX-Slow: ",
                        b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n{"):
            with self.subTest(partial=partial), socket.socket() as listener:
                listener.bind(("127.0.0.1", 0))
                listener.listen()
                stopped = threading.Event()
                def serve():
                    conn, _ = listener.accept()
                    with conn:
                        conn.recv(4096)
                        try:
                            conn.sendall(partial)
                            end = time.monotonic() + 1.0
                            while time.monotonic() < end and not stopped.wait(0.02):
                                conn.sendall(b" ")
                        except OSError:
                            pass
                thread = threading.Thread(target=serve, daemon=True)
                thread.start()
                started = time.monotonic()
                try:
                    self.assertIsNone(sensors.read_tree(f"http://127.0.0.1:{listener.getsockname()[1]}", timeout=0.15))
                    self.assertLess(time.monotonic() - started, 0.75)
                finally:
                    stopped.set()
                    thread.join(1)


class CacheAndFileAuditTests(unittest.TestCase):
    def test_test_effect_library_is_cleaned_when_the_process_exits(self):
        result = subprocess.run([sys.executable, "-c", "import tests; print(tests._effects.name)"],
                                capture_output=True, text=True, check=True)
        self.assertFalse(Path(result.stdout.strip()).exists())

    def test_concurrent_catalog_requests_load_once_without_duplicates(self):
        calls = []
        def load():
            calls.append(1)
            time.sleep(0.02)
            return [{"id": "one"}]
        with mock.patch.object(api, "_CATALOG", []), mock.patch.object(api.lightsetup, "load_catalog", side_effect=load):
            with ThreadPoolExecutor(max_workers=8) as pool:
                results = list(pool.map(lambda _: api._catalog(), range(8)))
            self.assertEqual(len(calls), 1)
            self.assertTrue(all(r == [{"id": "one"}] for r in results))

    def test_atomic_writer_sweeps_old_orphans_but_keeps_active_and_unrelated_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            orphan = root / ".config.json.abcdefgh.tmp"
            active = root / ".config.json.abcdefgi.tmp"
            unrelated = root / ".other.json.abcdefgh.tmp"
            for path in (orphan, active, unrelated):
                path.write_text("partial", encoding="utf-8")
            old = time.time() - 3 * 24 * 3600
            os.utime(orphan, (old, old))
            os.utime(unrelated, (old, old))
            fileio.atomic_write_text(root / "config.json", "complete")
            self.assertFalse(orphan.exists())
            self.assertTrue(active.exists())
            self.assertTrue(unrelated.exists())

    def test_app_meta_uses_the_atomic_writer(self):
        meta = {"schema": 1, "minSupported": "1.0.0", "effectApi": [1], "notice": None}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory, "cache", "app-meta.json")
            response = mock.MagicMock()
            response.__enter__.return_value.read.return_value = json.dumps(meta).encode()
            with mock.patch.object(updates, "_meta_cache", return_value=path), \
                    mock.patch.object(fileio, "atomic_write_text", wraps=fileio.atomic_write_text) as write:
                self.assertEqual(updates.fetch_meta(opener=lambda *a, **kw: response), meta)
                write.assert_called_once()
                self.assertEqual(json.loads(path.read_text(encoding="utf-8")), meta)
