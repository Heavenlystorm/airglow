import base64
import json
import tempfile
import threading
import unittest
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from airglow import config, fans, lhmfans, sensors
from airglow.controller import Controller
from tests.fakes import FakeManager, FakeNzxt, signals

FIXTURE = json.loads((Path(__file__).parent / "fixtures" / "lhm_data.json").read_text(encoding="utf-8"))
BOARD = "/lpc/nct6701d/0/control/1"            # "Fan #2" on the captured board, spinning at 554 RPM


def kraken_tree(liquid=31.5):
    """What LHM 0.9 reports for an NZXT Kraken Z (KrakenV3.cs): liquid temperature, pump and fan controls."""
    def sensor(name, value, sid, kind):
        return {"Text": name, "Value": value, "SensorId": sid, "Type": kind, "Children": []}

    def group(name, *c):
        return {"Text": name, "Value": "", "Children": list(c)}

    return {"Text": "Sensor", "Children": [{"Text": "PC", "Value": "", "Children": [
        {"Text": "Nzxt Kraken Z", "Value": "", "HardwareId": "/hid/nzxt-kraken/0", "ImageURL": "images_icon/fan.png",
         "Children": [
             group("Temperatures", sensor("Liquid", f"{liquid} °C", "/hid/nzxt-kraken/0/temperature/0", "Temperature")),
             group("Controls", sensor("Pump Control", "60,0 %", "/hid/nzxt-kraken/0/control/0", "Control"),
                   sensor("Fans Control", "40,0 %", "/hid/nzxt-kraken/0/control/1", "Control")),
             group("Fans", sensor("Pump", "2100 RPM", "/hid/nzxt-kraken/0/fan/0", "Fan"),
                   sensor("Fans", "800 RPM", "/hid/nzxt-kraken/0/fan/1", "Fan")),
         ]}]}]}


class ChannelTests(unittest.TestCase):
    def test_board_headers_from_a_real_capture(self):
        found = {c["id"]: c for c in lhmfans.channels(sensors.flatten(FIXTURE))}
        self.assertEqual(len(found), 7)                                   # the board's seven headers
        self.assertTrue(all("/gpu" not in sid for sid in found))          # graphics card fans stay with the card
        fan2 = found[BOARD]
        self.assertEqual((fan2["name"], fan2["rpm"], fan2["hw"]), ("Fan #2", 554, "board"))
        self.assertAlmostEqual(fan2["duty"], 25.9)
        self.assertFalse(fan2["pump"])

    def test_cooler_pump_and_fans(self):
        found = {c["name"]: c for c in lhmfans.channels(sensors.flatten(kraken_tree()))}
        self.assertTrue(found["Pump Control"]["pump"])
        self.assertEqual(found["Pump Control"]["rpm"], 2100)
        self.assertFalse(found["Fans Control"]["pump"])
        self.assertEqual(found["Fans Control"]["device"], "Nzxt Kraken Z")

    def test_validate(self):
        self.assertEqual(lhmfans.validate({"mode": "curve", "duty": 5}), {"mode": "curve"})
        self.assertEqual(lhmfans.validate({"mode": "steady", "duty": "70"}), {"mode": "steady", "duty": 70})
        for bad in ({"mode": "steady", "duty": 10}, {"mode": "turbo"}, None, {"mode": "steady", "duty": "x"}):
            with self.assertRaises(ValueError):
                lhmfans.validate(bad)
        with self.assertRaises(ValueError):
            lhmfans.validate_id("not a sensor")


class LiquidTests(unittest.TestCase):
    def test_coolant_temperature_is_read_and_can_drive_the_fans(self):
        s = sensors.summary(sensors.flatten(kraken_tree(31.5)))
        self.assertEqual(s["liquid_temp"], 31.5)
        self.assertEqual(fans.source_temp({**s, "cpu_temp": 60, "gpu_temp": 50}, "liquid"), 31.5)
        self.assertIn("Nzxt Kraken Z pump", [f["name"] for f in s["fans"]])

    def test_without_a_coolant_reading_liquid_falls_back_to_the_hotter_part(self):
        self.assertIsNone(sensors.parse(FIXTURE)["liquid_temp"])
        self.assertEqual(fans.source_temp({"cpu_temp": 60, "gpu_temp": 50, "liquid_temp": None}, "liquid"), 60)

    def test_steady_speed_fails_fast(self):
        self.assertEqual(fans.steady_duty(70, 50, 60), 70)
        self.assertEqual(fans.steady_duty(70, config.SAFETY_TEMP, 60), 100)
        self.assertEqual(fans.steady_duty(40, None, 60), 60)              # no temperatures: at least failsafe
        self.assertEqual(fans.steady_duty(70, 32, 60, hottest=95), 100)  # cool coolant, overheating processor

    def test_an_unplugged_probe_is_no_coolant_reading(self):
        for bogus in (-40.0, 127.0):
            self.assertIsNone(sensors.summary(sensors.flatten(kraken_tree(bogus)))["liquid_temp"])


