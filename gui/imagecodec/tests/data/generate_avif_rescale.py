#!/usr/bin/env python3
"""Generate the AVIF rescale fixtures (`avifrs_*.avif`) and their answers
(`avif_rescale.txt`).

Each fixture is a picture encoded by libavif (1.4.2, through `imagecodecs`
2026.8.16) at one size, whose `ispe` -- the size the container declares for
the colour item and the alpha item -- is then rewritten to another. A reader
must bring the decoded frame to the declared size: libavif does it in
`avifImageScaleWithLimit`, with libyuv's `ScalePlane` (`ScalePlane_12` for
deeper samples) and `kFilterBox`, which pick their method from the two sizes.
The sizes here reach each method -- the box filter, bilinear down and up,
the fixed 1/2, 3/4, 3/8 and 1/4 and twice-the-size kernels, linear across,
rows alone, nearest -- for luma, chroma and alpha, at 8, 10 and 12 bits and
each subsampling. Every one is BT.601 at full range, so that the conversion
to RGB is libyuv's and the scaling is all that is new.

The answer for each is what Pillow decodes it to, as a CRC-32 of the bytes,
which `tests/avif.rs` compares this crate's pixels against: Pillow 12.1.1
(libavif 1.3.0, dav1d 1.5.1, libyuv 1909) when generated; and `--check` run
under Pillow 12.3.0 (libavif 1.4.2, dav1d 1.5.3, libyuv 1924) found every
answer the same -- libyuv's scaling did not change between the two. Both are
Windows builds, so libyuv's C, not its x86 SIMD, did the scaling (see
`src/avif/scale.rs`).

Deep and grey pictures have alpha, for the reason `generate_avif_pixels.py`
gives: Pillow converts pictures without alpha to 24-bit RGB, which libyuv
reaches from deep or grey pictures by other routes than this crate's BGRA.

Run from this directory, with a Python that has imagecodecs and Pillow:
    python generate_avif_rescale.py           write fixtures and answers
    python generate_avif_rescale.py --check   compare this Pillow's answers
"""

import hashlib
import struct
import sys
import zlib

OUT_PREFIX = "avifrs_"
ANSWERS = "avif_rescale.txt"

# name: (coded width, coded height, declared width, declared height, depth,
#        subsampling, alpha). The comment is libyuv's method for luma, then
#        for chroma where it differs.
FIXTURES = {
    "8_420_box": (64, 48, 20, 15, 8, "420", False),  # box
    "8_420_bilinear_down": (64, 48, 50, 37, 8, "420", False),
    "8_420_bilinear_up": (40, 30, 61, 47, 8, "420", False),
    "8_420_half": (64, 48, 32, 24, 8, "420", False),
    "8_420_three_quarters": (64, 48, 48, 36, 8, "420", False),
    "8_420_three_eighths": (64, 64, 24, 24, 8, "420", False),
    "8_420_quarter": (64, 48, 16, 12, 8, "420", False),
    "8_420_twice": (32, 24, 64, 48, 8, "420", False),
    "8_420_twice_less_one": (32, 24, 63, 47, 8, "420", False),
    "8_420_twice_as_wide": (40, 30, 80, 30, 8, "420", False),  # linear across
    "8_420_taller": (48, 40, 48, 64, 8, "420", False),  # rows alone
    "8_420_third_as_wide": (90, 40, 30, 40, 8, "420", False),  # nearest
    "8_420_odd": (37, 19, 53, 29, 8, "420", False),
    "8_420_wider_shorter": (40, 48, 61, 37, 8, "420", False),
    "8_420a_box": (64, 48, 20, 15, 8, "420", True),
    "8_420a_twice": (32, 24, 64, 48, 8, "420", True),
    "8_422a_three_quarters": (64, 48, 48, 36, 8, "422", True),
    "8_444a_bilinear_down": (64, 48, 45, 33, 8, "444", True),
    "8_400a_box": (64, 48, 21, 13, 8, "400", True),
    "10_420a_box": (64, 48, 20, 15, 10, "420", True),
    "10_420a_bilinear_up": (40, 30, 61, 47, 10, "420", True),
    "10_420a_twice": (32, 24, 64, 48, 10, "420", True),
    "10_422a_twice_as_wide": (40, 30, 80, 30, 10, "422", True),
    "10_444a_half": (64, 48, 32, 24, 10, "444", True),
    "12_420a_bilinear_down": (64, 48, 50, 37, 12, "420", True),
    "12_444a_three_eighths": (64, 64, 24, 24, 12, "444", True),
    "12_400a_quarter": (64, 48, 16, 12, 12, "400", True),
    "12_420a_taller": (48, 40, 48, 64, 12, "420", True),
}


