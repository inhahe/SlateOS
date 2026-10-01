#!/usr/bin/env python3
"""Generate the AVIF pixel fixtures (`avifpx_*.avif`) and their answers
(`avif_pixels.txt`).

Each fixture is a small picture made here from a fixed pattern and encoded by
libavif (1.4.2, through `imagecodecs` 2026.8.16) at one combination of the
things that choose a conversion path in libavif 1.3.0's `avifImageYUVToRGB`:
bit depth, chroma subsampling, alpha, range, matrix, and premultiplied alpha.
The answer for each is what Pillow 12.1.1 -- libavif 1.3.0 with dav1d and
libyuv -- decodes it to, as a CRC-32 of the bytes, which `tests/avif.rs`
compares this crate's pixels against.

Pillow converts to RGB when a file has no alpha and to RGBA when it has. The
crate always converts to libavif's BGRA, which for pictures with alpha takes
exactly the same libyuv functions as Pillow's RGBA (with U and V swapped and
red and blue exchanged back), and for 8-bit colour without alpha the same
arithmetic as Pillow's RGB. Deep or grey pictures without alpha take other
functions under RGB (libyuv has no deep RGB-24 functions, and no grey one), so
every deep and grey fixture has alpha.

Two things libavif's encoder is not asked to do are done to its output:
limited range, by clearing the full-range flag of the `colr` box (so the same
samples are read as limited range; the AV1 sequence header still says full,
and libavif, like this crate, takes the container's word), and premultiplied
alpha, by adding a `prem` reference from the colour item to the alpha item.
Other matrices are likewise written into the `colr` box after encoding, for
the ones libavif refuses to convert and so will not encode.

Run from this directory: python generate_avif_pixels.py
"""

import hashlib
import struct
import sys
import zlib

import imagecodecs
import numpy
from PIL import _avif

OUT_PREFIX = "avifpx_"


