#!/usr/bin/env python3
"""Regenerate the ICO and CUR fixtures next to this script, and their answers.

Chrome decodes icons with its older C++ code (`ICOImageDecoder` and
`BMPImageReader` in its icon mode), which cannot be built on its own as the
BMP generator builds Chrome's BMP decoder. So the answers come two ways:

* **Icons Pillow writes** -- PNG entries, and BMP entries with AND masks --
  are answered by Pillow's decode, which follows the same rules as Chrome for
  such files.
* **Icons written by hand**, for Chrome's own rules, are answered by those
  rules applied to the pixels, palette and mask each was built from -- not by
  a decoder -- and the generator checks that Pillow really does differ from
  the answer where the rule is Chrome's and not Pillow's: the best of two
  entries the same size is the deeper one (Pillow takes the shallower); a
  32-bit image whose alpha is all zero is opaque, under its AND mask
  (Pillow shows it transparent); an AND mask found where the pixels end, not
  where the entry's stated size ends; a 32-bit image whose alpha mask is
  zero is opaque (Pillow takes its fourth byte as alpha all the same).

Answers are text, as for BMP: width, height, then `AARRGGBB` per pixel -- or
`REFUSED` and why.

Usage
-----

    python gui/imagecodec/tests/data/generate_ico.py
"""

from __future__ import annotations

import io
import pathlib
import struct

from PIL import Image

import generate_bmp as bmp

HERE = pathlib.Path(__file__).parent

Pixel = tuple[int, int, int, int]


def ico(images: list[tuple[int, int, int, int, bytes]], cursor: bool = False, offsets: list[int] | None = None) -> bytes:
    """An icon file: each image as (width, height, colour count, bit depth or
    hot spot y, bytes), laid out after the directory in order."""
    head = struct.pack("<HHH", 0, 2 if cursor else 1, len(images))
    at = 6 + 16 * len(images)
    table, body = b"", b""
    for i, (w, h, colours, bits, data) in enumerate(images):
        offset = offsets[i] if offsets else at + len(body)
        table += struct.pack("<BBBBHHII", w % 256, h % 256, colours, 0, 1, bits, len(data), offset)
        body += data
    return head + table + body


def and_mask(mask: list[list[int]]) -> bytes:
    """The AND mask's rows, one bit a pixel, bottom row first."""
    return bmp.rows([bmp.packed(row, 1) for row in mask])


def bmp_entry(w: int, h: int, bits: int, pixel_rows: list[bytes], mask: list[list[int]] | None,
              palette: bytes = b"", size: int = 40, compression: int = 0, masks=(0, 0, 0, 0),
              top_down: bool = False) -> bytes:
    """A BMP as an icon holds it: no file header, the height doubled, the
    colour rows, then the AND mask."""
    header = bmp.dib(size, w, (-2 if top_down else 2) * h, bits, compression, masks=masks)
    xor = bmp.rows(pixel_rows, top_down=top_down)
    return header + palette + xor + (and_mask(mask) if mask is not None else b"")


def answer_text(w: int, h: int, pixels: list[Pixel]) -> str:
    return " ".join([str(w), str(h)] + [f"{a:02X}{r:02X}{g:02X}{b:02X}" for r, g, b, a in pixels]) + "\n"


def pillow_answer(data: bytes) -> list[Pixel] | None:
    try:
        img = Image.open(io.BytesIO(data))
        img.load()
        return list(img.convert("RGBA").get_flattened_data())
    except Exception:
        return None


def masked(pixels: list[list[Pixel]], mask: list[list[int]]) -> list[Pixel]:
    """Chrome's rule without alpha: opaque, and clear where the mask says."""
    return [(0, 0, 0, 0) if m else (r, g, b, 255) for row, mrow in zip(pixels, mask) for (r, g, b, _), m in zip(row, mrow)]


def picture(w: int, h: int) -> list[list[Pixel]]:
    return [[(r, g, b, 255) for r, g, b in row] for row in bmp.photo(w, h)]