def answer(data):
    """Pillow's decode of `data`, as `generate_avif_pixels.py` records it."""
    from PIL import _avif

    try:
        decoder = _avif.AvifDecoder(data, "auto", 1)
        size, _, mode, *_ = decoder.get_info()
        pixels = decoder.get_frame(0)[0]
    except Exception as e:  # noqa: BLE001 - a refusal is an answer
        return f"refused\t{type(e).__name__}"
    return f"{mode}\t{size[0]}x{size[1]}\t{zlib.crc32(pixels) & 0xFFFFFFFF:08x}"


def set_ispe(data, width, height):
    """Rewrite every `ispe` property (colour's and alpha's) to width x height."""
    from generate_avif_pixels import boxes, find

    data = bytearray(data)
    _, _, meta_payload, meta_end = find(data, b"meta")
    _, _, iprp_payload, iprp_end = find(data, b"iprp", meta_payload + 4, meta_end)
    _, _, ipco_payload, ipco_end = find(data, b"ipco", iprp_payload, iprp_end)
    count = 0
    for kind, at, payload, end in boxes(data, ipco_payload, ipco_end):
        if kind == b"ispe":
            if end - at != 20:
                raise ValueError("an ispe box of an unexpected size")
            data[payload + 4 : payload + 12] = struct.pack(">II", width, height)
            count += 1
    if count == 0:
        raise ValueError("no ispe box")
    return bytes(data)


def encode(spec):
    import imagecodecs
    from generate_avif_pixels import pattern

    width, height, target_w, target_h, depth, fmt, alpha = spec
    channels = (2 if alpha else 1) if fmt == "400" else (4 if alpha else 3)
    pixels = pattern(width, height, depth, channels)
    data = imagecodecs.avif_encode(
        pixels,
        level=90,
        speed=6,
        bitspersample=depth,
        pixelformat="YUV" + fmt,
        primaries=1,
        transfer=13,
        matrix=6,
        numthreads=1,
    )
    return set_ispe(data, target_w, target_h)


def main():
    from PIL import __version__ as pillow, features

    lines = [
        f"# AVIF rescale answers: fixture, then Pillow {pillow}'s (libavif"
        f" {features.version('avif')}'s) mode,",
        "# size and CRC-32, or 'refused'. Generated by generate_avif_rescale.py;",
        "# the last column pins each fixture file by SHA-256.",
    ]
    for name, spec in FIXTURES.items():
        data = encode(spec)
        path = f"{OUT_PREFIX}{name}.avif"
        with open(path, "wb") as f:
            f.write(data)
        got = answer(data)
        size = f"{spec[2]}x{spec[3]}"
        if f"\t{size}\t" not in f"\t{got}\t":
            raise SystemExit(f"{path}: Pillow did not bring it to {size}: {got}")
        lines.append(f"{path}\t{got}\t{hashlib.sha256(data).hexdigest()}")
        print(lines[-1], file=sys.stderr)
    with open(ANSWERS, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines) + "\n")


def check():
    """Decode every fixture with this Python's Pillow and compare the answers
    on record: for a second Pillow, with another libavif and libyuv."""
    from PIL import __version__ as pillow, features

    bad = 0
    with open(ANSWERS, encoding="utf-8") as f:
        for line in f:
            if line.startswith("#"):
                continue
            path, *want, digest = line.rstrip("\n").split("\t")
            with open(path, "rb") as g:
                data = g.read()
            if hashlib.sha256(data).hexdigest() != digest:
                raise SystemExit(f"{path}: not the file the answers were made from")
            got = answer(data)
            if got != "\t".join(want):
                print(f"{path}: {got} where the answers say {want}")
                bad += 1
    print(f"Pillow {pillow} (libavif {features.version('avif')}): {bad} differ")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(check() if sys.argv[1:] == ["--check"] else main())
