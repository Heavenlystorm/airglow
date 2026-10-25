import re
import unittest
from pathlib import Path

from airglow import config

ROOT = Path(__file__).resolve().parent.parent
WEB = ROOT / "web"


class DesignListTests(unittest.TestCase):
    """The design ids are repeated in four places; this keeps them from drifting apart."""

    def test_every_design_is_in_every_place(self):
        css = (WEB / "css" / "designs.css").read_text(encoding="utf-8")
        js = (WEB / "js" / "design.js").read_text(encoding="utf-8")
        window = (ROOT / "airglow" / "window.py").read_text(encoding="utf-8")
        self.assertRegex(css, r"\[data-design\]\s*\{")                 # the Graphite reset previews fall back to
        blocks = set(re.findall(r'\[data-design="([a-z]+)"\]', css))
        self.assertEqual(blocks | {"graphite"}, set(config.DESIGNS))
        self.assertEqual(set(re.findall(r'id: "([a-z]+)"', js)), set(config.DESIGNS))
        for design in config.DESIGNS:
            self.assertRegex(window, rf'"{design}":\s*\("#')

    def test_index_loads_the_cached_design_before_the_stylesheet(self):
        html = (WEB / "index.html").read_text(encoding="utf-8")
        self.assertLess(html.index("js/design-boot.js"), html.index("css/app.css"))
        self.assertLess(html.index("css/app.css"), html.index("css/designs.css"))
        self.assertIn('data-design="graphite"', html)

    def test_the_cached_design_uses_one_key(self):
        for name in ("design.js", "design-boot.js"):
            self.assertIn("airglow.design", (WEB / "js" / name).read_text(encoding="utf-8"))

    def test_default_overview_has_the_cost_tile(self):
        tiles = config.DEFAULT_OVERVIEW["tiles"]
        self.assertTrue(any(t.get("sensors") == ["app/energy_cost"] for t in tiles))
        self.assertEqual(config.validate_overview(config.DEFAULT_OVERVIEW), config.DEFAULT_OVERVIEW)
