"""A diagnostics report for bug reports: what Airglow found, its settings, a sensor dump and the logs, in one zip.

Everything personal is taken out before it's written: the Windows user name and home folder, the computer name, and
device serial numbers (OpenRGB device ids contain them; they're replaced by a short hash so ids still line up
between the files). Nothing is sent anywhere; the user attaches the zip to an issue themselves.
"""

import datetime
import hashlib
import json
import os
import platform
import re
import socket
import sys
import zipfile

from . import __version__, conflicts, paths, sensors, tools

# Vendors whose USB controllers Airglow could drive natively (listed so a report shows what's plugged in).
USB_VENDORS = {0x1E71: "NZXT", 0x0B05: "ASUS"}
_OPENRGB_ID = re.compile(r"(openrgb:[a-z0-9\-]+:)([a-z0-9\-]+)")


def _secrets():
    """Strings that identify the user or the PC, longest first (so a home path goes before the bare name)."""
    found = set()
    for key in ("USERNAME", "COMPUTERNAME", "USERDOMAIN", "USERPROFILE", "LOCALAPPDATA", "APPDATA", "ONEDRIVE"):
        v = os.environ.get(key)
        if v and len(v) > 2:
            found.add(v)
    try:
        found.add(socket.gethostname())
    except OSError:
        pass
    for v in list(found):
        found.add(v.replace("\\", "/"))
        found.add(v.replace("\\", "\\\\"))                          # as it appears inside JSON strings
    return sorted((s for s in found if len(s) > 2), key=len, reverse=True)


def scrub(text, secrets=None):
    for s in secrets if secrets is not None else _secrets():
        text = re.sub(re.escape(s), "<redacted>", text, flags=re.I)
    return _OPENRGB_ID.sub(lambda m: m.group(1) + "s" + hashlib.sha256(m.group(2).encode()).hexdigest()[:8], text)


def _usb_devices():
    try:
        import hid
        out = []
        for d in hid.enumerate():
            if d["vendor_id"] in USB_VENDORS:
                out.append({"vendor": USB_VENDORS[d["vendor_id"]], "vid": f"{d['vendor_id']:04X}",
                            "pid": f"{d['product_id']:04X}", "interface": d.get("interface_number"),
                            "usage_page": f"{d.get('usage_page', 0):04X}", "product": d.get("product_string")})
        return out
    except Exception as e:  # a report must still work without USB access
        return [{"error": str(e)}]


def report(ctrl, store):
    """Build the report files as {name: text} (not yet scrubbed)."""
    snap = ctrl.snapshot()
    about = {
        "airglow": __version__,
        "frozen": bool(getattr(sys, "frozen", False)),
        "python": platform.python_version(),
        "windows": platform.platform(),
        "created": datetime.datetime.now().isoformat(timespec="seconds"),
        "devices": snap["devices"],
        "openrgb": snap["openrgb"],
        "inputs": snap["inputs"],
        "fans": snap["fans"],
        "override": snap["override"],
        "fps": snap["fps"],
        "healthy": snap["healthy"],
        "usb": _usb_devices(),
        "conflicts": conflicts.detect(),
    }
    settings = {"settings": store.settings(), "zones": store.zones(), "curves": store.curves(),
                "presets": store.get_presets(), "automations": store.get_rules()}
    files = {"about.json": json.dumps(about, indent=2, ensure_ascii=False, default=str),
             "settings.json": json.dumps(settings, indent=2, ensure_ascii=False, default=str)}
    tree = sensors.read_tree(store.settings()["lhm_url"], headers=tools.lhm_headers())
    if tree is not None:                                        # in data.json shape, so it can become a test fixture
        files["lhm_data.json"] = json.dumps(tree, indent=1, ensure_ascii=False)
    files["readings.json"] = json.dumps(list(ctrl.readings.values()), indent=1, ensure_ascii=False, default=str)
    for log in sorted(paths.log_dir().glob("airglow.log*")):
        try:
            files[f"logs/{log.name}"] = log.read_text(encoding="utf-8", errors="replace")
        except OSError:
            pass
    return files


def write(ctrl, store, folder=None):
    """Write a scrubbed zip and return its path."""
    folder = folder or paths.data_dir() / "diagnostics"
    folder.mkdir(parents=True, exist_ok=True)
    name = f"airglow-diagnostics-{datetime.datetime.now():%Y%m%d-%H%M%S}.zip"
    secrets = _secrets()
    path = folder / name
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        for fname, text in report(ctrl, store).items():
            z.writestr(fname, scrub(text, secrets))
    return path
