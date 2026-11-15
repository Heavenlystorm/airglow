import unittest
from types import SimpleNamespace

import numpy as np

from airglow.lighting.effects import AudioSpectrum


class SpectrumFidelityTests(unittest.TestCase):
    def test_direction_spans_bass_to_treble_at_both_diagonal_extremes(self):
        effect = AudioSpectrum()
        ctx = SimpleNamespace(audio=SimpleNamespace(bands=np.array([0., 0., 0., 1.])))
        for angle, points in ((45, [[0, 0], [1, 1]]), (225, [[1, 1], [0, 0]]),
                              (135, [[1, 0], [0, 1]]), (315, [[0, 1], [1, 0]])):
            with self.subTest(angle=angle):
                colors = effect.render(SimpleNamespace(pos=np.array(points, dtype=float)), 0,
                                       {"angle": angle, "sensitivity": 100, "hue": 0}, ctx)
                self.assertTrue(np.allclose(colors[0], [0, 0, 0], atol=1e-7), colors[0])
                self.assertTrue(np.allclose(colors[1], [1, 0, 0], atol=1e-5), colors[1])

    def test_midpoint_uses_middle_audio_band_instead_of_wrapping_at_negative_angles(self):
        effect = AudioSpectrum()
        ctx = SimpleNamespace(audio=SimpleNamespace(bands=np.array([0., 0., 1., 0.])))
        zone = SimpleNamespace(pos=np.array([[.5, .5]]))
        for angle in (0, 45, 135, 180, 225, 270, 315):
            color = effect.render(zone, 0, {"angle": angle, "sensitivity": 100, "hue": 0}, ctx)[0]
            self.assertGreater(color[1], .9, f"middle band should remain green at {angle} degrees")
