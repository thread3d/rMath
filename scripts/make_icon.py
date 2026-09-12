#!/usr/bin/env python3
"""Draw the rMath app icon (a plotted curve) and build an .icns via iconutil.

Usage: make_icon.py <out.icns> [workdir]
"""

import math
import os
import shutil
import subprocess
import sys

from PIL import Image, ImageDraw

S = 1024  # master size


def draw_master() -> Image.Image:
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    # Rounded background with a subtle vertical gradient.
    bg_top = (26, 58, 110)
    bg_bottom = (12, 28, 58)
    grad = Image.new("RGB", (1, S))
    for y in range(S):
        t = y / (S - 1)
        grad.putpixel(
            (0, y),
            (
                int(bg_top[0] + (bg_bottom[0] - bg_top[0]) * t),
                int(bg_top[1] + (bg_bottom[1] - bg_top[1]) * t),
                int(bg_top[2] + (bg_bottom[2] - bg_top[2]) * t),
            ),
        )
    grad = grad.resize((S, S))
    mask = Image.new("L", (S, S), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, S - 1, S - 1], radius=210, fill=255)
    img.paste(grad, (0, 0), mask)

    # Plot area
    m = 190
    left, top, right, bottom = m, m + 20, S - m, S - m
    d = ImageDraw.Draw(img)

    # Grid
    for i in range(1, 4):
        x = left + (right - left) * i / 4
        d.line([(x, top), (x, bottom)], fill=(255, 255, 255, 40), width=6)
        y = top + (bottom - top) * i / 4
        d.line([(left, y), (right, y)], fill=(255, 255, 255, 40), width=6)

    # Axes
    d.line([(left, bottom), (right, bottom)], fill=(255, 255, 255, 170), width=12)
    d.line([(left, top), (left, bottom)], fill=(255, 255, 255, 170), width=12)

    # Curve: a sine wave plus a parabola accent
    pts = []
    for k in range(0, 201):
        t = k / 200
        x = left + (right - left) * t
        y = bottom - (bottom - top) * (0.5 + 0.42 * math.sin(t * 2 * math.pi * 1.5))
        pts.append((x, y))
    d.line(pts, fill=(255, 209, 102, 255), width=26, joint="curve")

    pts2 = []
    for k in range(0, 101):
        t = k / 100
        x = left + (right - left) * t
        y = bottom - (bottom - top) * (0.30 * (t ** 2))
        pts2.append((x, y))
    d.line(pts2, fill=(120, 200, 255, 235), width=20, joint="curve")

    # Small filled dot at the origin of the parabola
    d.ellipse([left - 16, bottom - 16, left + 16, bottom + 16], fill=(255, 255, 255, 255))
    return img


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: make_icon.py <out.icns>", file=sys.stderr)
        return 2
    out_icns = sys.argv[1]
    work = os.path.dirname(os.path.abspath(out_icns)) or "."
    iconset = os.path.join(work, "rMath.iconset")
    shutil.rmtree(iconset, ignore_errors=True)
    os.makedirs(iconset, exist_ok=True)

    master = draw_master()
    sizes = [16, 32, 64, 128, 256, 512, 1024]
    for size in sizes:
        img = master.resize((size, size), Image.LANCZOS)
        img.save(os.path.join(iconset, f"icon_{size}x{size}.png"))
        if size >= 32:
            half = size // 2
            master.resize((size, size), Image.LANCZOS).save(
                os.path.join(iconset, f"icon_{half}x{half}@2x.png")
            )

    r = subprocess.run(
        ["iconutil", "-c", "icns", iconset, "-o", out_icns],
        capture_output=True,
        text=True,
    )
    if r.returncode != 0:
        print("iconutil failed:", r.stderr, file=sys.stderr)
        return 1
    shutil.rmtree(iconset, ignore_errors=True)
    print("wrote", out_icns)
    return 0


if __name__ == "__main__":
    sys.exit(main())
