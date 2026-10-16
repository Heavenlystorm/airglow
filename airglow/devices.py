"""USB HID drivers for the NZXT Control Hub and ASUS Aura LED controller.

The packet formats match what the open-source liquidctl and OpenRGB projects document for these
controllers (see THIRD_PARTY_NOTICES.md); no code from either is included. Packet construction is kept in pure
functions so it can be unit-tested without hardware.
"""

import logging
import threading
import time

log = logging.getLogger(__name__)

ORDERS = {"RGB": (0, 1, 2), "RBG": (0, 2, 1), "BGR": (2, 1, 0), "BRG": (2, 0, 1), "GBR": (1, 2, 0), "GRB": (1, 0, 2)}


def reorder(rgb, order):
    return [rgb[i] for i in ORDERS[order]]


class DeviceError(Exception):
    pass


# ---------------------------------------------------------------- packet builders (pure)

def nzxt_color_packets(rgb, channel, leds=40):
    """Direct-colour packets for one hub channel: 20 LEDs per packet in GRB order, then a commit."""
    data = reorder(rgb, "GRB") * leds
    packets = []
    for packet_no, start in enumerate(range(0, len(data), 60)):
        packets.append([0x22, 0x10 | packet_no, 1 << channel, 0x00] + data[start:start + 60])
    packets.append([0x22, 0xA0, 1 << channel, 0x00, 0x01, 0x00, 0x00, 0x28,
                    0x00, 0x00, 0x80, 0x00, 0x32, 0x00, 0x00, 0x01])
    return packets


def nzxt_stream_packets(colors, channel):
    """Direct-colour packets for an arbitrary LED list (any count up to 40), GRB order, then a commit."""
    data = [v for rgb in colors for v in reorder(rgb, "GRB")]
    packets = [[0x22, 0x10 | n, 1 << channel, 0x00] + data[s:s + 60] for n, s in enumerate(range(0, len(data), 60))]
    packets.append([0x22, 0xA0, 1 << channel, 0x00, 0x01, 0x00, 0x00, 0x28,
                    0x00, 0x00, 0x80, 0x00, 0x32, 0x00, 0x00, 0x01])
    return packets


def nzxt_parse_accessories(report, channels):
    """LED info report (0x21 0x03): up to 6 daisy-chained accessory ids per channel, from byte 15."""
    return [[report[15 + ch * 6 + i] for i in range(6) if report[15 + ch * 6 + i]] for ch in range(channels)]


def nzxt_fan_packet(duties, ports=5):
    """duties: {port_index: percent}. Unlisted ports keep their speed (not in the mask)."""
    packet = [0x62, 0x01, 0x00] + [0] * ports
    for port, duty in duties.items():
        if not 0 <= port < ports:
            raise ValueError(f"fan port {port} out of range")
        packet[2] |= 1 << port
        packet[3 + port] = max(0, min(100, round(duty)))
    return packet


def nzxt_parse_fans(report, ports=5):
    """Fan status report (0x67 0x02): RPM little-endian from byte 24, duty from byte 40."""
    return [{"rpm": report[24 + i * 2] | (report[25 + i * 2] << 8), "duty": report[40 + i]} for i in range(ports)]


def aura_direct_packets(idx, colors, count):
    """Direct-mode colour stream for one Aura channel; the last packet carries the apply flag."""
    packets, sent = [], 0
    while count > 0:
        n = min(count, 20)
        count -= n
        flag = 0x80 if count == 0 else 0x00
        packets.append([0xEC, 0x40, flag | idx, sent, n] + colors[sent * 3:(sent + n) * 3])
        sent += n
    return packets


def aura_parse_config(table):
    """Config table (0xEC 0x30): returns (argb_channel_count, leds_on_mainboard_direct_channel)."""
    argb = table[6]
    headers = table[33]
    if headers == 3:            # firmware quirk, handled the same way SignalRGB does
        board = max(table[31] - 1, 0) + 1
    else:
        board = max(table[31] - headers, 0) + headers
    return argb, board


