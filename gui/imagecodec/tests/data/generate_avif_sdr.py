#!/usr/bin/env python3
"""Generate imagecodec's SDR AVIF fixtures whose colour Chrome converts, and
Chrome's own pixels for them (design-decisions 1381).

`tests/avif_sdr.rs` holds imagecodec's pixels for each NAME.avif to
NAME.chrome.png: a screenshot of the picture as Chrome itself shows it, on
an sRGB screen. Chrome decodes an AVIF to its Y'CbCr planes and converts
them on the GPU as it does video -- in floating point, then its colour
conversion to the screen's -- which a probe of 2048 codes under eight
taggings showed (2026-10-10, Chrome 154): its pixels are that model's to
within one level, and not libavif's 8-bit RGB's.

Each fixture is 256 x 128: 512 patches of 8 x 8, each a fixed
pseudo-random Y'CbCr triple (the first few greys and extremes), lossless
8-bit 4:4:4, its `nclx` box saying:

- **avifsdr_bt2020.avif**: BT.2020, studio range, BT.2020's 10-bit curve
  (the sRGB curve to Chrome).
- **avifsdr_p3.avif**: Display P3's primaries, the sRGB curve, BT.709's
  matrix, full range -- a phone's.
- **avifsdr_bt601.avif**: BT.601 (SMPTE 170M), studio range.
- **avifsdr_gamma22.avif**: BT.709's primaries, a power of 2.2.
- **avifsdr_bt2020_unsaid_curve.avif**: BT.2020's primaries and matrix, the
  transfer unspecified: MIAF's default, the sRGB curve -- still converted.
- **avifsdr_bt709.avif**: BT.709 throughout, studio range: not converted,
  libavif's own conversion (whose blue weight libyuv caps, up to 15 levels
  from Chrome's floating point in saturated blues and yellows).

Run from this directory, on Windows, with gyan.dev's ffmpeg (libaom) and
Chrome (154 when these were made):

    python generate_avif_sdr.py [--chrome PATH]
"""

import argparse
import os
import struct
import subprocess
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
PATCH, COLS, ROWS = 8, 32, 16

# name: (setparams/encoder names: primaries, transfer (filter, encoder),
# matrix, range)
FIXTURES = {
    "avifsdr_bt2020": ("bt2020", ("bt2020-10", "bt2020-10"), "bt2020nc", "tv"),
    "avifsdr_p3": ("smpte432", ("iec61966-2-1", "iec61966-2-1"), "bt709", "pc"),
    "avifsdr_bt601": ("smpte170m", ("smpte170m", "smpte170m"), "smpte170m", "tv"),
    "avifsdr_gamma22": ("bt709", ("bt470m", "gamma22"), "bt709", "pc"),
    "avifsdr_bt2020_unsaid_curve": ("bt2020", ("unknown", "unknown"), "bt2020nc", "pc"),
    "avifsdr_bt709": ("bt709", ("bt709", "bt709"), "bt709", "tv"),
}


def codes():
    seed = 12345
    out = []
    for _ in range(COLS * ROWS):
        t = []
        for _ in range(3):
            seed = (seed * 1103515245 + 12345) & 0x7FFFFFFF
            t.append(seed >> 23)
        out.append(t)
    fixed = [[0, 128, 128], [255, 128, 128], [128, 128, 128], [16, 128, 128],
             [235, 128, 128], [128, 0, 0], [128, 255, 255], [128, 0, 255],
             [128, 255, 0], [128, 127, 129], [128, 129, 127]]
    out[:len(fixed)] = fixed
    return out


