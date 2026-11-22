import threading
import unittest
from concurrent.futures import ThreadPoolExecutor
from types import SimpleNamespace
from unittest.mock import Mock, patch

from airglow import api
from airglow.deskdisplay import DeskWindow, display_view


class CloseEvent(threading.Event):
    def __iadd__(self, callback):
        self.callback = callback
        return self

    def close(self):
        self.set()
        self.callback()


def fake_window():
    window = Mock()
    window.events.closed = CloseEvent()
    window.events.loaded = CloseEvent()
    window.on_top = False
    return window


class DeskDisplayTests(unittest.TestCase):
    def test_projects_only_pinned_readings_with_gaps_and_actual_times(self):
        store, ctrl = Mock(), Mock()
        store.desk_preferences.return_value = {"pins": ["cpu", "lost"], "temp_unit": "f", "design": "firefly"}
        ctrl.readings = {"cpu": {"name": "CPU package", "value": 39.5, "unit": "°C"}, "disk": {"value": 3}}
        ctrl.readings_history.get.return_value = {"t": [31, 40, 65, 99, 105],
                                                 "series": {"cpu": [28, 32, None, 39.5, 54], "lost": []}}
        with patch("airglow.deskdisplay.time.time", return_value=100):
            view = display_view(store, ctrl)
        self.assertEqual(view["times"], [40, 65, 99])
        self.assertEqual([r["id"] for r in view["readings"]], ["cpu", "lost"])
        self.assertEqual(view["readings"][0]["samples"], [32, None, 39.5])
        self.assertEqual(view["readings"][1]["samples"], [None] * 3)
        self.assertIsNone(view["readings"][1]["value"])
        self.assertEqual(view["temp_unit"], "f")
        ctrl.readings_history.get.assert_called_once_with(["cpu", "lost"])

    def test_concurrent_open_reuses_one_window_and_manual_close_allows_reopen(self):
        first, second = fake_window(), fake_window()
        create = Mock(side_effect=[first, second])
        desk = DeskWindow("http://127.0.0.1:8000/", create)
        with ThreadPoolExecutor(max_workers=4) as pool:
            states = list(pool.map(lambda _: desk.open(), range(4)))
        self.assertTrue(all(s["open"] for s in states))
        create.assert_called_once()
        self.assertEqual(create.call_args.args[1], "http://127.0.0.1:8000/desk.html")
        self.assertTrue(desk.pin(True)["pinned"])
        first.events.closed.close()
        self.assertFalse(desk.status()["open"])
        self.assertFalse(desk.open()["pinned"])
        first.events.closed.callback()
        self.assertTrue(desk.status()["open"], "late close from previous window must not clear the replacement")
        desk.shutdown()
        desk.shutdown()
        second.destroy.assert_called_once()
        with self.assertRaisesRegex(RuntimeError, "closing"):
            desk.open()

    def test_open_during_native_form_creation_keeps_the_original_window(self):
        first = fake_window()
        first.native = None
        create = Mock(return_value=first)
        desk = DeskWindow("http://127.0.0.1:8000", create)
        self.assertTrue(desk.open()["open"])
        self.assertTrue(desk.open()["open"])
        create.assert_called_once()
        first.show.assert_not_called()

    def test_native_failure_and_browser_fallback_are_actionable(self):
        server = SimpleNamespace(ctrl=Mock(), store=Mock(), hooks={})
        self.assertFalse(api.route(server, "POST", "/api/desk-display/open", {})["native"])
        for value, expected in [(True, 409), (1, 400), ("yes", 400), (None, 400)]:
            with self.subTest(value=value), self.assertRaises(api.ApiError) as caught:
                api.route(server, "POST", "/api/desk-display/pin", {"pinned": value})
            self.assertEqual(caught.exception.status, expected)
        native = DeskWindow("http://127.0.0.1:8000", Mock(return_value=None))
        server.hooks["desk_display"] = native
        with self.assertRaisesRegex(api.ApiError, "could not open"):
            api.route(server, "POST", "/api/desk-display/open", {})
        with self.assertRaisesRegex(api.ApiError, "before pinning"):
            api.route(server, "POST", "/api/desk-display/pin", {"pinned": False})

    def test_custom_selection_keeps_requested_order_and_has_a_bound(self):
        ctrl, store = Mock(), Mock()
        store.desk_preferences.side_effect = lambda: {"pins":["overview"]}
        ctrl.readings = {}
        ctrl.readings_history.get.return_value = {"t": [], "series": {}}
        self.assertEqual([r["id"] for r in display_view(store,ctrl,reading_ids=["second","first","second"])["readings"]],
                         ["second","first"])
        self.assertEqual(display_view(store,ctrl,reading_ids=[])["readings"], [])
        with self.assertRaisesRegex(ValueError, "four readings"):
            display_view(store,ctrl,reading_ids=["one"]*5)
