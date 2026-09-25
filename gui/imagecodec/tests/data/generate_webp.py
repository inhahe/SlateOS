#!/usr/bin/env python3
"""Regenerate the WebP fixtures next to this script.

Every answer is Pillow's own decode of its fixture -- Pillow being libwebp
underneath, the decoder behind every browser that shows WebP -- and the tests
compare with no tolerance at all: lossless WebP is exact by definition, and
lossy WebP is too, since VP8 decoding is specified to the bit and the colour
conversion is libwebp's own (see `src/webp/lossy/yuv.rs`).

Lossless
--------

Written by Pillow; answers are text: width, height, then `AARRGGBB` per pixel.
Chosen to make libwebp's encoder use each of the format's tools: every
transform (predictor, colour, subtract-green, colour indexing at each of its
four pixel-bundling widths), the colour cache, backward references, meta
prefix codes on a larger picture, and both the simple and the extended
(`VP8X`) container. Several encoder efforts (`method`) are used because they
choose differently among those tools.

Lossy
-----

Answers are PNGs (RGBA, written by Pillow from its decode): a lossy picture's
answer as text would be ten times the size, and the PNG decoder that reads it
back is itself held to Pillow by `tests/png*.rs`.

Written three ways, to reach every feature of the format:

* **Pillow** (libwebp with its defaults: four segments, the normal loop
  filter, one coefficient partition) at several sizes, qualities and efforts,
  with and without alpha;
* **libwebp with settings Pillow does not expose**, through the `webp` package's
  bindings: the simple loop filter, no filter, sharpness, one segment, two to
  eight coefficient partitions, the skip flag, an uncompressed alpha plane;
* **by hand**, for what no encoder writes: alpha planes under each of the four
  predictive filters, stored raw and compressed, wrapped around a
  libwebp-encoded frame; and frames whose first partition `vp8rewrite.py` has
  rewritten -- segments by delta, segments with no map and with no values,
  loop-filter deltas under a strong filter, every quantiser delta, the colour
  space bits -- keeping libwebp's macroblocks. Those are exactly where
  decoders can disagree, and the answer is whatever libwebp makes of them.

One more frame comes from **libvpx** (through ffmpeg), a different encoder with
different habits: loop-filter deltas, the skip flag, its own probability
updates.

Usage
-----

    python gui/imagecodec/tests/data/generate_webp.py

Requires Pillow with WebP support, the `webp` package (`pip install webp`), and
`ffmpeg` built with libvpx on the PATH.
"""

from __future__ import annotations

import io
import math
import pathlib
import random
import struct
import subprocess
import tempfile

from PIL import Image

import vp8rewrite

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
    rng = random.Random(1305)
    palette = [(rng.randrange(256), rng.randrange(256), rng.randrange(256), 255) for _ in range(n)]
    img = Image.new("RGBA", (w, h))
    img.putdata([palette[rng.randrange(n)] for _ in range(w * h)])
    return img


def noise(w: int, h: int, seed: int) -> Image.Image:
    """Every sample random: the most a block can hold, so a lossy encoder
    spends its largest coefficients and every subblock mode on it."""
    rng = random.Random(seed)
    img = Image.new("RGB", (w, h))
    img.putdata([(rng.randrange(256), rng.randrange(256), rng.randrange(256)) for _ in range(w * h)])
    return img


def patches(w: int, h: int) -> Image.Image:
    """Flat colour with one flat patch: most macroblocks have no coefficients,
    so an encoder codes them with the skip flag."""
    img = Image.new("RGB", (w, h), (40, 90, 160))
    px = img.load()
    for y in range(20, min(h, 45)):
        for x in range(40, min(w, 70)):
            px[x, y] = (200, 60, 30)
    return img


# ---------------------------------------------------------------------------
# Answers
# ---------------------------------------------------------------------------


def decoded(data: bytes) -> Image.Image:
    im = Image.open(io.BytesIO(data))
    im.load()
    return im.convert("RGBA")


def write_answer(name: str, data: bytes) -> None:
    """Pillow's decode as text: width, height, `AARRGGBB` per pixel."""
    rgba = decoded(data)
    raw = rgba.tobytes()
    words = [str(rgba.width), str(rgba.height)]
    for k in range(0, len(raw), 4):
        r, g, b, a = raw[k : k + 4]
        words.append(f"{a:02X}{r:02X}{g:02X}{b:02X}")
    with open(HERE / f"{name}.txt", "w", encoding="ascii", newline="\n") as out:
        out.write(" ".join(words) + "\n")


