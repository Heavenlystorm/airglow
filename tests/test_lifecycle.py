import tempfile
import unittest
from contextlib import ExitStack
from pathlib import Path
from unittest import mock

from airglow import app, config, system, tools, tray, window
from airglow.lighting import studio
from airglow.transactions import RecoveryError
from tests.fakes import FakeManager


class StartupTests(unittest.TestCase):
    def setUp(self):
        self.events = []
        self.patch(app, "setup_logging")
        self.patch(app.install, "run_hooks")
        self.patch(app.updates, "apply_pending")
        self.patch(app, "claim_single_instance", return_value=True)
        self.patch(app, "close_mutex", side_effect=self.record("mutex close"))
        temp = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.patch(app.paths, "data_dir", return_value=temp)
        self.patch(app.ctypes, "windll")
        self.patch(app.autostart, "repair")
        self.offered = self.patch(app, "offer_webview2_download")
        self.dialog = self.patch(app, "message_box")
        self.runtime = self.patch(app.webview2, "installed", return_value=True)
        self.store = self.patch(app, "Store")
        self.store.return_value.config = {"port": 8787}
        self.store.return_value.dir = temp
        self.store.return_value.settings.return_value = {"design": "firefly", "windows_shortcut": config.SHORTCUT_OFF}
        self.ctrl = self.patch(app, "Controller")
        self.ctrl.return_value.shutdown.side_effect = self.record("controller stop")
        self.updater = self.patch(app.updates, "Updater").return_value
        self.updater.stop.side_effect = self.record("updater stop")
        self.server = self.patch(app, "start_server").return_value
        self.server.server_address = ("127.0.0.1", 8787)
        self.server.hooks = {}
        self.server.shutdown.side_effect = self.record("api stop")
        self.server.server_close.side_effect = self.record("api close")
        self.thread = self.patch(app.threading, "Thread").return_value
        self.thread.is_alive.return_value = True
        self.patch(studio.HOST, "close", side_effect=self.record("host stop"))
        self.patch(studio.HOST, "url")
        self.tray = self.patch(tray, "Tray")
        self.tray.return_value.stop.side_effect = self.record("tray stop")
        self.window = self.enterContext(mock.patch("airglow.window.Window"))
        self.window.return_value.destroy.side_effect = self.record("window close")
        self.patch(app.install, "mark_running", side_effect=self.record("marker write"))
        self.patch(app.install, "clear_running", side_effect=self.record("marker clear"))
        # The normal crash hooks are process-global.
        self.patch(app.sys, "excepthook")
        self.patch(app.threading, "excepthook")

    def patch(self, target, attribute, **kwargs):
        return self.enterContext(mock.patch.object(target, attribute, **kwargs))

    def record(self, event):
        return lambda: self.events.append(event)

    def test_background_start_preflights_runtime_before_acquiring_hardware(self):
        self.runtime.return_value = False
        with self.assertLogs(app.log, level="ERROR") as messages:
            self.assertEqual(app.main(["--background"]), 1)
        self.assertIn("Evergreen Runtime", messages.output[0])
        self.ctrl.assert_not_called()
        self.window.assert_not_called()
        self.offered.assert_not_called()
        self.assertEqual(self.events, ["mutex close"])

    def test_window_constructor_failure_closes_bound_api_without_shutdown(self):
        self.window.side_effect = RuntimeError("window creation failed")
        with self.assertLogs(app.log, level="ERROR"):
            self.assertEqual(app.main([]), 1)
        self.dialog.assert_called_once()
        self.assertIn("api close", self.events)
        self.assertIn("controller stop", self.events)
        self.assertIn("updater stop", self.events)
        self.assertNotIn("api stop", self.events)
        self.assertEqual(self.events[-1], "mutex close")

    def test_tray_start_failure_closes_resources_even_when_api_stop_fails(self):
        self.tray.return_value.start.side_effect = RuntimeError("tray registration failed")
        self.server.shutdown.side_effect = OSError("api stop failed")
        with self.assertLogs(app.log, level="ERROR") as messages:
            self.assertEqual(app.main([]), 1)
        self.assertTrue(any("api stop failed" in entry for entry in messages.output))
        self.dialog.assert_called_once()
        self.assertIn("api close", self.events)
        self.assertIn("window close", self.events)
        self.assertIn("tray stop", self.events)
        self.assertIn("controller stop", self.events)
        self.assertLess(self.events.index("host stop"), self.events.index("controller stop"))
        self.assertNotIn("marker write", self.events)
        self.assertEqual(self.events[-1], "mutex close")

    def test_two_quit_requests_stop_backend_resources_once(self):
        def quitting(on_start):
            self.updater.quit_app()
            self.updater.quit_app()

        self.window.run.side_effect = quitting
        self.assertEqual(app.main([]), 0)
        for event in ("tray stop", "controller stop", "marker clear"):
            self.assertEqual(self.events.count(event), 1, self.events)
        self.assertEqual(self.events[-1], "mutex close")

    def test_native_shortcut_service_becomes_available_with_the_window_and_defaults_to_off(self):
        loaded = self.window.return_value.win.events.loaded
        loaded.__iadd__.return_value = loaded

        def run(on_start):
            shortcut = self.server.hooks["window_shortcut"]
            self.assertFalse(shortcut.status()["available"])
            loaded.__iadd__.call_args.args[0]()
            self.assertTrue(shortcut.status()["available"])
            self.assertFalse(shortcut.status()["registered"])
            self.assertEqual(shortcut.status()["saved"], config.SHORTCUT_OFF)

        self.window.run.side_effect = run
        self.assertEqual(app.main([]), 0)
        self.assertFalse(self.server.hooks["window_shortcut"].status()["available"])

    def test_quit_can_retry_when_the_first_window_destruction_fails(self):
        def quitting(on_start):
            self.updater.quit_app()
            self.updater.quit_app()
            self.assertEqual(self.window.return_value.destroy.call_count, 2)

        self.window.run.side_effect = quitting
        self.window.return_value.destroy.side_effect = [OSError("window still closing"), None]
        with self.assertLogs(app.log, level="INFO") as messages:
            self.assertEqual(app.main([]), 0)
        self.assertEqual(sum(line.endswith(":quitting") for line in messages.output), 1)
        self.assertEqual(self.events.count("controller stop"), 1)

    def test_settings_recovery_failure_reports_its_diagnostic_and_releases_instance(self):
        self.store.side_effect = RecoveryError("Keep the recovery record and settings files.")
        with self.assertLogs(app.log, level="ERROR"):
            self.assertEqual(app.main([]), 1)
        self.assertIn("Keep the recovery record", self.dialog.call_args.args[0])
        self.ctrl.assert_not_called()
        self.assertEqual(self.events, ["mutex close"])


