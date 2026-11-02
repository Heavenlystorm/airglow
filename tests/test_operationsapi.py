import importlib.util
import tempfile
from types import SimpleNamespace
import unittest

from airglow.config import Store
from airglow.controller import Controller
from tests.fakes import FakeManager, FakeNzxt, signals


class OperationsApiTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.store = Store(self.tmp.name)
        self.dev = FakeNzxt()
        self.manager = FakeManager(self.dev)
        self.manager.maintain(True)
        self.ctrl = Controller(self.store, manager=self.manager, read_sensors=lambda: {"online": False},
                               signals=signals, async_writes=False)
        self.server = SimpleNamespace(ctrl=self.ctrl, store=self.store)

    def route(self, method, path, body=None, query=None):
        self.assertIsNotNone(importlib.util.find_spec("airglow.operationsapi"), "operations routes are required")
        from airglow.operationsapi import route
        return route(self.server, method, path, body or {}, query)

    def test_reading_tools_does_not_take_control_and_recheck_is_explicit(self):
        status = self.route("GET", "/api/control/status")
        self.assertIn("events", status)
        self.assertEqual(self.route("GET", "/api/cooling/sessions")["sessions"], [])
        self.assertEqual(self.dev.hub.duties, [])
        self.assertEqual(self.manager.resets, 0)
        response = self.route("POST", "/api/control/recheck", {"target": "devices"})
        self.assertTrue(response["requested"])
        self.assertFalse(response["completed"])
        self.assertEqual(self.manager.resets, 1)
        self.assertEqual(self.dev.hub.duties, [])

    def test_record_export_replay_and_delete_route_contract(self):
        self.route("POST", "/api/cooling/start", {"annotation": "No sensors"})
        result = self.route("POST", "/api/cooling/stop")
        sid = result["session"]["id"]
        self.assertTrue(result["saved"])
        self.assertEqual(self.route("GET", "/api/cooling/session", query={"id": [sid]})["samples"], [])
        self.assertTrue(self.route("GET", "/api/cooling/export", query={"id": [sid], "format": ["csv"]})["filename"].endswith(".csv"))
        self.assertFalse(self.route("POST", "/api/cooling/replay", {"id": sid, "points": [[30, 30], [80, 100]]})["applied"])
        self.assertEqual(self.route("POST", "/api/cooling/delete", {"id": sid})["sessions"], [])
        self.assertEqual(self.dev.hub.duties, [])

    def test_unknown_routes_and_invalid_requests_are_errors(self):
        from airglow.api import ApiError
        with self.assertRaises(ApiError) as caught:
            self.route("POST", "/api/control/recheck", {"target": "fans"})
        self.assertEqual(caught.exception.status, 400)
        with self.assertRaises(ApiError) as caught:
            self.route("GET", "/api/cooling/nope")
        self.assertEqual(caught.exception.status, 404)
