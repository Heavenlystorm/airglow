"""Writes build/version_info.txt (the Windows file properties of Airglow.exe) from airglow/__init__.py, so the version
number lives in one place. build.ps1 runs this before PyInstaller.

    python tools/version_info.py      # prints the version
"""

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

from airglow import APP_NAME, __version__  # noqa: E402

PUBLISHER = "The Airglow contributors"


def numbers(version):
    parts = [int(p) for p in version.split("-")[0].split(".")[:3]]
    return tuple(parts + [0] * (4 - len(parts)))


def render(version=__version__):
    n = numbers(version)
    return f"""VSVersionInfo(
  ffi=FixedFileInfo(filevers={n}, prodvers={n}, mask=0x3f, flags=0x0, OS=0x40004,
                    fileType=0x1, subtype=0x0, date=(0, 0)),
  kids=[
    StringFileInfo([StringTable('040904B0', [
      StringStruct('CompanyName', '{PUBLISHER}'),
      StringStruct('FileDescription', '{APP_NAME} lighting and fan control'),
      StringStruct('FileVersion', '{version}'),
      StringStruct('InternalName', '{APP_NAME}'),
      StringStruct('LegalCopyright', 'Copyright (c) 2026 {PUBLISHER}. MIT licence.'),
      StringStruct('OriginalFilename', '{APP_NAME}.exe'),
      StringStruct('ProductName', '{APP_NAME}'),
      StringStruct('ProductVersion', '{version}')])]),
    VarFileInfo([VarStruct('Translation', [1033, 1200])])
  ]
)
"""


if __name__ == "__main__":
    out = ROOT / "build" / "version_info.txt"
    out.parent.mkdir(exist_ok=True)
    out.write_text(render(), encoding="utf-8")
    print(__version__)