class ControllerCleanupTests(unittest.TestCase):
    def test_power_and_devices_close_if_controller_thread_never_started(self):
        with tempfile.TemporaryDirectory() as directory:
            manager = FakeManager()
            ctrl = app.Controller(app.Store(directory), manager=manager, read_sensors=dict)
            ctrl._power = mock.Mock()
            ctrl._thread = app.threading.Thread()
            with mock.patch.object(ctrl.engine, "stop", side_effect=OSError("lighting driver failed")), \
                    mock.patch.object(manager, "close") as close, \
                    self.assertLogs("airglow.controller", level="ERROR") as messages:
                ctrl.shutdown()
                ctrl.shutdown()
        ctrl._power.stop.assert_called_once_with()
        close.assert_called_once_with()
        self.assertTrue(any("lighting driver failed" in entry for entry in messages.output))

    def test_lighting_writer_stops_if_engine_thread_never_started(self):
        with tempfile.TemporaryDirectory() as directory:
            ctrl = app.Controller(app.Store(directory), manager=FakeManager(), read_sensors=dict)
            ctrl.engine._thread = app.threading.Thread()
            writer = ctrl.engine._writers["test-device"] = mock.Mock()
            ctrl.engine.stop()
        writer.stop.assert_called_once_with()

    def test_socket_closes_even_when_preview_cleanup_raises(self):
        server = app.api.Server.__new__(app.api.Server)
        server.socket = mock.Mock()
        server.previews = mock.Mock()
        server.previews.close.side_effect = OSError("preview close failed")
        with self.assertRaisesRegex(OSError, "preview close failed"):
            server.server_close()
        server.socket.close.assert_called_once_with()


