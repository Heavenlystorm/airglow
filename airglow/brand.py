"""The Airglow mark: a horizon, the limb of a planet, and the thin band of light above it.

Real airglow is the faint green glow of the upper atmosphere (oxygen at 557.7 nm), which is where the app icon's
green arc comes from. The tray icon is single-colour and takes on the colour of your lights.
"""

import math

from PIL import Image, ImageDraw

AIRGLOW_GREEN = (110, 240, 160)
INK = (236, 235, 230)
TILE = (18, 20, 24)


def _draw_mark(draw, s, ox, oy, scale, body, glow):
    """Draw the mark (designed on a 64-unit grid) at offset (ox, oy) with `scale` pixels per unit."""
    def p(x, y):
        return ox + x * scale, oy + y * scale
    w = 4.5 * scale
    # glow arc: centre (32, 44), radius 25, from 195 to 345 degrees (Pillow measures clockwise from 3 o'clock)
    cx, cy = p(32, 44)
    r = 25 * scale
    ro = r + w / 2                                         # Pillow draws arc strokes inward from the box
    draw.arc([cx - ro, cy - ro, cx + ro, cy + ro], 195, 345, fill=glow, width=round(w))
    for a in (195, 345):                                   # round caps
        ex, ey = cx + r * math.cos(math.radians(a)), cy + r * math.sin(math.radians(a))
        draw.ellipse([ex - w / 2, ey - w / 2, ex + w / 2, ey + w / 2], fill=glow)
    # planet limb: half disc, centre (32, 44), radius 14
    r2 = 14 * scale
    draw.pieslice([cx - r2, cy - r2, cx + r2, cy + r2], 180, 360, fill=body)
    # horizon
    x0, y0 = p(6, 44)
    x1, _ = p(58, 44)
    draw.line([x0, y0, x1, y0], fill=body, width=round(w))
    for x in (x0, x1):
        draw.ellipse([x - w / 2, y0 - w / 2, x + w / 2, y0 + w / 2], fill=body)


def mark(rgb, size=64):
    """Single-colour mark on a transparent background (tray icon), drawn at 4x and downscaled."""
    s = size * 4
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    color = tuple(rgb) + (255,)
    k = s / 57                                    # the mark is wide and short: fill the width, centre it vertically
    _draw_mark(ImageDraw.Draw(img), s, s / 2 - 32 * k, s / 2 - 31.5 * k, k, color, color)
    return img.resize((size, size), Image.LANCZOS)


def app_icon(size=256):
    """Full-colour app icon: the mark on a dark rounded tile, with the arc in airglow green."""
    s = size * 4
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([0, 0, s - 1, s - 1], radius=s * 0.22, fill=TILE + (255,))
    scale = s / 64 * 0.82
    ox, oy = s / 2 - 32 * scale, s / 2 - 31.5 * scale
    _draw_mark(d, s, ox, oy, scale, INK + (255,), AIRGLOW_GREEN + (255,))
    return img.resize((size, size), Image.LANCZOS)


def write_ico(path):
    sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256]
    big = app_icon(256)
    big.save(path, sizes=[(n, n) for n in sizes], append_images=[app_icon(n) for n in sizes[:-1]])


if __name__ == "__main__":
    import sys
    write_ico(sys.argv[1] if len(sys.argv) > 1 else "airglow.ico")
    app_icon(512).save("airglow-preview.png")
    mark((0, 155, 222), 64).save("airglow-tray-preview.png")
