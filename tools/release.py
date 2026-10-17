"""Small release helpers for build.ps1 and .github/workflows/release.yml.

    python tools/release.py check-tag v1.6.0          # fails unless the tag matches airglow/__init__.py; prints
                                                     # version=, channel= and prerelease= for $GITHUB_OUTPUT
    python tools/release.py channel                  # stable, or beta for versions like 1.6.0-beta.1
    python tools/release.py notes 1.6.0 [file]       # that version's section of CHANGELOG.md (to a UTF-8 file)
    python tools/release.py scoop 1.6.0 <zip> <url>  # the Scoop manifest for a release's portable zip
"""

import hashlib
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

from airglow import __version__  # noqa: E402
from airglow.updates import UPDATE_REPO  # noqa: E402

VERSION_RE = r"\d+\.\d+\.\d+(?:-(?:beta|rc)\.\d+)?"


def channel(version):
    return "beta" if "-" in version else "stable"


def check_tag(tag, version=__version__):
    if not re.fullmatch(rf"v{VERSION_RE}", tag):
        raise SystemExit(f"{tag} isn't a release tag (v1.6.0 or v1.6.0-beta.1)")
    if tag[1:] != version:
        raise SystemExit(f"tag {tag} doesn't match the version in airglow/__init__.py ({version})")
    return {"version": version, "channel": channel(version), "prerelease": str(channel(version) == "beta").lower()}


def notes(version, changelog=None):
    """The CHANGELOG.md section for this version. A beta without its own section uses "Unreleased"."""
    text = (changelog if changelog is not None else (ROOT / "CHANGELOG.md").read_text(encoding="utf-8"))
    sections = {}
    for m in re.finditer(r"^## (.+?)\s*$(.*?)(?=^## |\Z)", text, re.MULTILINE | re.DOTALL):
        sections[m.group(1).strip()] = m.group(2).strip()
    if version in sections:
        return sections[version]
    if channel(version) == "beta" and "Unreleased" in sections:
        return sections["Unreleased"]
    raise SystemExit(f"CHANGELOG.md has no section for {version}")


def scoop_manifest(version, zip_path, url):
    sha = hashlib.sha256(Path(zip_path).read_bytes()).hexdigest()
    repo = f"https://github.com/{UPDATE_REPO}"
    return {
        "version": version,
        "description": "RGB lighting, fan curves and sensors for Windows PCs, in one app.",
        "homepage": repo,
        "license": "MIT",
        "url": url,
        "hash": sha,
        "shortcuts": [["Airglow.exe", "Airglow"]],
        "checkver": {"github": repo},
        "autoupdate": {"url": url.replace(version, "$version")},
    }


def main(argv):
    cmd, args = (argv[0], argv[1:]) if argv else ("", [])
    if cmd == "check-tag" and len(args) == 1:
        print("\n".join(f"{k}={v}" for k, v in check_tag(args[0]).items()))
    elif cmd == "channel" and not args:
        print(channel(__version__))
    elif cmd == "notes" and len(args) in (1, 2):
        text = notes(args[0]) + "\n"
        if len(args) == 2:
            Path(args[1]).write_text(text, encoding="utf-8")
        else:
            sys.stdout.buffer.write(text.encode("utf-8"))
    elif cmd == "scoop" and len(args) == 3:
        print(json.dumps(scoop_manifest(*args), indent=2))
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main(sys.argv[1:])
