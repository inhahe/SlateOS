#!/usr/bin/env python3
"""Generate the AVIF sequence fixtures (`avifseq_*.avif`) and every sequence's
answers (`avif_frames.txt`), frame by frame.

The sequences are libavif's four from the corpus (`avif_colors_animated_*`:
8-bit, 8-bit with an alpha track, one beside an audio track, and a 12-bit one
with alpha whose key frames are 0, 2 and 3 only) and three made here from one
eight-frame RGBA sequence that Pillow encodes (libavif 1.3.0 with aom):

  avifseq_8_rgba_durations    a different duration for each frame, two of them
                              10 ms or less; plays forever (libavif's default)
  avifseq_8_rgba_loop2        the same, its colour track's duration rewritten
                              to three times its edit's segment: libavif reads
                              that as two repetitions after the first play
  avifseq_8_rgba_short_alpha  the same, its alpha track's one chunk cut to six
                              samples: frames 6 and 7 have no alpha, and
                              libavif stops there (NO_IMAGES_REMAINING)

For each sequence the answer is what Pillow 12.1.1 (libavif 1.3.0, dav1d)
decodes, frame by frame through `seek`: each frame's duration in milliseconds
and the CRC-32 of its RGB or RGBA bytes -- or `end` where libavif has no image.
Every sequence here takes a path on which this crate's conversion agrees with
Pillow's to the bit (8-bit colour, or alpha: see generate_avif_pixels.py).
Pillow does not report the repetition count, so that column comes from this
script's own reading of the colour track's `tkhd` and `elst`, by libavif's
rule -- a second implementation of it, not the crate's.

Run from this directory: python generate_avif_frames.py
"""

import hashlib
import io
import struct
import sys
import zlib

import numpy
from PIL import Image

CORPUS = [
    "avif_colors_animated_8bpc.avif",
    "avif_colors_animated_8bpc_alpha_exif_xmp.avif",
    "avif_colors_animated_8bpc_audio.avif",
    "avif_colors_animated_12bpc_keyframes_0_2_3.avif",
]
DURATIONS = [10, 20, 40, 80, 160, 7, 1000, 33]


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


def child(data, parent, kind, skip=0):
    """The first box of `kind` in the payload of `parent` (its tuple)."""
    _, _, payload, end = parent
    for box in boxes(data, payload + skip, end):
        if box[0] == kind:
            return box
    return None


def tracks(data):
    """Each `trak` box, with whether it is an auxiliary (`auxl`) track."""
    moov = next(b for b in boxes(data, 0, len(data)) if b[0] == b"moov")
    out = []
    for trak in boxes(data, moov[2], moov[3]):
        if trak[0] != b"trak":
            continue
        tref = child(data, trak, b"tref")
        aux = tref is not None and child(data, tref, b"auxl") is not None
        out.append((trak, aux))
    return out


def repetition(data):
    """libavif's repetition count for the colour track: `forever`, or
    `count N` -- `avifParseTrackBox` after its `tkhd` and `edts`."""
    trak = next(t for t, aux in tracks(data) if not aux)
    tkhd = child(data, trak, b"tkhd")
    version = data[tkhd[2]]
    if version == 1:
        duration = struct.unpack(">Q", data[tkhd[2] + 28 : tkhd[2] + 36])[0]
    else:
        duration = struct.unpack(">I", data[tkhd[2] + 20 : tkhd[2] + 24])[0]
        duration = (1 << 64) - 1 if duration == 0xFFFFFFFF else duration
    edts = child(data, trak, b"edts")
    if edts is None:
        return "forever"  # unknown: Chrome and Firefox loop
    elst = child(data, edts, b"elst")
    ev, flags = data[elst[2]], int.from_bytes(data[elst[2] + 1 : elst[2] + 4], "big")
    if flags & 1 == 0:
        return "count 0"
    at = elst[2] + 8
    segment = (struct.unpack(">Q", data[at : at + 8])[0] if ev == 1
               else struct.unpack(">I", data[at : at + 4])[0])
    if duration == (1 << 64) - 1:
        return "forever"
    count = duration // segment + (1 if duration % segment else 0) - 1
    return f"count {count}" if count < 2**31 else "forever"


