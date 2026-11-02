import hashlib
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from airglow import fxformat, fxlibrary


def source(name, author=""):
    return fxformat.to_html({"name": name, "author": author, "layers": [{"type": "solid"}]})


class PackLibraryTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.lib = fxlibrary.Library(self.tmp.name)

    def install(self, items):
        self.assertTrue(callable(getattr(self.lib, "install_pack", None)), "atomic pack installation is missing")
        return self.lib.install_pack(items)

    def test_copy_uses_fresh_ids_and_preserves_existing_source(self):
        original = self.lib.save_mine(source("Sunset"))
        result = self.install([{"source": source("Sunset"), "origin": "mine"},
                               {"source": source("Sunset"), "origin": "mine"}])
        self.assertEqual([e["fx_id"] for e in result], ["fx:me.sunset-2", "fx:me.sunset-3"])
        self.assertEqual(Path(original["path"]).read_text(encoding="utf-8"), source("Sunset"))
        self.assertEqual(self.install([]), [])

    def test_every_source_is_validated_before_any_file_is_installed(self):
        with self.assertRaises(fxformat.FormatError):
            self.install([{"source": source("Valid"), "origin": "mine"},
                          {"source": "<script src='https://bad.invalid/x.js'></script>", "origin": "mine"}])
        self.assertEqual(list(Path(self.tmp.name).rglob("*.html")), [])
        self.assertEqual(self.lib.rev, 0)

    def test_later_write_failure_removes_only_new_files_and_preserves_revision(self):
        old = self.lib.save_mine(source("Before"))
        revision = self.lib.rev
        write = fxlibrary._write

        def fail_second(path, text):
            if "second" in path.name:
                raise OSError("disk full")
            return write(path, text)

        with mock.patch.object(fxlibrary, "_write", side_effect=fail_second):
            with self.assertRaisesRegex(OSError, "disk full"):
                self.install([{"source": source("First"), "origin": "mine"},
                              {"source": source("Second"), "origin": "mine"}])
        self.assertEqual(self.lib.rev, revision)
        self.assertEqual([e["fx_id"] for e in self.lib.all()], [old["fx_id"]])
        self.assertEqual(Path(old["path"]).read_text(encoding="utf-8"), source("Before"))

    def test_community_requires_checksum_and_cannot_replace_an_existing_file(self):
        text = source("Shared", "jane-dev")
        digest = hashlib.sha256(text.encode()).hexdigest()
        with self.assertRaises(fxformat.FormatError):
            self.install([{"source": text, "origin": "community", "sha256": "wrong"}])
        result = self.install([{"source": text, "origin": "community", "sha256": digest}])
        self.assertEqual(result[0]["origin"], "community")
        with self.assertRaisesRegex(fxformat.FormatError, "already installed"):
            self.install([{"source": text, "origin": "community", "sha256": digest}])
        self.assertEqual(len(self.lib.all()), 1)


if __name__ == "__main__":
    unittest.main()
