import importlib
import unittest

from airglow import fxformat


def solid(name, color):
    return fxformat.to_html({"name": name, "layers": [{"type": "solid", "params": {"color": color}}]})


class BlendTests(unittest.TestCase):
    def build(self, **changes):
        module = importlib.util.find_spec("airglow.fxblend")
        self.assertIsNotNone(module, "saved-effect blend compiler is available")
        body = {"a": {"source": solid("Red", "#ff0000")},
                "b": {"source": solid("Blue", "#0000ff")}, "amount": 50, "name": "Dusk"}
        body.update(changes)
        return importlib.import_module("airglow.fxblend").build(body)

    def test_self_contained_effect_and_exposed_blend(self):
        effect = fxformat.validate(self.build())
        self.assertEqual(effect["kind"], "code")
        self.assertEqual(effect["params"][0]["id"], "blend")
        self.assertEqual(effect["params"][0]["default"], 50)
        self.assertIn("#ff0000", effect["code"])
        self.assertIn("#0000ff", effect["code"])
        self.assertIn("sha256", effect["code"])

    def test_rejects_invalid_values_and_nested_or_identical_effects(self):
        for value in (None, True, "50", -1, 101, float("nan"), float("inf")):
            with self.subTest(amount=value), self.assertRaises(fxformat.FormatError):
                self.build(amount=value)
        for value in (None, [], {}, {"source": 5}, {"source": "<script>fetch('https://bad')</script>"},
                      {"source": solid("Red", "#ff0000")}, {"source": self.build()}):
            with self.subTest(endpoint=value), self.assertRaises(fxformat.FormatError):
                self.build(b=value)

    def test_permissions_and_attribution_survive_snapshot(self):
        source = fxformat.to_html({"name": "Music", "author": "creator", "license": "MIT",
                                  "permissions": ["audio"], "code": "airglow.effect({point(){return [0,0,0]}});"})
        effect = fxformat.validate(self.build(b={"source": source}))
        self.assertEqual(effect["permissions"], ["audio"])
        self.assertIn('"author":"creator"', effect["code"])
        self.assertIn('"license":"MIT"', effect["code"])

    def test_combined_size_is_bounded(self):
        source = fxformat.to_html({"name": "Large", "code": "/*" + "x" * 30000 + "*/\nairglow.effect({point(){return [0,0,0]}});"})
        with self.assertRaises(fxformat.FormatError):
            self.build(b={"source": source})

    def test_invalid_endpoint_control_does_not_become_infinity_or_code(self):
        source = fxformat.to_html({"name": "Control", "code": "airglow.effect({point(){return [0,0,0]}});",
            "params": [{"id": "speed", "label": "Speed", "type": "number", "min": 0, "max": 100, "step": 1, "default": 50}]})
        for values in ([], {"speed": float("nan")}, {"speed": "Infinity"}, {"speed": "alert(1)"}):
            with self.subTest(values=values), self.assertRaises(fxformat.FormatError):
                self.build(b={"source": source, "values": values})


if __name__ == "__main__":
    unittest.main()
