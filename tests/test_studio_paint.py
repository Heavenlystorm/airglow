import unittest
from types import SimpleNamespace

import numpy as np

from airglow import fxformat
from airglow import previewhost
from airglow.lighting import studio, geometry


class PaintedLayerTests(unittest.TestCase):
    def test_hardware_preserves_api_paint_coordinates_even_when_effect_positions_are_clamped(self):
        canvas = np.array([[.123456, .234567], [-.16, 1.2]])
        points = studio._points(SimpleNamespace(canvas=canvas, pos=geometry.normalised(canvas)))
        self.assertEqual([p[6] if len(p) > 6 else None for p in points], [[772, 2346], [-1000, 12000]])
        self.assertEqual(points[1][:2], [0.0, 200.0], "existing effects retain their clamped coordinates")
        self.assertEqual(previewhost._points(points), points)

    def test_native_preview_rejects_invalid_optional_paint_coordinates(self):
        for pair in ([True, 0], [1.5, 1], [100001, 0], [1], "1,2"):
            with self.subTest(pair=pair), self.assertRaises(ValueError):
                previewhost._points([[0, 0, None, 0, 0, "zone", pair]])

    def test_painted_positions_survive_file_round_trip_and_area_mask(self):
        paint = {"v": 1, "points": [[-1000, 12000], [5000, 5000]]}
        source = fxformat.to_html({"name": "Painted", "layers": [{"type": "solid", "mask": "front", "paint": paint}]})
        layer = fxformat.validate(source)["layers"][0]
        self.assertEqual(layer.get("paint"), paint)
        self.assertEqual(layer["mask"], "front")
        plain = fxformat.validate(fxformat.to_html({"name": "Plain", "layers": [{"type": "solid"}]}))["layers"][0]
        self.assertNotIn("paint", plain)

    def test_paint_is_strictly_bounded_and_rejects_duplicates(self):
        for paint in (None, {}, {"v": 2, "points": []}, {"v": True, "points": []}, {"v": 1, "points": [[True, 0]]},
                      {"v": 1, "points": [[.5, 1]]}, {"v": 1, "points": [[100001, 0]]},
                      {"v": 1, "points": [[1, 2, 3]]}, {"v": 1, "points": [[1, 2], [1, 2]]},
                      {"v": 1, "points": [[x, 0] for x in range(2049)]}):
            with self.subTest(paint=str(paint)[:80]), self.assertRaises(fxformat.FormatError):
                fxformat.to_html({"name": "Invalid paint", "layers": [{"type": "solid", "paint": paint}]})

    def test_empty_paint_round_trips_and_total_file_limit_still_applies(self):
        effect = {"name": "Blank mask", "layers": [{"type": "solid", "paint": {"v": 1, "points": []}}]}
        self.assertEqual(fxformat.validate(fxformat.to_html(effect))["layers"][0].get("paint"), {"v": 1, "points": []})
        effect["layers"] *= 12
        for layer in effect["layers"]:
            layer["paint"]["points"] = [[x, 100000] for x in range(2048)]
        with self.assertRaises(fxformat.FormatError):
            fxformat.to_html(effect)


if __name__ == "__main__":
    unittest.main()
