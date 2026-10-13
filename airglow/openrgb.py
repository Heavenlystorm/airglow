"""Minimal OpenRGB SDK client (network protocol, written from the published spec).

We ask for protocol version 3; servers encode controller data for the version the client asks for, so this
works with OpenRGB 0.6 through 1.x. Only what Airglow needs is implemented: list controllers, switch to
direct ("custom") mode, resize ARGB zones, and push LED colours.
"""

import logging
import socket
import struct
import threading

log = logging.getLogger(__name__)

MAGIC = b"ORGB"
CLIENT_PROTOCOL = 3

REQUEST_CONTROLLER_COUNT = 0
REQUEST_CONTROLLER_DATA = 1
REQUEST_PROTOCOL_VERSION = 40
SET_CLIENT_NAME = 50
DEVICE_LIST_UPDATED = 100
RESIZE_ZONE = 1000
UPDATE_LEDS = 1050
UPDATE_ZONE_LEDS = 1051
SET_CUSTOM_MODE = 1100

ZONE_SINGLE, ZONE_LINEAR, ZONE_MATRIX = 0, 1, 2
DEVICE_TYPES = ["Motherboard", "Memory", "Graphics card", "Cooler", "LED strip", "Keyboard", "Mouse", "Mouse mat",
                "Headset", "Headset stand", "Gamepad", "Light", "Speaker", "Virtual", "Storage", "Case", "Microphone",
                "Accessory", "Keypad", "Laptop", "Monitor"]


class OpenRGBError(Exception):
    pass


# ---------------------------------------------------------------- encoding / decoding

def header(dev_idx, pkt_id, size):
    return MAGIC + struct.pack("<III", dev_idx, pkt_id, size)


def color_u32(rgb):
    r, g, b = rgb
    return r | (g << 8) | (b << 16)


def encode_zone_leds(zone_idx, colors):
    body = struct.pack("<IH", zone_idx, len(colors)) + b"".join(struct.pack("<I", color_u32(c)) for c in colors)
    return struct.pack("<I", 4 + len(body)) + body


def encode_leds(colors):
    body = struct.pack("<H", len(colors)) + b"".join(struct.pack("<I", color_u32(c)) for c in colors)
    return struct.pack("<I", 4 + len(body)) + body


class _Reader:
    def __init__(self, data):
        self.d, self.i = data, 0

    def take(self, fmt):
        v = struct.unpack_from("<" + fmt, self.d, self.i)
        self.i += struct.calcsize("<" + fmt)
        return v if len(v) > 1 else v[0]

    def string(self):
        n = self.take("H")
        s = self.d[self.i:self.i + n].split(b"\0", 1)[0].decode("utf-8", "replace")
        self.i += n
        return s


def parse_controller(data, version):
    """Decode a REQUEST_CONTROLLER_DATA reply into a plain dict."""
    r = _Reader(data)
    r.take("I")                                        # data size
    ctrl = {"type": r.take("i"), "name": r.string()}
    ctrl["vendor"] = r.string() if version >= 1 else ""
    ctrl["description"], ctrl["version"], ctrl["serial"], ctrl["location"] = r.string(), r.string(), r.string(), r.string()
    num_modes = r.take("H")
    ctrl["active_mode"] = r.take("i")
    modes = []
    for _ in range(num_modes):
        m = {"name": r.string(), "value": r.take("i"), "flags": r.take("I")}
        r.take("II")                                   # speed min/max
        if version >= 3:
            r.take("II")                               # brightness min/max
        r.take("II")                                   # colours min/max
        r.take("I")                                    # speed
        if version >= 3:
            r.take("I")                                # brightness
        r.take("II")                                   # direction, colour mode
        n = r.take("H")
        r.take(f"{n}I") if n else None
        modes.append(m)
    ctrl["modes"] = modes
    zones = []
    for _ in range(r.take("H")):
        z = {"name": r.string(), "type": r.take("i")}
        z["leds_min"], z["leds_max"], z["leds_count"] = r.take("III")
        matrix_len = r.take("H")
        if matrix_len:
            h, w = r.take("II")
            r.take(f"{h * w}I") if h * w else None
        zones.append(z)
    ctrl["zones"] = zones
    leds = []
    for _ in range(r.take("H")):
        leds.append(r.string())
        r.take("I")
    ctrl["led_names"] = leds
    n = r.take("H")
    raw = r.take(f"{n}I") if n else ()
    raw = (raw,) if isinstance(raw, int) else raw
    ctrl["colors"] = [(c & 0xFF, (c >> 8) & 0xFF, (c >> 16) & 0xFF) for c in raw]
    ctrl["type_name"] = DEVICE_TYPES[ctrl["type"]] if 0 <= ctrl["type"] < len(DEVICE_TYPES) else "Device"
    return ctrl


# ---------------------------------------------------------------- client

