#!/usr/bin/env python3
"""Regenerate the PNG conformance fixtures next to this script.

Why these files exist
---------------------

Every test inside `src/png.rs` decodes a PNG that `src/png.rs`'s own test
helpers built. That is the right way to test a *specific* rule — it is the only
way to hand the decoder a deliberately broken file — but it shares one weakness
with every self-built fixture: if the decoder and the fixture builder read the
specification the same wrong way, both agree and the test passes.

These files are written by Pillow, which is libpng underneath, which is the
reference implementation. Nothing in this repository chose their bytes: the
filter type on each row, the Huffman tables, the chunk layout and the exact
interlace packing are all libpng's decisions, and several of them are choices
our own test helpers never make (our helpers always use filter 0 and stored
DEFLATE blocks).

The `.txt` beside each `.png` is the answer, taken from Pillow's *decoder*
rather than from ours: width, height, then one `AARRGGBB` per pixel in
row-major order. So the test compares two independent implementations of the
whole path, and a disagreement is a real disagreement rather than a restatement.

Two encoders, because one of them cannot write everything
---------------------------------------------------------

Pillow silently ignores `interlace=True` — it accepts the keyword and writes a
progressive-scan flag of 0 — so the Adam7 fixtures are written by **ImageMagick**
instead, which does. That is a happy accident: interlacing is the part of PNG
most likely to be got subtly wrong, and now the files that test it come from a
third independent implementation. The answers beside them are still read back
through Pillow, so no single library both writes a fixture and grades it.

Usage
-----

    python gui/imagecodec/tests/data/generate.py

Requires Pillow, and `magick` (ImageMagick 7) on PATH for the four interlaced
files. Regenerating should produce byte-identical files for a given pair of
versions; if a new version changes the compressed bytes that is fine and
expected — the `.txt` answers are what the test actually asserts, and those are
a property of the picture, not of the encoder.
"""

from __future__ import annotations

import pathlib
import subprocess
import zlib

from PIL import Image

HERE = pathlib.Path(__file__).parent

W, H = 9, 7


def ramp(x: int, y: int) -> tuple[int, int, int, int]:
    """A picture with structure in both axes.

    Flat colour would let a decoder that mixed up rows and columns pass, and a
    pure horizontal ramp would let one that transposed the image pass. This has
    a different gradient per channel per axis, plus an alpha that varies with
    both, so any transposition, off-by-one or channel swap shows up.
    """
    return (
        (x * 28) % 256,
        (y * 36) % 256,
        (x * 13 + y * 31) % 256,
        255 - (x + y) * 9,
    )


def write_expected(name: str, img: Image.Image) -> None:
    """Dump Pillow's own decode of the file we just wrote, as ARGB.

    One correction is applied, and only one. Pillow decodes 16-bit greyscale
    into mode `I;16` correctly — the sample values it reports are exactly the
    ones the file contains — but its `convert("RGBA")` then *clips* anything
    over 255 instead of reducing the depth, so a sample of 4096 comes out as
    white rather than as 16. That is a documented quirk of Pillow's I->L
    conversion and not a statement about PNG, so the reduction is done here
    instead: take the high byte, which is what RFC 2083's own recommendation
    (and libpng's `png_set_strip_16`) does.

    Note what is *not* being asserted by us: the inflate, the scanline filters,
    the byte order of the two-byte samples and the chunk walk are all still
    Pillow's answer. Only the last step, 16 bits to 8, is ours — and it is a
    decision this crate documents rather than a parsing question.
    """
    if img.mode in ("I;16", "I;16B", "I"):
        # Not `point()`: Pillow compiles a point function for mode I into a
        # scale-and-offset pair and cannot express a shift, so the reduction is
        # done over the samples it reports.
        deep = img.load()
        wide = Image.new("L", img.size)
        wide.putdata(
            [deep[x, y] >> 8 for y in range(img.height) for x in range(img.width)]
        )
        img = wide
    rgba = img.convert("RGBA")
    px = rgba.load()
    words = []
    for y in range(rgba.height):
        for x in range(rgba.width):
            r, g, b, a = px[x, y]
            words.append(f"{a:02X}{r:02X}{g:02X}{b:02X}")
    body = "\n".join(
        " ".join(words[i : i + rgba.width]) for i in range(0, len(words), rgba.width)
    )
    (HERE / f"{name}.txt").write_text(
        f"{rgba.width} {rgba.height}\n{body}\n", newline="\n"
    )


def emit(name: str, img: Image.Image, **save_kw) -> None:
    path = HERE / f"{name}.png"
    img.save(path, format="PNG", **save_kw)
    # Read it back through Pillow rather than trusting `img`: for palette and
    # low-bit-depth modes the round trip is where the quantisation happens, and
    # the answer has to be what is *in the file*.
    with Image.open(path) as back:
        back.load()
        write_expected(name, back)


