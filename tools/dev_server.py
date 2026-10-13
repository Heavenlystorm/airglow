"""UI development server: your real sensors, a simulated sample rig, a separate data folder, port 8790.

    python tools/dev_server.py
"""

import logging
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from airglow import api, config  # noqa: E402
from airglow.controller import Controller  # noqa: E402
from tests.fakes import FakeHub, FakeLightDevice, FakeManager  # noqa: E402

logging.basicConfig(level=logging.INFO)


class DevNzxt(FakeLightDevice):
    def __init__(self):
        super().__init__("nzxt-hub", "NZXT Control Hub", zones=(("ch1", 24), ("ch2", 24), ("ch3", 0), ("ch4", 0), ("ch5", 8)),
                         type_name="Fan and lighting hub")
        for z, name in zip(self.zones, ["Channel 1", "Channel 2", "Channel 3", "Channel 4", "Channel 5"]):
            z.name, z.max_leds = name, 40
        for z in (self.zones[0], self.zones[1], self.zones[4]):
            z.shape = "fans"
        self.hub = FakeHub()


aura = FakeLightDevice("asus-aura", "ASUS Aura", zones=(("board", 1), ("argb1", 30), ("argb2", 30), ("argb3", 30)),
                       type_name="Motherboard lighting")
gpu = FakeLightDevice("openrgb:gigabyte-rtx-5080", "Gigabyte GeForce RTX 5080 Gaming OC",
                      zones=(("z0", 1), ("z1", 1), ("z2", 1), ("z3", 1)), kind="openrgb", type_name="Graphics card")
gpu.vendor = "Gigabyte"
for z, name in zip(gpu.zones, ["Right fan", "Left fan", "Center fan", "Side"]):
    z.name = name
for z, name in zip(aura.zones, ["Motherboard", "ARGB header 1", "ARGB header 2", "ARGB header 3"]):
    z.name, z.max_leds = name, (1 if name == "Motherboard" else 120)
kb = FakeLightDevice("openrgb:keychron-k2-he", "Keychron K2 HE", zones=(("z0", 85),), kind="openrgb", type_name="Keyboard")
kb.vendor = "Keychron"

data = Path(tempfile.gettempdir()) / "airglow-dev2"
store = config.Store(data, legacy_dir=None)
manager = FakeManager(DevNzxt(), aura)
manager.bridge.devices = {gpu.id: gpu, kb.id: kb}
manager.bridge.status = "Connected, 2 devices"
gpu.open(); kb.open()
ctrl = Controller(store, manager=manager)
ctrl.start()
server = api.Server(8790, ctrl, store)
print("dev UI on http://127.0.0.1:8790  (data in", data, ")")
server.serve_forever()