def save(name: str, img: Image.Image, **kw) -> None:
    """A lossless fixture, written by Pillow, with its text answer."""
    buf = io.BytesIO()
    img.save(buf, "WEBP", **kw)
    data = buf.getvalue()
    (HERE / f"{name}.webp").write_bytes(data)
    write_answer(name, data)


def keep(name: str, data: bytes) -> None:
    """A lossy fixture, with Pillow's decode of it as a PNG answer."""
    (HERE / f"{name}.webp").write_bytes(data)
    decoded(data).save(HERE / f"{name}.png", optimize=True)


# ---------------------------------------------------------------------------
# Writing lossy WebPs
# ---------------------------------------------------------------------------


def pillow(name: str, img: Image.Image, **kw) -> None:
    buf = io.BytesIO()
    img.save(buf, "WEBP", **kw)
    keep(name, buf.getvalue())


# libwebp's `WebPConfig` (src/webp/encode.h): every field an int, in this
# order. The `webp` bindings declare only some, so the rest are set through
# the struct's memory, after checking the layout against the ones it knows.
CONFIG_FIELDS = [
    "lossless", "quality", "method", "image_hint", "target_size", "target_PSNR",
    "segments", "sns_strength", "filter_strength", "filter_sharpness", "filter_type",
    "autofilter", "alpha_compression", "alpha_filtering", "alpha_quality", "pass",
    "show_compressed", "preprocessing", "partitions", "partition_limit",
    "emulate_jpeg_size", "thread_level", "low_memory", "near_lossless", "exact",
    "use_delta_palette", "use_sharp_yuv",
]


def libwebp(img: Image.Image, quality: float = 75, method: int = 4, **fields) -> bytes:
    """Encode with libwebp and chosen settings. Note: at `method` 3 and above
    libwebp buffers its tokens and writes one coefficient partition whatever
    `partitions` says, so the partitioned fixtures use a lower effort."""
    import webp
    from webp import ffi

    for known in ("segments", "filter_type", "alpha_quality", "pass"):
        assert ffi.offsetof("WebPConfig", known) == 4 * CONFIG_FIELDS.index(known), known
    config = webp.WebPConfig.new(quality=quality, method=method)
    ints = ffi.cast("int *", config.ptr)
    for key, value in fields.items():
        ints[CONFIG_FIELDS.index(key)] = value
    assert config.validate(), fields
    return bytes(webp.WebPPicture.from_pil(img).encode(config).buffer())


def chunks_of(data: bytes) -> list[tuple[bytes, bytes]]:
    assert data[:4] == b"RIFF" and data[8:12] == b"WEBP"
    at, out = 12, []
    while at + 8 <= len(data):
        tag, size = data[at : at + 4], struct.unpack("<I", data[at + 4 : at + 8])[0]
        out.append((tag, data[at + 8 : at + 8 + size]))
        at += 8 + size + (size & 1)
    return out


def chunk(data: bytes, tag: bytes) -> bytes:
    return next(p for t, p in chunks_of(data) if t == tag)


def riff(chunks: list[tuple[bytes, bytes]]) -> bytes:
    body = b"WEBP"
    for tag, payload in chunks:
        body += tag + struct.pack("<I", len(payload)) + payload + (b"\0" if len(payload) & 1 else b"")
    return b"RIFF" + struct.pack("<I", len(body)) + body


def vp8x(w: int, h: int, flags: int) -> tuple[bytes, bytes]:
    return (b"VP8X", bytes([flags, 0, 0, 0]) + (w - 1).to_bytes(3, "little") + (h - 1).to_bytes(3, "little"))


ALPHA_FLAG = 0x10


