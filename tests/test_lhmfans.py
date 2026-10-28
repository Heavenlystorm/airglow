import base64
import json
import tempfile
import threading
import unittest
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from unittest import mock

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

    def test_errors_are_kept_and_retried_after_a_backoff(self):
        self.rec.fail = "Specified sensor can not be set"
        self.drv.apply({BOARD: 40})
        self.assertEqual(self.drv.errors[BOARD], "Specified sensor can not be set")
        self.rec.fail = None
        self.drv.apply({BOARD: 40})                                   # still backing off: a refusal is not retried
        self.assertEqual(self.rec.calls, [])                          # every two seconds, forever
        self.drv._retry_at[BOARD] = 0.0                               # the backoff has passed
        self.drv.apply({BOARD: 40})
        self.assertEqual(self.rec.calls, [(BOARD, 40)])
        self.assertNotIn(BOARD, self.drv.errors)

    def test_one_slow_channel_does_not_use_up_the_whole_tick(self):
        ids = [f"/lpc/0/control/{i}" for i in range(10)]
        self.drv.apply({sid: 40 for sid in ids})
        self.assertLessEqual(len(self.rec.calls), self.drv.SENDS_PER_TICK)
        self.drv.apply({sid: 40 for sid in ids})                      # the rest follow on the next tick
        self.assertEqual(len({sid for sid, _ in self.rec.calls}), len(ids))

    def test_changing_curve_does_not_starve_later_channels(self):
        ids = [f"/lpc/0/control/{i}" for i in range(10)]
        with mock.patch.object(lhmfans.time, "monotonic", return_value=100):
            self.drv.apply({sid: 30 for sid in ids})
        with mock.patch.object(lhmfans.time, "monotonic", return_value=102):
            self.drv.apply({sid: 35 for sid in ids})
        self.assertEqual({sid for sid, _ in self.rec.calls}, set(ids))
        self.assertEqual([self.drv.sent[sid][0] for sid in ids[6:]], [35] * 4)

    def test_failsafe_reaches_every_owned_channel_in_one_call(self):
        ids = [f"/lpc/0/control/{i}" for i in range(lhmfans.MAX_CHANNELS)]
        for _ in range(6):
            self.drv.apply({sid: 20 for sid in ids})
        self.rec.calls.clear()
        self.assertEqual(self.drv.failsafe(60), len(ids))
        self.assertEqual(dict(self.rec.calls), dict.fromkeys(ids, 60))

    def test_full_speed_bypasses_backoff_after_a_transient_error(self):
        self.drv.apply({BOARD: 20})
        self.rec.fail = "timed out"
        self.drv.apply({BOARD: 25})
        self.rec.fail = None
        self.rec.calls.clear()
        self.drv.apply({BOARD: 100})
        self.assertEqual(self.rec.calls, [(BOARD, 100)])
        self.assertNotIn(BOARD, self.drv.errors)

    def test_full_speed_reaches_all_channels_even_after_normal_budget_is_spent(self):
        normal = {f"/lpc/0/control/{i}": 40 for i in range(6)}
        urgent = {f"/cooler/control/{i}": 100 for i in range(10)}
        self.drv.apply({**normal, **urgent})
        self.assertEqual(dict(self.rec.calls), {**normal, **urgent})

    def test_failsafe_preserves_a_faster_speed(self):
        self.drv.apply({BOARD: 100})
        self.drv.failsafe(60)
        self.assertTrue(all(duty == 100 for _, duty in self.rec.calls))

    def test_a_failed_send_still_hands_the_fan_back(self):
        """Regression (1.6.5): a speed change that failed made Airglow forget the fan, so "Leave alone", quitting
        and the failsafe all skipped it, leaving it at the last speed that got through."""
        self.drv.apply({BOARD: 40})
        self.rec.fail = "LibreHardwareMonitor didn't answer (timed out)"
        self.drv.apply({BOARD: 55})
        self.rec.fail = None
        self.drv.apply({})                                            # switched back to "Leave alone"
        self.assertEqual(self.rec.calls[-1], (BOARD, None))
        self.assertEqual(self.drv.sent, {})

    def test_a_failed_send_is_in_the_failsafe_and_released_on_quit(self):
        self.drv.apply({BOARD: 40})
        self.rec.fail = "timed out"
        self.drv.apply({BOARD: 55})
        self.rec.fail = None
        self.assertEqual(self.drv.failsafe(60), 1)
        self.assertEqual(self.rec.calls[-1], (BOARD, 60))
        self.drv.release_all()
        self.assertEqual(self.rec.calls[-1], (BOARD, None))

    def test_a_failed_hand_back_is_tried_again_after_backoff(self):
        self.drv.apply({BOARD: 40})
        self.rec.fail = "timed out"
        with self.assertLogs("airglow.lhmfans", "WARNING") as logs, \
                mock.patch.object(lhmfans.time, "monotonic", return_value=100):
            self.drv.apply({})
            self.drv.apply({})                                        # backing off, logged once
        self.assertEqual(len(logs.records), 1)
        self.rec.fail = None
        with mock.patch.object(lhmfans.time, "monotonic", return_value=114):
            self.drv.apply({})
        self.assertEqual(self.rec.calls, [(BOARD, 40)])
        with mock.patch.object(lhmfans.time, "monotonic", return_value=115):
            self.drv.apply({})
        self.assertEqual(self.rec.calls[-1], (BOARD, None))
        self.assertEqual((self.drv.sent, self.drv.errors), ({}, {}))

    def test_failed_hand_backs_are_bounded_and_do_not_starve_later_channels(self):
        ids = [f"/lpc/0/control/{i}" for i in range(lhmfans.MAX_CHANNELS)]
        self.drv.sent = {sid: (40, 0) for sid in ids}
        send = mock.Mock(side_effect=OSError("timed out"))
        self.drv._send = send
        with mock.patch.object(lhmfans.time, "monotonic", return_value=100), \
                self.assertLogs("airglow.lhmfans", "WARNING"):
            for _ in range(6):
                before = send.call_count
                self.drv.apply({})
                self.assertLessEqual(send.call_count - before, self.drv.SENDS_PER_TICK)
        self.assertEqual([c.args[1] for c in send.call_args_list], ids)  # each tried once, including the last one
        with mock.patch.object(lhmfans.time, "monotonic", return_value=114):
            self.drv.apply({})
        self.assertEqual(send.call_count, len(ids))
        with mock.patch.object(lhmfans.time, "monotonic", return_value=115):
            self.drv.apply({})
        self.assertEqual(send.call_count, len(ids) + self.drv.SENDS_PER_TICK)

    def test_hand_backs_and_speed_changes_share_one_budget(self):
        released = [f"/lpc/0/control/{i}" for i in range(5)]
        targets = {f"/cooler/control/{i}": 45 for i in range(5)}
        self.drv.sent = {sid: (40, 0) for sid in released}
        with mock.patch.object(lhmfans.time, "monotonic", return_value=100):
            self.drv.apply(targets)
        self.assertEqual(len(self.rec.calls), self.drv.SENDS_PER_TICK)
        with mock.patch.object(lhmfans.time, "monotonic", return_value=102):
            self.drv.apply(targets)
        self.assertEqual(dict(self.rec.calls), {**dict.fromkeys(released), **targets})
        self.assertEqual(set(self.drv.sent), set(targets))

    def test_slow_hand_backs_reach_all_channels_before_retrying_the_first(self):
        ids = [f"/lpc/0/control/{i}" for i in range(lhmfans.MAX_CHANNELS)]
        self.drv.sent = {sid: (40, 0) for sid in ids}
        clock, attempts = [100.0], []

        def timeout(_base, sid, _duty, _headers):
            attempts.append(sid)
            clock[0] += 0.6                       # an actual timeout consumes the backoff of earlier channels
            raise OSError("timed out")

        self.drv._send = timeout
        with mock.patch.object(lhmfans.time, "monotonic", side_effect=lambda: clock[0]), \
                self.assertLogs("airglow.lhmfans", "WARNING"):
            for _ in range(6):
                before = len(attempts)
                self.drv.apply({})
                self.assertLessEqual(len(attempts) - before, self.drv.SENDS_PER_TICK)
                clock[0] += 2.0
        self.assertEqual(attempts[:len(ids)], ids)
        self.assertGreater(len(attempts), len(ids))  # early backoffs did expire; they did not monopolize the budget

    def test_thermal_protection_runs_before_hand_backs(self):
        released = [f"/lpc/0/control/{i}" for i in range(10)]
        self.drv.sent = {sid: (40, 0) for sid in released}
        self.drv.apply({"/cooler/control/0": 100})
        self.assertEqual(self.rec.calls[0], ("/cooler/control/0", 100))
        self.assertEqual(len(self.rec.calls), 1 + self.drv.SENDS_PER_TICK)

    def test_shutdown_hand_back_bypasses_release_backoff(self):
        self.drv.apply({BOARD: 40})
        self.rec.fail = "timed out"
        with self.assertLogs("airglow.lhmfans", "WARNING"):
            self.drv.apply({})
        self.rec.fail = None
        self.drv.release_all()
        self.assertEqual(self.rec.calls[-1], (BOARD, None))
        self.assertEqual(self.drv.sent, {})
        self.assertEqual(self.drv._release_retry_at, {})


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
        self.store.set_fan_channel("/hid/nzxt-kraken/0/control/0", {"mode": "steady", "duty": 20})
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

    def test_startup_grace_never_suppresses_a_hot_parts_pump_override(self):
        pump = "/hid/nzxt-kraken/0/control/0"
        self.store.update_settings({"temp_source": "gpu"})
        self.store.set_fan_channel(pump, {"mode": "steady", "duty": 20})
        self.temps.update(gpu_temp=None)
        self.ctrl.tick()
        self.assertIsNone(self.ctrl.fan_temp)
        self.assertFalse(self.ctrl._temp_seen)
        self.assertEqual(self.sent()[pump], 20)               # quiet grace while the known CPU is cool
        self.temps["cpu_temp"] = 95.0
        self.ctrl.tick()
        self.ctrl.tick()
        self.assertFalse(self.ctrl._temp_seen)               # selected GPU reading is still unavailable
        self.assertEqual(self.ctrl.fan_duty, 100)
        self.assertEqual(self.sent()[pump], 100)

    def test_thermal_protection_retries_a_channel_after_a_transient_failure(self):
        self.store.set_fan_channel(BOARD, {"mode": "curve"})
        self.rec.fail = "timed out"
        self.ctrl.tick()
        self.rec.fail = None
        self.temps["cpu_temp"] = 95.0
        self.ctrl.tick()
        self.ctrl.tick()
        self.assertEqual(self.sent()[BOARD], 100)

    def test_settings_are_saved_and_checked(self):
        self.store.set_fan_channel(BOARD, {"mode": "steady", "duty": 55})
        again = config.Store(self.tmp.name)
        self.assertEqual(again.fan_channels(), {BOARD: {"mode": "steady", "duty": 55}})
        self.assertNotIn("fan_channels", again.settings())
        with self.assertRaises(ValueError):
            self.store.set_fan_channel(BOARD, {"mode": "steady", "duty": 5})


if __name__ == "__main__":
    unittest.main()
