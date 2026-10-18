import unittest

from airglow import webview2

RUNTIME = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
CANARY = "{65C35B14-6C1D-4122-AC46-7148CC9D6497}"
EDGE_BROWSER = "{56EB18F8-B008-4CBD-B6D2-8C97FE7E9062}"


class FakeKey:
    def __init__(self, value):
        self.value = value

    def __enter__(self):
        return self

    def __exit__(self, *a):
        return False


class FakeRegistry:
    """Stands in for winreg: only the listed (hive, path) keys exist, each holding its "pv" version."""
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE = "HKCU", "HKLM"

    def __init__(self, keys):
        self.keys = keys

    def OpenKey(self, hive, path):
        if (hive, path) not in self.keys:
            raise FileNotFoundError(path)
        return FakeKey(self.keys[(hive, path)])

    def QueryValueEx(self, key, name):
        assert name == "pv"
        return key.value, 1


def versions(keys):
    return list(webview2._registry_versions(FakeRegistry(keys)))


class InstalledTests(unittest.TestCase):
    def test_a_current_runtime_counts(self):
        self.assertTrue(webview2.installed(["131.0.2903.112"]))

    def test_nothing_found_means_missing(self):
        self.assertFalse(webview2.installed([]))

    def test_a_runtime_that_is_too_old_or_unreadable_does_not_count(self):
        self.assertFalse(webview2.installed(["0", "85.0.564.41", "not a version", ""]))
        self.assertTrue(webview2.installed(["0", "86.0.622.0"]))

    def test_one_good_entry_is_enough(self):
        self.assertTrue(webview2.installed(["0", "garbage", "120.0.1.1"]))


class RegistryTests(unittest.TestCase):
    def test_machine_wide_runtime_on_64_bit_windows(self):
        key = ("HKLM", rf"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{RUNTIME}")
        self.assertEqual(versions({key: "131.0.2903.112"}), ["131.0.2903.112"])

    def test_per_user_runtime(self):
        key = ("HKCU", rf"SOFTWARE\Microsoft\EdgeUpdate\Clients\{RUNTIME}")
        self.assertEqual(versions({key: "130.0.1.1"}), ["130.0.1.1"])

    def test_32_bit_windows_and_the_other_channels(self):
        self.assertEqual(versions({("HKLM", rf"SOFTWARE\Microsoft\EdgeUpdate\Clients\{RUNTIME}"): "99.0"}), ["99.0"])
        self.assertEqual(versions({("HKCU", rf"SOFTWARE\Microsoft\EdgeUpdate\Clients\{CANARY}"): "140.0"}), ["140.0"])

    def test_an_empty_registry_means_missing(self):
        self.assertEqual(versions({}), [])
        self.assertFalse(webview2.installed(webview2._registry_versions(FakeRegistry({}))))

    def test_other_edge_update_products_are_ignored(self):
        key = ("HKLM", rf"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{EDGE_BROWSER}")
        self.assertEqual(versions({key: "131.0.1.1"}), [])


if __name__ == "__main__":
    unittest.main()