def forward_filter(method: int, plane: bytes, w: int, h: int) -> bytes:
    """An alpha plane through one of the four predictive filters (RFC 9649):
    none, horizontal, vertical, gradient. The first row is predicted from the
    left, the first column from above, the first sample from nothing."""

    def at(x: int, y: int) -> int:
        return plane[y * w + x]

    out = bytearray(len(plane))
    for y in range(h):
        for x in range(w):
            if method == 0 or (x == 0 and y == 0):
                predicted = 0
            elif y == 0:
                predicted = at(x - 1, 0)
            elif x == 0:
                predicted = at(0, y - 1)
            elif method == 1:
                predicted = at(x - 1, y)
            elif method == 2:
                predicted = at(x, y - 1)
            else:
                predicted = max(0, min(255, at(x - 1, y) + at(x, y - 1) - at(x - 1, y - 1)))
            out[y * w + x] = (at(x, y) - predicted) & 0xFF
    return bytes(out)


def with_alpha(frame_of: Image.Image, plane: bytes, filter_method: int, compressed: bool) -> bytes:
    """An extended WebP: a libwebp-encoded frame of `frame_of` and an `ALPH`
    chunk holding `plane` through `filter_method`, raw or as a lossless image
    stream whose green channel is the plane -- the header-less stream that
    follows a VP8L chunk's five-byte header."""
    w, h = frame_of.size
    buf = io.BytesIO()
    frame_of.save(buf, "WEBP", quality=70)
    stored = forward_filter(filter_method, plane, w, h)
    if compressed:
        green = Image.frombytes("RGBA", (w, h), bytes(v for g in stored for v in (0, g, 0, 255)))
        lossless = io.BytesIO()
        green.save(lossless, "WEBP", lossless=True, exact=True, method=4)
        payload = chunk(lossless.getvalue(), b"VP8L")[5:]
    else:
        payload = stored
    header = (1 if compressed else 0) | filter_method << 2
    return riff([vp8x(w, h, ALPHA_FLAG), (b"ALPH", bytes([header]) + payload), (b"VP8 ", chunk(buf.getvalue(), b"VP8 "))])


def rewritten(base: bytes, **changes) -> bytes:
    """`base`'s frame with its first partition rewritten by `vp8rewrite`."""
    frame = vp8rewrite.read_frame(chunk(base, b"VP8 "))
    for key, value in changes.items():
        assert key in frame, key
        frame[key] = value
    if not (frame["segmentation"] and frame["update_map"]):
        for mb in frame["mbs"]:
            mb["segment"] = None
    return riff([(b"VP8 ", vp8rewrite.write_frame(frame))])


def libvpx(img: Image.Image, *options: str) -> bytes:
    """A key frame from libvpx (through ffmpeg), in a simple-format WebP."""
    w, h = img.size
    with tempfile.TemporaryDirectory() as scratch:
        ivf = pathlib.Path(scratch) / "frame.ivf"
        subprocess.run(
            ["ffmpeg", "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{w}x{h}",
             "-i", "-", "-frames:v", "1", "-c:v", "libvpx", "-pix_fmt", "yuv420p", *options,
             "-f", "ivf", str(ivf)],
            input=img.convert("RGB").tobytes(),
            check=True,
        )
        data = ivf.read_bytes()
    # An IVF file: a 32-byte header, then each frame after a 12-byte header
    # whose first four bytes are its size.
    size = struct.unpack("<I", data[32:36])[0]
    return riff([(b"VP8 ", data[44 : 44 + size])])


def lossless_fixtures() -> None:
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


