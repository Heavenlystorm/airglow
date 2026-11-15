import os
import socket
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

from airglow import autostart, fxlibrary, install, paths, studioapi, tools
from tests.isolation import run_python


class TestIsolationTests(unittest.TestCase):
    def test_application_defaults_live_in_one_temporary_home(self):
        scratch = Path(tempfile.gettempdir()).resolve()
        home = Path.home().resolve()
        self.assertTrue(home.is_relative_to(scratch), home)
        self.assertEqual(home, Path(os.environ["HOME"]))
        self.assertEqual(home, Path(os.environ["USERPROFILE"]))
        self.assertEqual(paths.data_dir(), home / "AppData" / "Roaming" / "Airglow")
        self.assertEqual(install.running_marker(), paths.data_dir() / "running")
        self.assertEqual(fxlibrary.library().root, paths.data_dir() / "effects")
        self.assertEqual(autostart.INSTALL_DIR, home / "AppData" / "Local" / "Airglow" / "current")
        self.assertEqual(studioapi.editor_dir(), home / "Documents" / "Airglow" / "Effects")
        self.assertTrue(tools.tools_dir().is_relative_to(home))

    def test_running_marker_is_created_and_cleared_inside_the_isolated_profile(self):
        marker = install.running_marker()
        self.assertTrue(marker.is_relative_to(Path(tempfile.gettempdir())))
        install.mark_running()
        self.addCleanup(install.clear_running)
        self.assertEqual(marker.read_text(encoding="ascii"), str(os.getpid()))
        install.clear_running()
        self.assertFalse(marker.exists())

    def test_registry_mutations_are_denied_before_windows_receives_them(self):
        import winreg

        # Zero is an invalid handle. Even if the guard regresses this cannot mutate any registry key.
        with self.assertRaisesRegex(AssertionError, "winreg.SetValue"):
            winreg.SetValueEx(0, "test-isolation", 0, winreg.REG_SZ, "test")

    def test_unapproved_child_cannot_run(self):
        with self.assertRaisesRegex(AssertionError, "subprocess.Popen"):
            subprocess.run([sys.executable, "-c", "pass"], check=True, timeout=5)

    def test_fixture_child_inherits_the_isolated_profile(self):
        child = run_python("from airglow import paths; print(paths.data_dir())")
        self.assertEqual(Path(child.stdout.strip()), paths.data_dir())

    def test_only_test_hid_and_dynamic_lighting_backends_can_open(self):
        import asyncio
        import hid
        from airglow.dynlight import DynamicLightingBridge

        with self.assertRaisesRegex(AssertionError, "HID device open"):
            hid.device()

        enumeration = SimpleNamespace(find_all_async_aqs_filter=mock.AsyncMock(return_value=[]))
        with mock.patch.dict(sys.modules, {
            "winrt.windows.devices.enumeration": SimpleNamespace(DeviceInformation=enumeration),
            "winrt.windows.devices.lights": SimpleNamespace(LampArray=SimpleNamespace(get_device_selector=str)),
        }), self.assertRaisesRegex(AssertionError, "Dynamic Lighting device open"):
            asyncio.run(DynamicLightingBridge._scan())

    def test_external_socket_connection_is_denied(self):
        with socket.socket() as conn:
            # The invalid destination port also prevents a connection when the guard is absent.
            with self.assertRaisesRegex(AssertionError, "socket.connect"):
                conn.connect(("127.0.0.1", 0))

    def test_external_address_resolution_is_denied(self):
        with self.assertRaisesRegex(AssertionError, "socket.getaddrinfo"):
            socket.getaddrinfo("192.0.2.1", 80)
