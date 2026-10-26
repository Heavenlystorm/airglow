"""Bake the speck-grain tiles once, as PNGs.

1.10.0 built the speck at runtime with an inline SVG feTurbulence filter on a pseudo-element, through
mix-blend-mode. That costs a full-size noise raster and a compositing layer per tile. A 64px RGBA tile is decoded
once and repeated by the compositor as part of the element's own background: no filter, no blend, no layer.

The alpha is baked to the exact strength each design uses, so no `opacity` is needed either.

    python tools/make_grain.py
"""
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "web" / "img"
SIZE = 64


def speckle(seed, rgb, alpha_max, alpha_gamma=2.0):
    """Sparse bright specks: alpha follows noise**gamma, so most pixels are clear and a few are bright."""
    img = Image.new("RGBA", (SIZE, SIZE))
    px = img.load()
    for y in range(SIZE):
        for x in range(SIZE):
            h = (x * 374761393 + y * 668265263 + seed * 2246822519) & 0xFFFFFFFF
            h = ((h ^ (h >> 13)) * 1274126177) & 0xFFFFFFFF
            v = ((h ^ (h >> 16)) & 0xFFFF) / 65535.0
            px[x, y] = (rgb[0], rgb[1], rgb[2], int((v ** alpha_gamma) * alpha_max))
    return img


OUT.mkdir(parents=True, exist_ok=True)
speckle(7, (232, 220, 200), alpha_max=11).save(OUT / "grain-warm.png", optimize=True)
speckle(11, (236, 232, 224), alpha_max=29, alpha_gamma=2.4).save(OUT / "grain-white.png", optimize=True)
for name in ("grain-warm.png", "grain-white.png"):
    print(name, (OUT / name).stat().st_size, "bytes")
