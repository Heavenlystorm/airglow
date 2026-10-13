import socket
import struct
import threading
import unittest

from airglow import openrgb


def s(text):
    b = text.encode() + b"\0"
    return struct.pack("<H", len(b)) + b


def controller_blob(version=3, name="NZXT RGB & Fan Controller", zones=(("Channel 1", 1, 0, 40, 24),)):
    """Encode a controller the way the OpenRGB server does for protocol <= 3."""
    body = struct.pack("<i", 4) + s(name)
    if version >= 1:
        body += s("NZXT")
    body += s("desc") + s("1.0") + s("SER123") + s("HID: /dev")
    body += struct.pack("<H", 1) + struct.pack("<i", 0)                        # one mode, active 0
    body += s("Direct") + struct.pack("<iI", 0, 0x20) + struct.pack("<II", 0, 0)
    if version >= 3:
        body += struct.pack("<II", 0, 100)
    body += struct.pack("<II", 0, 0) + struct.pack("<I", 0)
    if version >= 3:
        body += struct.pack("<I", 100)
    body += struct.pack("<II", 0, 1) + struct.pack("<H", 0)
    body += struct.pack("<H", len(zones))
    total = 0
    for zname, ztype, lmin, lmax, count in zones:
        body += s(zname) + struct.pack("<iIII", ztype, lmin, lmax, count) + struct.pack("<H", 0)
        total += count
    body += struct.pack("<H", total) + b"".join(s(f"LED {i}") + struct.pack("<I", i) for i in range(total))
    body += struct.pack("<H", total) + b"".join(struct.pack("<I", 0x0000FF) for _ in range(total))
    return struct.pack("<I", 4 + len(body)) + body


class ParseTests(unittest.TestCase):
    def test_round_trip_v3(self):
        c = openrgb.parse_controller(controller_blob(3), 3)
        self.assertEqual((c["name"], c["vendor"], c["serial"]), ("NZXT RGB & Fan Controller", "NZXT", "SER123"))
        self.assertEqual(c["modes"][0]["name"], "Direct")
        self.assertEqual(c["zones"][0]["leds_count"], 24)
        self.assertEqual(len(c["led_names"]), 24)
        self.assertEqual(c["colors"][0], (255, 0, 0))
        self.assertEqual(c["type_name"], "LED strip")

    def test_version_0_has_no_vendor(self):
        c = openrgb.parse_controller(controller_blob(0), 0)
        self.assertEqual(c["vendor"], "")
        self.assertEqual(c["zones"][0]["name"], "Channel 1")

    def test_zone_led_encoding(self):
        data = openrgb.encode_zone_leds(2, [(1, 2, 3), (255, 0, 128)])
        size, zone, n = struct.unpack_from("<IIH", data)
        self.assertEqual((size, zone, n), (len(data), 2, 2))
        self.assertEqual(struct.unpack_from("<II", data, 10), (0x030201, 0x8000FF))


class FakeServer(threading.Thread):
    """Speaks just enough of the protocol for the client tests; records what it received."""

    def __init__(self, server_version=4, notify=False):
        super().__init__(daemon=True)
        self.sock = socket.socket()
        self.sock.bind(("127.0.0.1", 0))
        self.sock.listen(1)
        self.port = self.sock.getsockname()[1]
        self.server_version, self.notify = server_version, notify
        self.received = []

    def _recv(self, conn, n):
        buf = b""
        while len(buf) < n:
            chunk = conn.recv(n - len(buf))
            if not chunk:
                raise ConnectionError
            buf += chunk
        return buf

    def run(self):
        conn, _ = self.sock.accept()
        self.conn = conn
        try:
            while True:
                head = self._recv(conn, 16)
                dev, pid, size = struct.unpack("<III", head[4:])
                data = self._recv(conn, size) if size else b""
                self.received.append((dev, pid, data))
                if pid == openrgb.REQUEST_PROTOCOL_VERSION:
                    conn.sendall(openrgb.header(0, pid, 4) + struct.pack("<I", self.server_version))
                elif pid == openrgb.REQUEST_CONTROLLER_COUNT:
                    if self.notify:                      # notification interleaved before the reply
                        conn.sendall(openrgb.header(0, openrgb.DEVICE_LIST_UPDATED, 0))
                    conn.sendall(openrgb.header(0, pid, 4) + struct.pack("<I", 1))
                elif pid == openrgb.REQUEST_CONTROLLER_DATA:
                    version = struct.unpack("<I", data)[0]
                    blob = controller_blob(version)
                    conn.sendall(openrgb.header(dev, pid, len(blob)) + blob)
        except (ConnectionError, OSError):
            pass


class ClientTests(unittest.TestCase):
    def test_negotiates_version_and_lists_controllers(self):
        srv = FakeServer(server_version=5)
        srv.start()
        c = openrgb.OpenRGBClient(port=srv.port)
        c.connect()
        self.assertEqual(c.version, 3)                    # min(client 3, server 5)
        ctrls = c.controllers()
        self.assertEqual(ctrls[0]["vendor"], "NZXT")
        c.set_custom_mode(0)
        c.update_zone(0, 0, [(10, 20, 30)] * 24)
        c.close()
        srv.join(2)
        ids = [pid for _, pid, _ in srv.received]
        self.assertEqual(ids[:2], [openrgb.REQUEST_PROTOCOL_VERSION, openrgb.SET_CLIENT_NAME])
        self.assertIn(openrgb.SET_CUSTOM_MODE, ids)
        upd = next(d for _, pid, d in srv.received if pid == openrgb.UPDATE_ZONE_LEDS)
        self.assertEqual(struct.unpack_from("<IH", upd, 4), (0, 24))

    def test_notification_during_request_is_remembered(self):
        srv = FakeServer(notify=True)
        srv.start()
        c = openrgb.OpenRGBClient(port=srv.port)
        c.connect()
        self.assertEqual(len(c.controllers()), 1)
        c.close()

    def test_server_going_away_closes_client_without_hanging(self):
        """Regression: close() inside a locked section deadlocked the lighting engine when OpenRGB quit."""
        srv = FakeServer()
        srv.start()
        c = openrgb.OpenRGBClient(port=srv.port)
        c.connect()
        c.controllers()
        srv.conn.shutdown(socket.SHUT_RDWR)       # OpenRGB quits: its end of the connection goes away
        srv.conn.close()
        done = threading.Event()

        def engine_like_loop():
            for _ in range(200):
                c.poll_notifications()
                if not c.connected:
                    break
                try:
                    c.update_zone(0, 0, [(1, 2, 3)])
                except openrgb.OpenRGBError:
                    pass
            done.set()

        threading.Thread(target=engine_like_loop, daemon=True).start()
        self.assertTrue(done.wait(5), "client hung while handling a dropped connection")
        self.assertFalse(c.connected)

    def test_connection_refused(self):
        c = openrgb.OpenRGBClient(port=1, timeout=0.3)
        with self.assertRaises(OSError):
            c.connect()
        self.assertFalse(c.connected)


if __name__ == "__main__":
    unittest.main()