def encode(name, tags):
    cs = codes()
    w, h = PATCH * COLS, PATCH * ROWS
    planes = []
    for plane in range(3):
        rows = []
        for r in range(ROWS):
            line = bytearray()
            for c in range(COLS):
                line += bytes([cs[r * COLS + c][plane]]) * PATCH
            rows.append(bytes(line) * PATCH)
        planes.append(b"".join(rows))
    raw = b"".join(planes)
    prim, (trc_filter, trc_encoder), mat, rng = tags
    path = os.path.join(HERE, f"{name}.avif")
    subprocess.run(["ffmpeg", "-hide_banner", "-v", "error", "-y", "-f", "rawvideo",
                    "-pix_fmt", "yuv444p", "-s", f"{w}x{h}", "-i", "-", "-vf",
                    f"setparams=color_trc={trc_filter}:color_primaries={prim}:colorspace={mat}"
                    f":range={rng}",
                    "-c:v", "libaom-av1", "-aom-params", "lossless=1", "-still-picture", "1",
                    "-color_primaries", prim, "-color_trc", trc_encoder, "-colorspace", mat,
                    "-color_range", rng, path], input=raw, check=True)
    back = subprocess.run(["ffmpeg", "-hide_banner", "-v", "error", "-i", path, "-f", "rawvideo",
                           "-pix_fmt", "yuv444p", "-"], capture_output=True, check=True).stdout
    if back != raw:
        raise SystemExit(f"{path}: not lossless")
    return w, h


def read_png(path):
    with open(path, "rb") as f:
        data = f.read()
    pos, chunks = 8, []
    width = height = bpp = None
    while pos < len(data):
        n, kind = struct.unpack(">I4s", data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + n]
        if kind == b"IHDR":
            width, height, depth, ctype = struct.unpack(">IIBB", body[:10])
            assert depth == 8 and ctype in (2, 6), (depth, ctype)
            bpp = 3 if ctype == 2 else 4
        elif kind == b"IDAT":
            chunks.append(body)
        pos += 12 + n
    raw = zlib.decompress(b"".join(chunks))
    stride = width * bpp
    rows, prev, at = [], bytearray(stride), 0
    for _ in range(height):
        f = raw[at]
        line = bytearray(raw[at + 1:at + 1 + stride])
        at += 1 + stride
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b = prev[i]
            c = prev[i - bpp] if i >= bpp else 0
            if f == 1:
                line[i] = (line[i] + a) & 255
            elif f == 2:
                line[i] = (line[i] + b) & 255
            elif f == 3:
                line[i] = (line[i] + (a + b) // 2) & 255
            elif f == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        rows.append(bytes(line))
        prev = line
    return bpp, rows


def write_png(path, w, h, rgb_rows):
    raw = b"".join(b"\x00" + row for row in rgb_rows)

    def chunk(kind, body):
        return (struct.pack(">I", len(body)) + kind + body
                + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF))

    png = (b"\x89PNG\r\n\x1a\n"
           + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(raw, 9))
           + chunk(b"IEND", b""))
    with open(path, "wb") as f:
        f.write(png)


def chrome_pixels(chrome, name, w, h):
    html = os.path.join(HERE, "tmp_avif_sdr.html")
    shot = os.path.join(HERE, "tmp_avif_sdr.png")
    with open(html, "w", encoding="utf-8") as f:
        f.write(f'<html><body style="margin:0;background:#000">'
                f'<img src="{name}.avif" width="{w}" height="{h}" style="display:block">'
                f'</body></html>')
    try:
        subprocess.run([chrome, "--headless=new", f"--screenshot={shot}",
                        f"--window-size={w},{h + 40}", "--hide-scrollbars",
                        "--force-device-scale-factor=1", "--force-color-profile=srgb",
                        "--virtual-time-budget=5000", "--allow-file-access-from-files",
                        "file:///" + html.replace(os.sep, "/")],
                       check=True, capture_output=True, timeout=180)
        bpp, rows = read_png(shot)
    finally:
        for p in (html, shot):
            if os.path.exists(p):
                os.remove(p)
    crop = [b"".join(rows[y][x * bpp:x * bpp + 3] for x in range(w)) for y in range(h)]
    write_png(os.path.join(HERE, f"{name}.chrome.png"), w, h, crop)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--chrome", default="C:/Program Files/Google/Chrome/Application/chrome.exe")
    a = ap.parse_args()
    for name, tags in FIXTURES.items():
        w, h = encode(name, tags)
        chrome_pixels(a.chrome, name, w, h)
        print(name, flush=True)


if __name__ == "__main__":
    main()
