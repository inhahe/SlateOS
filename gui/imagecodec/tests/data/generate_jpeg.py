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

Scaled decodes
--------------

A thumbnail is decoded at a half, a quarter or an eighth of the picture's size
directly, and libjpeg-turbo's way of doing that is as much a de facto standard
as its full decode. So every fixture also has `<answer>_s2.txt`, `_s4.txt` and
`_s8.txt`: TurboJPEG's own scaled decode (through `simplejpeg`, whose full
decode is Pillow's to the bit), which `tests/jpeg_sampling.rs` holds
`decode_scaled` to. For a progressive pair the script checks that TurboJPEG
decodes both twins to the same pixels at each scale, so again one answer
serves both.

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


def write_pixels(name: str, size: tuple[int, int], pixels) -> None:
    (w, h) = size
    words = [str(w), str(h)]
    for (r, g, b) in pixels:
        words.append(f"FF{r:02X}{g:02X}{b:02X}")
    # `newline="\n"`: the fixtures are LF on every platform, and Windows'
    # text mode would otherwise write CRLF into them.
    with open(HERE / f"{name}.txt", "w", encoding="ascii", newline="\n") as out:
        out.write(" ".join(words) + "\n")


def write_answer(name: str, path: pathlib.Path) -> None:
    size, pixels = decoded(path)
    write_pixels(name, size, pixels)


SCALES = (2, 4, 8)


def scaled(path: pathlib.Path, factor: int) -> tuple[tuple[int, int], list[tuple[int, int, int]]]:
    """TurboJPEG's decode of a file at `1 / factor` of its size."""
    data = path.read_bytes()
    height, width, colorspace, _ = simplejpeg.decode_jpeg_header(data)
    want = (-(-height // factor), -(-width // factor))
    grey = colorspace == "Gray"
    out = simplejpeg.decode_jpeg(
        data,
        colorspace="GRAY" if grey else "RGB",
        min_height=want[0],
        min_width=want[1],
        min_factor=factor,
    )
    assert out.shape[:2] == want, f"{path.name} at 1/{factor}: {out.shape} is not {want}"
    rows = out.reshape(want[0], want[1], -1)
    pixels = [
        (int(p[0]), int(p[0]), int(p[0])) if grey else (int(p[0]), int(p[1]), int(p[2]))
        for row in rows
        for p in row
    ]
    return (want[1], want[0]), pixels


def write_scaled_answers(name: str, path: pathlib.Path, twin: pathlib.Path | None = None) -> None:
    for factor in SCALES:
        size, pixels = scaled(path, factor)
        if twin is not None:
            assert scaled(twin, factor) == (size, pixels), (
                f"{name}: TurboJPEG decodes the twins differently at 1/{factor}"
            )
        write_pixels(f"{name}_s{factor}", size, pixels)


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
    write_scaled_answers(f"{name}_progressive", prog, twin=base)


def turbojpeg(img: Image.Image, name: str, subsampling: str) -> None:
    """A baseline file in a layout only TurboJPEG writes, and its answer."""
    data = simplejpeg.encode_jpeg(
        np.asarray(img), quality=85, colorspace="RGB", colorsubsampling=subsampling
    )
    path = HERE / f"{name}.jpg"
    path.write_bytes(data)
    write_answer(name, path)
    write_scaled_answers(name, path)


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
        path = save(photo(4, 9), name, quality=85, subsampling=subsampling)
        write_answer(name, path)
        write_scaled_answers(name, path)
    # Five is three: the narrowest plane the filter runs over.
    path = save(photo(5, 9), "jpegslim420", quality=85, subsampling=2)
    write_answer("jpegslim420", path)
    write_scaled_answers("jpegslim420", path)


if __name__ == "__main__":
    main()
