"""Other RGB software that fights over the same devices. We only detect; closing happens when the user asks."""

import logging

from . import system

log = logging.getLogger(__name__)

# exe (lower case) -> (product name, what it conflicts with)
KNOWN = {
    "signalrgb.exe": ("SignalRGB", "lighting on every device"),
    "signalrgblauncher.exe": ("SignalRGB", "lighting on every device"),
    "icue.exe": ("Corsair iCUE", "Corsair lighting and fans"),
    "nzxt cam.exe": ("NZXT CAM", "the NZXT hub's lighting and fans"),
    "lightingservice.exe": ("ASUS Aura service", "motherboard lighting"),
    "armourycrate.exe": ("ASUS Armoury Crate", "motherboard lighting"),
    "armourycrate.service.exe": ("ASUS Armoury Crate", "motherboard lighting"),
    "rgbfusion.exe": ("Gigabyte RGB Fusion", "Gigabyte graphics card and board lighting"),
    "gcc.exe": ("Gigabyte Control Center", "Gigabyte graphics card and board lighting"),
    "mysticlight.exe": ("MSI Mystic Light", "MSI board lighting"),
    "msi.centralserver.exe": ("MSI Center", "MSI board lighting"),
    "lconnect3.exe": ("Lian Li L-Connect", "Lian Li fans and lighting"),
    "l-connect 3.exe": ("Lian Li L-Connect", "Lian Li fans and lighting"),
    "ledkeeper2.exe": ("MSI Mystic Light", "MSI board and graphics card lighting"),
    "polychrome rgb.exe": ("ASRock Polychrome", "ASRock board lighting"),
    "lghub.exe": ("Logitech G HUB", "Logitech keyboard and mouse lighting"),
    "lghub_agent.exe": ("Logitech G HUB", "Logitech keyboard and mouse lighting"),
    "steelseriesgg.exe": ("SteelSeries GG", "SteelSeries keyboard and mouse lighting"),
    "steelseriesengine.exe": ("SteelSeries GG", "SteelSeries keyboard and mouse lighting"),
    "steelseriesengine3.exe": ("SteelSeries Engine", "SteelSeries keyboard and mouse lighting"),
}

# Peripheral software people keep running for their mouse and keyboard settings: only a conflict while Airglow
# lights one of that brand's devices (peripherals start switched off in Airglow).
ONLY_FOR_VENDORS = {"Logitech G HUB": {"logitech"}, "SteelSeries GG": {"steelseries"},
                    "SteelSeries Engine": {"steelseries"}}


def detect(lit_vendors=None):
    """Conflicting programs that are running. lit_vendors: lower-case vendors of the devices Airglow lights, so
    peripheral software only counts when it matters; None reports everything (diagnostics)."""
    running = system.running_exes()
    found = {}
    for exe, (name, what) in KNOWN.items():
        if exe not in running:
            continue
        if lit_vendors is not None and name in ONLY_FOR_VENDORS and not ONLY_FOR_VENDORS[name] & set(lit_vendors):
            continue
        found.setdefault(name, {"name": name, "conflicts_with": what, "exes": []})["exes"].append(exe)
    return list(found.values())


def close(name):
    """Terminate the processes of one detected product (user-initiated). Returns what couldn't be closed."""
    import psutil
    targets = {exe for exe, (n, _) in KNOWN.items() if n == name}
    if not targets:
        raise ValueError(f"'{name}' isn't a known RGB app")
    failed = []
    for p in psutil.process_iter(["name"]):
        if (p.info.get("name") or "").lower() in targets:
            try:
                p.terminate()
                p.wait(timeout=4)
                log.info("closed %s (pid %d) at the user's request", p.info["name"], p.pid)
            except (psutil.AccessDenied, psutil.TimeoutExpired):
                failed.append(p.info["name"])
            except psutil.NoSuchProcess:
                pass
    system._proc_cache = (0.0, frozenset())        # re-scan on the next check
    return failed
