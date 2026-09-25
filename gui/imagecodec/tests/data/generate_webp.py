#!/usr/bin/env python3
"""Regenerate the WebP fixtures next to this script.

Every fixture is written by Pillow, which is libwebp underneath, and every
answer is Pillow's own decode of the file: width, height, then `AARRGGBB` per
pixel. Lossless WebP is exact by definition -- it stores the encoder's input,
alpha and the colour under transparent pixels included -- so the tests compare
with no tolerance at all.

The lossless fixtures are chosen to make libwebp's encoder use each of the
format's tools: every transform (predictor, colour, subtract-green, colour
indexing at each of its four pixel-bundling widths), the colour cache, backward
references, meta prefix codes on a larger picture, and both the simple and the
extended (`VP8X`) container. Several encoder efforts (`method`) are used
because they choose differently among those tools.

Usage
-----

    python gui/imagecodec/tests/data/generate_webp.py

Requires Pillow with WebP support.
"""

from __future__ import annotations

import io
import math
import pathlib

from PIL import Image

HERE = pathlib.Path(__file__).parent


def photo(w: int, h: int) -> Image.Image:
    """The same picture `generate_jpeg.py` draws: gradients, texture, a disc."""
    img = Image.new("RGB", (w, h))
    px = img.load()
    for y in range(h):
        for x in range(w):
            r = int(128 + 100 * math.sin(x / 7.0) * math.cos(y / 11.0))
            g = int((x * 255) / max(1, w - 1))
            b = int((y * 255) / max(1, h - 1))
            if (x + 2 * y) % 5 == 0:
                r, g, b = 255 - r, 255 - g, 255 - b
            cx, cy = w * 0.62, h * 0.4
            if (x - cx) ** 2 + (y - cy) ** 2 < (min(w, h) * 0.22) ** 2:
                r, g, b = 250, 40, 60
            px[x, y] = (max(0, min(255, r)), g, b)
    return img


def translucent(w: int, h: int) -> Image.Image:
    """A picture with every kind of alpha: opaque, clear, and in between, and
    colours under the clear parts that only an exact encoder keeps."""
    img = photo(w, h).convert("RGBA")
    px = img.load()
    for y in range(h):
        for x in range(w):
            r, g, b, _ = px[x, y]
            if x < w // 4:
                a = 0
            elif x < w // 2:
                a = (x * 255 // w + y * 7) % 256
            else:
                a = 255
            px[x, y] = (r, g, b, a)
    return img


def few_colours(w: int, h: int, n: int) -> Image.Image:
    """A picture of exactly `n` colours, so colour indexing is worth using, in
    stripes and blocks that give the predictor and the copies something too."""
    palette = [((i * 97) % 256, (i * 57 + 40) % 256, (i * 23 + 90) % 256, 255) for i in range(n)]
    img = Image.new("RGBA", (w, h))
    px = img.load()
    for y in range(h):
        for x in range(w):
            px[x, y] = palette[((x // 3) + (y // 2) * 5 + (x * y) % 3) % n]
    return img


def scattered(w: int, h: int, n: int) -> Image.Image:
    """`n` colours, each pixel's chosen at random (seeded, so the file is the
    same every time)."""
    import random

    rng = random.Random(1305)
    palette = [(rng.randrange(256), rng.randrange(256), rng.randrange(256), 255) for _ in range(n)]
    img = Image.new("RGBA", (w, h))
    img.putdata([palette[rng.randrange(n)] for _ in range(w * h)])
    return img


def write_answer(name: str, data: bytes) -> None:
    im = Image.open(io.BytesIO(data))
    im.load()
    rgba = im.convert("RGBA")
    raw = rgba.tobytes()
    words = [str(rgba.width), str(rgba.height)]
    for k in range(0, len(raw), 4):
        r, g, b, a = raw[k : k + 4]
        words.append(f"{a:02X}{r:02X}{g:02X}{b:02X}")
    with open(HERE / f"{name}.txt", "w", encoding="ascii", newline="\n") as out:
        out.write(" ".join(words) + "\n")


def save(name: str, img: Image.Image, **kw) -> None:
    buf = io.BytesIO()
    img.save(buf, "WEBP", **kw)
    data = buf.getvalue()
    (HERE / f"{name}.webp").write_bytes(data)
    write_answer(name, data)


def main() -> None:
    small = photo(97, 61)
    save("webp_lossless_photo", small, lossless=True, method=6, quality=100)
    save("webp_lossless_fast", small, lossless=True, method=0, quality=0)
    save("webp_lossless_big", photo(203, 149), lossless=True, method=4, quality=75)
    save("webp_lossless_alpha", translucent(61, 37), lossless=True, method=6, exact=True)
    for n, name in [(2, "webp_lossless_2c"), (4, "webp_lossless_4c"), (13, "webp_lossless_13c")]:
        save(name, few_colours(47, 29, n), lossless=True, method=6, exact=True)
    # A hundred colours scattered with no pattern a predictor could use: a
    # palette is the one tool that pays, and with more than sixteen colours
    # its indices are not bundled.
    save("webp_lossless_100c", scattered(47, 29, 100), lossless=True, method=6, exact=True)
    save("webp_lossless_1x1", photo(1, 1), lossless=True)
    save("webp_lossless_column", photo(1, 23), lossless=True, method=6)
    save("webp_lossless_row", photo(23, 1), lossless=True, method=6)
    # Metadata puts the picture in the extended container, behind a VP8X.
    save("webp_lossless_extended", small, lossless=True, method=3, exif=b"Exif\x00\x00MM\x00*\x00\x00\x00\x08\x00\x00")


if __name__ == "__main__":
    main()
