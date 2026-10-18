"""UI development server: a simulated sample rig, a separate data folder, port 8790. No hardware is touched.

    python tools/dev_server.py           # your real sensors (LibreHardwareMonitor, if it's running)
    python tools/dev_server.py --demo    # made-up sensors too, for screenshots or a PC without LibreHardwareMonitor
"""

import json
import os
import logging
import math
import random
import sys
import tempfile
import threading
import time
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from airglow import api, config, updates  # noqa: E402
from airglow.controller import Controller  # noqa: E402
from airglow.plugins import asus_aura_usb, nzxt_hue2  # noqa: E402
from tests.fakes import FakeHub, FakeLightDevice, FakeManager  # noqa: E402

logging.basicConfig(level=logging.INFO)
# the galleries read this checkout's profiles/ and effects/, so new ones can be tried before they're on GitHub
os.environ.setdefault("AIRGLOW_GALLERY_URL", (Path(__file__).resolve().parent.parent / "profiles").as_uri())
os.environ.setdefault("AIRGLOW_EFFECTS_URL", (Path(__file__).resolve().parent.parent / "effects").as_uri())
DEMO = "--demo" in sys.argv
DEMO_LHM_PORT = 8791


class DevNzxt(FakeLightDevice):
    plugin, parameters = "nzxt_hue2", nzxt_hue2.PARAMETERS

    def __init__(self):
        super().__init__("nzxt-hub", "NZXT Control Hub", zones=(("ch1", 24), ("ch2", 24), ("ch3", 0), ("ch4", 0), ("ch5", 8)),
                         type_name="Fan and lighting hub")
        for z, name in zip(self.zones, ["Channel 1", "Channel 2", "Channel 3", "Channel 4", "Channel 5"]):
            z.name, z.max_leds = name, 40
        for z in (self.zones[0], self.zones[1], self.zones[4]):
            z.shape = "fans"
        for z in self.zones:
            z.connector = "nzxt"
        # what the hub reports on a common build: two F360 RGB Core (case version) and an F120 RGB Core
        f360, f120 = nzxt_hue2.ACCESSORIES[0x1D], nzxt_hue2.ACCESSORIES[0x17]
        self.zones[0].detected = self.zones[1].detected = [{"name": f360["name"], "nzxt_id": 0x1D}]
        self.zones[4].detected = [{"name": f120["name"], "nzxt_id": 0x17}]
        self.zones[0].fans = self.zones[1].fans = 3
        self.hub = FakeHub()


def sample_rig():
    aura = FakeLightDevice("asus-aura", "ASUS Aura", zones=(("board", 1), ("argb1", 30), ("argb2", 30), ("argb3", 30)),
                           type_name="Motherboard lighting")
    aura.plugin, aura.parameters = "asus_aura_usb", asus_aura_usb.PARAMETERS
    aura.params = {p["id"]: p["default"] for p in aura.parameters}
    for z, name in zip(aura.zones, ["Motherboard", "ARGB header 1", "ARGB header 2", "ARGB header 3"]):
        z.name, z.max_leds = name, (1 if name == "Motherboard" else 120)
    aura.zones[0].resizable, aura.zones[0].connector = False, None          # the board's own LEDs are built in
    gpu = FakeLightDevice("openrgb:sample-graphics-card:1", "Sample Graphics Card",
                          zones=(("z0", 1), ("z1", 1), ("z2", 1), ("z3", 1)), kind="openrgb", type_name="Graphics card")
    gpu.vendor = "Sample"
    for z, name in zip(gpu.zones, ["Right fan", "Left fan", "Center fan", "Side"]):
        z.name, z.max_leds, z.resizable, z.connector = name, 1, False, None
    kb = FakeLightDevice("openrgb:sample-keyboard:1", "Sample Keyboard", zones=(("z0", 85),), kind="openrgb",
                         type_name="Keyboard")
    kb.vendor = "Sample"
    kb.zones[0].max_leds, kb.zones[0].resizable, kb.zones[0].connector = 85, False, None
    return DevNzxt(), aura, gpu, kb


