#!/usr/bin/env python3
"""Regenerate the progressive-JPEG fixtures next to this script.

Each picture is written twice by Pillow (libjpeg underneath): once baseline,
once progressive, at the same quality and chroma subsampling. libjpeg's
progressive encoder sends exactly the quantised coefficients its baseline
encoder would -- progression reorders them and splits them into bit planes, it
does not change them -- so a correct progressive decoder must produce *the same
pixels* from the progressive file as from the baseline one. That is the test
`tests/jpeg_progressive.rs` makes, and it needs no tolerance.

The `.txt` beside each progressive file is Pillow's own decode of it (width,
height, then one `AARRGGBB` per pixel), for the second, independent check:
agreement with a reference decoder to within inverse-DCT rounding.

Usage
-----

    python gui/imagecodec/tests/data/generate_jpeg.py

Requires Pillow (10.2 or later, for `restart_marker_blocks`).
"""

from __future__ import annotations

import io
import math
import pathlib

from PIL import Image

HERE = pathlib.Path(__file__).parent


def photo(w: int, h: int) -> Image.Image:
    """Something with the frequencies a photograph has.

    Smooth gradients (which the low coefficients carry), a fine diagonal
    texture and sharp-edged disc (which need the high ones, and so exercise
    every refinement pass), and different structure per channel so a channel
    swap or a mis-upsampled chroma plane cannot hide.
    """
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


def write_answer(name: str, path: pathlib.Path) -> None:
    img = Image.open(path)
    img.load()
    rgb = img.convert("RGB")
    w, h = rgb.size
    words = [str(w), str(h)]
    raw = rgb.tobytes()
    for i in range(0, len(raw), 3):
        r, g, b = raw[i], raw[i + 1], raw[i + 2]
        words.append(f"FF{r:02X}{g:02X}{b:02X}")
    (HERE / f"{name}.txt").write_text(" ".join(words) + "\n")


def save(img: Image.Image, name: str, **kw) -> pathlib.Path:
    path = HERE / f"{name}.jpg"
    img.save(path, "JPEG", **kw)
    return path


def main() -> None:
    small = photo(61, 37)
    big = photo(203, 149)
    grey = photo(45, 29).convert("L")

    cases = [
        ("jpeg420", small, dict(quality=85, subsampling=2)),
        ("jpeg444", small, dict(quality=90, subsampling=0)),
        ("jpeg422", small, dict(quality=80, subsampling=1)),
        ("jpeggrey", grey, dict(quality=85)),
        ("jpegbig420", big, dict(quality=75, subsampling=2)),
    ]
    for name, img, kw in cases:
        save(img, f"{name}_baseline", **kw)
        prog = save(img, f"{name}_progressive", progressive=True, **kw)
        write_answer(f"{name}_progressive", prog)

    # Restart markers inside a progressive stream: the end-of-band run and
    # the DC predictors must both start again at each one.
    save(small, "jpeg420r_baseline", quality=85, subsampling=2, restart_marker_blocks=3)
    prog = save(
        small,
        "jpeg420r_progressive",
        quality=85,
        subsampling=2,
        progressive=True,
        restart_marker_blocks=3,
    )
    write_answer("jpeg420r_progressive", prog)


if __name__ == "__main__":
    main()
