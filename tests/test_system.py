import unittest

from airglow import system


class PowerEventTests(unittest.TestCase):
    def setUp(self):
        self.calls = []
        self.events = system.PowerEvents({
            name: (lambda n=name: self.calls.append(n))
            for name in ("suspend", "resume", "shutdown", "shutdown_cancelled", "lock", "unlock")
        })

    def test_the_two_shutdown_messages_are_handled_once(self):
        # Windows asks first (WM_QUERYENDSESSION) and then says it's going (WM_ENDSESSION): Airglow used to turn the
        # lights off and set the failsafe fan speed twice, a few hundred milliseconds apart.
        self.events._wndproc(None, system.WM_QUERYENDSESSION, 0, 0)
        self.events._wndproc(None, system.WM_ENDSESSION, 1, 0)
        self.assertEqual(self.calls, ["shutdown"])

    def test_a_cancelled_shutdown_can_be_followed_by_a_new_one(self):
        self.events._wndproc(None, system.WM_QUERYENDSESSION, 0, 0)
        self.events._wndproc(None, system.WM_ENDSESSION, 0, 0)
        self.assertEqual(self.calls, ["shutdown", "shutdown_cancelled"])
        self.events._wndproc(None, system.WM_QUERYENDSESSION, 0, 0)
        self.assertEqual(self.calls[-1], "shutdown")

    def test_sleep_and_lock_still_go_through(self):
        self.events._wndproc(None, system.WM_POWERBROADCAST, system.PBT_APMSUSPEND, 0)
        self.events._wndproc(None, system.WM_WTSSESSION_CHANGE, system.WTS_SESSION_LOCK, 0)
        self.events._wndproc(None, system.WM_WTSSESSION_CHANGE, system.WTS_SESSION_UNLOCK, 0)
        self.events._wndproc(None, system.WM_POWERBROADCAST, system.PBT_APMRESUMEAUTOMATIC, 0)
        self.assertEqual(self.calls, ["suspend", "lock", "unlock", "resume"])