# ---------------------------------------------------------------- made-up LibreHardwareMonitor data (--demo)

# fan and pump controls the demo LHM was told to set (POST /Sensor?action=Set), like LHM's own Control.SetSoftware
DEMO_CONTROLS = {}


def demo_tree(t):
    """A LibreHardwareMonitor data.json with readings that drift like a PC under a light load."""
    wave = lambda period, lo, hi: lo + (hi - lo) * (0.5 + 0.5 * math.sin(t * 2 * math.pi / period))  # noqa: E731
    jitter = lambda v, n: v + random.uniform(-n, n)                                                   # noqa: E731
    cpu_t, gpu_t = jitter(wave(90, 46, 62), 0.6), jitter(wave(140, 41, 55), 0.3)
    cpu_w, gpu_w = jitter(wave(60, 45, 95), 3), jitter(wave(110, 60, 180), 5)

    def sensor(name, value, unit, sid):
        return {"Text": name, "Value": f"{value:.1f} {unit}", "SensorId": sid, "Children": []}

    def group(name, *c):
        return {"Text": name, "Value": "", "Children": list(c)}

    def control(name, default, sid):
        v = DEMO_CONTROLS.get(sid, default)
        return {"Text": name, "Value": f"{v:.1f} %", "SensorId": sid, "Type": "Control", "Children": []}

    def rpm(name, sid, ctl, default, full):
        return {"Text": name, "Value": f"{jitter(full * DEMO_CONTROLS.get(ctl, default) / 100, 8):.0f} RPM",
                "SensorId": sid, "Type": "Fan", "Children": []}

    def hw(name, hid, img, *groups):
        return {"Text": name, "Value": "", "HardwareId": hid, "ImageURL": f"images_icon/{img}", "Children": list(groups)}

    return {"Text": "Sensor", "Children": [{"Text": "DEMO-PC", "Value": "", "Children": [
        hw("Sample 8-Core Processor", "/amdcpu/0", "cpu.png",
           group("Temperatures", sensor("Core (Tctl/Tdie)", cpu_t, "°C", "/amdcpu/0/temperature/2")),
           group("Load", sensor("CPU Total", jitter(wave(45, 12, 38), 2), "%", "/amdcpu/0/load/0")),
           group("Powers", sensor("Package", cpu_w, "W", "/amdcpu/0/power/0")),
           group("Clocks", sensor("Cores (Average)", jitter(4650, 60), "MHz", "/amdcpu/0/clock/0"))),
        hw("Sample Graphics Card", "/gpu-nvidia/0", "nvidia.png",
           group("Temperatures", sensor("GPU Core", gpu_t, "°C", "/gpu-nvidia/0/temperature/0"),
                 sensor("GPU Hot Spot", gpu_t + 9, "°C", "/gpu-nvidia/0/temperature/2")),
           group("Load", sensor("GPU Core", jitter(wave(70, 5, 45), 3), "%", "/gpu-nvidia/0/load/0")),
           group("Powers", sensor("GPU Package", gpu_w, "W", "/gpu-nvidia/0/power/0")),
           group("Fans", sensor("GPU Fan 1", jitter(wave(140, 900, 1300), 20), "RPM", "/gpu-nvidia/0/fan/1"))),
        hw("Sample NVMe SSD", "/nvme/0", "hdd.png",
           group("Temperatures", sensor("Composite Temperature", jitter(44, 0.4), "°C", "/nvme/0/temperature/0"))),
        hw("Sample Motherboard", "/motherboard", "mainboard.png",
           hw("Nuvoton NCT6798D", "/lpc/nct6798d/0", "chip.png",
              group("Fans", rpm("Fan #1", "/lpc/nct6798d/0/fan/0", "/lpc/nct6798d/0/control/0", 45, 1500),
                    rpm("Fan #2", "/lpc/nct6798d/0/fan/1", "/lpc/nct6798d/0/control/1", 40, 1400),
                    rpm("Fan #3", "/lpc/nct6798d/0/fan/2", "/lpc/nct6798d/0/control/2", 40, 1400)),
              group("Controls", control("Fan #1", 45, "/lpc/nct6798d/0/control/0"),
                    control("Fan #2", 40, "/lpc/nct6798d/0/control/1"),
                    control("Fan #3", 40, "/lpc/nct6798d/0/control/2")))),
        hw("Nzxt Kraken Z", "/nzxt/krakenz/0", "fan.png",
           group("Temperatures", sensor("Liquid", jitter(wave(200, 30, 34), 0.1), "°C", "/nzxt/krakenz/0/temperature/0")),
           group("Controls", control("Pump Control", 60, "/nzxt/krakenz/0/control/0"),
                 control("Fans Control", 40, "/nzxt/krakenz/0/control/1")),
           group("Fans", rpm("Pump", "/nzxt/krakenz/0/fan/0", "/nzxt/krakenz/0/control/0", 60, 2800),
                 rpm("Fans", "/nzxt/krakenz/0/fan/1", "/nzxt/krakenz/0/control/1", 40, 1800))),
    ]}]}