def frames(width, height):
    """Eight RGBA frames: a bar sweeping across ramps, and alpha that changes
    with it, so no two frames are alike and every one has partial alpha."""
    y, x = numpy.mgrid[0:height, 0:width].astype(numpy.float64)
    out = []
    for i in range(len(DURATIONS)):
        bar = numpy.abs(x - i * width / len(DURATIONS)) < 4
        r = numpy.where(bar, 250, x * 255 / (width - 1))
        g = y * 255 / (height - 1)
        b = numpy.full_like(x, 40 + 25 * i)
        a = numpy.where(bar, 255, 60 + ((x + y + 3 * i) % 16) * 12)
        rgba = numpy.stack([r, g, b, a], axis=-1)
        out.append(Image.fromarray(numpy.clip(numpy.rint(rgba), 0, 255).astype(numpy.uint8), "RGBA"))
    return out


def encode():
    images = frames(48, 32)
    buf = io.BytesIO()
    images[0].save(buf, "AVIF", save_all=True, append_images=images[1:],
                   duration=DURATIONS, quality=90, max_threads=1)
    return buf.getvalue()


def loop_twice(data):
    """The colour track's `tkhd` duration made three segments long."""
    data = bytearray(data)
    trak = next(t for t, aux in tracks(data) if not aux)
    tkhd = child(data, trak, b"tkhd")
    elst = child(data, child(data, trak, b"edts"), b"elst")
    ev = data[elst[2]]
    at = elst[2] + 8
    segment = (struct.unpack(">Q", data[at : at + 8])[0] if ev == 1
               else struct.unpack(">I", data[at : at + 4])[0])
    data[elst[2] + 3] |= 1  # repeating
    if data[tkhd[2]] == 1:
        data[tkhd[2] + 28 : tkhd[2] + 36] = struct.pack(">Q", 3 * segment)
    else:
        data[tkhd[2] + 20 : tkhd[2] + 24] = struct.pack(">I", 3 * segment)
    return bytes(data)


def short_alpha(data, keep=6):
    """The alpha track's `stsc` entry cut to `keep` samples a chunk."""
    data = bytearray(data)
    trak = next(t for t, aux in tracks(data) if aux)
    stbl = child(data, child(data, child(data, trak, b"mdia"), b"minf"), b"stbl")
    stsc = child(data, stbl, b"stsc")
    entries = struct.unpack(">I", data[stsc[2] + 4 : stsc[2] + 8])[0]
    assert entries == 1, "expected one stsc entry"
    at = stsc[2] + 8
    first, per, desc = struct.unpack(">III", data[at : at + 12])
    assert per == len(DURATIONS), per
    data[at + 4 : at + 8] = struct.pack(">I", keep)
    return bytes(data)


def answer(name):
    data = open(name, "rb").read()
    im = Image.open(name)
    parts = []
    for index in range(im.n_frames):
        try:
            im.seek(index)
            im.load()
        except Exception:  # libavif's NO_IMAGES_REMAINING, raised by Pillow
            parts.append("end")
            continue
        crc = zlib.crc32(im.tobytes()) & 0xFFFFFFFF
        parts.append(f"{im.info['duration']}:{crc:08x}")
    return "\t".join([
        name, im.mode, f"{im.size[0]}x{im.size[1]}", str(im.n_frames),
        repetition(data), " ".join(parts), hashlib.sha256(data).hexdigest(),
    ])


def main():
    base = encode()
    made = {
        "avifseq_8_rgba_durations.avif": base,
        "avifseq_8_rgba_loop2.avif": loop_twice(base),
        "avifseq_8_rgba_short_alpha.avif": short_alpha(base),
    }
    for name, data in made.items():
        with open(name, "wb") as f:
            f.write(data)
    lines = [
        "# AVIF sequence answers, one line per file: fixture, Pillow 12.1.1's (libavif",
        "# 1.3.0's) mode, size and frame count, the repetition by libavif's rule, then",
        "# each frame's 'duration_ms:crc32' of its bytes, or 'end' where libavif has no",
        "# image. Generated by generate_avif_frames.py; the last column pins the file.",
    ]
    lines += [answer(name) for name in CORPUS + list(made)]
    with open("avif_frames.txt", "w", newline="\n") as f:
        f.write("\n".join(lines) + "\n")
    print("\n".join(lines))


if __name__ == "__main__":
    sys.exit(main())
