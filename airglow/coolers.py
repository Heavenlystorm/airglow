"""Which all-in-one liquid cooler is in the PC, when it shows up over USB (airglow/data/coolers.json).

For telling you what you have, and whether its pump and fans can follow Airglow (through LibreHardwareMonitor, on
the Fans tab). Airglow doesn't drive these coolers' lights itself. Coolers whose pump ring is wired to an ARGB header
are invisible to software; the setup guide counts their LEDs instead.
"""

import json
import logging

from . import paths

log = logging.getLogger(__name__)

_KNOWN = None


def known():
    """{(vid, pid): {"brand", "name", "lhm"}} from the data file, read once."""
    global _KNOWN
    if _KNOWN is None:
        _KNOWN = {}
        try:
            raw = json.loads((paths.PACKAGE_DIR / "data" / "coolers.json").read_text(encoding="utf-8"))
            for c in raw["coolers"]:
                _KNOWN[(int(c["vid"], 16), int(c["pid"], 16))] = {"brand": c["brand"], "name": c["name"],
                                                                   "lhm": bool(c.get("lhm"))}
        except (OSError, ValueError, KeyError, TypeError) as e:
            log.error("cooler list not loaded: %s", e)
    return _KNOWN


def detect(hid_list):
    """Coolers in an hid.enumerate() list, one entry per model: [{"brand", "name", "lhm", "usb"}]."""
    found = {}
    for info in hid_list:
        key = (info.get("vendor_id"), info.get("product_id"))
        c = known().get(key)
        if c and key not in found:
            found[key] = {**c, "usb": f"{key[0]:04X}:{key[1]:04X}"}
    return list(found.values())


def scan():
    try:
        import hid
        return detect(hid.enumerate())
    except Exception as e:  # no USB access: just don't name a cooler
        log.debug("cooler scan failed: %s", e)
        return []
