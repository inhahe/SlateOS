"""Chrome's pixels for ordinary (SDR) video whose colour Chrome converts:
the fixture `src/managed.rs`'s tests hold the SDR conversion to.

Each tagging is one lossless 8-bit 4:4:4 VP9 frame -- a 64 x 32 grid of
8 x 8 patches, each a fixed pseudo-random Y'CbCr triple (the same 2048
every time, a few greys and extremes first) -- whose colour is said, all
four parts of it, by Matroska's `Colour` (mkvmerge), which Chrome takes
over VP9's own field when all four are given.  Shown paused at its first
frame in headless Chrome, screenshotted, and each patch's centre read.

Measured with Chrome 154 (2026-10-10).  Its pixels are those of a
floating-point conversion -- Y'CbCr to R'G'B', clamped; the transfer's
curve (Chrome's `GetTransferFunction`: the sRGB curve for BT.709's,
BT.601's and BT.2020's); the primaries to sRGB's (skcms, Bradford); the
sRGB curve's inverse; 8 bits -- to within one level everywhere, and exactly
in 94-98% of channels: Chrome's own arithmetic rounds a few values within
a twentieth of a level of one half the other way.

Run with no arguments to remeasure (needs Chrome, ffmpeg with libvpx and
mkvmerge); `--from-json FILE` rewrites the fixture from a saved
measurement.  Writes chrome_sdr.bin beside this file:

    b"CSDR", version (u8, 1), codes (u16 LE), taggings (u8),
    per tagging: primaries, transfer, matrix, range (u8 each; range 1 is
                 the studio range and 2 the full one, as Matroska says),
    codes x [Y, Cb, Cr] (u8),
    per tagging: codes x [R, G, B] (u8), Chrome's.
"""
import json
import os
import struct
import subprocess
import sys
import tempfile
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "chrome_sdr.bin")
CHROME = "C:/Program Files/Google/Chrome/Application/chrome.exe"
MKVMERGE = "D:/utils/mkvtoolnix-99.0/mkvmerge.exe"
PATCH, COLS, ROWS = 8, 64, 32

# primaries, transfer, matrix, range -- each said, as Chrome needs.
TAGGINGS = [
    (1, 1, 1, 2),    # BT.709 at full range: no conversion, the baseline
    (6, 6, 6, 1),    # BT.601, 525 lines (SMPTE 170M's primaries)
    (5, 6, 5, 1),    # BT.601, 625 lines (BT.470 B and G's primaries)
    (9, 14, 9, 1),   # BT.2020, not HDR
    (12, 1, 1, 2),   # Display P3's primaries
    (1, 4, 1, 2),    # a power of 2.2
    (1, 8, 1, 2),    # linear
]


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


def measure(tags, cs, work):
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
    raw = b"".join(planes) * 3
    tmp = os.path.join(work, "frame.webm")
    video = os.path.join(work, "tagged.webm")
    subprocess.run(["ffmpeg", "-hide_banner", "-v", "error", "-y", "-f", "rawvideo",
                    "-pix_fmt", "yuv444p", "-s", f"{w}x{h}", "-r", "10", "-i", "-",
                    "-c:v", "libvpx-vp9", "-lossless", "1", "-profile:v", "1", tmp],
                   input=raw, check=True)
    p, t, m, rng = tags
    subprocess.run([MKVMERGE, "-q", "--webm", "-o", video,
                    "--colour-primaries", f"0:{p}", "--colour-transfer-characteristics", f"0:{t}",
                    "--colour-matrix-coefficients", f"0:{m}", "--colour-range", f"0:{rng}", tmp],
                   check=True)
    html = os.path.join(work, "video.html")
    with open(html, "w", encoding="utf-8") as f:
        f.write(f'<html><body style="margin:0;background:#000">'
                f'<video src="tagged.webm" width="{w}" height="{h}" muted preload="auto" '
                f'style="display:block;object-fit:fill"></video></body></html>')
    shot = os.path.join(work, "shot.png")
    subprocess.run([CHROME, "--headless=new", f"--screenshot={shot}", f"--window-size={w},{h + 40}",
                    "--hide-scrollbars", "--force-device-scale-factor=1", "--force-color-profile=srgb",
                    "--virtual-time-budget=8000", "--allow-file-access-from-files",
                    "file:///" + html.replace(os.sep, "/")],
                   check=True, capture_output=True, timeout=180)
    bpp, rows = read_png(shot)
    got = []
    for r in range(ROWS):
        y = r * PATCH + PATCH // 2
        for c in range(COLS):
            x = c * PATCH + PATCH // 2
            got.append(list(rows[y][x * bpp:x * bpp + 3]))
    return got


def write(cs, results):
    out = bytearray(b"CSDR")
    out += struct.pack("<BHB", 1, len(cs), len(TAGGINGS))
    for tags in TAGGINGS:
        out += bytes(tags)
    for code in cs:
        out += bytes(code)
    for tags in TAGGINGS:
        for px in results[tags]:
            out += bytes(px)
    with open(OUT, "wb") as f:
        f.write(out)
    print(f"wrote {OUT}: {len(out)} bytes")


def main():
    cs = codes()
    if len(sys.argv) == 3 and sys.argv[1] == "--from-json":
        with open(sys.argv[2], encoding="utf-8") as f:
            saved = json.load(f)
        assert saved["codes"] == cs, "the measurement is of other codes"
        results = {t: saved["p{}_t{}_m{}_r{}".format(*t)] for t in TAGGINGS}
    else:
        results = {}
        with tempfile.TemporaryDirectory() as work:
            for tags in TAGGINGS:
                results[tags] = measure(tags, cs, work)
                print("measured", tags, flush=True)
    write(cs, results)


if __name__ == "__main__":
    main()
