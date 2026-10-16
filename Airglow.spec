# PyInstaller spec for Airglow (one-folder build). Run via build.ps1.
# pystray (LGPL) is collected as plain .py files so users can swap in a modified copy.

import os

from PyInstaller.utils.hooks import collect_submodules

root = os.path.abspath(SPECPATH)

a = Analysis(
    [os.path.join(root, "run.py")],
    pathex=[root],
    datas=[
        (os.path.join(root, "web"), "web"),
        (os.path.join(root, "airglow", "data"), os.path.join("airglow", "data")),
        (os.path.join(root, "airglow.ico"), "."),
        (os.path.join(root, "LICENSE"), "."),
        (os.path.join(root, "THIRD_PARTY_NOTICES.md"), "."),
    ],
    # device plugins are found at runtime with pkgutil, so PyInstaller can't see them either
    hiddenimports=collect_submodules("airglow.plugins") + ["pystray._win32", "soundcard.mediafoundation",
                   # Windows Dynamic Lighting (imported lazily, so PyInstaller can't see them)
                   "winrt.runtime", "winrt.windows.devices.lights", "winrt.windows.devices.enumeration",
                   "winrt.windows.foundation", "winrt.windows.foundation.collections",
                   "winrt.windows.foundation.numerics", "winrt.windows.ui"],
    excludes=["tkinter", "matplotlib", "IPython", "pytest"],
    module_collection_mode={"pystray": "py"},
    noarchive=False,
)
pyz = PYZ(a.pure)
exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name="Airglow",
    icon=os.path.join(root, "airglow.ico"),
    version=os.path.join(root, "build", "version_info.txt"),
    console=False,
    upx=False,
)
coll = COLLECT(exe, a.binaries, a.datas, name="Airglow", upx=False)
