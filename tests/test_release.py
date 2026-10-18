import hashlib
import tempfile
import unittest
from pathlib import Path

from tools import release

CHANGELOG = """# Changelog

## Unreleased
- Self-updating.

## 1.5.0
- Profiles.

## 1.4.0
- Guided setup.
"""


class ReleaseToolTests(unittest.TestCase):
    def test_tag_must_match_the_version(self):
        self.assertEqual(release.check_tag("v1.6.0", "1.6.0"), {"version": "1.6.0", "channel": "stable", "prerelease": "false"})
        self.assertEqual(release.check_tag("v1.6.0-beta.2", "1.6.0-beta.2")["channel"], "beta")
        for tag, version in (("v1.6.0", "1.5.0"), ("1.6.0", "1.6.0"), ("v1.6", "1.6"), ("v1.6.0-foo", "1.6.0-foo")):
            with self.assertRaises(SystemExit):
                release.check_tag(tag, version)

    def test_notes(self):
        self.assertEqual(release.notes("1.5.0", CHANGELOG), "- Profiles.")
        self.assertEqual(release.notes("1.6.0-beta.1", CHANGELOG), "- Self-updating.")
        with self.assertRaises(SystemExit):
            release.notes("1.6.0", CHANGELOG)                 # a stable release needs its own section

    def test_the_real_changelog_covers_this_version(self):
        from airglow import __version__
        self.assertTrue(release.notes(__version__))

    def test_scoop_manifest(self):
        with tempfile.TemporaryDirectory() as tmp:
            z = Path(tmp) / "Airglow-stable-Portable.zip"
            z.write_bytes(b"zip")
            url = "https://github.com/Heavenlystorm/airglow/releases/download/v1.6.0/Airglow-stable-Portable.zip"
            m = release.scoop_manifest("1.6.0", z, url)
        self.assertEqual(m["hash"], hashlib.sha256(b"zip").hexdigest())
        self.assertEqual(m["autoupdate"]["url"],
                         "https://github.com/Heavenlystorm/airglow/releases/download/v$version/Airglow-stable-Portable.zip")



class EntryPointTests(unittest.TestCase):
    def test_numpy_gets_one_blas_thread(self):
        """OpenBLAS reads these when numpy is first imported: run.py has to set them before importing Airglow."""
        src = (Path(__file__).resolve().parent.parent / "run.py").read_text(encoding="utf-8")
        first_import = src.index("from airglow")
        for var in ("OPENBLAS_NUM_THREADS", "OMP_NUM_THREADS"):
            self.assertLess(src.index(var), first_import)


if __name__ == "__main__":
    unittest.main()
