import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from airglow.windowlayout import WindowLayout, fit, foreground_window, placement


class NativePlacementTests(unittest.TestCase):
    def test_recalling_an_already_maximized_window_does_not_restore_its_normal_size(self):
        with patch("airglow.windowlayout.ctypes.windll", create=True) as native:
            native.user32.IsIconic.return_value = False
            foreground_window(123)
            native.user32.ShowWindow.assert_not_called()
            native.user32.IsIconic.return_value = True
            foreground_window(123)
            self.assertEqual(native.user32.ShowWindow.call_args.args[1], 9)
            self.assertEqual(native.user32.SetForegroundWindow.call_count, 2)

    def test_round_trip_on_left_display_with_taskbar_and_non_integer_scale(self):
        work = (-1920, 44, 1920, 1036)
        for frame in [(-1920, 44, 680, 500), (-630, 242, 630, 790), (-1700, 567, 550, 390)]:
            self.assertEqual(fit(placement(frame, work), work, (320,240)), frame)

    def test_disconnected_display_fits_inside_the_new_work_area(self):
        old = (1920,0,2560,1390)
        relative = placement((3400,880,1080,510), old)
        self.assertEqual(fit(relative, (0,18,1366,710), (560,440)), (790,288,576,440))
        self.assertEqual(fit(relative, (0,0,480,390), (560,440)), (0,0,480,390))

    def test_missing_and_corrupt_geometry_leave_a_valid_default(self):
        for invalid in [[], "0", [-.01,0,.3,.2], [0,0,0,.9], [True,1,1,1], [float("nan"),1,1,1]]:
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                fit(invalid, (0,0,1920,1080), (560,440))
        with tempfile.TemporaryDirectory() as d:
            file=Path(d)/"placement.json"
            self.assertFalse(WindowLayout(file).desk_pinned())
            file.write_text("damaged")
            self.assertFalse(WindowLayout(file).desk_pinned())
            file.write_text(json.dumps({"desk":{"pinned":True},"unrelated":{"pinned":True}}))
            self.assertTrue(WindowLayout(file).desk_pinned())