class OpenRGBClient:
    def __init__(self, host="127.0.0.1", port=6742, name="Airglow", timeout=2.0):
        self.host, self.port, self.name, self.timeout = host, port, name, timeout
        self.sock = None
        self.version = 0
        self.devices_changed = False
        # Reentrant: close() is called from inside locked sections when the connection drops (a plain Lock
        # deadlocked the lighting engine when OpenRGB was closed).
        self._lock = threading.RLock()

    @property
    def connected(self):
        return self.sock is not None

    def connect(self):
        s = socket.create_connection((self.host, self.port), timeout=self.timeout)
        s.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        self.sock = s
        try:
            reply = self._request(0, REQUEST_PROTOCOL_VERSION, struct.pack("<I", CLIENT_PROTOCOL), expect_reply=True,
                                  timeout=1.0)
            self.version = min(CLIENT_PROTOCOL, struct.unpack("<I", reply[:4])[0])
        except socket.timeout:
            self.version = 0                            # protocol 0 servers don't answer this request
        self._send(0, SET_CLIENT_NAME, self.name.encode() + b"\0")
        log.info("connected to OpenRGB at %s:%d (protocol %d)", self.host, self.port, self.version)

    def close(self):
        with self._lock:
            if self.sock:
                try:
                    self.sock.close()
                except OSError:
                    pass
                self.sock = None

    # -- wire
    def _send(self, dev_idx, pkt_id, payload=b""):
        try:
            self.sock.sendall(header(dev_idx, pkt_id, len(payload)) + payload)
        except OSError as e:
            self.close()
            raise OpenRGBError(f"OpenRGB connection lost ({e})") from e

    def _recv_exact(self, n):
        buf = b""
        while len(buf) < n:
            chunk = self.sock.recv(n - len(buf))
            if not chunk:
                raise OpenRGBError("OpenRGB closed the connection")
            buf += chunk
        return buf

    def _read_packet(self):
        head = self._recv_exact(16)
        if head[:4] != MAGIC:
            raise OpenRGBError("unexpected data from OpenRGB")
        dev_idx, pkt_id, size = struct.unpack("<III", head[4:])
        return dev_idx, pkt_id, self._recv_exact(size) if size else b""

    def _request(self, dev_idx, pkt_id, payload=b"", expect_reply=True, timeout=None):
        self._send(dev_idx, pkt_id, payload)
        if not expect_reply:
            return None
        old = self.sock.gettimeout()
        if timeout is not None:
            self.sock.settimeout(timeout)
        try:
            while True:
                _, rid, data = self._read_packet()
                if rid == DEVICE_LIST_UPDATED:          # asynchronous notification; remember and keep reading
                    self.devices_changed = True
                    continue
                if rid == pkt_id:
                    return data
        except socket.timeout:
            raise
        except OSError as e:
            self.close()
            raise OpenRGBError(f"OpenRGB connection lost ({e})") from e
        finally:
            if self.sock:
                self.sock.settimeout(old)

    # -- API
    def controllers(self):
        with self._lock:
            count = struct.unpack("<I", self._request(0, REQUEST_CONTROLLER_COUNT)[:4])[0]
            out = []
            for i in range(count):
                payload = struct.pack("<I", self.version) if self.version >= 1 else b""
                out.append({"index": i, **parse_controller(self._request(i, REQUEST_CONTROLLER_DATA, payload), self.version)})
            self.devices_changed = False
            return out

    def set_custom_mode(self, dev_idx):
        with self._lock:
            self._send(dev_idx, SET_CUSTOM_MODE)

    def resize_zone(self, dev_idx, zone_idx, size):
        with self._lock:
            self._send(dev_idx, RESIZE_ZONE, struct.pack("<ii", zone_idx, size))

    def update_zone(self, dev_idx, zone_idx, colors):
        with self._lock:
            self._send(dev_idx, UPDATE_ZONE_LEDS, encode_zone_leds(zone_idx, colors))

    def update_leds(self, dev_idx, colors):
        with self._lock:
            self._send(dev_idx, UPDATE_LEDS, encode_leds(colors))

    def poll_notifications(self):
        """Drain pending notifications without blocking (called from the engine loop)."""
        if not self.sock:
            return
        with self._lock:
            self.sock.setblocking(False)
            try:
                while True:
                    try:
                        head = self.sock.recv(16, socket.MSG_PEEK)
                    except (BlockingIOError, InterruptedError):
                        break
                    if len(head) < 16:
                        if not head:
                            self.close()
                        break
                    self.sock.setblocking(True)
                    _, pid, _ = self._read_packet()
                    self.sock.setblocking(False)
                    if pid == DEVICE_LIST_UPDATED:
                        self.devices_changed = True
            except OSError:
                self.close()
            finally:
                if self.sock:
                    self.sock.settimeout(self.timeout)
