import tempfile
import unittest

from airglow import config
from airglow.lighting.engine import Engine
from tests.fakes import FakeLightDevice, FakeManager


class EngineTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.store = config.Store(self.tmp.name)
        self.dev = FakeLightDevice("d", "Dev", zones=(("z1", 6), ("z2", 0)))
        self.manager = FakeManager(self.dev)
        self.manager.maintain(True)
        self.store.ensure_zones([self.dev.info()])
        self.engine = Engine(self.manager, self.store, sensors=lambda: {"cpu_temp": 50, "gpu_temp": 40}, async_writes=False)
        self.engine.set_scene({"effect": "solid", "params": {"color": "#ff8000"}, "brightness": 50, "on": True})

    def tearDown(self):
        self.tmp.cleanup()

    def test_renders_with_brightness_and_skips_empty_zones(self):
        self.engine.tick()
        self.assertEqual(self.dev.last("z1"), [(128, 64, 0)] * 6)
        self.assertIsNone(self.dev.last("z2"))                    # 0 LEDs -> disabled by default
        self.assertEqual(self.engine.frames["d/z1"], "804000" * 6)

    def test_unchanged_frames_are_not_resent(self):
        self.engine.tick(); self.engine.tick(); self.engine.tick()
        self.assertEqual(len([w for w in self.dev.writes if w[0] == "z1"]), 1)

    def test_keepalive_resends(self):
        self.engine.tick()
        key = "d/z1"
        raw, sent = self.engine._last[key]
        self.engine._last[key] = (raw, sent - self.engine.KEEPALIVE - 1)
        self.engine.tick()
        self.assertEqual(len([w for w in self.dev.writes if w[0] == "z1"]), 2)

    def test_color_order_applies_to_device_but_not_preview(self):
        cfg = dict(self.store.zones()["d/z1"], order="GRB")
        self.store.set_zone("d/z1", cfg, 12)
        self.engine.invalidate("d/z1")
        self.engine.tick()
        self.assertEqual(self.dev.last("z1")[0], (64, 128, 0))
        self.assertEqual(self.engine.frames["d/z1"][:6], "804000")

    def test_lights_off(self):
        self.engine.set_scene({"effect": "solid", "params": {"color": "#ffffff"}, "brightness": 100, "on": False})
        self.engine.tick()
        self.assertEqual(set(self.dev.last("z1")), {(0, 0, 0)})

    def test_zone_override(self):
        cfg = dict(self.store.zones()["d/z1"], override={"effect": "solid", "params": {"color": "#0000ff"}})
        self.store.set_zone("d/z1", cfg, 12)
        self.engine.tick()
        self.assertEqual(self.dev.last("z1")[0], (0, 0, 128))

    def test_identify_sweep(self):
        self.engine.identify("d/z1")
        self.engine.tick()
        first = self.dev.last("z1")
        self.assertIn(first[0], [(0, 255, 0), (10, 10, 10)])            # flashing green start marker
        self.assertTrue(all(c == (10, 10, 10) for c in first[1:]))

    def test_write_error_closes_device_and_reconnects(self):
        self.engine.tick()
        self.dev.fail_writes = True
        self.engine.invalidate()
        self.engine.tick()
        self.assertFalse(self.dev.online)
        self.dev.fail_writes = False
        self.manager.maintain(True)                 # reconnects happen on the device manager's thread
        self.engine.tick()
        self.assertTrue(self.dev.online)
        self.assertTrue(self.dev.last("z1"))

    def test_animated_effect_changes_over_time(self):
        self.engine.set_scene({"effect": "rainbow", "params": {}, "brightness": 100, "on": True})
        self.engine.tick()
        a = self.dev.last("z1")
        self.engine._t0 -= 2.0
        self.engine.tick()
        self.assertNotEqual(a, self.dev.last("z1"))

    def test_tick_never_reconnects_devices(self):
        """The lighting loop must not do slow housekeeping (USB scans, OpenRGB connects)."""
        calls = []
        self.manager.maintain = lambda *a, **k: calls.append(1)
        for _ in range(5):
            self.engine.tick()
        self.assertEqual(calls, [])

    def test_async_writer_writes_latest_frame_in_background(self):
        import time as _t
        eng = Engine(self.manager, self.store, async_writes=True)
        eng.set_scene({"effect": "solid", "params": {"color": "#00ff00"}, "brightness": 100, "on": True})
        eng.tick()
        eng._writers["d"].wait_idle(2)
        self.assertEqual(self.dev.last("z1"), [(0, 255, 0)] * 6)
        eng.blackout()
        self.assertEqual(set(self.dev.last("z1")), {(0, 0, 0)})
        eng.stop()

    def test_reconnected_device_gets_full_resend(self):
        self.engine.tick()
        n = len(self.dev.writes)
        self.engine.tick()
        self.assertEqual(len(self.dev.writes), n)               # unchanged: nothing sent
        self.dev.close(); self.engine.tick()                     # goes offline...
        self.manager.maintain(True); self.engine.tick()          # ...and comes back
        self.assertGreater(len(self.dev.writes), n)

    def test_blackout(self):
        self.engine.tick()
        self.engine.blackout()
        self.assertEqual(set(self.dev.last("z1")), {(0, 0, 0)})


if __name__ == "__main__":
    unittest.main()
