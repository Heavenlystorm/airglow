import math
import unittest
from types import SimpleNamespace as NS

import numpy as np

from airglow.lighting import effects, geometry


def zone(layout):
    pos, local, fan = geometry.positions(layout)
    return NS(pos=geometry.normalised(pos), local=local, fan=fan, n=len(pos))


CTX = NS(cpu_temp=60.0, gpu_temp=45.0, screen=None, audio=NS(bands=np.linspace(0, 1, 16), level=0.5, bass=0.8))


class GeometryTests(unittest.TestCase):
    def test_fans_are_rings(self):
        lay = {"shape": "fans", "fans": 2, "leds_per_fan": 8, "x": 0.8, "y": 0.5, "angle": 0, "size": 0.2}
        pos, local, fan = geometry.positions(lay)
        self.assertEqual(pos.shape, (16, 2))
        self.assertEqual(sorted(set(fan.tolist())), [0, 1])
        centre = pos[fan == 0].mean(axis=0)
        radii = np.linalg.norm(pos[fan == 0] - centre, axis=1)
        self.assertTrue(np.allclose(radii, 0.1))
        self.assertAlmostEqual(pos[0][1], centre[1] - 0.1)          # first LED at 12 o'clock

    def test_strip_and_reverse(self):
        lay = {"shape": "strip", "leds": 5, "x": 0.5, "y": 0.5, "angle": 0, "length": 0.4}
        pos, local, _ = geometry.positions(lay)
        self.assertAlmostEqual(pos[0][0], 0.3)
        self.assertAlmostEqual(pos[-1][0], 0.7)
        rpos, rlocal, _ = geometry.positions({**lay, "reverse": True})
        self.assertAlmostEqual(rpos[0][0], 0.7)

    def test_validate_layout(self):
        ok = geometry.validate_layout({"shape": "fans", "fans": "3", "leds_per_fan": 8, "x": 1, "y": 0.5}, 40)
        self.assertEqual((ok["fans"], ok["leds_per_fan"]), (3, 8))
        with self.assertRaisesRegex(ValueError, "at most 40"):
            geometry.validate_layout({"shape": "fans", "fans": 6, "leds_per_fan": 8, "x": 1, "y": 0.5}, 40)
        with self.assertRaises(ValueError):
            geometry.validate_layout({"shape": "blob"}, 40)

    def test_long_openrgb_strips_can_be_saved(self):
        # a 600-LED WLED strip through OpenRGB: Airglow places it by itself, so saving it must work too
        ok = geometry.validate_layout({"shape": "strip", "leds": 600, "x": 1, "y": 0.5}, 600)
        self.assertEqual(ok["leds"], 600)
        dense = geometry.validate_layout({"shape": "fans", "fans": 1, "leds_per_fan": 96, "x": 1, "y": 0.5}, 120)
        self.assertEqual(dense["leds_per_fan"], 96)


class EffectTests(unittest.TestCase):
    LAYOUT = {"shape": "fans", "fans": 3, "leds_per_fan": 8, "x": 0.8, "y": 0.5, "angle": 0, "size": 0.16}

    def test_every_effect_returns_valid_colours(self):
        z = zone(self.LAYOUT)
        for e in effects.EFFECTS.values():
            out = e.render(z, 3.21, effects.clean_params(e.id, {}), CTX)
            self.assertEqual(out.shape, (24, 3), e.id)
            self.assertTrue(np.isfinite(out).all() and out.min() >= -1e-9 and out.max() <= 1 + 1e-9, e.id)

    def test_reactive_effects_go_dark_without_input(self):
        z = zone(self.LAYOUT)
        empty = NS(cpu_temp=None, gpu_temp=None, screen=None, audio=None)
        for eid in ("screen", "spectrum"):
            out = effects.EFFECTS[eid].render(z, 0, effects.clean_params(eid, {}), empty)
            self.assertEqual(out.max(), 0, eid)

    def test_temperature_maps_between_colours(self):
        z = zone(self.LAYOUT)
        p = effects.clean_params("temperature", {"cool": "#0000ff", "hot": "#ff0000", "low": 40, "high": 80, "source": "cpu"})
        cold = effects.EFFECTS["temperature"].render(z, 0, p, NS(cpu_temp=30, gpu_temp=None))[0]
        mid = effects.EFFECTS["temperature"].render(z, 0, p, NS(cpu_temp=60, gpu_temp=None))[0]
        self.assertTrue(np.allclose(cold, [0, 0, 1]))
        self.assertTrue(np.allclose(mid, [0.5, 0, 0.5]))

    def test_comet_runs_around_each_fan(self):
        z = zone(self.LAYOUT)
        out = effects.EFFECTS["comet"].render(z, 0.0, effects.clean_params("comet", {"color": "#ffffff", "background": "#000000"}), CTX)
        per_fan = out.reshape(3, 8, 3).sum(axis=2)
        self.assertTrue(np.allclose(per_fan[0], per_fan[1]))         # same position on every fan

    def test_gradient_loops_seamlessly(self):
        z = zone({"shape": "strip", "leds": 50, "x": 0.8, "y": 0.5, "angle": 0, "length": 1.6})
        p = effects.clean_params("gradient", {"colors": ["#ff0000", "#0000ff"], "speed": 0})
        out = effects.EFFECTS["gradient"].render(z, 0, p, CTX)
        self.assertLess(np.abs(np.diff(out, axis=0)).max(), 0.2)     # no hard jumps

    def test_hsv(self):
        self.assertTrue(np.allclose(effects.hsv(np.array([0.0]), 1, 1)[0], [1, 0, 0]))
        self.assertTrue(np.allclose(effects.hsv(np.array([1 / 3]), 1, 1)[0], [0, 1, 0]))

    def test_clean_params(self):
        p = effects.clean_params("rainbow", {"speed": 500})
        self.assertEqual(p["speed"], 100)                            # clamped
        self.assertEqual(p["size"], 50)                              # default filled in
        with self.assertRaises(ValueError):
            effects.clean_params("solid", {"color": "red"})
        with self.assertRaises(ValueError):
            effects.clean_params("gradient", {"colors": ["#ffffff"]})
        with self.assertRaises(ValueError):
            effects.clean_params("temperature", {"source": "ram"})
        with self.assertRaises(ValueError):
            effects.clean_params("nope", {})

    def test_catalog_is_serialisable(self):
        import json
        cat = effects.catalog()
        json.dumps(cat)
        self.assertEqual({e["id"] for e in cat}, set(effects.EFFECTS))
        self.assertTrue(all(e["description"].endswith(".") for e in cat))

    def test_speed_mapping_is_monotonic(self):
        vals = [effects.speed_hz(s) for s in range(0, 101, 10)]
        self.assertEqual(vals, sorted(vals))
        self.assertTrue(math.isclose(effects.speed_hz(0), 0.03))


class InputOptionTests(unittest.TestCase):
    def test_screen_effect_picks_a_display(self):
        from airglow.lighting.inputs import ScreenSampler
        self.assertEqual(effects.clean_params("screen", {})["display"], "1")
        s = ScreenSampler()
        s._thread = NS(is_alive=lambda: True)                       # don't start a capture thread
        s.touch(display=2)
        self.assertEqual(s.display, 2)


if __name__ == "__main__":
    unittest.main()