def lossy_fixtures() -> None:
    small = photo(61, 37)
    # Pillow: libwebp's defaults.
    pillow("webp_lossy_photo", photo(97, 61), quality=75)
    pillow("webp_lossy_q100", small, quality=100)
    pillow("webp_lossy_q5", small, quality=5)
    pillow("webp_lossy_noise", noise(48, 32, 1), quality=100, method=6)
    pillow("webp_lossy_big", photo(131, 97), quality=85, method=6)
    pillow("webp_lossy_odd", photo(17, 9), quality=80)
    pillow("webp_lossy_even", photo(18, 10), quality=80)
    pillow("webp_lossy_1x1", photo(1, 1), quality=80)
    pillow("webp_lossy_column", photo(1, 23), quality=80)
    pillow("webp_lossy_row", photo(23, 1), quality=80)
    pillow("webp_lossy_alpha", translucent(61, 37), quality=80)
    # Below 100, libwebp quantises the alpha plane and says so in the header.
    pillow("webp_lossy_alpha_levels", translucent(61, 37), quality=80, alpha_quality=30)

    # libwebp, with settings Pillow does not expose.
    keep("webp_lossy_simple", libwebp(small, filter_type=0, filter_strength=40))
    keep("webp_lossy_unfiltered", libwebp(small, filter_strength=0))
    keep("webp_lossy_sharp", libwebp(small, filter_sharpness=7, filter_strength=80))
    keep("webp_lossy_one_segment", libwebp(small, segments=1))
    keep("webp_lossy_partitions", libwebp(photo(61, 150), method=2, partitions=3))
    keep("webp_lossy_skip", libwebp(patches(97, 61), quality=90, method=2, partitions=1))
    keep("webp_lossy_alpha_raw", libwebp(translucent(61, 37), alpha_compression=0, alpha_filtering=1))

    # By hand: every alpha filter, raw and compressed.
    w, h = 29, 19
    plane = bytes((x * 5 + y * 11 + (x * y) % 17) & 0xFF for y in range(h) for x in range(w))
    base = photo(w, h)
    for method, filter_name in enumerate(["none", "horizontal", "vertical", "gradient"]):
        keep(f"webp_alpha_raw_{filter_name}", with_alpha(base, plane, method, compressed=False))
        keep(f"webp_alpha_lossless_{filter_name}", with_alpha(base, plane, method, compressed=True))

    # By hand: header settings no encoder writes, on libwebp's macroblocks
    # (four segments, some subblock-predicted).
    frame = libwebp(small, quality=60)
    keep(
        "webp_lossy_segment_deltas",
        rewritten(
            frame,
            absolute=0,
            segment_quant=[-10, None, 7, 30],
            # Segment 3 lands above 63 before the frame's delta brings it
            # back: libwebp clamps once, after every adjustment.
            segment_filter=[-5, None, 12, 40],
            level=50,
            lf_deltas=1,
            lf_update=1,
            ref_deltas=[-30, None, None, None],
            mode_deltas=[9, None, None, None],
        ),
    )
    keep("webp_lossy_segments_unmapped", rewritten(frame, update_map=0))
    keep("webp_lossy_segments_unvalued", rewritten(frame, update_data=0))
    keep(
        "webp_lossy_strong",
        rewritten(
            frame,
            segmentation=0,
            level=63,
            sharpness=0,
            lf_deltas=1,
            lf_update=1,
            ref_deltas=[-20, 5, None, -7],
            mode_deltas=[20, None, 3, None],
        ),
    )
    keep("webp_lossy_quant_deltas", rewritten(frame, quant_deltas=[-6, 5, -4, 3, -2]))
    keep("webp_lossy_colour_space", rewritten(frame, colour_space=1, clamping=1))

    # Corrupt files, whose right answer is still libwebp's. Two bits flipped
    # in `webp_lossy_odd`: one in its first partition (it now updates
    # different probabilities and codes a skip flag), one making the first
    # byte of its coefficients 0xFF, which no encoder writes -- the coder's
    # value then exceeds its range, and libwebp's way of reading a
    # coefficient's sign decides what the picture becomes.
    odd = bytearray((HERE / "webp_lossy_odd.webp").read_bytes())
    odd[58] ^= 0x02
    odd[74] ^= 0x01
    keep("webp_lossy_corrupt_coder", bytes(odd))
    # `webp_lossy_photo` with its frame cut by three bytes and its chunk
    # sizes made to match: odd-length now, the frame's decoder reads the
    # chunk's padding byte where its data ran out, as libwebp's demuxer
    # hands it, and the picture decodes.
    whole = (HERE / "webp_lossy_photo.webp").read_bytes()
    size = struct.unpack("<I", whole[16:20])[0] - 3
    keep("webp_lossy_short_by_padding", riff([(b"VP8 ", whole[20 : 20 + size])]))

    # libvpx: a different encoder's habits. Its quantiser held coarse enough
    # that its loop filter is on -- a lone key frame otherwise gets so fine a
    # one that the filter level is zero -- so that its per-mode filter deltas
    # are used; and its coefficients in four partitions.
    keep("webp_lossy_libvpx", libvpx(photo(131, 97), "-qmin", "45", "-qmax", "50", "-b:v", "1M", "-slices", "4"))


def main() -> None:
    lossless_fixtures()
    lossy_fixtures()


if __name__ == "__main__":
    main()
