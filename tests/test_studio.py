import base64
import hashlib
import json
import os
import tempfile
import threading
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

import numpy as np

from airglow import fxformat, fxgallery, fxlibrary
from airglow.lighting import effects, studio

ROOT = Path(__file__).resolve().parent.parent


def builder(**kw):
    return fxformat.to_html({"name": "Aurora Wave", "layers": [{"type": "gradient"}, {"type": "sparkles", "blend": "add"}], **kw})


CODE = "airglow.effect({ render(ctx, frame) { ctx.canvas.getContext('2d').fillRect(0, 0, 10, 10); } });"


class FormatTests(unittest.TestCase):
    def test_builder_round_trip(self):
        eff = fxformat.validate(builder(author="jane-dev", tags=["ambient"], license="MIT"))
        self.assertEqual((eff["id"], eff["kind"], eff["tags"], eff["license"]), ("jane-dev.aurora-wave", "layers", ["ambient"], "MIT"))
        self.assertEqual(eff["layers"][1]["params"]["density"], 30)          # defaults filled in
        self.assertIsNone(eff["code"])

    def test_exposed_settings_become_controls(self):
        eff = fxformat.validate(fxformat.to_html({"name": "X", "layers": [
            {"type": "wave", "params": {"speed": 70}, "expose": {"speed": "Wave speed", "color": "Colour"}}]}))
        self.assertEqual([(p["id"], p["type"]) for p in eff["params"]], [("l0_speed", "number"), ("l0_color", "color")])
        self.assertEqual(eff["schema"][0]["default"], 70)                     # the control starts where the layer is
        self.assertEqual(eff["schema"][0]["type"], "range")                   # drawn like a built-in effect's slider

    def test_music_layers_ask_for_audio(self):
        eff = fxformat.validate(fxformat.to_html({"name": "Beat", "layers": [{"type": "beat"}]}))
        self.assertEqual(eff["permissions"], ["audio"])

    def test_code_effect(self):
        src = fxformat.to_html({"name": "Dots", "code": CODE + " // </script> in a comment",
                                "params": [{"id": "speed", "label": "Speed", "type": "number", "min": 0, "max": 5, "step": 0.5, "default": 1},
                                           {"id": "mode", "label": "Mode", "type": "select", "options": ["a", "b"], "default": "b"}]})
        eff = fxformat.validate(src)
        self.assertEqual(eff["kind"], "code")
        self.assertIn("<\\/script>", eff["code"])                             # can't end its own script early
        self.assertEqual(eff["schema"][1]["options"], [{"value": "a", "label": "a"}, {"value": "b", "label": "b"}])

    def test_rejects(self):
        ok = builder()
        cases = {
            "no marker": ok.replace('<meta name="airglow-effect" content="1">', ""),
            "newer api": ok.replace('content="1">', 'content="9">', 1),
            "other tags": ok + "<img src=x>",
            "external script": ok.replace("<script type", '<script src="https://x"></script><script type'),
            "two kinds": ok + "<script>1</script>",
            "bad author": ok.replace("<title>", '<meta name="author" content="Not A Login!"><title>'),
            "bad licence": ok.replace("<title>", '<meta name="license" content="Proprietary"><title>'),
            "bad permission": ok.replace("<title>", '<meta name="permissions" content="camera"><title>'),
            "too big": ok + "<!--" + "x" * fxformat.MAX_BYTES + "-->",
            "unknown layer": fxformat.to_html({"name": "X", "layers": [{"type": "solid"}]}).replace('"solid"', '"laser"'),
        }
        for why, text in cases.items():
            with self.subTest(why), self.assertRaises(fxformat.FormatError):
                fxformat.validate(text)

    def test_layer_limits(self):
        with self.assertRaises(fxformat.FormatError):
            fxformat.to_html({"name": "X", "layers": [{"type": "solid"}] * (fxformat.MAX_LAYERS + 1)})
        with self.assertRaises(fxformat.FormatError):
            fxformat.to_html({"name": "X", "layers": [{"type": "solid", "mask": "roof"}]})
        with self.assertRaises(fxformat.FormatError):
            fxformat.to_html({"name": "X", "layers": [{"type": "rainbow", "palette": True}]})

    def test_effect_wide_controls_and_palette(self):
        eff = fxformat.validate(fxformat.to_html({
            "name": "X", "expose": {"palette": "Your colours", "speed": "Pace"},
            "master": {"palette": ["#112233", "#445566"], "speed": 60},
            "layers": [{"type": "gradient", "palette": True}]}))
        self.assertEqual([(p["id"], p["type"]) for p in eff["params"]], [("m_palette", "colors"), ("m_speed", "number")])
        self.assertEqual(eff["master"]["palette"], ["#112233", "#445566"])
        self.assertTrue(eff["layers"][0]["palette"])
        self.assertEqual([s["type"] for s in eff["schema"]], ["colors", "range"])

    def test_layer_ids_keep_control_ids_when_reordered(self):
        src = fxformat.to_html({"name": "X", "layers": [
            {"id": "a1", "type": "wave", "expose": {"speed": "Wave speed"}},
            {"id": "b2", "type": "pulse", "expose": {"speed": "Breath speed"}}]})
        eff = fxformat.validate(src)
        self.assertEqual([p["id"] for p in eff["params"]], ["la1_speed", "lb2_speed"])
        moved = fxformat.to_html({"name": "X", "layers": list(reversed(eff["layers"]))})
        self.assertEqual([p["id"] for p in fxformat.validate(moved)["params"]], ["lb2_speed", "la1_speed"])

    def test_v1_layer_list_still_loads(self):
        src = builder()
        cfg = json.loads(src.split('id="layers">', 1)[1].split("</script>", 1)[0])
        self.assertEqual(cfg["v"], 2)
        legacy = src.split('id="layers">', 1)[0] + 'id="layers">' + json.dumps(cfg["layers"]) + "</script>\n"
        eff = fxformat.validate(legacy)
        self.assertEqual(eff["layers"][0]["type"], "gradient")
        self.assertNotIn("id", eff["layers"][0])

    def test_reacting_layer_asks_for_audio(self):
        eff = fxformat.validate(fxformat.to_html({"name": "X", "layers": [
            {"type": "wave", "react": {"what": "speed", "amount": 50}}]}))
        self.assertEqual(eff["permissions"], ["audio"])
        self.assertEqual(eff["layers"][0]["react"], {"what": "speed", "amount": 50})

    def test_colours_can_be_exposed(self):
        eff = fxformat.validate(fxformat.to_html({"name": "X", "layers": [
            {"type": "gradient", "expose": {"colors": "Colours"}}]}))
        self.assertEqual([(p["id"], p["type"]) for p in eff["params"]], [("l0_colors", "colors")])

    def test_a_palette_control_in_a_code_effect(self):
        eff = fxformat.validate(fxformat.to_html({
            "name": "X", "code": CODE,
            "params": [{"id": "palette", "label": "Colours", "type": "colors",
                        "default": ["#112233", "#445566"]}]}))
        self.assertEqual((eff["params"][0]["type"], eff["schema"][0]["type"]), ("colors", "colors"))
        self.assertEqual(eff["schema"][0]["default"], ["#112233", "#445566"])

    def test_master_defaults_match_the_catalogue(self):
        data = json.loads((ROOT / "web" / "runtime" / "layers.json").read_text(encoding="utf-8"))
        self.assertEqual(data["master"], fxformat.MASTER_DEFAULTS)

    def test_scan_flags_for_reviewers(self):
        self.assertEqual(fxformat.scan(builder()), [])
        flags = fxformat.scan("fetch('x'); new WebSocket('y'); eval('z'); localStorage.x = 1")
        self.assertEqual(len(flags), 4)

    def test_layer_catalogue_is_valid(self):
        types, blends = fxformat.layer_types()
        for tid, t in types.items():
            with self.subTest(tid):
                values = effects.clean_values(t["params"], {})                # every default passes its own spec
                self.assertEqual(set(values), {p["id"] for p in t["params"]})
                self.assertIn(t["group"], ("Colour", "Motion", "Music"))
        self.assertEqual(blends[0], "normal")


class LibraryTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.lib = fxlibrary.Library(self.tmp.name)

    def test_save_keeps_its_file_when_renamed(self):
        e = self.lib.save_mine(builder())
        self.assertEqual(e["fx_id"], "fx:me.aurora-wave")
        renamed = self.lib.save_mine(builder().replace("Aurora Wave", "Northern Lights"), e["fx_id"])
        self.assertEqual(renamed["fx_id"], "fx:me.aurora-wave")
        self.assertEqual(renamed["name"], "Northern Lights")
        second = self.lib.save_mine(builder())
        self.assertEqual(second["fx_id"], "fx:me.aurora-wave-2")
        self.assertEqual(len(self.lib.all()), 2)

    def test_a_file_whose_name_is_not_an_effect_id_is_not_listed(self):
        # it could never be played or deleted (get() rejects the id), so it must not appear as a card at all
        folder = self.lib._dir("mine", create=True)
        (folder / "Aurora.html").write_text(builder(), encoding="utf-8")
        self.assertEqual(self.lib.all(), [])
        self.assertIsNone(self.lib.get("fx:Aurora"))

    def test_install_checks_the_hash_and_keeps_one_rollback(self):
        v1 = builder(author="jane-dev")
        with self.assertRaises(fxformat.FormatError):
            self.lib.install(v1, "0" * 64)
        e = self.lib.install(v1, hashlib.sha256(v1.encode()).hexdigest())
        self.assertEqual((e["fx_id"], e["origin"]), ("fx:jane-dev.aurora-wave", "community"))
        v2 = v1.replace('"density": 30', '"density": 80')
        self.lib.install(v2, hashlib.sha256(v2.encode()).hexdigest())
        self.assertEqual(self.lib.rollback(e["fx_id"])["layers"][1]["params"]["density"], 30)

    def test_same_size_edits_on_one_timestamp_are_seen(self):
        # two writes inside one file-time tick keep (mtime, size), so the library mustn't trust its cache for its own writes
        write = fxlibrary._write

        def same_mtime(path, text):
            write(path, text)
            os.utime(path, ns=(1_700_000_000 * 10**9,) * 2)

        density = lambda e: e["layers"][1]["params"]["density"]
        with mock.patch.object(fxlibrary, "_write", same_mtime):
            e = self.lib.save_mine(builder())
            edited = self.lib.save_mine(builder().replace('"density": 30', '"density": 90'), e["fx_id"])
            self.assertEqual((density(edited), density(self.lib.get(e["fx_id"]))), (90, 90))
            self.assertNotEqual(edited["sha256"], e["sha256"])
            v1 = builder(author="jane-dev")
            v2 = v1.replace('"density": 30', '"density": 80')
            c = self.lib.install(v1, hashlib.sha256(v1.encode()).hexdigest())
            self.assertEqual(density(self.lib.install(v2, hashlib.sha256(v2.encode()).hexdigest())), 80)
            self.assertEqual(density(self.lib.rollback(c["fx_id"])), 30)

    def test_revocation_turns_off_matching_versions(self):
        src = builder(author="jane-dev")
        sha = hashlib.sha256(src.encode()).hexdigest()
        self.lib.install(src, sha)
        self.assertEqual(self.lib.apply_revocations([{"id": "jane-dev.aurora-wave", "sha256": ["f" * 64], "reason": ""}]), [])
        off = self.lib.apply_revocations([{"id": "jane-dev.aurora-wave", "sha256": [sha], "reason": "Strobes"}])
        self.assertEqual(off[0]["reason"], "Strobes")
        self.assertIsNone(self.lib.get("fx:jane-dev.aurora-wave"))

    def test_bad_files_are_skipped_and_ids_checked(self):
        (Path(self.tmp.name) / "mine").mkdir()
        (Path(self.tmp.name) / "mine" / "me.broken.html").write_text("<p>hi</p>", encoding="utf-8")
        self.assertEqual(self.lib.all(), [])
        self.assertIsNone(self.lib.get("fx:../../etc"))
        self.assertIsNone(self.lib.get("solid"))

    def test_saved_studio_scene_survives_a_fresh_start(self):
        # in a new process the settings load before anything imports lighting/studio.py; the scene must survive
        import subprocess
        import sys
        self.lib.save_mine(builder())
        (Path(self.tmp.name) / "state.json").write_text(json.dumps(
            {"scene": {"effect": "fx:me.aurora-wave", "params": {}, "brightness": 80, "on": True}}), encoding="utf-8")
        code = ("import sys; from airglow import fxlibrary; fxlibrary.LIBRARY = fxlibrary.Library(sys.argv[1]); "
                "from airglow.config import Store; print(Store(sys.argv[1]).state['scene']['effect'])")
        r = subprocess.run([sys.executable, "-c", code, self.tmp.name], capture_output=True, text=True, cwd=ROOT, check=True)
        self.assertEqual(r.stdout.strip(), "fx:me.aurora-wave", r.stderr)

    def test_registry_knows_studio_effects(self):
        self.lib.save_mine(builder())
        with mock.patch.object(studio.HOST, "_library", self.lib):
            self.assertIn("fx:me.aurora-wave", effects.EFFECTS)
            self.assertIn("fx:me.aurora-wave", [e["id"] for e in effects.catalog()])
            self.assertEqual(effects.clean_params("fx:me.aurora-wave", {}), {})
            self.assertNotIn("fx:me.nothing", effects.EFFECTS)
        self.assertIn("rainbow", effects.EFFECTS)

    def test_the_engine_looks_at_an_effects_file_at_most_once_a_second(self):
        self.lib.save_mine(builder())
        now = [100.0]
        with (mock.patch.object(studio.HOST, "_library", self.lib), mock.patch.object(studio, "_looked_up", {}),
              mock.patch.object(studio.time, "monotonic", lambda: now[0]),
              mock.patch.object(self.lib, "get", wraps=self.lib.get) as get):
            first = effects.EFFECTS.get("fx:me.aurora-wave")
            for _ in range(10):                                          # every zone, every frame
                self.assertIs(effects.EFFECTS.get("fx:me.aurora-wave"), first)
            self.assertEqual(get.call_count, 1)
            self.assertIsNone(effects.EFFECTS.get("fx:me.nothing"))      # a missing one is remembered too
            self.assertIsNone(effects.EFFECTS.get("fx:me.nothing"))
            self.assertEqual(get.call_count, 2)
            self.lib.save_mine(builder(tags=["edited"]), "fx:me.aurora-wave")                # an edit in Airglow
            get.reset_mock()
            edited = effects.EFFECTS.get("fx:me.aurora-wave")
            self.assertIsNot(edited, first)                              # shows at once
            effects.EFFECTS.get("fx:me.aurora-wave")
            self.assertEqual(get.call_count, 1)
            now[0] += studio.RECHECK_SECONDS                             # a file changed by hand: within a second
            effects.EFFECTS.get("fx:me.aurora-wave")
            self.assertEqual(get.call_count, 2)
            self.lib.set_draft(builder())
            effects.EFFECTS.get(fxlibrary.DRAFT)
            effects.EFFECTS.get(fxlibrary.DRAFT)
            self.assertNotIn(fxlibrary.DRAFT, studio._looked_up)          # the Studio's draft is never cached


class HostTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.lib = fxlibrary.Library(self.tmp.name)
        self.now = 100.0
        self.spawned = []

        def spawn(url):
            proc = mock.Mock(pid=4242)
            proc.poll.return_value = None
            self.spawned.append(url)
            return proc
        self.host = studio.EffectHost(self.lib, spawn=spawn, clock=lambda: self.now)
        self.host.url = "http://127.0.0.1:9999"
        self.eff = studio.JsEffect(self.lib.save_mine(builder()))

    def zone(self, key, n=3):
        return SimpleNamespace(key=key, n=n, spot="front", pos=np.array([[0.1 * i, 0.5] for i in range(n)]))

    def events(self):
        out, self.host._events = self.host._events, []
        return out

    def test_the_host_sends_per_fan_positions(self):
        z = SimpleNamespace(key="hub/1", n=4, spot="front", pos=np.array([[0.1 * i, 0.5] for i in range(4)]),
                            local=np.array([0.0, 0.25, 0.5, 0.75]), fan=np.array([0, 0, 1, 1]))
        pts = studio._points(z)
        self.assertEqual([p[3] for p in pts], [0.0, 0.25, 0.5, 0.75])
        self.assertEqual([p[4] for p in pts], [0, 0, 1, 1])
        self.assertTrue(all(p[5] == "hub/1" for p in pts))

    def test_render_loads_once_and_returns_the_hosts_colours(self):
        z = self.zone("hub/1")
        self.assertEqual(self.host.render(self.eff, z, 0.0, {}).shape, (3, 3))
        self.assertEqual(len(self.spawned), 1)
        self.assertIn("/runtime/host.html#token=", self.spawned[0])
        self.host.tick(0.0)
        ev = [e for e in self.events() if e["type"] != "tick"]
        self.assertEqual([e["type"] for e in ev], ["load"])                  # one load, LED positions included
        self.assertEqual(ev[0]["points"][2][:3], [64.0, 100.0, "front"])     # 0.2 across, halfway down, its area
        self.assertEqual(ev[0]["points"][2][3:], [1.0, 0, "hub/1"])          # the end of its strip, its fan, its zone
        iid = ev[0]["id"]
        colours = bytes([255, 0, 0, 0, 255, 0, 0, 0, 255])
        self.host.frames({"frames": {iid: base64.b64encode(colours).decode()}})
        out = self.host.render(self.eff, z, 0.1, {})
        self.assertEqual(out.tolist(), [[1, 0, 0], [0, 1, 0], [0, 0, 1]])
        self.assertEqual(self.events(), [])                                  # nothing new to send

    def test_an_effect_started_while_the_host_runs_gets_every_led(self):
        # the bug from 1.6.0-beta.1: switching to another Studio effect while the host page was connected sent
        # "load" with no LEDs and the positions separately, and a page still starting the effect dropped them
        first = self.zone("a", 2)
        self.host.render(self.eff, first, 0, {})
        self.host.tick(0.0)
        self.events()
        other = studio.JsEffect(self.lib.save_mine(builder().replace("Aurora Wave", "Ember")))
        for key, n in (("a", 2), ("b", 4), ("c", 1)):
            self.host.render(other, self.zone(key, n), 0.1, {})
        self.assertEqual([e for e in self.events() if e["type"] in ("load", "points")], [])   # nothing half-done
        self.host.tick(0.1)
        loads = [e for e in self.events() if e["type"] == "load"]
        self.assertEqual(len(loads), 1)
        self.assertEqual(len(loads[0]["points"]), 7)
        self.host.render(other, self.zone("d", 3), 0.2, {})                  # a zone added later still follows
        self.assertEqual([len(e["points"]) for e in self.events() if e["type"] == "points"], [10])

    def test_an_edit_reloads_with_every_led(self):
        z = self.zone("a", 5)
        self.host.render(self.eff, z, 0, {})
        self.host.tick(0.0)
        self.events()
        edited = studio.JsEffect(self.lib.save_mine(builder().replace('"density": 30', '"density": 90'), self.eff.fx_id))
        self.host.render(edited, z, 0.1, {})
        self.host.tick(0.1)
        loads = [e for e in self.events() if e["type"] == "load"]
        self.assertEqual((len(loads), len(loads[0]["points"])), (1, 5))

    def test_zones_share_one_instance(self):
        self.host.render(self.eff, self.zone("a", 2), 0, {})
        self.host.render(self.eff, self.zone("b", 4), 0, {})
        self.assertEqual(len(self.host.instances), 1)
        inst = next(iter(self.host.instances.values()))
        self.assertEqual(len(inst.points), 6)

    def test_slider_moves_update_the_same_instance(self):
        z = self.zone("a")
        self.host.render(self.eff, z, 0, {"l0_speed": 10})
        self.host.tick(0.0)
        self.events()
        self.host.render(self.eff, z, 0, {"l0_speed": 20})
        self.assertEqual(len(self.host.instances), 1)
        self.assertEqual([e["type"] for e in self.events()], ["params"])

    def test_problems_turn_the_effect_off_until_retried(self):
        z = self.zone("a")
        self.host.render(self.eff, z, 0, {})
        iid = next(iter(self.host.instances))
        self.host.frames({"problems": {iid: {"reason": "hung"}}})
        self.assertIn("stopped answering", self.host.problems[self.eff.fx_id]["text"])
        self.assertEqual(self.host.render(self.eff, z, 0, {}).tolist(), [[0, 0, 0]] * 3)
        self.host.retry(self.eff.fx_id)
        self.host.render(self.eff, z, 0, {})
        self.assertEqual(len(self.host.instances), 1)

    def test_at_most_four_effects(self):
        for i in range(5):
            e = studio.JsEffect(self.lib.save_mine(builder().replace("Aurora Wave", f"Effect {i}")))
            self.host.render(e, self.zone(f"z{i}"), 0, {})
        self.assertEqual(len(self.host.instances), studio.MAX_INSTANCES)
        self.assertEqual(next(iter(self.host.problems.values()))["reason"], "busy")

    def test_a_busy_notice_does_not_turn_off_the_effect_that_is_live(self):
        # A gallery preview asking for a fifth instance used to mark the *live* effect busy, and a busy problem
        # stopped every zone using that effect from drawing at all.
        z = self.zone("a")
        self.host.render(self.eff, z, 0, {})
        iid = next(iter(self.host.instances))
        self.host.frames({"frames": {iid: base64.b64encode(bytes([200, 100, 50] * 3)).decode()}})
        self.host.problems[self.eff.fx_id] = {"effect": self.eff.fx_id, "reason": "busy", "text": "only 4"}
        colours = self.host.render(self.eff, z, 0, {})
        self.assertGreater(float(colours.max()), 0.5, "a busy notice must not black the live effect")
        self.assertNotIn(self.eff.fx_id, self.host.problems, "it is drawing again, so the notice is stale")

    def test_unused_effects_unload_and_the_host_stops(self):
        self.host.render(self.eff, self.zone("a"), 0, {})
        self.events()
        self.now += studio.UNUSED_SECONDS + 1
        self.host.tick(1.0)
        self.assertEqual(self.host.instances, {})
        self.assertEqual(self.events()[0]["type"], "unload")
        self.now += studio.IDLE_STOP_SECONDS + 1
        with mock.patch.object(self.host, "stop") as stop:
            self.host.tick(2.0)
        stop.assert_called_once()

    def test_a_stuck_host_is_restarted(self):
        z = self.zone("a")
        self.host.render(self.eff, z, 0, {})
        self.now += studio.START_GRACE + studio.FRAME_TIMEOUT + 1
        self.host.render(self.eff, z, 0, {})                                 # the engine keeps drawing it
        with mock.patch.object(self.host, "stop", side_effect=lambda: setattr(self.host, "proc", None)):
            self.host.tick(5.0)
        self.assertEqual(len(self.spawned), 2)

    def test_a_crashed_host_waits_a_moment_before_starting_again(self):
        # Restarting three milliseconds after the old process died hit WebView2's profile lock and fail-fast crashed
        # the new host straight away: three starts in 78 ms, then the effect was turned off.
        self.host.render(self.eff, self.zone("a"), 0, {})
        other = studio.JsEffect(self.lib.save_mine(builder().replace("Aurora Wave", "Ember")))
        self.host.render(other, self.zone("b"), 0, {})
        self.host.tick(0.0)
        self.assertEqual(len(self.spawned), 1)
        self.host.proc.poll.return_value = 3221226091                 # 0xC0000409: a fail-fast crash
        self.host.tick(0.1)
        self.assertEqual(len(self.spawned), 1)                        # not straight away
        self.assertEqual(len(self.host.instances), 2)                 # two effects: neither is blamed for it
        self.now += studio.RESTART_GAP + 0.1
        self.host.tick(0.2)
        self.assertEqual(len(self.spawned), 2)                        # once the old one has gone, a new one

    def test_an_effect_that_crashes_the_host_before_its_first_frame_is_turned_off(self):
        self.host.render(self.eff, self.zone("a"), 0, {})
        self.host.tick(0.0)                                           # loaded, but not one frame came back
        self.host.proc.poll.return_value = 3221226091
        self.host.tick(0.1)
        self.assertEqual(self.host.instances, {})                     # only the effect that was loading is dropped
        self.assertEqual(self.host.problems[self.eff.fx_id]["reason"], "crashed")

    def test_closing_never_starts_another_host(self):
        self.host.render(self.eff, self.zone("a"), 0, {})
        self.host.close()
        self.assertEqual(self.host.proc, None)
        self.host.render(self.eff, self.zone("a"), 1, {})             # the lighting thread can still draw one frame
        self.host.tick(1.0)
        self.assertEqual(len(self.spawned), 1)

    def test_stream_sends_loads_then_ticks(self):
        self.host.render(self.eff, self.zone("a"), 0, {})
        self.host.tick(0.5, None)
        sent, stop = [], threading.Event()

        def send(event, data):
            sent.append(event)
            if event == "tick":
                stop.set()
        self.host.stream(send, stop)
        self.assertEqual(sent[0], "load")
        self.assertIn("tick", sent)

    def test_memory_slow_growth_restarts_fast_growth_turns_off(self):
        z = self.zone("a")
        self.host.render(self.eff, z, 0, {})
        self.host.tick(0.0)
        stop = mock.Mock(side_effect=lambda: setattr(self.host, "proc", None))
        with mock.patch.object(studio, "_tree_memory", return_value=studio.MEMORY_LIMIT + 1),              mock.patch.object(self.host, "stop", stop):
            self.now += studio.RUNAWAY_SECONDS + 600                       # ten minutes later: a fresh host, same effect
            self.host._last_frame = self.now
            self.host.render(self.eff, z, 1, {})
            self.host.tick(1.0)
            self.assertEqual((len(self.spawned), self.host.problems), (2, {}))
            self.assertEqual(len(self.host.instances), 1)
            self.now += 5                                                  # and full again straight away: off
            self.host._last_mem_check = 0
            self.host.render(self.eff, z, 2, {})
            self.host.tick(2.0)
        self.assertEqual(self.host.problems[self.eff.fx_id]["reason"], "memory")

    def test_packed_frames_set_colours_and_keep_the_host_alive(self):
        z = self.zone("hub/1")
        self.host.render(self.eff, z, 0.0, {})
        self.host.tick(0.0)
        iid = next(iter(self.host.instances))
        colours = bytes([255, 0, 0, 0, 255, 0, 0, 0, 255])
        packed = bytes([len(iid)]) + iid.encode() + len(colours).to_bytes(2, "little") + colours
        self.now += 3
        self.host.packed_frames(packed + bytes([2]) + b"i9" + (3).to_bytes(2, "little") + b"\x01")  # a cut-off record
        self.assertEqual(self.host._last_frame, self.now)
        np.testing.assert_allclose(self.host.render(self.eff, z, 0.1, {}), np.eye(3))
        self.now += 3
        self.host.packed_frames(b"")                                        # nothing changed: still alive
        self.assertEqual(self.host._last_frame, self.now)
        np.testing.assert_allclose(self.host.render(self.eff, z, 0.2, {}), np.eye(3))   # the last colours stay

    def test_token_is_checked(self):
        self.assertTrue(self.host.check_token(self.host.token))
        self.assertFalse(self.host.check_token("nope"))
        self.assertFalse(self.host.check_token(None))


