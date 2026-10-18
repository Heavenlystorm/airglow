import socket
import struct
import threading
import time
import unittest

from airglow import openrgb


def s(text):
    b = text.encode() + b"\0"
    return struct.pack("<H", len(b)) + b


def controller_blob(version=3, name="NZXT RGB & Fan Controller", zones=(("Channel 1", 1, 0, 40, 24),),
                    segments=(), matrix=None, ctype=4, drift=False):
    """Encode a controller the way the OpenRGB server does (layout from Documentation/OpenRGBSDK.md).
    segments: (name, start, count) for the first zone; matrix: (rows, cols, cells) for the first zone;
    drift: add a field this client doesn't know, as a future server might."""
    body = struct.pack("<i", ctype) + s(name)
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
    for zi, (zname, ztype, lmin, lmax, count) in enumerate(zones):
        body += s(zname) + struct.pack("<iIII", ztype, lmin, lmax, count)
        if zi == 0 and matrix:
            rows, cols, cells = matrix
            body += struct.pack("<H", 8 + 4 * len(cells)) + struct.pack("<II", rows, cols)
            body += b"".join(struct.pack("<I", c) for c in cells)
        else:
            body += struct.pack("<H", 0)
        if version >= 4:
            segs = segments if zi == 0 else ()
            body += struct.pack("<H", len(segs))
            body += b"".join(s(n) + struct.pack("<iII", 1, start, cnt) for n, start, cnt in segs)
        if version >= 5:
            body += struct.pack("<I", 0)                                   # zone flags
        if drift and version >= 5:
            body += struct.pack("<I", 7)
        total += count
    body += struct.pack("<H", total) + b"".join(s(f"LED {i}") + struct.pack("<I", i) for i in range(total))
    body += struct.pack("<H", total) + b"".join(struct.pack("<I", 0x0000FF) for _ in range(total))
    if version >= 5:
        body += struct.pack("<H", 0) + struct.pack("<I", 0)                   # LED display names, flags
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

    def __init__(self, server_version=4, notify=False, drift=False):
        super().__init__(daemon=True)
        self.sock = socket.socket()
        self.sock.bind(("127.0.0.1", 0))
        self.sock.listen(1)
        self.port = self.sock.getsockname()[1]
        self.server_version, self.notify, self.drift = server_version, notify, drift
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
                    blob = controller_blob(version, drift=self.drift)
                    conn.sendall(openrgb.header(dev, pid, len(blob)) + blob)
        except (ConnectionError, OSError):
            pass


class ProtocolFiveTests(unittest.TestCase):
    def test_segments_matrix_and_flags(self):
        blob = controller_blob(5, segments=(("Fan 1", 0, 8), ("Fan 2", 8, 8), ("Fan 3", 16, 8)))
        c = openrgb.parse_controller(blob, 5)
        self.assertEqual([g["name"] for g in c["zones"][0]["segments"]], ["Fan 1", "Fan 2", "Fan 3"])
        self.assertEqual(c["zones"][0]["flags"], 0)
        grid = controller_blob(5, name="Keyboard", zones=(("Keys", 2, 4, 4, 4),), ctype=5,
                               matrix=(2, 3, [0, 1, openrgb.NO_LED, 2, openrgb.NO_LED, 3]))
        k = openrgb.parse_controller(grid, 5)
        self.assertEqual(k["zones"][0]["matrix"], {"rows": 2, "cols": 3, "map": [0, 1, -1, 2, -1, 3]})

    def test_layout_mismatch_is_an_error_not_garbage(self):
        with self.assertRaises(openrgb.OpenRGBError):
            openrgb.parse_controller(controller_blob(5, drift=True), 5)
        with self.assertRaises(openrgb.OpenRGBError):
            openrgb.parse_controller(controller_blob(3), 5)

    def test_client_falls_back_to_protocol_3(self):
        srv = FakeServer(server_version=5, drift=True)
        srv.start()
        c = openrgb.OpenRGBClient(port=srv.port)
        c.connect()
        ctrls = c.controllers()
        self.assertEqual(ctrls[0]["zones"][0]["leds_count"], 24)
        asked = [struct.unpack("<I", d)[0] for _, pid, d in srv.received if pid == openrgb.REQUEST_CONTROLLER_DATA]
        self.assertEqual(asked, [5, 3])
        c.close()
        srv.join(2)


