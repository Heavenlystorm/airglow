import unittest

from airglow import devices
from tests.fakes import FakeHidBackend, FakeHidDevice

# Real reports captured from an NZXT Control Hub and an ASUS Aura controller.
NZXT_FAN_REPORT = bytes.fromhex(
    "67 02 c5 86 d0 11 77 e7 f8 50 9e 4f ff f5 05 ff ff ff ff ff ff 00 00 00 17 04 20 04 00 00 00 00 "
    "a6 04 00 00 00 00 00 00 26 26 19 19 19 00 00 00 26 26 19 19 19 00 00 00 00 00 00 00 00 00 00 00")
AURA_CONFIG = bytes.fromhex(
    "ec 30 00 00 1e 9f 03 01 00 00 78 3c 00 01 00 00 78 3c 00 01 00 00 78 3c 00 00 00 00 00 00 00 00 "
    "03 03 01 f4") + bytes(29)


class PacketTests(unittest.TestCase):
    def test_fixtures_are_full_reports(self):
        self.assertEqual(len(NZXT_FAN_REPORT), 64)
        self.assertEqual(len(AURA_CONFIG), 65)

    def test_nzxt_color_packets(self):
        packets = devices.nzxt_color_packets([255, 10, 0], channel=2, leds=40)
        self.assertEqual(len(packets), 3)                      # 2 x 20 LEDs + commit
        self.assertEqual(packets[0][:4], [0x22, 0x10, 0x04, 0x00])
        self.assertEqual(packets[1][:2], [0x22, 0x11])
        self.assertEqual(packets[0][4:7], [10, 255, 0])        # GRB order
        self.assertEqual(len(packets[0]), 4 + 60)
        self.assertEqual(packets[-1][:3], [0x22, 0xA0, 0x04])

    def test_nzxt_fan_packet_masks_only_given_ports(self):
        p = devices.nzxt_fan_packet({0: 40, 4: 101})
        self.assertEqual(p[:3], [0x62, 0x01, 0b10001])
        self.assertEqual(p[3], 40)
        self.assertEqual(p[7], 100)                            # clamped
        self.assertEqual(p[4:7], [0, 0, 0])

    def test_nzxt_fan_packet_rejects_bad_port(self):
        with self.assertRaises(ValueError):
            devices.nzxt_fan_packet({7: 50})

    def test_parse_real_fan_report(self):
        f = devices.nzxt_parse_fans(NZXT_FAN_REPORT)
        self.assertEqual([x["rpm"] for x in f], [1047, 1056, 0, 0, 1190])
        self.assertEqual([x["duty"] for x in f], [38, 38, 25, 25, 25])

    def test_aura_packets_apply_flag_on_last(self):
        colors = [1, 2, 3] * 45
        packets = devices.aura_direct_packets(1, colors, 45)
        self.assertEqual([p[2] for p in packets], [0x01, 0x01, 0x81])
        self.assertEqual([(p[3], p[4]) for p in packets], [(0, 20), (20, 20), (40, 5)])
        self.assertEqual(packets[2][5:8], [1, 2, 3])

    def test_parse_real_aura_config(self):
        self.assertEqual(devices.aura_parse_config(AURA_CONFIG), (3, 1))

    def test_reorder(self):
        self.assertEqual(devices.reorder([1, 2, 3], "GRB"), [2, 1, 3])
        self.assertEqual(devices.reorder([1, 2, 3], "BGR"), [3, 2, 1])


class HubTests(unittest.TestCase):
    def test_open_reads_fans_and_color_writes_padded_packets(self):
        dev = FakeHidDevice(reads=[list(NZXT_FAN_REPORT)])
        hub = devices.NzxtControlHub(backend=FakeHidBackend(device=dev))
        hub.open()
        self.assertEqual(hub.connected_fans(), [0, 1, 4])
        dev.writes.clear()
        hub.set_color([0, 255, 0])
        self.assertEqual(len(dev.writes), 5 * 3)              # 5 channels x (2 data + commit)
        self.assertTrue(all(len(w) == 64 for w in dev.writes))

    def test_missing_device(self):
        hub = devices.NzxtControlHub(backend=FakeHidBackend(present=False))
        with self.assertRaises(devices.DeviceError):
            hub.open()

    def test_failed_write_raises(self):
        hub = devices.NzxtControlHub(backend=FakeHidBackend(device=FakeHidDevice(fail_writes=True)))
        with self.assertRaises(devices.DeviceError):
            hub.open()

    def test_aura_writes_are_65_bytes(self):
        dev = FakeHidDevice(reads=[[], [0xEC, 0x02] + list(b"AULA3-AR32-0309") + [0] * 48, list(AURA_CONFIG)])
        aura = devices.AsusAura(backend=FakeHidBackend(device=dev))
        aura.open()
        self.assertEqual(aura.model, "AULA3-AR32-0309")
        self.assertEqual((aura.argb_channels, aura.board_leds), (3, 1))
        self.assertTrue(all(len(w) == 65 for w in dev.writes))


class AuraZoneTests(unittest.TestCase):
    def test_only_reported_headers_become_zones(self):
        from airglow.plugins.asus_aura_usb import AuraDevice
        aura = devices.AsusAura(backend=FakeHidBackend(device=FakeHidDevice()))
        dev = AuraDevice(aura)
        self.assertEqual(len(dev.zones), 4)                         # board + 3 until the board has answered
        aura.argb_channels, aura.config_known = 1, True
        dev._set_zones()
        self.assertEqual([z.id for z in dev.zones], ["board", "argb1"])


if __name__ == "__main__":
    unittest.main()