# ---------------------------------------------------------------- device classes

class HidDevice:
    vid = pid = 0
    interface = None
    write_len = 64
    name = "device"

    def __init__(self, backend=None, path=None):
        if backend is None:
            import hid as backend
        self._hid = backend
        self.path = path                # a specific controller when several of the same model are plugged in
        self._dev = None
        self._lock = threading.RLock()

    def _matches(self, info):
        if self.path is not None and info.get("path") != self.path:
            return False
        return self.interface is None or info.get("interface_number") == self.interface

    def open(self):
        info = next((d for d in self._hid.enumerate(self.vid, self.pid) if self._matches(d)), None)
        if not info:
            raise DeviceError(f"{self.name} isn't connected")
        dev = self._hid.device()
        try:
            dev.open_path(info["path"])
        except OSError as e:
            raise DeviceError(f"{self.name} is busy. Close SignalRGB or other RGB software.") from e
        dev.set_nonblocking(1)
        self._dev = dev
        log.info("%s opened", self.name)

    def close(self):
        with self._lock:
            if self._dev:
                try:
                    self._dev.close()
                except OSError:
                    pass
                self._dev = None

    @property
    def is_open(self):
        return self._dev is not None

    def write(self, packet):
        if not self._dev:
            raise DeviceError(f"{self.name} isn't open")
        data = bytes(packet[: self.write_len]) + bytes(max(0, self.write_len - len(packet)))
        if self._dev.write(data) < 0:
            raise DeviceError(f"{self.name}: write failed")

    def read(self, size=64, timeout_ms=0):
        dev = self._dev                         # another thread may close the device between check and use
        if not dev:
            raise DeviceError(f"{self.name} isn't open")
        return dev.read(size, timeout_ms) if timeout_ms else dev.read(size)


class NzxtControlHub(HidDevice):
    """NZXT HUE 2 generation controllers: Control Hub (5 ARGB channels, 5 fan ports), RGB & Fan Controller,
    Smart Device V2, HUE 2. They share one protocol and differ in channel counts (see plugins/nzxt_hue2.py)."""

    vid, pid, name = 0x1E71, 0x2022, "NZXT Control Hub"
    write_len = 64
    LEDS_PER_CHANNEL = 40
    FAN_PORTS = 5

    def __init__(self, backend=None, pid=0x2022, name="NZXT Control Hub", led_channels=5, fan_ports=5, path=None):
        super().__init__(backend, path)
        self.pid, self.name = pid, name
        self.led_channels = led_channels
        self.FAN_PORTS = fan_ports
        self.channels = led_channels
        self.accessories = None          # per channel: list of accessory ids, filled by the LED info report
        self.fans = [{"rpm": 0, "duty": 0} for _ in range(self.FAN_PORTS)]

    def open(self):
        super().open()
        with self._lock:
            if self.led_channels:
                self.write([0x20, 0x03])                              # LED info -> channel count + accessories
            else:
                self.accessories = []                                 # fans-only model
            if self.FAN_PORTS:
                self.write([0x60, 0x03])                              # fan report config
                self.write([0x60, 0x02, 0x01, 0xE8, 3, 0x01, 0xE8, 3])   # fan report every second
            # The LED info reply arrives a few ms later; wait for it so zones start with the real LED counts.
            deadline = time.monotonic() + 0.5
            while self.accessories is None and time.monotonic() < deadline:
                p = self.read(64, 50)
                if p:
                    self._handle(p)
        self.poll()

    def _handle(self, p):
        if p[0] == 0x21 and p[1] == 0x03 and p[14]:
            self.channels = min(p[14], self.led_channels)
            self.accessories = nzxt_parse_accessories(p, self.channels)
        elif p[0] == 0x67 and p[1] == 0x02:
            self.fans = nzxt_parse_fans(p, self.FAN_PORTS)

    def poll(self):
        with self._lock:
            for _ in range(64):
                p = self.read(64)
                if not p:
                    break
                self._handle(p)

    def connected_fans(self):
        return [i for i, f in enumerate(self.fans) if f["rpm"] > 0]

    def set_color(self, rgb):
        with self._lock:
            for ch in range(self.channels):
                for packet in nzxt_color_packets(rgb, ch, self.LEDS_PER_CHANNEL):
                    self.write(packet)

    def set_channel_colors(self, channel, colors):
        if not colors or not 0 <= channel < self.channels:
            return
        with self._lock:
            for packet in nzxt_stream_packets(colors[: self.LEDS_PER_CHANNEL], channel):
                self.write(packet)

    def set_fan_duty(self, duties):
        if duties and self.FAN_PORTS:
            with self._lock:
                self.write(nzxt_fan_packet(duties, self.FAN_PORTS))


