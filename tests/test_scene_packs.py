"""Scene packs install validated, portable effects without changing a controller."""
import copy
import hashlib
import json
import threading
import tempfile
import unittest
from types import SimpleNamespace
from unittest import mock

from airglow import fxformat, fxlibrary, studioapi


def source(name="Evening", **kw):
    return fxformat.to_html({"name": name, "layers": [{"type": "solid"}], **kw})


def pack(text=None, origin="mine"):
    text = text or source()
    return {"format": "airglow-scene-pack", "version": 1, "name": "Evening rig",
            "effects": [{"key": "evening", "source": text, "origin": origin,
                         "sha256": hashlib.sha256(text.encode()).hexdigest()}],
            "areas": [{"area": "front", "effect": "evening", "values": {}}]}


class ScenePackTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.lib = fxlibrary.Library(self.tmp.name)
        self.server = SimpleNamespace(ctrl=SimpleNamespace(base_scene={"effect": "solid"}, trial=None), hooks={})
        patch = mock.patch.object(studioapi, "_lib", return_value=self.lib)
        patch.start(); self.addCleanup(patch.stop)
        gallery = mock.patch("airglow.fxgallery.listing", return_value={"effects": [], "revoked": []})
        self.gallery = gallery.start(); self.addCleanup(gallery.stop)

    def route(self, action, body):
        return studioapi.route(self.server, "POST", "/api/studio/packs/" + action, body, {})

    def test_preview_validates_without_installing_or_touching_controller(self):
        result = self.route("preview", {"pack": pack()})
        self.assertEqual(result["effects"][0]["conflict"], "none")
        self.assertEqual(result["areas"][0]["area"], "front")
        self.assertEqual(self.lib.all(), [])
        self.assertEqual(self.server.ctrl.base_scene, {"effect": "solid"})

    def test_import_requires_explicit_preview_and_installs_portable_area_values(self):
        value = pack()
        with self.assertRaises(studioapi.StudioError):
            self.route("import", {"pack": value})
        p = self.route("preview", {"pack": value})
        result = self.route("import", {"pack": value, "token": p["token"], "confirm": True})
        self.assertEqual(len(self.lib.all()), 1)
        self.assertEqual(result["areas"], [{"area": "front", "fx_id": "fx:me.evening", "values": {}}])
        self.assertEqual(self.server.ctrl.base_scene, {"effect": "solid"})
        self.assertIsNone(self.server.ctrl.trial)

    def test_conflicts_require_choice_and_stale_preview_is_rejected(self):
        self.lib.save_mine(source())
        # A real changed source (metadata is part of its identity).
        value = pack(source(description="Second version"))
        p = self.route("preview", {"pack": value})
        self.assertEqual(p["effects"][0]["conflict"], "different")
        with self.assertRaises(studioapi.StudioError):
            self.route("import", {"pack": value, "token": p["token"], "confirm": True})
        result = self.route("import", {"pack": value, "token": p["token"], "confirm": True, "choices": {"evening": "copy"}})
        self.assertEqual(result["areas"][0]["fx_id"], "fx:me.evening-2")
        with self.assertRaises(studioapi.StudioError):
            self.route("import", {"pack": value, "token": p["token"], "confirm": True, "choices": {"evening": "keep"}})

    def test_rejects_paths_hardware_ids_unknown_controls_invalid_sources_and_limits(self):
        cases = []
        v = pack(); v["areas"][0]["device_id"] = "usb:123"; cases.append(v)
        v = pack(); v["areas"][0]["area"] = "../../mine"; cases.append(v)
        v = pack(); v["areas"][0]["values"] = {"invented": 10}; cases.append(v)
        v = pack(); v["effects"][0]["source"] += "<img src=x>"; cases.append(v)
        v = pack(source() + " " * 32000); cases.append(v)
        v = pack(); v["name"] = "a" * 262144; cases.append(v)
        v = pack(); v["areas"].append(copy.deepcopy(v["areas"][0])); cases.append(v)
        for v in cases:
            with self.subTest(v=str(v)[:90]), self.assertRaises(studioapi.StudioError):
                self.route("preview", {"pack": v})
        self.assertEqual(self.lib.all(), [])

    def test_community_provenance_and_revocation_cannot_be_bypassed_as_mine(self):
        text = source(author="jane", license="MIT")
        value = pack(text, "community")
        with self.assertRaises(studioapi.StudioError): self.route("preview", {"pack": value})
        entry = fxformat.validate(text)
        self.gallery.return_value = {"effects": [{"id": entry["id"], "sha256": entry["sha256"]}], "revoked": []}
        self.assertEqual(self.route("preview", {"pack": value})["effects"][0]["origin"], "community")
        self.gallery.return_value["revoked"] = [{"id": entry["id"], "sha256": [], "reason": "withdrawn"}]
        value["effects"][0]["origin"] = "mine"
        with self.assertRaises(studioapi.StudioError): self.route("preview", {"pack": value})

    def test_export_and_import_on_second_library_preserve_source_credit_and_named_areas(self):
        eff = self.lib.save_mine(source(license="MIT", remix_of="solid"))
        out = self.route("export", {"name": "Portable", "fx_ids": [eff["fx_id"]],
                                   "areas": [{"area": "desk", "fx_id": eff["fx_id"], "values": {}}]})
        self.assertNotIn("path", str(out["pack"]))
        with tempfile.TemporaryDirectory() as second:
            self.lib = fxlibrary.Library(second)
            with mock.patch.object(studioapi, "_lib", return_value=self.lib):
                p = self.route("preview", out)
                imported = self.route("import", {**out, "token": p["token"], "confirm": True})
                self.assertEqual(imported["areas"][0]["area"], "desk")
                self.assertEqual(self.lib.all()[0]["license"], "MIT")
                self.assertEqual(self.lib.all()[0]["remix_of"], "solid")

    def test_revocation_is_rechecked_at_import_after_a_successful_preview(self):
        value = pack(source(author="jane", license="MIT"), "community")
        eff = fxformat.validate(value["effects"][0]["source"])
        self.gallery.return_value = {"effects": [{"id": eff["id"], "sha256": eff["sha256"]}], "revoked": []}
        p = self.route("preview", {"pack": value})
        self.gallery.return_value["revoked"] = [{"id": eff["id"], "sha256": [eff["sha256"]], "reason": "withdrawn"}]
        with self.assertRaises(studioapi.StudioError):
            self.route("import", {"pack": value, "token": p["token"], "confirm": True})
        self.assertEqual(self.lib.all(), [])

    def test_invalid_later_effect_never_installs_an_earlier_effect(self):
        value = pack()
        value["effects"].append({**value["effects"][0], "key": "broken", "source": "<title>invalid</title>"})
        with self.assertRaises(studioapi.StudioError):
            self.route("import", {"pack": value, "confirm": True, "token": "ignored"})
        self.assertEqual(self.lib.all(), [])

    def test_invalid_draft_values_do_not_replace_a_running_draft(self):
        original = source("Working")
        self.lib.set_draft(original)
        changed = source("Invalid preview", expose={"palette": "Colours"})
        with self.assertRaises((studioapi.StudioError, ValueError)):
            studioapi.route(self.server, "POST", "/api/studio/draft", {
                "source": changed, "lights": True, "values": {"m_palette": ["bad"]}}, {})
        self.assertEqual(self.lib.get(fxlibrary.DRAFT)["source"], original)

    def test_copied_community_source_remains_revocable_after_library_reopen(self):
        text = source(author="jane", license="MIT")
        value = pack(text, "community")
        entry = fxformat.validate(text)
        self.gallery.return_value = {"effects": [{"id": entry["id"], "sha256": entry["sha256"]}], "revoked": []}
        original = self.lib.install(text, entry["sha256"])
        preview = self.route("preview", {"pack": value})
        copied = self.route("import", {"pack": value, "token": preview["token"], "confirm": True,
                                        "choices": {"evening": "copy"}})
        reopened = fxlibrary.Library(self.tmp.name)
        off = reopened.apply_revocations([{"id": entry["id"], "sha256": [entry["sha256"]], "reason": "withdrawn"}])
        self.assertEqual({e["fx_id"] for e in off}, {original["fx_id"], copied["installed"][0]["fx_id"]})
        self.assertEqual(reopened.all(), [])

    def test_concurrent_imports_cannot_reuse_the_same_preview(self):
        value = pack()
        preview = self.route("preview", {"pack": value})
        body = {"pack": value, "token": preview["token"], "confirm": True}
        entered, release, second_entered = threading.Event(), threading.Event(), threading.Event()
        install = self.lib.install_pack
        outcomes = []
        def blocked(items):
            if entered.is_set():
                second_entered.set()
            else:
                entered.set()
                self.assertTrue(release.wait(3))
            return install(items)
        def run():
            try: outcomes.append(self.route("import", body))
            except studioapi.StudioError as error: outcomes.append(error.status)
        with mock.patch.object(self.lib, "install_pack", side_effect=blocked):
            first = threading.Thread(target=run); first.start()
            self.assertTrue(entered.wait(3))
            second = threading.Thread(target=run); second.start()
            second_entered.wait(.2)
            release.set()
            first.join(3); second.join(3)
        self.assertFalse(first.is_alive() or second.is_alive())
        self.assertEqual(len(self.lib.all()), 1)
        self.assertEqual(sum(x == 409 for x in outcomes), 1)

    def test_export_limit_includes_human_readable_file_formatting(self):
        from airglow import scenepacks
        ids = []
        for i in range(8):
            text = source(name=f"Evening {i}")
            ids.append(self.lib.save_mine(text + " " * (29000 - len(text.encode())))["fx_id"])
        payload = {"name": "Portable café", "fx_ids": ids}
        result = self.route("export", payload)["pack"]
        compact = len(json.dumps(result, ensure_ascii=False, separators=(",", ":")).encode())
        # Model a boundary that accepts the transport but cannot round-trip the actual downloaded file.
        with mock.patch.object(scenepacks, "MAX_BYTES", compact + 10):
            with self.assertRaises(studioapi.StudioError):
                self.route("export", payload)


if __name__ == "__main__": unittest.main()