def pattern(width, height, depth, channels):
    """A picture with smooth ramps, sharp edges and every chroma phase, in
    samples of `depth` bits."""
    y, x = numpy.mgrid[0:height, 0:width].astype(numpy.float64)
    top = (1 << depth) - 1
    r = (x / max(width - 1, 1)) * top
    g = (y / max(height - 1, 1)) * top
    b = ((x + y) % 7 < 3) * top * 0.8 + ((x * 3 + y * 5) % 11) * top / 40
    a = (((x // 3 + y // 2) % 4) + 0.5) / 4.0 * top
    a[0, 0] = 0
    a[-1, -1] = top
    planes = {
        1: [0.3 * r + 0.6 * g + 0.1 * b],
        2: [0.3 * r + 0.6 * g + 0.1 * b, a],
        3: [r, g, b],
        4: [r, g, b, a],
    }[channels]
    dtype = numpy.uint8 if depth == 8 else numpy.uint16
    out = numpy.stack([numpy.clip(numpy.rint(p), 0, top) for p in planes], axis=-1).astype(dtype)
    return out if channels > 1 else out[..., 0]


def boxes(data, start, end):
    """(type, box start, payload start, box end) of each box in data[start:end]."""
    at = start
    while at + 8 <= end:
        size, kind = struct.unpack(">I4s", data[at : at + 8])
        header = 8
        if size == 1:
            size = struct.unpack(">Q", data[at + 8 : at + 16])[0]
            header = 16
        elif size == 0:
            size = end - at
        yield kind, at, at + header, at + size
        at += size


def find(data, kind, start=0, end=None):
    for box in boxes(data, start, len(data) if end is None else end):
        if box[0] == kind:
            return box
    raise ValueError(f"no {kind!r} box")


def set_colr(data, *, primaries=None, transfer=None, matrix=None, full_range=None):
    """Rewrite the nclx `colr` box's fields in place."""
    data = bytearray(data)
    at = data.find(b"colrnclx")
    if at < 0:
        raise ValueError("no nclx colr box")
    fields = at + 8
    p, t, m = struct.unpack(">HHH", data[fields : fields + 6])
    flag = data[fields + 6]
    p = p if primaries is None else primaries
    t = t if transfer is None else transfer
    m = m if matrix is None else matrix
    if full_range is not None:
        flag = (flag & 0x7F) | (0x80 if full_range else 0)
    data[fields : fields + 7] = struct.pack(">HHHB", p, t, m, flag)
    return bytes(data)


def add_prem(data):
    """Add `prem` (colour item premultiplied by alpha item) to `iref`, and move
    every `iloc` extent after the insertion point along by its size."""
    data = bytearray(data)
    _, meta_at, meta_payload, meta_end = find(data, b"meta")
    children = meta_payload + 4
    _, _, pitm_payload, _ = find(data, b"pitm", children, meta_end)
    pitm_version = data[pitm_payload]
    colour = (
        struct.unpack(">H", data[pitm_payload + 4 : pitm_payload + 6])[0]
        if pitm_version == 0
        else struct.unpack(">I", data[pitm_payload + 4 : pitm_payload + 8])[0]
    )
    _, iref_at, iref_payload, iref_end = find(data, b"iref", children, meta_end)
    wide = data[iref_payload] != 0
    alpha = None
    for kind, _, payload, _ in boxes(data, iref_payload + 4, iref_end):
        if kind == b"auxl":
            if wide:
                alpha = struct.unpack(">I", data[payload : payload + 4])[0]
            else:
                alpha = struct.unpack(">H", data[payload : payload + 2])[0]
    if alpha is None:
        raise ValueError("no auxl reference: no alpha item")
    if wide:
        new = struct.pack(">I4sIHI", 18, b"prem", colour, 1, alpha)
    else:
        new = struct.pack(">I4sHHH", 14, b"prem", colour, 1, alpha)
    grow = len(new)
    # Sizes of iref and meta (neither uses a 64-bit size in libavif's output).
    for box_at in (iref_at, meta_at):
        size = struct.unpack(">I", data[box_at : box_at + 4])[0]
        data[box_at : box_at + 4] = struct.pack(">I", size + grow)
    # iloc: move every file-offset extent that lies past the insertion point.
    _, _, iloc_payload, _ = find(data, b"iloc", children, meta_end)
    version = data[iloc_payload]
    at = iloc_payload + 4
    offset_size, length_size = data[at] >> 4, data[at] & 15
    base_size, index_size = data[at + 1] >> 4, data[at + 1] & 15
    at += 2
    if version < 2:
        count = struct.unpack(">H", data[at : at + 2])[0]
        at += 2
    else:
        count = struct.unpack(">I", data[at : at + 4])[0]
        at += 4

    def read(n):
        nonlocal at
        value = int.from_bytes(data[at : at + n], "big") if n else 0
        at += n
        return value

    def write(where, n, value):
        data[where : where + n] = value.to_bytes(n, "big")

    for _ in range(count):
        read(2 if version < 2 else 4)  # item ID
        method = read(2) & 15 if version in (1, 2) else 0
        read(2)  # data reference index
        base_at = at
        base = read(base_size)
        extents = read(2)
        # A base offset past the insertion moves every extent with it;
        # otherwise each extent past it moves on its own.
        move_base = method == 0 and base_size > 0 and base >= iref_end
        for _ in range(extents):
            if version in (1, 2) and index_size:
                read(index_size)
            offset_at = at
            offset = read(offset_size)
            read(length_size)
            if method == 0 and not move_base and base + offset >= iref_end:
                write(offset_at, offset_size, offset + grow)
        if move_base:
            write(base_at, base_size, base + grow)
    data[iref_end:iref_end] = new
    return bytes(data)


# name: (width, height, depth, format, alpha, full range, primaries, matrix,
#        premultiplied, colr matrix override)
FIXTURES = {
    # libyuv, 8-bit
    "8_420_709": (37, 19, 8, "420", False, True, 1, 1, False, None),
    "8_420a_601_limited": (36, 18, 8, "420", True, False, 1, 6, False, None),
    "8_422_601": (37, 19, 8, "422", False, True, 1, 6, False, None),
    "8_422a_709_limited": (36, 18, 8, "422", True, False, 1, 1, False, None),
    "8_444a_2020": (37, 19, 8, "444", True, True, 9, 9, False, None),
    "8_444_2020_limited": (36, 18, 8, "444", False, False, 9, 9, False, None),
    "8_400a_601": (37, 19, 8, "400", True, True, 1, 6, False, None),
    "8_420a_derived_709": (37, 18, 8, "420", True, True, 1, 12, False, None),
    # libyuv, 10-bit
    "10_420a_709": (37, 19, 10, "420", True, True, 1, 1, False, None),
    "10_420a_709_even": (36, 18, 10, "420", True, True, 1, 1, False, None),
    "10_422a_2020_limited": (37, 19, 10, "422", True, False, 9, 9, False, None),
    "10_444a_601": (36, 18, 10, "444", True, True, 1, 6, False, None),
    "10_400a_709": (37, 19, 10, "400", True, True, 1, 1, False, None),
    # libyuv, 12-bit
    "12_420a_709_limited": (37, 19, 12, "420", True, False, 1, 1, False, None),
    "12_422a_601": (36, 18, 12, "422", True, True, 1, 6, False, None),
    "12_444a_2020": (37, 19, 12, "444", True, True, 9, 9, False, None),
    "12_400a_601_limited": (36, 18, 12, "400", True, False, 1, 6, False, None),
    # libavif's own arithmetic
    "8_444a_identity": (37, 19, 8, "444", True, True, 1, 0, False, None),
    "10_444a_identity": (36, 18, 10, "444", True, True, 1, 0, False, None),
    "8_444a_fcc": (37, 19, 8, "444", True, True, 1, 4, False, None),
    "10_444a_smpte240": (36, 18, 10, "444", True, True, 1, 7, False, None),
    "8_420a_fcc": (37, 19, 8, "420", True, True, 1, 4, False, None),
    "8_420a_fcc_even": (36, 18, 8, "420", True, True, 1, 4, False, None),
    "10_422a_smpte240_limited": (37, 19, 10, "422", True, False, 1, 7, False, None),
    "8_420a_ycgco": (37, 19, 8, "420", True, True, 1, 8, False, None),
    "8_444a_ycgco": (36, 18, 8, "444", True, True, 1, 8, False, None),
    "8_420a_derived_p3": (37, 19, 8, "420", True, True, 12, 12, False, None),
    "8_400a_fcc": (36, 18, 8, "400", True, True, 1, 4, False, None),
    "10_400a_smpte240": (37, 19, 10, "400", True, True, 1, 7, False, None),
    "12_420a_fcc_limited": (36, 18, 12, "420", True, False, 1, 4, False, None),
    "8_444a_unassigned_15": (37, 19, 8, "444", True, True, 1, 6, False, 15),
    # premultiplied alpha
    "8_420a_709_prem": (37, 19, 8, "420", True, True, 1, 1, True, None),
    "8_420a_fcc_prem": (36, 18, 8, "420", True, True, 1, 4, True, None),
    "10_444a_601_prem": (37, 19, 10, "444", True, True, 1, 6, True, None),
    "8_400a_601_prem": (36, 18, 8, "400", True, True, 1, 6, True, None),
    # conversions libavif refuses
    "8_444_ycgco_limited": (37, 19, 8, "444", False, False, 1, 8, False, None),
    "8_444_bt2020cl": (36, 18, 8, "444", False, True, 9, 9, False, 10),
    "8_444_ictcp": (37, 19, 8, "444", False, True, 9, 9, False, 14),
    "8_444_reserved_3": (36, 18, 8, "444", False, True, 1, 6, False, 3),
}


def encode(spec):
    width, height, depth, fmt, alpha, full, primaries, matrix, prem, override = spec
    channels = (2 if alpha else 1) if fmt == "400" else (4 if alpha else 3)
    pixels = pattern(width, height, depth, channels)
    data = imagecodecs.avif_encode(
        pixels,
        level=90,
        speed=6,
        bitspersample=depth,
        pixelformat="YUV" + fmt,
        primaries=primaries,
        transfer=13,
        matrix=matrix,
        numthreads=1,
    )
    if not full:
        data = set_colr(data, full_range=False)
    if override is not None:
        data = set_colr(data, matrix=override)
    if prem:
        data = add_prem(data)
    return data


def answer(data):
    try:
        decoder = _avif.AvifDecoder(data, "auto", 1)
        size, _, mode, *_ = decoder.get_info()
        pixels = decoder.get_frame(0)[0]
    except Exception as e:  # noqa: BLE001 - a refusal is an answer
        return f"refused\t{type(e).__name__}"
    return f"{mode}\t{size[0]}x{size[1]}\t{zlib.crc32(pixels) & 0xFFFFFFFF:08x}"


def main():
    lines = [
        "# AVIF pixel answers: fixture, then Pillow 12.1.1's (libavif 1.3.0's) mode,",
        "# size and CRC-32 of frame 0, or 'refused'. Generated by generate_avif_pixels.py;",
        "# the last column pins each fixture file by SHA-256.",
    ]
    for name, spec in FIXTURES.items():
        data = encode(spec)
        path = f"{OUT_PREFIX}{name}.avif"
        with open(path, "wb") as f:
            f.write(data)
        lines.append(f"{path}\t{answer(data)}\t{hashlib.sha256(data).hexdigest()}")
        print(lines[-1], file=sys.stderr)
    with open("avif_pixels.txt", "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