class RestartTests(unittest.TestCase):
    def test_own_native_process_id_can_be_verified_without_waiting(self):
        started = system.current_process_started_at()
        self.assertGreater(started, 0)
        self.assertFalse(system.wait_for_process_exit(app.os.getpid(), started, 0))

    def test_tray_launches_one_child_with_exact_owner_identity_before_quit(self):
        events = []
        quit_app = lambda: events.append("quit")
        with mock.patch.object(tray.Tray, "_image"), mock.patch.object(tray.pystray, "Icon"), \
                mock.patch.object(system, "current_process_started_at", create=True, return_value=4321), \
                mock.patch.object(tools, "self_command", side_effect=lambda args: ("Airglow.exe", args)), \
                mock.patch.object(tray.subprocess, "Popen", side_effect=lambda args, **kw: events.append(args)):
            ctrl = mock.Mock()
            ctrl.effective_scene = {"on": True}
            ctrl.engine.wind_down.return_value = {"phase": "idle"}
            icon = tray.Tray(ctrl, mock.Mock(), mock.Mock(), quit_app)
            restart = next(item for item in icon._menu().items if item.text == "Restart Airglow")
            restart(icon.icon)
            restart(icon.icon)
        self.assertEqual(events, [
            ["Airglow.exe", "--restart-from-pid", str(app.os.getpid()), "4321"], "quit",
        ])

    def test_slow_owner_exit_precedes_mutex_claim_and_consumes_restart_args(self):
        events = []
        with ExitStack() as stack:
            def patch(target, attribute, **kwargs):
                return stack.enter_context(mock.patch.object(target, attribute, **kwargs))

            patch(app.install, "run_hooks")
            patch(app, "setup_logging")
            patch(app, "running_port", return_value=8787)
            patch(app, "close_mutex")
            patch(app, "Store")
            patch(app.webview2, "installed", return_value=False)
            patch(app, "offer_webview2_download")
            patch(system, "wait_for_process_exit",
                  side_effect=lambda *args, **kwargs: events.append(("wait", args, kwargs)) or True)
            patch(app, "claim_single_instance", side_effect=lambda *a, **k: events.append(("claim",)) or True)
            apply = patch(app.updates, "apply_pending")
            with self.assertLogs(app.log, level="ERROR"):
                self.assertEqual(app.main(["--restart-from-pid", "1234", "4321"]), 1)
        self.assertEqual(events, [("wait", (1234, 4321, 60), {}), ("claim",)])
        apply.assert_called_once_with([])

    def test_timeout_recovers_without_claiming_mutex_or_opening_hardware(self):
        with mock.patch.object(app.install, "run_hooks"), mock.patch.object(app, "setup_logging"), \
                mock.patch.object(app, "running_port", return_value=8787), \
                mock.patch.object(app, "Store"), \
                mock.patch.object(system, "wait_for_process_exit", return_value=False), \
                mock.patch.object(app, "claim_single_instance", return_value=False) as claim, \
                mock.patch.object(app, "message_box") as dialog:
            with self.assertLogs(app.log, level="ERROR"):
                self.assertEqual(app.main(["--restart-from-pid", "1234", "4321"]), 1)
        claim.assert_not_called()
        self.assertIn("60 seconds", dialog.call_args.args[0])

    def test_reused_pid_does_not_wait_on_unrelated_process(self):
        kernel = mock.Mock()
        kernel.OpenProcess.return_value = 42
        with mock.patch.object(system, "kernel32", kernel), \
                mock.patch.object(system, "_process_started_at", return_value=4322):
            self.assertTrue(system.wait_for_process_exit(1234, 4321, 60))
        kernel.WaitForSingleObject.assert_not_called()
        kernel.CloseHandle.assert_called_once_with(42)

    def test_wait_uses_same_verified_handle_and_releases_it_after_timeout(self):
        kernel = mock.Mock()
        kernel.OpenProcess.return_value = 42
        kernel.WaitForSingleObject.return_value = 258
        with mock.patch.object(system, "kernel32", kernel), \
                mock.patch.object(system, "_process_started_at", return_value=4321):
            self.assertFalse(system.wait_for_process_exit(1234, 4321, 60))
        kernel.WaitForSingleObject.assert_called_once_with(42, 60000)
        kernel.CloseHandle.assert_called_once_with(42)


class NativeUiLifecycleTests(unittest.TestCase):
    def test_power_stop_wakes_message_pump_and_ignores_further_power_events(self):
        handle = 42
        suspend = mock.Mock()
        events = system.PowerEvents({"suspend": suspend})
        events._hwnd = handle
        events._thread = mock.Mock()
        events._thread.is_alive.return_value = False
        with mock.patch.object(system, "user32") as user32:
            events.stop()
            events.stop()
            events._wndproc(handle, system.WM_POWERBROADCAST, system.PBT_APMSUSPEND, 0)
        user32.PostMessageW.assert_called_once_with(handle, system.WM_CLOSE, 0, 0)
        events._thread.join.assert_called_once_with(timeout=2)
        suspend.assert_not_called()

    def test_tray_start_reports_an_icon_loop_failure_to_startup(self):
        with mock.patch.object(tray.Tray, "_image"), mock.patch.object(tray.pystray, "Icon") as icon, \
                mock.patch.object(tray.threading, "excepthook"):
            icon.return_value.run.side_effect = OSError("notification area unavailable")
            app_tray = tray.Tray(mock.Mock(), mock.Mock(), mock.Mock(), mock.Mock())
            self.addCleanup(app_tray.stop)
            with self.assertRaisesRegex(RuntimeError, "tray"):
                app_tray.start()

    def test_quit_before_native_window_is_ready_closes_it_when_shown(self):
        native = mock.Mock()
        native.events.closing = mock.MagicMock()
        native.events.shown = mock.MagicMock()
        native.events.closed.is_set.return_value = False
        with mock.patch.object(window.webview, "create_window", return_value=native):
            app_window = window.Window("http://127.0.0.1:8787/")
        app_window.destroy()
        native.destroy.assert_not_called()
        app_window._style_title_bar()
        app_window.destroy()
        native.destroy.assert_called_once_with()