def serve_demo_lhm():
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            body = json.dumps(demo_tree(time.time())).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_POST(self):                     # LHM's /Sensor?action=Set&id=...&value=<percent or null>
            q = dict(urllib.parse.parse_qsl(urllib.parse.urlparse(self.path).query))
            if q.get("value") == "null":
                DEMO_CONTROLS.pop(q.get("id"), None)
            else:
                DEMO_CONTROLS[q.get("id")] = float(q.get("value", 0))
            body = b'{"result": "ok"}'
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *a):
            pass

    srv = ThreadingHTTPServer(("127.0.0.1", DEMO_LHM_PORT), Handler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()


class DemoFeed:
    """A pretend release feed, so the update flow can be tried in the dev UI: there's always a newer version, and it
    "downloads" in a few seconds. Nothing is installed."""

    class Asset:
        Version, NotesMarkdown = "9.9.9", "A pretend update from the dev server."

    class Info:
        DeltasToTarget = []

    def check_for_updates(self):
        time.sleep(1)
        info = self.Info()
        info.TargetFullRelease = self.Asset()
        return info

    def download_updates(self, info, progress=None):
        for p in range(0, 101, 20):
            time.sleep(0.6)
            progress and progress(p)

    def wait_exit_then_apply_updates(self, info, **kw):
        print("dev server: would quit now and let Update.exe install", info.TargetFullRelease.Version)


if __name__ == "__main__":
    data = Path(tempfile.gettempdir()) / ("airglow-demo" if DEMO else "airglow-dev2")
    store = config.Store(data)
    if DEMO:
        serve_demo_lhm()
        store.update_settings({"lhm_url": f"http://127.0.0.1:{DEMO_LHM_PORT}"})
    nzxt, aura, gpu, kb = sample_rig()
    manager = FakeManager(nzxt, aura)
    manager.bridge.devices = {gpu.id: gpu, kb.id: kb}
    manager.bridge.status = "Connected, 2 devices"
    gpu.open(); kb.open()
    ctrl = Controller(store, manager=manager)
    ctrl.start()
    updater = updates.Updater(store, kind="velopack", managers=lambda channel: [DemoFeed()], fetch_meta=lambda: None)
    server = api.Server(8790, ctrl, store, updater=updater)
    from airglow import fxlibrary
    from airglow.lighting import studio
    fxlibrary.LIBRARY = fxlibrary.Library(data / "effects")     # the dev server keeps its effects with its data
    studio.HOST.url = "http://127.0.0.1:8790"
    print("dev UI on http://127.0.0.1:8790  (data in", data, ")", "with demo sensors" if DEMO else "")
    server.serve_forever()
