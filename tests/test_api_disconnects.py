"""A cancelled preview request is a disconnected client, not an internal API failure."""
from types import SimpleNamespace
import unittest
from unittest.mock import Mock

from airglow.api import Handler


class ResponseDisconnectTests(unittest.TestCase):
    def handler(self):
        return SimpleNamespace(send_response=Mock(), send_header=Mock(), end_headers=Mock(),
                               wfile=SimpleNamespace(write=Mock()), close_connection=False)

    def test_cancelled_body_response_closes_connection_without_second_response(self):
        handler = self.handler()
        handler.wfile.write.side_effect = ConnectionAbortedError(10053, "client cancelled preview")
        Handler._send(handler, 200, {"ok": True})
        self.assertTrue(handler.close_connection)
        handler.send_response.assert_called_once_with(200)

    def test_disconnect_while_headers_flush_closes_connection(self):
        handler = self.handler()
        handler.end_headers.side_effect = BrokenPipeError("client left")
        Handler._send(handler, 200, {"ok": True})
        self.assertTrue(handler.close_connection)
        handler.wfile.write.assert_not_called()

    def test_payload_errors_are_not_hidden_as_client_disconnects(self):
        handler = self.handler()
        with self.assertRaises(TypeError):
            Handler._send(handler, 200, {"bad": object()})
        handler.send_response.assert_not_called()
