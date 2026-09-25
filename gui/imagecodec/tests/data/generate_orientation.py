#!/usr/bin/env python3
"""Regenerate the EXIF orientation fixtures next to this script.

One 24x16 picture, stored eight times -- as a JPEG and as a PNG -- under each
EXIF orientation, answered by Pillow's decode turned by
`ImageOps.exif_transpose`: what the orientation means is not in dispute, and
Pillow applies it as Chrome does.

Where Chrome reads the orientation differently from Pillow, the answer is
Chrome's, made from Pillow's unturned decode by the turn Chrome's rule gives
(`src/orientation.rs`), and the generator checks that Pillow's own answer
differs: an orientation only in the EXIF sub-directory (Chrome follows the
pointer; Pillow does not), one stored as a `LONG` (Chrome takes only a
`SHORT`), and a PNG `eXIf` chunk after the image data (Chrome reads the
chunks before it).

Answers are text: width, height, then `AARRGGBB` per pixel. The JPEG ones are
compared to within rounding, as every JPEG fixture is; the PNG ones exactly.

Usage
-----

    python gui/imagecodec/tests/data/generate_orientation.py
"""

from __future__ import annotations

import io
import pathlib
import struct
import zlib

from PIL import Image, ImageOps

import generate_bmp as bmp

HERE = pathlib.Path(__file__).parent

# What each orientation does to the stored picture, as exif_transpose does it.
TURN = {
    1: None,
    2: Image.Transpose.FLIP_LEFT_RIGHT,
    3: Image.Transpose.ROTATE_180,
    4: Image.Transpose.FLIP_TOP_BOTTOM,
    5: Image.Transpose.TRANSPOSE,
    6: Image.Transpose.ROTATE_270,
    7: Image.Transpose.TRANSVERSE,
    8: Image.Transpose.ROTATE_90,
}


def tiff(entries: list[tuple[int, int, int, int]], sub: list[tuple[int, int, int, int]] | None = None,
         big: bool = False) -> bytes:
    """A TIFF structure: a first directory of (tag, type, count, value)
    entries, and optionally a second one after it."""
    e = ">" if big else "<"
    out = (b"MM\0*" if big else b"II*\0") + struct.pack(e + "I", 8)

    def directory(items: list[tuple[int, int, int, int]]) -> bytes:
        body = struct.pack(e + "H", len(items))
        for tag, kind, count, value in items:
            body += struct.pack(e + "HHI", tag, kind, count)
            body += struct.pack(e + "HH", value, 0) if kind == 3 else struct.pack(e + "I", value)
        return body + struct.pack(e + "I", 0)

    out += directory(entries)
    if sub is not None:
        out += directory(sub)
    return out


def orientation_exif(value: int, kind: int = 3, big: bool = False) -> bytes:
    return b"Exif\0\0" + tiff([(0x0112, kind, 1, value)], big=big)


def picture() -> Image.Image:
    img = Image.new("RGB", (24, 16))
    img.putdata([p for row in bmp.photo(24, 16) for p in row])
    return img


def answer_text(img: Image.Image) -> str:
    rgba = img.convert("RGBA")
    words = [str(rgba.width), str(rgba.height)]
    words += [f"{a:02X}{r:02X}{g:02X}{b:02X}" for r, g, b, a in rgba.get_flattened_data()]
    return " ".join(words) + "\n"


def turned(img: Image.Image, value: int) -> Image.Image:
    method = TURN.get(value)
    return img.transpose(method) if method is not None else img


def pillow_answer(data: bytes) -> Image.Image:
    img = Image.open(io.BytesIO(data))
    img.load()
    return ImageOps.exif_transpose(img).convert("RGB")


def raw_answer(data: bytes) -> Image.Image:
    img = Image.open(io.BytesIO(data))
    img.load()
    return img.convert("RGB")


def jpeg(img: Image.Image, exif: bytes) -> bytes:
    buf = io.BytesIO()
    img.save(buf, "JPEG", quality=95, subsampling=0, exif=exif)
    return buf.getvalue()


def png(img: Image.Image, exif: bytes) -> bytes:
    buf = io.BytesIO()
    img.save(buf, "PNG", exif=exif)
    return buf.getvalue()


def png_chunks(data: bytes) -> list[tuple[bytes, bytes]]:
    at, out = 8, []
    while at < len(data):
        n = struct.unpack(">I", data[at : at + 4])[0]
        out.append((data[at + 4 : at + 8], data[at + 8 : at + 8 + n]))
        at += 12 + n
    return out


def png_file(chunks: list[tuple[bytes, bytes]]) -> bytes:
    out = b"\x89PNG\r\n\x1a\n"
    for kind, body in chunks:
        out += struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))
    return out


def main() -> None:
    img = picture()
    files: dict[str, tuple[bytes, str]] = {}
    for value in range(1, 9):
        data = jpeg(img, orientation_exif(value))
        files[f"orient_jpeg_{value}.jpg"] = (data, answer_text(pillow_answer(data)))
        data = png(img, orientation_exif(value))
        files[f"orient_png_{value}.png"] = (data, answer_text(pillow_answer(data)))

    def chrome_only(name: str, data: bytes, value: int) -> None:
        want = answer_text(turned(raw_answer(data), value))
        assert answer_text(pillow_answer(data)) != want, f"{name}: Pillow agrees; not a test of Chrome's rule"
        files[name] = (data, want)

    # Orientation only in the EXIF sub-directory: Chrome follows the pointer.
    sub = b"Exif\0\0" + tiff([(0x8769, 4, 1, 8 + 2 + 12 + 4)], [(0x0112, 3, 1, 6)])
    chrome_only("orient_jpeg_sub_ifd.jpg", jpeg(img, sub), 6)
    # Stored as a LONG: not an orientation to Chrome.
    chrome_only("orient_jpeg_long.jpg", jpeg(img, orientation_exif(6, kind=4)), 1)
    # Big-endian, and a value that names nothing.
    data = jpeg(img, orientation_exif(8, big=True))
    files["orient_jpeg_big_endian.jpg"] = (data, answer_text(pillow_answer(data)))
    data = jpeg(img, orientation_exif(9))
    files["orient_jpeg_invalid.jpg"] = (data, answer_text(raw_answer(data)))
    # Two EXIF blocks: the first counts.
    data = jpeg(img, orientation_exif(6))
    app1 = data.index(b"\xff\xe1")
    length = struct.unpack(">H", data[app1 + 2 : app1 + 4])[0]
    second = orientation_exif(3)
    data = data[: app1 + 2 + length] + b"\xff\xe1" + struct.pack(">H", len(second) + 2) + second + data[app1 + 2 + length :]
    files["orient_jpeg_two_exif.jpg"] = (data, answer_text(turned(raw_answer(data), 6)))
    # A PNG eXIf after the image data: Chrome has read the headers by then.
    chunks = png_chunks(png(img, orientation_exif(6)))
    exif = [c for c in chunks if c[0] == b"eXIf"]
    rest = [c for c in chunks if c[0] not in (b"eXIf", b"IEND")]
    moved = png_file(rest + exif + [(b"IEND", b"")])
    chrome_only("orient_png_exif_after_idat.png", moved, 1)

    for name, (data, text) in files.items():
        (HERE / name).write_bytes(data)
        with open(HERE / (name.rsplit(".", 1)[0] + ".txt"), "w", encoding="ascii", newline="\n") as out:
            out.write(text)
        print(f"  {name}: {' '.join(text.split()[:2])}")


if __name__ == "__main__":
    main()