def interlaced(name: str, source: str, color_type: int) -> None:
    """Re-encode `source` as Adam7 with ImageMagick, and grade it with Pillow.

    `color_type` is pinned explicitly because ImageMagick optimises: handed a
    greyscale or an RGBA picture it will happily write a *palette*, and the
    fixture would then test the same code path as the one before it.
    """
    out = HERE / f"{name}.png"
    subprocess.run(
        [
            "magick",
            str(HERE / f"{source}.png"),
            "-interlace",
            "PNG",
            "-define",
            f"png:color-type={color_type}",
            str(out),
        ],
        check=True,
    )
    header = out.read_bytes()[16:29]
    if header[12] != 1:
        raise SystemExit(f"{name}: ImageMagick wrote interlace={header[12]}, wanted 1")
    with Image.open(out) as back:
        back.load()
        write_expected(name, back)


def main() -> None:
    rgba = Image.new("RGBA", (W, H))
    rgba.putdata([ramp(x, y) for y in range(H) for x in range(W)])

    rgb = rgba.convert("RGB")
    gray = rgb.convert("L")

    # Truecolour, 8 bits, no alpha — the commonest wallpaper there is.
    emit("rgb8", rgb)
    # Truecolour with alpha: the straight-alpha contract lives or dies here.
    emit("rgba8", rgba)
    # Greyscale: one sample that must reach all three colour channels.
    emit("gray8", gray)
    # Greyscale with alpha.
    emit("graya8", gray.convert("LA"))
    # Palette at four bits per index — two pixels to a byte, so a decoder that
    # reads the low nibble first gets a picture with its columns swapped.
    emit("palette4", rgb.convert("P", palette=Image.ADAPTIVE, colors=16))
    # Palette at a full byte per index, and with tRNS: per-entry transparency.
    emit(
        "palette8_trns",
        rgb.convert("P", palette=Image.ADAPTIVE, colors=200),
        transparency=3,
    )
    # Maximum compression, so the encoder actually uses dynamic Huffman tables
    # and every scanline filter it thinks is worthwhile.
    emit("rgb8_filtered", rgb, optimize=True, compress_level=9)

    # Pillow will not write 1/2/4-bit greyscale or 16-bit samples directly, so
    # those four are assembled here from raw scanlines. Written by hand, but
    # the *answers* still come from Pillow reading the result back.
    emit_lowdepth()
    emit_gray16()

    # Adam7, via ImageMagick. Same pictures, seven passes: a misplaced pass
    # shows up as a scramble rather than as a subtle shift, and a pass counted
    # when it has no pixels shows up only on images narrower than eight.
    interlaced("gray8_interlaced", "gray8", 0)
    interlaced("rgb8_interlaced", "rgb8", 2)
    interlaced("palette8_interlaced", "palette8_trns", 3)
    interlaced("rgba8_interlaced", "rgba8", 6)


def png_bytes(ihdr: bytes, idat: bytes) -> bytes:
    def chunk(kind: bytes, data: bytes) -> bytes:
        return (
            len(data).to_bytes(4, "big")
            + kind
            + data
            + (zlib.crc32(kind + data) & 0xFFFFFFFF).to_bytes(4, "big")
        )

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", idat)
        + chunk(b"IEND", b"")
    )


def emit_lowdepth() -> None:
    """1-, 2- and 4-bit greyscale, packed most-significant-sample first."""
    for depth in (1, 2, 4):
        maxv = (1 << depth) - 1
        per_byte = 8 // depth
        raw = bytearray()
        for y in range(H):
            raw.append(0)  # filter: None
            row = bytearray()
            acc = 0
            filled = 0
            for x in range(W):
                v = (x + y) % (maxv + 1)
                acc = (acc << depth) | v
                filled += 1
                if filled == per_byte:
                    row.append(acc)
                    acc, filled = 0, 0
            if filled:
                row.append(acc << (depth * (per_byte - filled)))
            raw.extend(row)
        ihdr = (
            W.to_bytes(4, "big")
            + H.to_bytes(4, "big")
            + bytes([depth, 0, 0, 0, 0])
        )
        name = f"gray{depth}"
        (HERE / f"{name}.png").write_bytes(png_bytes(ihdr, zlib.compress(bytes(raw), 9)))
        with Image.open(HERE / f"{name}.png") as back:
            back.load()
            write_expected(name, back)


def emit_gray16() -> None:
    """16-bit greyscale: the high byte is the one that survives to 8 bits."""
    raw = bytearray()
    for y in range(H):
        raw.append(0)
        for x in range(W):
            v = ((x * 4096 + y * 512) % 65536)
            raw.extend(v.to_bytes(2, "big"))
    ihdr = W.to_bytes(4, "big") + H.to_bytes(4, "big") + bytes([16, 0, 0, 0, 0])
    (HERE / "gray16.png").write_bytes(png_bytes(ihdr, zlib.compress(bytes(raw), 9)))
    with Image.open(HERE / "gray16.png") as back:
        back.load()
        write_expected("gray16", back)


if __name__ == "__main__":
    main()
    print(f"wrote fixtures to {HERE}")