def ring(w: int, h: int) -> list[list[int]]:
    """A mask that clears the corners and a hole in the middle."""
    return [[1 if (x + y < 3) or (x - w // 2) ** 2 + (y - h // 2) ** 2 < 4 else 0 for x in range(w)] for y in range(h)]


def bgra_rows(pixels: list[list[Pixel]]) -> list[bytes]:
    return [b"".join(bytes([b, g, r, a]) for r, g, b, a in row) for row in pixels]


def fixtures() -> tuple[dict[str, bytes], dict[str, str], set[str]]:
    """Every fixture, the answers made by rule, and which of those Pillow must
    get wrong."""
    files: dict[str, bytes] = {}
    answers: dict[str, str] = {}
    pillow_differs: set[str] = set()
    w, h = 16, 16
    pic = picture(w, h)
    mask = ring(w, h)

    # By Pillow: PNG entries at several sizes; BMP entries with masks.
    rgba = Image.new("RGBA", (48, 48))
    rgba.putdata([(r, g, b, (x * 11 + y * 7) % 256) for y, row in enumerate(bmp.photo(48, 48)) for x, (r, g, b) in enumerate(row)])
    for name, kw in [
        ("ico_pillow_png", dict(sizes=[(16, 16), (32, 32), (48, 48)])),
        ("ico_pillow_bmp", dict(sizes=[(16, 16), (32, 32)], bitmap_format="bmp")),
    ]:
        buf = io.BytesIO()
        rgba.save(buf, "ICO", **kw)
        files[name] = buf.getvalue()
    buf = io.BytesIO()
    rgba.convert("P").save(buf, "ICO", sizes=[(32, 32)], bitmap_format="bmp")
    files["ico_pillow_bmp_palette"] = buf.getvalue()

    # By hand, each rule.
    def keep(name: str, data: bytes, pixels: list[Pixel] | None, differs: bool = False) -> None:
        files[name] = data
        answers[name] = answer_text(w, h, pixels) if pixels is not None else "REFUSED\n"
        if differs:
            pillow_differs.add(name)

    colours8 = bmp.colours(256, 5)
    idx = [[(x * 7 + y * 3) % 256 for x in range(w)] for y in range(h)]
    pal_pixels = [[(*colours8[i], 255) for i in row] for row in idx]
    keep("ico_bmp_8", ico([(w, h, 0, 8, bmp_entry(w, h, 8, [bytes(r) for r in idx], mask, bmp.palette(colours8)))]),
         masked(pal_pixels, mask))
    idx4 = [[i % 16 for i in row] for row in idx]
    keep("ico_bmp_4", ico([(w, h, 16, 4, bmp_entry(w, h, 4, [bmp.packed(r, 4) for r in idx4], mask, bmp.palette(colours8[:16])))]),
         masked([[(*colours8[i], 255) for i in row] for row in idx4], mask))
    idx1 = [[i % 2 for i in row] for row in idx]
    keep("ico_bmp_1", ico([(w, h, 2, 1, bmp_entry(w, h, 1, [bmp.packed(r, 1) for r in idx1], mask, bmp.palette(colours8[:2])))]),
         masked([[(*colours8[i], 255) for i in row] for row in idx1], mask))
    keep("ico_bmp_24", ico([(w, h, 0, 24, bmp_entry(w, h, 24, [bmp.bgr([p[:3] for p in r]) for r in pic], mask))]),
         masked(pic, mask))
    # 16-bit, 5-6-5 bit fields after the header.
    words = [[((x * 2) << 11) | ((y * 4) << 5) | ((x + y) % 32) for x in range(w)] for y in range(h)]
    lut5 = [round(v * 255 / 31) for v in range(32)]
    lut6 = [round(v * 255 / 63) for v in range(64)]
    p565 = [[(lut5[(v >> 11) & 31], lut6[(v >> 5) & 63], lut5[v & 31], 255) for v in row] for row in words]
    keep("ico_bmp_565", ico([(w, h, 0, 16, bmp_entry(w, h, 16, [bmp.le16(r) for r in words], mask,
                                                     bmp.le32([0xF800, 0x07E0, 0x001F]), compression=3))]),
         masked(p565, mask))

    # 32-bit: real alpha, so the mask is not read -- even a mask of all ones.
    alpha = [[(r, g, b, (x * 16 + y) % 256 or 1) for x, (r, g, b, _) in enumerate(row)] for y, row in enumerate(pic)]
    keep("ico_bmp_32_alpha", ico([(w, h, 0, 32, bmp_entry(w, h, 32, bgra_rows(alpha), [[1] * w] * h))]),
         [p for row in alpha for p in row])
    # 32-bit, alpha all zero: opaque, under the mask. Pillow shows it clear.
    zero = [[(r, g, b, 0) for r, g, b, _ in row] for row in pic]
    keep("ico_bmp_32_zero_alpha", ico([(w, h, 0, 32, bmp_entry(w, h, 32, bgra_rows(zero), mask))]),
         masked(pic, mask), differs=True)
    # 32-bit, alpha zero for the first rows decoded (the bottom ones), then
    # not: everything decoded before the first non-zero alpha is cleared to
    # transparent black; later zero-alpha pixels keep their colour.
    late = [[(r, g, b, 0 if y >= 12 else 200) for r, g, b, _ in row] for y, row in enumerate(pic)]
    first_nonzero_row = 11  # the bottom-up decode reaches row 11 after rows 15..12
    expected = [
        (0, 0, 0, 0) if y > first_nonzero_row else p
        for y, row in enumerate(late) for p in row
    ]
    keep("ico_bmp_32_alpha_late", ico([(w, h, 0, 32, bmp_entry(w, h, 32, bgra_rows(late), None))]), expected, differs=True)
    # 32-bit bit fields, V3 header with an alpha mask.
    keep("ico_bmp_32_bitfields_alpha", ico([(w, h, 0, 32, bmp_entry(
        w, h, 32, bgra_rows(alpha), None, size=56, compression=3, masks=(0xFF0000, 0xFF00, 0xFF, 0xFF000000)))]),
         [p for row in alpha for p in row])
    # 32-bit bit fields with no alpha mask: opaque, masked.
    keep("ico_bmp_32_bitfields_no_alpha", ico([(w, h, 0, 32, bmp_entry(
        w, h, 32, bgra_rows(alpha), mask, size=56, compression=3, masks=(0xFF0000, 0xFF00, 0xFF, 0)))]),
         masked(pic, mask), differs=True)

    # Choosing: two 16x16 entries, 8 and 32 bits; and a larger 4-bit one
    # that wins over both.
    e8 = bmp_entry(w, h, 8, [bytes(r) for r in idx], mask, bmp.palette(colours8))
    e32 = bmp_entry(w, h, 32, bgra_rows(alpha), [[0] * w] * h)
    keep("ico_best_is_deepest", ico([(w, h, 0, 8, e8), (w, h, 0, 32, e32)]), [p for row in alpha for p in row], differs=True)
    # The mask is read where the pixels end. This entry's stated size runs 64
    # bytes further, which Pillow takes the mask from the end of.
    keep("ico_mask_after_pixels", ico([(w, h, 0, 8, e8 + bytes([0xFF]) * 64)]), masked(pal_pixels, mask), differs=True)
    # A cursor: its hot spot where an icon keeps its depth, the depth from
    # its colour count.
    keep("ico_cursor", ico([(w, h, 0, 5, e8)], cursor=True), masked(pal_pixels, mask))

    # Run-length encoded, 8-bit: skipped pixels stay clear, then the mask.
    runs = bytes([8, 1, 4, 2, 0, 0] + [0, 2, 3, 2] + [5, 3, 0, 0] * 1 + [0, 1])
    rle = bmp.dib(40, w, 2 * h, 8, 1, 4) + bmp.palette(colours8[:4]) + runs + and_mask(mask)
    rle_pixels = [[(0, 0, 0, 0)] * w for _ in range(h)]
    for x in range(8):
        rle_pixels[15][x] = (*colours8[1], 255)
    for x in range(8, 12):
        rle_pixels[15][x] = (*colours8[2], 255)
    for x in range(3, 8):
        rle_pixels[12][x] = (*colours8[3], 255)
    rle_expected = [(0, 0, 0, 0) if mask[y][x] else rle_pixels[y][x] for y in range(h) for x in range(w)]
    keep("ico_bmp_rle8", ico([(w, h, 4, 8, rle)]), rle_expected)

    # Refused.
    png16 = io.BytesIO()
    Image.new("RGBA", (16, 16), (1, 2, 3, 4)).save(png16, "PNG")
    keep("ico_refused_png_wrong_size", ico([(32, 32, 0, 32, png16.getvalue())]), None)
    keep("ico_refused_bmp_wrong_size", ico([(32, 32, 0, 8, e8)]), None)
    keep("ico_refused_os2_header", ico([(w, h, 0, 24, bmp.core(w, 2 * h, 24) + bmp.rows([bmp.bgr([p[:3] for p in r]) for r in pic]) + and_mask(mask))]), None)
    keep("ico_refused_short_mask", ico([(w, h, 0, 8, e8[:-5])]), None)
    keep("ico_refused_offset_in_directory", ico([(w, h, 0, 8, e8)], offsets=[10]), None)
    keep("ico_refused_best_is_broken", ico([(w, h, 0, 8, e8), (32, 32, 0, 8, e8)]), None)
    keep("ico_refused_no_entries", struct.pack("<HHH", 0, 1, 0), None)
    # Pillow writes an AND mask's rows unpadded -- three bytes each for a
    # 24-pixel icon, not four -- which Chrome reads as a mask cut short.
    buf = io.BytesIO()
    Image.new("P", (24, 24), 3).save(buf, "ICO", sizes=[(24, 24)], bitmap_format="bmp")
    keep("ico_refused_pillow_unpadded_mask", buf.getvalue(), None)
    keep("ico_refused_rle_absolute_past_row", ico([(w, h, 4, 8, bmp.dib(40, w, 2 * h, 8, 1, 4) + bmp.palette(colours8[:4])
                                                    + bytes([14, 1, 0, 3, 1, 2, 3, 0, 0, 1]) + and_mask(mask))]), None)
    return files, answers, pillow_differs


def main() -> None:
    files, answers, pillow_differs = fixtures()
    for name, data in files.items():
        (HERE / f"{name}.ico").write_bytes(data)
        by_pillow = pillow_answer(data)
        if name in answers:
            text = answers[name]
            if name in pillow_differs:
                got = answer_text(w=16, h=16, pixels=by_pillow) if by_pillow else "REFUSED\n"
                assert got != text, f"{name}: Pillow agrees, so this does not test Chrome's rule"
        else:
            assert by_pillow is not None, name
            img = Image.open(io.BytesIO(data))
            text = answer_text(img.width, img.height, by_pillow)
        (HERE / f"{name}.txt").write_text(text)
        print(f"  {name}: {text.split()[0] if text.startswith('REFUSED') else ' '.join(text.split()[:2])}")


if __name__ == "__main__":
    main()
