#!/usr/bin/env python3
"""Regenerate the JPEG fixtures next to this script.

Progressive pairs
-----------------

Each picture is written twice by Pillow (libjpeg underneath): once baseline,
once progressive, at the same quality and chroma subsampling. libjpeg's
progressive encoder sends exactly the quantised coefficients its baseline
encoder would -- progression reorders them and splits them into bit planes, it
does not change them -- so a correct progressive decoder must produce *the same
pixels* from the progressive file as from the baseline one. That is the test
`tests/jpeg_progressive.rs` makes, and it needs no tolerance.

The `.txt` beside each progressive file is Pillow's own decode of it (width,
height, then one `AARRGGBB` per pixel), for the second, independent check:
agreement with a reference decoder to within inverse-DCT rounding. It is the
answer for the baseline twin as well, which this script checks rather than
assumes: libjpeg decodes the two to the same pixels.

Chroma layouts
--------------

`tests/jpeg_sampling.rs` checks every way a file can lay out its colour
against the same reference, because bringing subsampled colour back up to
full resolution is the decoder's choice and libjpeg's is the one every other
decoder is measured by. Pillow writes 4:4:4, 4:2:2 and 4:2:0 only, so 4:4:0
(colour halved down only) and 4:1:1 (quartered across) are written by
libjpeg-turbo's own TurboJPEG API, through `simplejpeg`. Those are baseline
only -- TurboJPEG writes no progressive files -- and their answers are still
Pillow's decode, like every other fixture's.

The narrow fixtures are the edge of libjpeg's filter: a colour plane two
samples wide is repeated rather than interpolated (`downsampled_width > 2` in
`jinit_upsampler`), and three is the narrowest it filters.

Usage
-----

    python gui/imagecodec/tests/data/generate_jpeg.py

Requires Pillow (10.2 or later, for `restart_marker_blocks`), and `simplejpeg`
with NumPy for the TurboJPEG-written layouts.
"""

from __future__ import annotations

import math
import pathlib

import numpy as np
import simplejpeg
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


def decoded(path: pathlib.Path) -> tuple[tuple[int, int], list[tuple[int, int, int]]]:
    """Pillow's decode of a file: its size and its pixels as RGB."""
    img = Image.open(path)
    img.load()
    rgb = img.convert("RGB")
    raw = rgb.tobytes()
    return rgb.size, [tuple(raw[i : i + 3]) for i in range(0, len(raw), 3)]


def write_answer(name: str, path: pathlib.Path) -> None:
    (w, h), pixels = decoded(path)
    words = [str(w), str(h)]
    for (r, g, b) in pixels:
        words.append(f"FF{r:02X}{g:02X}{b:02X}")
    # `newline="\n"`: the fixtures are LF on every platform, and Windows'
    # text mode would otherwise write CRLF into them.
    with open(HERE / f"{name}.txt", "w", encoding="ascii", newline="\n") as out:
        out.write(" ".join(words) + "\n")


def save(img: Image.Image, name: str, **kw) -> pathlib.Path:
    path = HERE / f"{name}.jpg"
    img.save(path, "JPEG", **kw)
    return path


def pair(img: Image.Image, name: str, **kw) -> None:
    """A baseline and progressive twin, and the answer for both."""
    base = save(img, f"{name}_baseline", **kw)
    prog = save(img, f"{name}_progressive", progressive=True, **kw)
    assert decoded(base) == decoded(prog), f"{name}: libjpeg decodes the twins differently"
    write_answer(f"{name}_progressive", prog)


def turbojpeg(img: Image.Image, name: str, subsampling: str) -> None:
    """A baseline file in a layout only TurboJPEG writes, and its answer."""
    data = simplejpeg.encode_jpeg(
        np.asarray(img), quality=85, colorspace="RGB", colorsubsampling=subsampling
    )
    path = HERE / f"{name}.jpg"
    path.write_bytes(data)
    write_answer(name, path)


def main() -> None:
    small = photo(61, 37)
    big = photo(203, 149)
    grey = photo(45, 29).convert("L")

    pair(small, "jpeg420", quality=85, subsampling=2)
    pair(small, "jpeg444", quality=90, subsampling=0)
    pair(small, "jpeg422", quality=80, subsampling=1)
    pair(grey, "jpeggrey", quality=85)
    pair(big, "jpegbig420", quality=75, subsampling=2)
    # Restart markers inside a progressive stream: the end-of-band run and
    # the DC predictors must both start again at each one.
    pair(small, "jpeg420r", quality=85, subsampling=2, restart_marker_blocks=3)

    turbojpeg(small, "jpeg440", "440")
    turbojpeg(small, "jpeg411", "411")

    # Four pixels across is two colour samples: repeated, not filtered.
    for name, subsampling in [("jpegnarrow420", 2), ("jpegnarrow422", 1)]:
        write_answer(name, save(photo(4, 9), name, quality=85, subsampling=subsampling))
    # Five is three: the narrowest plane the filter runs over.
    write_answer("jpegslim420", save(photo(5, 9), "jpegslim420", quality=85, subsampling=2))


if __name__ == "__main__":
    main()