class DeviceShapeTests(unittest.TestCase):
    def test_fans_on_a_header_and_keyboard_grid(self):
        from airglow import hw
        from airglow.lighting import geometry
        ctrl = openrgb.parse_controller(controller_blob(5, name="ARGB", zones=(("Header", 1, 0, 120, 24),),
                                                        segments=(("Fan 1", 0, 8), ("Fan 2", 8, 8), ("Fan 3", 16, 8))), 5)
        dev = hw.OpenRGBDevice(bridge=None, ctrl={"index": 0, **ctrl})
        self.assertEqual((dev.zones[0].shape, dev.zones[0].fans), ("fans", 3))
        kb = openrgb.parse_controller(controller_blob(5, name="Keyboard", zones=(("Keys", 2, 4, 4, 4),), ctype=5,
                                                      matrix=(2, 3, [0, 1, openrgb.NO_LED, 2, openrgb.NO_LED, 3])), 5)
        z = hw.OpenRGBDevice(bridge=None, ctrl={"index": 0, **kb}).zones[0]
        self.assertEqual(z.shape, "matrix")
        layout = geometry.validate_layout({"shape": "matrix", "leds": 4, "rows": 2, "cols": 3, "map": z.matrix["map"],
                                           "x": 0.8, "y": 0.5, "length": 0.6}, 4)
        pos, _, _ = geometry.positions(layout)
        self.assertEqual(pos.shape, (4, 2))
        self.assertLess(pos[0][1], pos[2][1])                          # LED 2 is on the second row
        self.assertAlmostEqual(pos[1][1], pos[0][1])


class ClientTests(unittest.TestCase):
    def test_negotiates_version_and_lists_controllers(self):
        srv = FakeServer(server_version=5)
        srv.start()
        c = openrgb.OpenRGBClient(port=srv.port)
        c.connect()
        self.assertEqual(c.version, 5)                    # min(client 5, server 5)
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

    def test_a_notification_that_stops_half_way_does_not_hang_the_client(self):
        """A header with no body after it used to block poll_notifications (and every lighting write) for ever."""
        srv = FakeServer()
        srv.start()
        c = openrgb.OpenRGBClient(port=srv.port, timeout=0.3)
        c.connect()
        c.controllers()
        srv.conn.sendall(openrgb.header(0, openrgb.DEVICE_LIST_UPDATED, 8))      # promises 8 bytes, sends none
        time.sleep(0.1)
        done = threading.Event()
        threading.Thread(target=lambda: (c.poll_notifications(), done.set()), daemon=True).start()
        self.assertTrue(done.wait(3), "poll_notifications waited for ever on a half-sent packet")
        self.assertFalse(c.connected)                    # the stream can't be trusted any more: reconnect
        srv.conn.close()

    def test_writing_after_another_thread_closed_is_an_openrgb_error(self):
        """Regression: the device thread closes the connection on a reset after sleep while the lights are mid-frame;
        that raised AttributeError, which the writer thread doesn't expect."""
        c = openrgb.OpenRGBClient()
        for call in (lambda: c.update_zone(0, 0, [(1, 2, 3)]), lambda: c.set_custom_mode(0), lambda: c.controllers()):
            with self.assertRaises(openrgb.OpenRGBError):
                call()

    def test_connection_refused(self):
        c = openrgb.OpenRGBClient(port=1, timeout=0.3)
        with self.assertRaises(OSError):
            c.connect()
        self.assertFalse(c.connected)


if __name__ == "__main__":
    unittest.main()