class GalleryTests(unittest.TestCase):
    def test_share_url_falls_back_to_paste_when_too_long(self):
        url, filled = fxgallery.share_url(builder(), "jane-dev", "aurora-wave")
        self.assertTrue(filled)
        self.assertTrue(url.startswith("https://github.com/Heavenlystorm/airglow/new/main/effects/jane-dev?filename=aurora-wave.html&value="))
        self.assertTrue(fxgallery.is_share_url(url))
        long_src = builder().replace("Aurora Wave", "A") + "<!--" + "x" * 9000 + "-->"
        url, filled = fxgallery.share_url(long_src, "jane-dev", "a")
        self.assertFalse(filled)
        self.assertLessEqual(len(url), fxgallery.MAX_URL)
        self.assertFalse(fxgallery.is_share_url("https://github.com/someone/else/new/main/effects/x?value=1"))

    def test_index_cleaning(self):
        good = {"id": "jane-dev.aurora", "name": "Aurora", "sha256": "a" * 64, "updated": "2026-10-01"}
        data = fxgallery.clean_index({"effects": [good, {"id": "me.x", "sha256": "a" * 64}, {"id": "Bad Id"},
                                                  {**good, "id": "sam.top", "featured": True, "sha256": "b" * 64}],
                                      "revoked": [{"id": "jane-dev.aurora", "sha256": [], "reason": "x"}, {"id": 5}]})
        self.assertEqual([e["id"] for e in data["effects"]], ["sam.top", "jane-dev.aurora"])     # featured first
        self.assertEqual(data["effects"][1]["path"], "jane-dev/aurora.html")
        self.assertEqual(len(data["revoked"]), 1)

    def test_the_repository_effects_and_index_agree(self):
        index = json.loads((ROOT / "effects" / "index.json").read_text(encoding="utf-8"))
        for e in index["effects"]:
            with self.subTest(e["id"]):
                author, _, slug = e["id"].partition(".")
                f = ROOT / "effects" / author / f"{slug}.html"
                eff = fxformat.validate(f.read_text(encoding="utf-8"))
                self.assertEqual(eff["id"], e["id"])