class Recorder:
    def __init__(self, fail=None):
        self.calls, self.fail = [], fail

    def __call__(self, base, sid, value, headers):
        if self.fail:
            raise OSError(self.fail)
        self.calls.append((sid, value))


class DriverTests(unittest.TestCase):
    def setUp(self):
        self.rec = Recorder()
        self.drv = lhmfans.LhmFans(lambda: "http://127.0.0.1:1", send=self.rec)

    def test_sends_only_changes_then_releases(self):
        self.drv.apply({BOARD: 40})
        self.drv.apply({BOARD: 40})
        self.drv.apply({BOARD: 45})
        self.assertEqual(self.rec.calls, [(BOARD, 40), (BOARD, 45)])
        self.drv.apply({})                                            # no longer ours: back to the BIOS curve
        self.assertEqual(self.rec.calls[-1], (BOARD, None))
        self.assertEqual(self.drv.sent, {})

    def test_resends_after_a_while_and_after_lhm_comes_back(self):
        self.drv.apply({BOARD: 40})
        self.drv.sent[BOARD] = (40, self.drv.sent[BOARD][1] - self.drv.RESEND - 1)
        self.drv.apply({BOARD: 40})
        self.drv.resend_all()
        self.drv.apply({BOARD: 40})
        self.assertEqual(self.rec.calls, [(BOARD, 40)] * 3)

    def test_errors_are_kept_and_retried(self):
        self.rec.fail = "Specified sensor can not be set"
        self.drv.apply({BOARD: 40})
        self.assertEqual(self.drv.errors[BOARD], "Specified sensor can not be set")
        self.rec.fail = None
        self.drv.apply({BOARD: 40})
        self.assertEqual(self.rec.calls, [(BOARD, 40)])
        self.assertNotIn(BOARD, self.drv.errors)


class FakeLhm:
    """LHM's web server, as far as POST /Sensor goes (HttpServer.cs in LHM 0.9.6)."""

    def __init__(self, result="ok", message=None):
        seen = self.seen = []

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):
                url = urllib.parse.urlparse(self.path)
                seen.append((url.path, dict(urllib.parse.parse_qsl(url.query)), self.headers.get("Authorization")))
                body = json.dumps({"result": result, **({"message": message} if message else {})}).encode()
                self.send_response(200)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *a):
                pass

        self.srv = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.url = f"http://127.0.0.1:{self.srv.server_address[1]}"
        threading.Thread(target=self.srv.serve_forever, daemon=True).start()

    def close(self):
        self.srv.shutdown()
        self.srv.server_close()


class HttpTests(unittest.TestCase):
    def test_set_and_release_over_http(self):
        lhm = FakeLhm()
        self.addCleanup(lhm.close)
        auth = {"Authorization": "Basic " + base64.b64encode(b"airglow:pw").decode()}
        lhmfans.http_set(lhm.url, BOARD, 55, auth)
        lhmfans.http_set(lhm.url, BOARD, None, auth)
        self.assertEqual(lhm.seen[0], ("/Sensor", {"action": "Set", "id": BOARD, "value": "55"}, auth["Authorization"]))
        self.assertEqual(lhm.seen[1][1]["value"], "null")

    def test_a_refusal_is_an_error_with_lhms_reason(self):
        lhm = FakeLhm("fail", "System.ArgumentException: Specified sensor '/x' can not be set\n   at ...")
        self.addCleanup(lhm.close)
        with self.assertRaisesRegex(OSError, "can not be set$"):
            lhmfans.http_set(lhm.url, BOARD, 55)

    def test_lhm_not_running(self):
        with self.assertRaises(OSError):
            lhmfans.http_set("http://127.0.0.1:1", BOARD, 55, timeout=0.3)


class ControllerTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.store = config.Store(self.tmp.name)
        self.temps = {"online": True, "cpu_temp": 50.0, "gpu_temp": 40.0}
        self.flat = sensors.flatten(FIXTURE) + sensors.flatten(kraken_tree())
        self.rec = Recorder()
        self.ctrl = Controller(self.store, manager=FakeManager(FakeNzxt()), read_sensors=lambda: dict(self.temps),
                               read_flat=lambda: self.flat, signals=signals, async_writes=False,
                               lhm_fans=lhmfans.LhmFans(lambda: "http://lhm", send=self.rec))

    def sent(self):
        return dict(self.rec.calls)

    def test_nothing_is_driven_until_you_choose(self):
        self.ctrl.tick()
        self.assertEqual(self.rec.calls, [])
        self.assertEqual(len(self.ctrl.fan_channels), 9)
        self.assertTrue(all(c["mode"] is None for c in self.ctrl.fan_channels))

    def test_curve_and_steady_channels(self):
        self.store.set_fan_channel(BOARD, {"mode": "curve"})
        self.store.set_fan_channel("/hid/nzxt-kraken/0/control/0", {"mode": "steady", "duty": 70})
        self.ctrl.tick()
        self.assertEqual(self.sent()[BOARD], self.ctrl.fan_duty)                   # same speed as the hub's fans
        self.assertEqual(self.sent()["/hid/nzxt-kraken/0/control/0"], 70)
        self.assertTrue(self.ctrl.drives_fans)
        self.temps["cpu_temp"] = 95.0                                              # very hot: everything flat out
        for _ in range(2):
            self.ctrl.tick()
        self.assertEqual(self.sent()[BOARD], 100)
        self.assertEqual(self.sent()["/hid/nzxt-kraken/0/control/0"], 100)

    def test_lost_temperatures_raise_a_slow_pump_to_the_failsafe(self):
        self.store.set_fan_channel("/hid/nzxt-kraken/0/control/0", {"mode": "steady", "duty": 40})
        self.ctrl._temp_seen = True
        self.temps = {"online": False, "cpu_temp": None, "gpu_temp": None}
        self.ctrl.tick()
        self.assertEqual(self.sent()["/hid/nzxt-kraken/0/control/0"], self.store.settings()["fan_failsafe_duty"])

    def test_lhm_going_away_sends_nothing_and_resends_when_back(self):
        self.store.set_fan_channel(BOARD, {"mode": "curve"})
        self.ctrl.tick()
        n = len(self.rec.calls)
        flat, self.flat = self.flat, []
        for _ in range(4):
            self.ctrl.tick()
        self.assertEqual(len(self.rec.calls), n)                 # nothing tried while LHM is down
        self.flat = flat
        self.ctrl.tick()
        self.ctrl.tick()
        self.assertGreater(len(self.rec.calls), n)

    def test_leaving_a_channel_alone_again_hands_it_back(self):
        self.store.set_fan_channel(BOARD, {"mode": "curve"})
        self.ctrl.tick()
        self.store.set_fan_channel(BOARD, None)
        self.ctrl.fan_channels_changed()
        self.ctrl.tick()
        self.ctrl.tick()
        self.assertEqual(self.rec.calls[-1], (BOARD, None))

    def test_failsafe_then_quit_hands_fans_back(self):
        self.store.set_fan_channel("/hid/nzxt-kraken/0/control/0", {"mode": "steady", "duty": 80})
        self.store.set_fan_channel(BOARD, {"mode": "curve"})
        self.ctrl.tick()
        self.ctrl.failsafe()
        failsafe = self.store.settings()["fan_failsafe_duty"]
        self.assertEqual(self.sent()[BOARD], max(failsafe, self.ctrl.fan_duty))
        self.assertEqual(self.sent()["/hid/nzxt-kraken/0/control/0"], 80)      # a faster pump keeps its speed
        self.ctrl.release_fans()
        self.assertEqual({sid for sid, v in self.rec.calls[-2:] if v is None}, {BOARD, "/hid/nzxt-kraken/0/control/0"})

    def test_full_speed_when_any_part_is_hot_whatever_the_curve_follows(self):
        # fans on the coolant: it stays at 32 °C while a processor whose pump stopped reaches 95 °C
        self.store.update_settings({"temp_source": "liquid"})
        self.store.set_fan_channel(BOARD, {"mode": "curve"})
        self.store.set_fan_channel("/hid/nzxt-kraken/0/control/0", {"mode": "steady", "duty": 60})
        self.temps.update(liquid_temp=32.0, cpu_temp=95.0)
        self.ctrl.tick()
        self.assertEqual(self.ctrl.fan_temp, 32.0)
        self.assertEqual(self.ctrl.fan_duty, 100)
        self.assertEqual(self.sent()[BOARD], 100)
        self.assertEqual(self.sent()["/hid/nzxt-kraken/0/control/0"], 100)

    def test_settings_are_saved_and_checked(self):
        self.store.set_fan_channel(BOARD, {"mode": "steady", "duty": 55})
        again = config.Store(self.tmp.name)
        self.assertEqual(again.fan_channels(), {BOARD: {"mode": "steady", "duty": 55}})
        self.assertNotIn("fan_channels", again.settings())
        with self.assertRaises(ValueError):
            self.store.set_fan_channel(BOARD, {"mode": "steady", "duty": 5})


if __name__ == "__main__":
    unittest.main()
