"""Ordinary (SDR) video whose colour Chrome converts, and Chrome's pixels
for each one's first frame: `tests/sdr.rs`'s fixtures and answers
(design-decisions 1381).

Two kinds:

* Fixtures made here, each a 256 x 128 frame of 512 8 x 8 patches of
  fixed pseudo-random Y'CbCr codes, lossless 4:4:4 at 8 bits -- so that
  every sample is the one written -- whose colour is said in the bitstream
  (VP9's colour space, AV1's sequence header), in the file (Matroska's
  `Colour`, by mkvmerge), both, or in pieces: which place Chrome takes a
  colour from, and when it converts it, as `src/colour.rs` has it. The
  bitstream is written to IVF first, so that the file says only what
  mkvmerge is told to.
* `generate_fixtures.py`'s fixtures whose colour Chrome converts (BT.601's
  and SMPTE 240M's primaries, said by VP9's colour space; BT.2020's, by an
  MP4 `colr`): their 4:2:0 and 4:4:4 pictures, as `frames.rs` plays them.
  Not `vp9_10bit_bt2020.webm`: headless Chrome cuts 10-bit video to 8 bits
  on its way to the screen, which a GPU does not (design-decisions 1379).

Each answer is Chrome's own pixels: the file in a <video> at its size,
paused at its first frame, in headless Chrome (154, 2026-10-10), the
frame's rectangle of the screenshot kept as NAME.chrome.png.

Run with no arguments to remake everything (needs Chrome, ffmpeg with
libvpx and libaom, and mkvmerge); with names to remake those.
"""
import os
import struct
import subprocess
import sys
import tempfile
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
CHROME = "C:/Program Files/Google/Chrome/Application/chrome.exe"
MKVMERGE = "D:/utils/mkvtoolnix-99.0/mkvmerge.exe"
PATCH, COLS, ROWS = 8, 32, 16

VP9 = ["-c:v", "libvpx-vp9", "-lossless", "1", "-profile:v", "1"]
AV1 = ["-c:v", "libaom-av1", "-aom-params", "lossless=1", "-cpu-used", "8"]

# name: (encoder, the bitstream's colour (setparams), the file's
# (mkvmerge: primaries, transfer, matrix, range), why)
MADE = {
    "sdr_vp9_bt601.webm": (
        VP9, "colorspace=bt470bg", None,
        "VP9's BT.601, nothing in the file: SMPTE 170M's three, converted"),
    "sdr_vp9_bt709_file_bt601.webm": (
        VP9, "colorspace=bt709", (6, 6, 6, 1),
        "the file's whole BT.601 over VP9's BT.709: libvpx's decoder asks the file first"),
    "sdr_vp9_bt709_file_pieces.webm": (
        VP9, "colorspace=bt709", (6, None, None, None),
        "VP9's BT.709 over the file's lone primaries: not converted"),
    "sdr_file_p3.webm": (
        VP9, None, (12, 1, 1, 2),
        "Display P3's primaries, said whole by the file"),
    "sdr_file_bt2020.webm": (
        VP9, None, (9, 14, 9, 1),
        "BT.2020, not HDR, said whole by the file"),
    "sdr_file_gamma22.webm": (
        VP9, None, (1, 4, 1, 2),
        "a power of 2.2, said whole by the file"),
    "sdr_av1_bt601_file_bt709.webm": (
        AV1, "color_primaries=smpte170m:color_trc=smpte170m:colorspace=smpte170m:range=tv",
        (1, 1, 1, 1),
        "AV1's whole BT.601 over the file's BT.709: dav1d's decoder asks the bitstream first"),
    "sdr_av1_file_bt601.webm": (
        AV1, None, (6, 6, 6, 1),
        "AV1 saying nothing, the file BT.601 whole"),
}

# generate_fixtures.py's fixtures Chrome converts: name -> (width, height).
PLAYED = {
    "vp9_full_range.webm": (176, 144),
    "vp9_444.webm": (176, 144),
    "vp9_smpte240.webm": (176, 144),
    "vp9_colr.mp4": (176, 144),
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


def make(name, encoder, bitstream, file_tags, work):
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
    raw = b"".join(planes) * 3
    ivf = os.path.join(work, "stream.ivf")
    args = ["ffmpeg", "-hide_banner", "-v", "error", "-y", "-f", "rawvideo",
            "-pix_fmt", "yuv444p", "-s", f"{w}x{h}", "-r", "10", "-i", "-"]
    if bitstream:
        # Tags only: -colorspace and the like as output options make
        # ffmpeg convert the samples.
        args += ["-vf", "setparams=" + bitstream]
    subprocess.run(args + encoder + ["-f", "ivf", ivf], input=raw, check=True)
    margs = [MKVMERGE, "-q", "--webm", "-o", os.path.join(HERE, name)]
    if file_tags:
        flags = ("--colour-primaries", "--colour-transfer-characteristics",
                 "--colour-matrix-coefficients", "--colour-range")
        for flag, value in zip(flags, file_tags):
            if value is not None:
                margs += [flag, f"0:{value}"]
    subprocess.run(margs + [ivf], check=True)
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


def screenshot(name, w, h, work):
    html = os.path.join(HERE, "chrome_sdr_page.html")
    with open(html, "w", encoding="utf-8") as f:
        f.write(f'<html><body style="margin:0;background:#000">'
                f'<video src="{name}" width="{w}" height="{h}" muted preload="auto" '
                f'style="display:block;object-fit:fill"></video></body></html>')
    shot = os.path.join(work, "shot.png")
    try:
        subprocess.run([CHROME, "--headless=new", f"--screenshot={shot}",
                        f"--window-size={max(w, 128)},{h + 40}", "--hide-scrollbars",
                        "--force-device-scale-factor=1", "--force-color-profile=srgb",
                        "--virtual-time-budget=8000", "--allow-file-access-from-files",
                        "file:///" + html.replace(os.sep, "/")],
                       check=True, capture_output=True, timeout=180)
    finally:
        os.remove(html)
    bpp, rows = read_png(shot)
    crop = []
    for y in range(h):
        row = rows[y]
        crop.append(b"".join(row[x * bpp:x * bpp + 3] for x in range(w)))
    write_png(os.path.join(HERE, f"{name}.chrome.png"), w, h, crop)


def main():
    only = sys.argv[1:]
    with tempfile.TemporaryDirectory() as work:
        for name, (encoder, bitstream, file_tags, why) in MADE.items():
            if only and name not in only:
                continue
            w, h = make(name, encoder, bitstream, file_tags, work)
            screenshot(name, w, h, work)
            print(f"{name}: {why}", flush=True)
        for name, (w, h) in PLAYED.items():
            if only and name not in only:
                continue
            screenshot(name, w, h, work)
            print(f"{name}: Chrome's first frame", flush=True)


if __name__ == "__main__":
    main()