class StudioApiTests(unittest.TestCase):
    """The /api/studio/ routes against a real local server (as test_api does)."""

    @classmethod
    def setUpClass(cls):
        from airglow import api, config
        from airglow.controller import Controller
        from tests.fakes import FakeLightDevice, FakeManager, FakeNzxt, signals
        cls.tmp = tempfile.TemporaryDirectory()
        cls.store = config.Store(cls.tmp.name)
        cls.lib = fxlibrary.Library(Path(cls.tmp.name) / "effects")
        cls.lib_patch = mock.patch.object(fxlibrary, "LIBRARY", cls.lib)
        cls.lib_patch.start()
        cls.opened = []
        cls.ctrl = Controller(cls.store, manager=FakeManager(FakeNzxt(), FakeLightDevice("strip", "Strip")),
                              signals=lambda: signals(), read_sensors=lambda: {"online": True, "cpu_temp": 40.0},
                              async_writes=False)
        cls.server = api.Server(0, cls.ctrl, cls.store, {"open_url": cls.opened.append})
        cls.port = cls.server.server_address[1]
        threading.Thread(target=cls.server.serve_forever, daemon=True).start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.lib_patch.stop()
        cls.tmp.cleanup()

    def request(self, method, path, body=None):
        import http.client
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=5)
        h = {"Host": f"127.0.0.1:{self.port}"}
        if method == "POST":
            h.update({"Content-Type": "application/json", "X-Airglow": "1"})
        conn.request(method, path, json.dumps(body).encode() if body is not None else None, h)
        r = conn.getresponse()
        data = json.loads(r.read() or b"null")
        conn.close()
        return r.status, data

    def test_build_save_use_and_share(self):
        parts = {"name": "Api Glow", "description": "Soft glow", "tags": ["calm"],
                 "layers": [{"type": "solid"}, {"type": "pulse", "expose": {"speed": "Speed"}}]}
        status, built = self.request("POST", "/api/studio/build", parts)
        self.assertEqual(status, 200)
        self.assertEqual(built["effect"]["params"][0]["id"], "l1_speed")
        status, saved = self.request("POST", "/api/studio/save", parts)
        fx_id = saved["effect"]["fx_id"]
        self.assertEqual((status, fx_id), (200, "fx:me.api-glow"))
        self.assertIn(fx_id, [e["id"] for e in self.request("GET", "/api/effects")[1]])
        status, r = self.request("POST", "/api/studio/use", {"fx_id": fx_id, "values": {"l1_speed": 80}})
        self.assertEqual((r["scene"]["effect"], r["scene"]["params"]), (fx_id, {"l1_speed": 80}))
        self.assertEqual(self.request("POST", "/api/studio/delete", {"fx_id": fx_id})[0], 409)   # it's on the lights
        self.assertEqual(self.request("POST", "/api/studio/share", {"fx_id": fx_id, "author": ""})[0], 400)
        status, sh = self.request("POST", "/api/studio/share", {"fx_id": fx_id, "author": "@Jane-Dev", "license": "MIT"})
        self.assertEqual(status, 200)
        self.assertTrue(sh["filled"])
        self.assertEqual(sh["path"], "effects/jane-dev/api-glow.html")
        self.assertIn('<meta name="author" content="jane-dev">', sh["source"])
        self.assertEqual(self.opened[-1], sh["url"])
        self.assertEqual(self.request("POST", "/api/studio/open-share", {"url": "https://evil.example/"})[0], 400)
        self.request("POST", "/api/scene", {"effect": "solid"})
        self.assertEqual(self.request("POST", "/api/studio/delete", {"fx_id": fx_id})[0], 200)

    def test_master_settings_and_stable_ids_through_the_api(self):
        parts = {"name": "Api Knobs", "description": "Two knobs", "expose": {"palette": "Colours", "speed": "Pace"},
                 "master": {"palette": ["#00e0c8", "#8a3dff"], "speed": 70, "brightness": 90, "react": 40},
                 "layers": [{"id": "aa", "type": "gradient", "palette": True},
                            {"id": "bb", "type": "solid", "expose": {"color": "Colour"}}]}
        status, built = self.request("POST", "/api/studio/build", parts)
        self.assertEqual(status, 200)
        eff = built["effect"]
        self.assertEqual([p["id"] for p in eff["params"]], ["m_palette", "m_speed", "lbb_color"])
        self.assertEqual((eff["master"]["speed"], eff["expose"]["palette"]), (70, "Colours"))
        self.assertTrue(eff["layers"][0]["palette"])
        self.assertEqual([l["id"] for l in eff["layers"]], ["aa", "bb"])
        status, saved = self.request("POST", "/api/studio/save", parts)
        fx_id = saved["effect"]["fx_id"]
        self.assertEqual(status, 200)
        summary = next(e for e in self.request("GET", "/api/studio/effects")[1]["effects"] if e["fx_id"] == fx_id)
        self.assertEqual((summary["master"]["brightness"], summary["expose"]["speed"]), (90, "Pace"))
        self.assertEqual(summary["layers"][1]["id"], "bb")
        status, r = self.request("POST", "/api/studio/use", {"fx_id": fx_id, "values": {"m_speed": 120, "lbb_color": "#ff0000"}})
        self.assertEqual(r["scene"]["params"], {"m_palette": ["#00e0c8", "#8a3dff"], "m_speed": 120, "lbb_color": "#ff0000"})
        self.request("POST", "/api/scene", {"effect": "solid"})
        self.assertEqual(self.request("POST", "/api/studio/delete", {"fx_id": fx_id})[0], 200)

    def test_delete_is_refused_while_a_profile_or_zone_uses_the_effect(self):
        parts = {"name": "Kept Glow", "description": "Soft glow", "layers": [{"type": "solid"}]}
        fx_id = self.request("POST", "/api/studio/save", parts)[1]["effect"]["fx_id"]
        before = self.store.get_presets()
        self.store.set_presets(before + [{"id": "kept", "name": "Kept", "effect": fx_id, "params": {}, "brightness": 50}])
        status, r = self.request("POST", "/api/studio/delete", {"fx_id": fx_id})
        self.assertEqual(status, 409)
        self.assertIn("the profile “Kept”", r["error"])
        self.store.set_presets(before)
        self.ctrl.manager.maintain(True)
        self.store.ensure_zones([d.info() for d in self.ctrl.manager.all().values()])
        key = "strip/z1"
        cfg = self.store.zones()[key]
        self.store.set_zone(key, {**cfg, "override": {"effect": fx_id, "params": {}}}, 16)
        status, r = self.request("POST", "/api/studio/delete", {"fx_id": fx_id})
        self.assertEqual(status, 409)
        self.assertIn("the zone", r["error"])
        self.store.set_zone(key, {**self.store.zones()[key], "override": None}, 16)
        self.assertEqual(self.request("POST", "/api/studio/delete", {"fx_id": fx_id})[0], 200)

    def test_draft_plays_as_a_studio_trial(self):
        src = builder()
        status, _ = self.request("POST", "/api/studio/draft", {"source": src, "lights": True})
        self.assertEqual(status, 200)
        state = self.request("GET", "/api/state")[1]
        self.assertEqual((state["trial"]["kind"], state["scene"]["effect"]), ("studio", "fx:draft"))
        self.request("POST", "/api/studio/draft", {"lights": False})
        self.assertIsNone(self.request("GET", "/api/state")[1]["trial"])

    def test_check_reports_errors(self):
        status, r = self.request("POST", "/api/studio/check", {"source": "<title>x</title>"})
        self.assertEqual(status, 400)
        self.assertIn("isn't an Airglow effect", r["error"])
        status, r = self.request("POST", "/api/studio/check", {"source": fxformat.to_html({"name": "F", "code": "fetch('x'); " + CODE})})
        self.assertEqual(status, 200)
        self.assertTrue(r["flags"])

    def test_host_routes_need_the_token(self):
        self.assertEqual(self.request("POST", "/api/fx/host/frames", {"token": "wrong"})[0], 403)
        self.assertEqual(self.request("GET", "/api/fx/host/stream?token=wrong")[0], 403)

    def test_packed_frames_need_the_token_header(self):
        import http.client

        def post(token, body=b""):
            conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=5)
            conn.request("POST", "/api/fx/host/frames", body, {
                "Host": f"127.0.0.1:{self.port}", "X-Airglow": "1", "Content-Type": "application/octet-stream",
                "X-Airglow-Token": token})
            r = conn.getresponse()
            r.read()
            conn.close()
            return r.status
        self.assertEqual(post("wrong", b"\x02i1\x03\x00abc"), 403)
        with mock.patch.object(studio.HOST, "packed_frames", return_value={"ok": True}) as packed:
            self.assertEqual(post(studio.HOST.token, b"\x02i1\x03\x00abc"), 200)
        packed.assert_called_once_with(b"\x02i1\x03\x00abc")

    def test_effect_frame_is_locked_down(self):
        import http.client
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=5)
        conn.request("GET", "/runtime/frame.html", headers={"Host": f"127.0.0.1:{self.port}"})
        csp = conn.getresponse().getheader("Content-Security-Policy")
        conn.close()
        self.assertIn("connect-src 'none'", csp)
        self.assertNotIn("unsafe-eval", csp)
        self.assertNotIn("unsafe-inline", csp)


if __name__ == "__main__":
    unittest.main()
