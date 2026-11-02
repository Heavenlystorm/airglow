import importlib.util
import unittest


class ControlStatusTests(unittest.TestCase):
    def make_status(self):
        self.assertIsNotNone(importlib.util.find_spec("airglow.controlstatus"), "runtime status module is required")
        from airglow.controlstatus import ControlStatus
        self.mono, self.wall = 10.0, 1000.0
        return ControlStatus(monotonic=lambda: self.mono, epoch=lambda: self.wall)

    def test_success_age_uses_monotonic_time_and_pending_is_not_success(self):
        status = self.make_status()
        status.observe("d", "Device", kind="lighting", owner="Airglow", online=True)
        status.queued("d")
        row = status.snapshot()["devices"][0]
        self.assertEqual(row["state"], "pending")
        self.assertIsNone(row["last_success_at"])
        status.succeeded("d", duration=0.25)
        self.mono += 3
        self.wall -= 500
        row = status.snapshot()["devices"][0]
        self.assertEqual(row["age_seconds"], 3)
        self.assertEqual(row["last_success_at"], 1000)
        self.assertEqual(row["duration_ms"], 250)

    def test_events_deduplicate_conditions_recover_and_bound_abandoned_identities(self):
        status = self.make_status()
        status.observe("d", "Device", kind="lighting", owner="Device", online=True)
        for _ in range(300):
            status.observe("d", "Device", kind="lighting", owner="Device", online=True)
        self.assertEqual(len(status.events()), 1)
        status.observe("d", "Device", kind="lighting", owner="Airglow", online=True)
        status.failed("d", "unplugged")
        status.failed("d", "unplugged")
        status.succeeded("d")
        events = status.events()
        self.assertTrue(any(e["kind"] == "ownership" for e in events))
        self.assertEqual(len([e for e in events if e["kind"] == "failure"]), 1)
        self.assertTrue(any(e["kind"] == "recovery" for e in events))
        for i in range(400):
            status.observe(str(i), "Device", kind="lighting", owner="Device", online=False)
        self.assertLessEqual(len(status.snapshot()["devices"]), 128)
        self.assertLessEqual(len(status.events()), 200)

    def test_success_ages_become_stale_but_disconnection_remains_explicit(self):
        status = self.make_status()
        status.observe("d", "Device", kind="lighting", owner="Airglow", online=True, stale_after=5)
        status.succeeded("d")
        self.mono += 6
        self.assertEqual(status.snapshot()["devices"][0]["state"], "stale")
        self.assertTrue(any(e["kind"] == "stale" for e in status.events()))
        status.succeeded("d")
        self.assertEqual(status.events()[-1]["kind"], "recovery")
        status.observe("d", "Device", kind="lighting", owner="Airglow", online=False)
        self.assertEqual(status.snapshot()["devices"][0]["state"], "disconnected")

    def test_pending_work_cannot_mask_stale_or_never_completed_output(self):
        status = self.make_status()
        status.observe("d", "Device", stale_after=5)
        status.queued("d")
        self.mono += 6
        self.assertEqual(status.snapshot()["devices"][0]["state"], "stale")
        status.succeeded("d")
        status.queued("d")
        self.mono += 6
        self.assertEqual(status.snapshot()["devices"][0]["state"], "stale")
        self.assertEqual(len([e for e in status.events() if e["kind"] == "stale"]), 2)
