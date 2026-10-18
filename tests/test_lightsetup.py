import tempfile
import unittest

from airglow import config, lightsetup
from airglow.hw import Zone
from airglow.lighting import geometry
from airglow.lighting.engine import Engine
from airglow.plugins.nzxt_hue2 import ACCESSORIES
from tests.fakes import FakeLightDevice, FakeManager


class CatalogTests(unittest.TestCase):
    def setUp(self):
        self.catalog = lightsetup.load_catalog()

    def test_catalog_loads_with_unique_ids(self):
        ids = [p["id"] for p in self.catalog]
        self.assertGreater(len(ids), 20)
        self.assertEqual(len(ids), len(set(ids)))

    def test_nzxt_products_match_what_the_hub_reports(self):
        nzxt = [p for p in self.catalog if p["nzxt_id"] is not None]
        self.assertEqual({p["nzxt_id"] for p in nzxt}, set(ACCESSORIES))
        for p in nzxt:
            acc = ACCESSORIES[p["nzxt_id"]]
            self.assertEqual(p["leds"] * p["fans"], acc["leds"], p["id"])

    def test_every_argb_header_offers_counted_products(self):
        counted = [p for p in self.catalog if "argb" in p["connect"] and p["leds"] is None]
        self.assertEqual({p["kind"] for p in counted}, {"fan", "strip", "ring"})

    def test_bad_products_are_rejected(self):
        good = {"id": "x-fan", "brand": "X", "name": "X fan", "kind": "fan", "connect": ["argb"], "leds": 12}
        lightsetup.validate_item(good)
        for bad in ({"id": "Bad Id"}, {"kind": "lamp"}, {"connect": ["usb"]}, {"leds": 0}, {"leds": True},
                    {"fans": 9}, {"name": ""}):
            with self.assertRaises(ValueError, msg=bad):
                lightsetup.validate_item({**good, **bad})


class LayoutTests(unittest.TestCase):
    def test_layouts_are_valid_for_every_spot_and_kind(self):
        for spot in lightsetup.SPOTS:
            for kind in lightsetup.KINDS:
                for slot in range(3):
                    layout = lightsetup.layout_for(kind, spot, slot, leds=8, fans=3, size_mm=140)
                    geometry.validate_layout(layout, 300)

    def test_fans_line_up_in_the_spot(self):
        front = lightsetup.layout_for("fan", "front", leds=8, fans=3, size_mm=120)
        self.assertEqual((front["shape"], front["fans"], front["leds_per_fan"], front["angle"]), ("fans", 3, 8, 90))
        second = lightsetup.layout_for("fan", "front", 1, leds=8, fans=3)
        self.assertGreater(second["x"], front["x"])                         # next to the first, not on top of it
        big = lightsetup.layout_for("fan", "top", leds=8, fans=1, size_mm=140)
        self.assertGreater(big["size"], front["size"])

    def test_unknown_spot_is_refused(self):
        with self.assertRaises(ValueError):
            lightsetup.layout_for("strip", "roof", leds=10)

    def test_move_layout_spreads_a_devices_zones(self):
        strip = {"shape": "strip", "leds": 1, "length": 0.05, "x": 0, "y": 0, "angle": 0, "reverse": False}
        xs = [lightsetup.move_layout(strip, "gpu", 0, i, 4)["x"] for i in range(4)]
        self.assertEqual(xs, sorted(xs))
        self.assertAlmostEqual(sum(xs) / 4, lightsetup.SPOTS["gpu"]["x"], places=3)


class PlanTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.store = config.Store(self.tmp.name)
        self.dev = FakeLightDevice("asus", "Board", zones=(("argb1", 30), ("argb2", 0)))
        self.dev.zones[0].max_leds = 120
        self.dev.open()
        self.store.ensure_zones([self.dev.info()])
        self.catalog = lightsetup.load_catalog()
        self.zone = self.dev.zones[0]

    def tearDown(self):
        self.tmp.cleanup()

    def plan(self, **body):
        return lightsetup.plan_header(self.store, "asus/argb1", self.zone, body, self.catalog)

    def test_counted_fans_split_their_leds(self):
        cfg = self.plan(component="generic-argb-fans", count=3, leds=48, spot="front")
        L = cfg["layout"]
        self.assertEqual((L["shape"], L["fans"], L["leds_per_fan"]), ("fans", 3, 16))
        self.assertEqual(cfg["name"], "Front fans")
        self.assertEqual(cfg["component"], {"id": "generic-argb-fans", "count": 3, "spot": "front"})
        self.store.set_zone("asus/argb1", cfg, 120)                           # valid as zone settings

    def test_counted_strip(self):
        cfg = self.plan(component="generic-argb-strip", leds=57, spot="top")
        self.assertEqual((cfg["layout"]["shape"], cfg["layout"]["leds"], cfg["name"]), ("strip", 57, "Top strip"))

    def test_known_product_needs_no_count(self):
        self.zone.connector = "nzxt"
        cfg = self.plan(component="nzxt-f360-rgb-core-case-version", spot="front")
        L = cfg["layout"]
        self.assertEqual((L["fans"], L["leds_per_fan"]), (3, 8))
        self.assertEqual(cfg["name"], "Front F360 RGB Core (case version)")

    def test_refusals(self):
        for body in ({"component": "nzxt-f120-rgb-core", "spot": "front"},          # NZXT part on an ARGB header
                     {"component": "generic-argb-strip", "leds": 121, "spot": "top"},  # more than the header drives
                     {"component": "generic-argb-strip", "spot": "top"},               # not counted
                     {"component": "generic-argb-strip", "leds": 20, "spot": "roof"},
                     {"component": "nope", "spot": "top"},
                     {"component": "generic-argb-fans", "leds": 20, "count": 9, "spot": "top"}):
            with self.assertRaises(ValueError, msg=body):
                self.plan(**body)

    def test_nothing_connected_switches_the_zone_off(self):
        cfg = self.plan(off=True)
        self.assertFalse(cfg["enabled"])
        self.assertEqual(cfg["component"]["id"], None)
        self.store.set_zone("asus/argb1", cfg, 120)

    def test_pending_until_set_up(self):
        info = [self.dev.info()]
        self.assertEqual(lightsetup.pending(self.store, info), ["asus/argb1"])   # argb2 has nothing plugged in
        self.store.set_zone("asus/argb1", self.plan(component="generic-argb-strip", leds=20, spot="top"), 120)
        self.assertEqual(lightsetup.pending(self.store, info), [])

    def test_pending_count_counts_a_devices_built_in_lights_once(self):
        gpu = FakeLightDevice("gpu", "GPU", zones=(("z0", 1), ("z1", 1)), kind="openrgb", type_name="Graphics card")
        for z in gpu.zones:
            z.resizable = False
        gpu.open()
        self.store.ensure_zones([self.dev.info(), gpu.info()])
        self.assertEqual(lightsetup.pending_count(self.store, [self.dev.info(), gpu.info()]), 2)

    def test_provisional_and_offline_zones_wait(self):
        self.dev.zones[0].provisional = True
        self.assertEqual(lightsetup.pending(self.store, [self.dev.info()]), [])
        self.dev.zones[0].provisional = False
        self.assertEqual(lightsetup.pending(self.store, [dict(self.dev.info(), online=False)]), [])

    def test_second_thing_in_a_spot_moves_along(self):
        first = self.plan(component="generic-argb-strip", leds=20, spot="top")
        self.store.set_zone("asus/argb1", first, 120)
        dev2 = FakeLightDevice("hub", "Hub", zones=(("ch1", 8),))
        self.store.ensure_zones([self.dev.info(), dev2.info()])
        second = lightsetup.plan_header(self.store, "hub/ch1", dev2.zones[0],
                                        {"component": "generic-argb-strip", "leds": 8, "spot": "top"}, self.catalog)
        self.assertGreater(second["layout"]["y"], first["layout"]["y"])

    def test_fixed_device_moves_together_and_lights_up(self):
        gpu = FakeLightDevice("gpu", "GPU", zones=(("z0", 1), ("z1", 1), ("z2", 1)), kind="openrgb",
                              type_name="Graphics card")
        for z in gpu.zones:
            z.resizable, z.connector = False, None
        self.store.ensure_zones([gpu.info()])
        plans = lightsetup.plan_fixed(self.store, gpu, "gpu")
        self.assertEqual(list(plans), ["gpu/z0", "gpu/z1", "gpu/z2"])
        xs = [p["layout"]["x"] for p in plans.values()]
        self.assertEqual(xs, sorted(xs))
        self.assertTrue(all(p["enabled"] and p["component"]["spot"] == "gpu" for p in plans.values()))


class ProbeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.store = config.Store(self.tmp.name)
        self.dev = FakeLightDevice("d", "Dev", zones=(("z1", 6), ("z2", 0)))
        self.manager = FakeManager(self.dev)
        self.manager.maintain(True)
        self.store.ensure_zones([self.dev.info()])
        self.engine = Engine(self.manager, self.store, async_writes=False)
        self.engine.set_scene({"effect": "solid", "params": {"color": "#ff0000"}, "brightness": 100, "on": True})

    def tearDown(self):
        self.tmp.cleanup()

    def test_count_lights_the_first_leds_over_the_whole_header(self):
        self.engine.probe("d/z1", "count", 4)
        self.engine.tick()
        colors = self.dev.last("z1")
        self.assertEqual(len(colors), self.dev.zones[0].max_leds)            # 12, not the 6 configured
        lit = [c for c in colors if any(c)]
        self.assertEqual(len(lit), 4)
        self.assertEqual(colors[3], (255, 140, 26))                           # the last one is amber
        self.assertTrue(all(c == (0, 0, 0) for c in colors[4:]))

    def test_probe_works_on_a_switched_off_zone_and_goes_dark_after(self):
        self.engine.probe("d/z2", "count", 1)
        self.engine.tick()
        self.assertEqual(len(self.dev.last("z2")), self.dev.zones[1].max_leds)
        self.engine.probe("d/z2", "off")
        self.engine.tick()
        self.assertTrue(all(c == (0, 0, 0) for c in self.dev.last("z2")))     # blanked once, not left lit
        writes = len(self.dev.writes)
        self.engine.tick()
        self.assertEqual(len([w for w in self.dev.writes[writes:] if w[0] == "z2"]), 0)  # zone is off again

    def test_probe_expires_by_itself(self):
        self.engine.probe("d/z1", "blink")
        mode, n, deadline = self.engine._probe["d/z1"]
        self.engine._probe["d/z1"] = (mode, n, deadline - Engine.PROBE_SECONDS - 1)
        self.engine.tick()
        self.assertEqual(len(self.dev.last("z1")), 12)                          # dark across the header
        self.engine.tick()
        self.assertEqual(self.dev.last("z1"), [(255, 0, 0)] * 6)               # back to the effect

    def test_zone_info_says_what_plugs_in(self):
        self.assertEqual(Zone("a", "A", 10, 5).info()["connector"], "argb")
        self.assertIsNone(Zone("b", "B", 1, 1, resizable=False).info()["connector"])


if __name__ == "__main__":
    unittest.main()


class CoolerTests(unittest.TestCase):
    def test_list_loads_with_unique_ids(self):
        from airglow import coolers
        self.assertGreater(len(coolers.known()), 30)

    def test_detects_a_kraken_once(self):
        from airglow import coolers
        hid_list = [{"vendor_id": 0x1E71, "product_id": 0x300E, "interface_number": i} for i in range(3)]
        hid_list.append({"vendor_id": 0x1E71, "product_id": 0x2022})                # a Control Hub isn't a cooler
        self.assertEqual(coolers.detect(hid_list), [{"brand": "NZXT", "name": "Kraken 2023", "lhm": True, "usb": "1E71:300E"}])
        self.assertEqual(coolers.detect([]), [])
