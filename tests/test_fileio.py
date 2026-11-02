"""Atomic writes retain the last complete file and leave no failed temporary files."""

import importlib
import tempfile
import unittest
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from unittest import mock


class AtomicTextTests(unittest.TestCase):
    def writer(self):
        self.assertIsNotNone(importlib.util.find_spec("airglow.fileio"), "shared atomic writer is missing")
        return importlib.import_module("airglow.fileio")

    def test_failed_promotion_preserves_the_target_and_cleans_temporary_file(self):
        fileio = self.writer()
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp, "settings.json")
            path.write_text("before", encoding="utf-8")
            with mock.patch.object(Path, "replace", side_effect=OSError("disk full")):
                with self.assertRaisesRegex(OSError, "disk full"):
                    fileio.atomic_write_text(path, "after")
            self.assertEqual(path.read_text(encoding="utf-8"), "before")
            self.assertEqual([p.name for p in Path(tmp).iterdir()], ["settings.json"])

    def test_permission_retry_is_bounded_and_keeps_the_old_file(self):
        fileio = self.writer()
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp, "state.json")
            path.write_text("before", encoding="utf-8")
            with mock.patch.object(Path, "replace", side_effect=PermissionError("locked")) as replace, \
                    mock.patch.object(fileio.time, "sleep"):
                with self.assertRaises(PermissionError):
                    fileio.atomic_write_text(path, "after")
            self.assertEqual(replace.call_count, 5)
            self.assertEqual(path.read_text(encoding="utf-8"), "before")
            self.assertEqual(len(list(Path(tmp).iterdir())), 1)

    def test_concurrent_writes_never_share_a_temporary_file(self):
        fileio = self.writer()
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp, "shared.json")
            contents = [str(n) * 10000 for n in range(8)]
            with ThreadPoolExecutor(max_workers=8) as pool:
                list(pool.map(lambda content: fileio.atomic_write_text(path, content), contents))
            self.assertIn(path.read_text(encoding="utf-8"), contents)
            self.assertEqual(len(list(Path(tmp).iterdir())), 1)


if __name__ == "__main__":
    unittest.main()