class AsusAura(HidDevice):
    """ASUS AURA LED Controller (0b05:19af, HID interface 2): mainboard LEDs + ARGB headers."""

    vid, pid, interface, name = 0x0B05, 0x19AF, 2, "ASUS Aura"
    write_len = 65
    LEDS_PER_ARGB = 120
    MAINBOARD_DIRECT_IDX = 4

    def __init__(self, argb_order="RGB", board_order="RGB", backend=None, pid=0x19AF, interface=2, path=None):
        super().__init__(backend, path)
        self.pid, self.interface = pid, interface
        self.argb_order, self.board_order = argb_order, board_order
        self.model = ""
        self.argb_channels = 0
        self.board_leds = 0
        self.config_known = False        # the board answered the config query, so argb_channels is real

    def _query(self, cmd, expect):
        self.write([0xEC, cmd])
        deadline = time.monotonic() + 0.5
        while time.monotonic() < deadline:
            p = self.read(65, 100)
            if p and len(p) > 1 and p[1] == expect:
                return p
        return None

    def open(self):
        super().open()
        with self._lock:
            for _ in range(32):
                if not self.read(65):
                    break
            fw = self._query(0x82, 0x02)
            if fw:
                self.model = bytes(fw[2:18]).decode(errors="replace").strip("\x00 ")
            cfg = self._query(0xB0, 0x30)
            if cfg:
                self.argb_channels, self.board_leds = aura_parse_config(cfg)
                self.config_known = True
            self.write([0xEC, 0x52, 0x53, 0x00, 0x01])
            for ch in range(self.argb_channels + 1):              # 0 = mainboard, 1.. = ARGB
                self.write([0xEC, 0x35, ch, 0x00, 0x00, 0xFF])    # direct mode

    def set_board_colors(self, colors):
        if not self.board_leds or not colors:
            return
        flat = [v for c in colors[: self.board_leds] for v in c]
        with self._lock:
            for p in aura_direct_packets(self.MAINBOARD_DIRECT_IDX, flat, min(len(colors), self.board_leds)):
                self.write(p)

    def set_argb_colors(self, index, colors):
        if not colors or not 0 <= index < self.argb_channels:
            return
        colors = colors[: self.LEDS_PER_ARGB]
        flat = [v for c in colors for v in c]
        with self._lock:
            for p in aura_direct_packets(index, flat, len(colors)):
                self.write(p)

    def set_color(self, rgb):
        with self._lock:
            if self.board_leds:
                colors = reorder(rgb, self.board_order) * self.board_leds
                for p in aura_direct_packets(self.MAINBOARD_DIRECT_IDX, colors, self.board_leds):
                    self.write(p)
            colors = reorder(rgb, self.argb_order) * self.LEDS_PER_ARGB
            for ch in range(self.argb_channels):
                for p in aura_direct_packets(ch, colors, self.LEDS_PER_ARGB):
                    self.write(p)
