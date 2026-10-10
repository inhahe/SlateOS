#!/usr/bin/env python3
"""Generate imagecodec's HDR AVIF fixtures, and Chrome's own pixels for them.

`tests/avif_hdr.rs` holds imagecodec's pixels for each NAME.avif to
NAME.chrome.png: a screenshot of the picture as Chrome itself shows it, on an
sRGB screen (design-decisions 1378). Not a transcription of Chrome's
arithmetic -- Chrome, run here headless.

- **avifhdr_pq.avif**: 24 patches of 8x8, lossless 10-bit 4:4:4, PQ over
  BT.2020 at the studio range: greys from 0 to 10 000 cd/m2 and colours, no
  light metadata (so the content's peak is taken as 1000 cd/m2).
- **avifhdr_pq_clli.avif**: the same with a `clli` box saying MaxCLL 4000
  (written in here: ffmpeg's AVIF muxer writes none), which Chrome's decoder
  reads -- checked: its pixels are the 4000 cd/m2 tone map's, and not the
  1000 one's.
- **avifhdr_hlg.avif**: 16 patches of HLG, the same way.
- **avifhdr_pq_420.avif**: 64x32 of noise at 4:2:0, every chroma sample
  different from its neighbours, so that every pixel's colour depends on how
  the chroma is brought up.

Run from this directory, on Windows, with gyan.dev's ffmpeg (libaom) and
Chrome (154 when these were made; the version is printed):

    python generate_avif_hdr.py [--chrome PATH]
"""

import argparse
import os
import random
import struct
import subprocess
import sys
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "..", "..", "video", "codec", "tests", "data"))
import chrome_hdr  # noqa: E402  (the reference the clli check is made by)

PATCH = 8
CHROME = "C:/Program Files/Google/Chrome/Application/chrome.exe"

M1, M2 = 2610 / 16384, 2523 / 4096 * 128
C1, C2, C3 = 3424 / 4096, 2413 / 4096 * 32, 2392 / 4096 * 32


def pq_inverse(nits):
    y = max(nits, 0) / 10000
    return ((C1 + C2 * y ** M1) / (1 + C3 * y ** M1)) ** M2


def to_yuv10(rgb):
    """R'G'B' (0..1) to 10-bit studio-range Y'CbCr, BT.2020 NCL."""
    kr, kb = 0.2627, 0.0593
    r, g, b = rgb
    y = kr * r + (1 - kr - kb) * g + kb * b
    cb = (b - y) / (2 * (1 - kb))
    cr = (r - y) / (2 * (1 - kr))
    return (round(64 + 876 * y), round(512 + 896 * cb), round(512 + 896 * cr))


def patches(transfer):
    """(label, R'G'B') for each patch."""
    out = []
    if transfer == 16:
        for nits in (0, 0.1, 1, 5, 20, 50, 100, 150, 203, 300, 500, 700, 1000, 2000, 4000, 10000):
            e = pq_inverse(nits)
            out.append((f"grey {nits}", (e, e, e)))
        for name, rgb in (("red", (1000, 0, 0)), ("green", (0, 1000, 0)), ("blue", (0, 0, 1000)),
                          ("red100", (100, 0, 0)), ("skin", (180, 110, 80)), ("sky", (60, 120, 300)),
                          ("yellow", (800, 700, 0)), ("teal", (0, 300, 280))):
            out.append((name, tuple(pq_inverse(v) for v in rgb)))
    else:
        for e in (0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.75, 0.8, 0.9, 1.0):
            out.append((f"grey {e}", (e, e, e)))
        for name, rgb in (("red", (0.75, 0, 0)), ("green", (0, 0.75, 0)), ("blue", (0, 0, 0.75)),
                          ("skin", (0.7, 0.55, 0.45))):
            out.append((name, rgb))
    return out


def encode(planes, width, height, pix_fmt, transfer, path):
    """Lossless 10-bit AV1 in AVIF, BT.2020, studio range -- checked lossless."""
    raw = b"".join(struct.pack(f"<{len(row)}H", *row) for plane in planes for row in plane)
    trc = {16: "smpte2084", 18: "arib-std-b67"}[transfer]
    subprocess.run(["ffmpeg", "-hide_banner", "-v", "error", "-y", "-f", "rawvideo",
                    "-pix_fmt", pix_fmt, "-s", f"{width}x{height}", "-i", "-", "-vf",
                    f"setparams=color_trc={trc}:color_primaries=bt2020:colorspace=bt2020nc:range=tv",
                    "-c:v", "libaom-av1", "-aom-params", "lossless=1", "-still-picture", "1",
                    "-color_primaries", "bt2020", "-color_trc", trc, "-colorspace", "bt2020nc",
                    "-color_range", "tv", path], input=raw, check=True)
    back = subprocess.run(["ffmpeg", "-hide_banner", "-v", "error", "-i", path, "-f", "rawvideo",
                           "-pix_fmt", pix_fmt, "-"], capture_output=True, check=True).stdout
    if back != raw:
        raise SystemExit(f"{path}: not lossless")


def patch_planes(transfer):
    yuvs = [to_yuv10(rgb) for _, rgb in patches(transfer)]
    width = PATCH * len(yuvs)
    planes = []
    for k in range(3):
        row = []
        for yuv in yuvs:
            row += [yuv[k]] * PATCH
        planes.append([row] * PATCH)
    return planes, width, PATCH


# --- a clli box, written into the item's properties ---------------------------

def boxes(data, start, end):
    """(kind, offset, size, header) of each box in data[start:end]."""
    out = []
    while start < end:
        size, kind = struct.unpack(">I4s", data[start:start + 8])
        out.append((kind, start, size, 8))
        start += size
    return out


def add_clli(data, max_cll, max_pall):
    """`data` with a `clli` property (MaxCLL, MaxPALL) in `ipco`, associated
    with the primary item in `ipma`; the boxes around them grown, and `iloc`'s
    offsets into `mdat` (which follows `meta`) moved by as much."""
    top = boxes(data, 0, len(data))
    meta = next(b for b in top if b[0] == b"meta")
    mdat = next(b for b in top if b[0] == b"mdat")
    if mdat[1] < meta[1]:
        raise SystemExit("mdat before meta: not handled")
    inner = boxes(data, meta[1] + 12, meta[1] + meta[2])
    iprp = next(b for b in inner if b[0] == b"iprp")
    iloc = next(b for b in inner if b[0] == b"iloc")
    pitm = next(b for b in inner if b[0] == b"pitm")
    primary = struct.unpack(">H", data[pitm[1] + 12:pitm[1] + 14])[0]
    ipco, ipma = boxes(data, iprp[1] + 8, iprp[1] + iprp[2])
    index = len(boxes(data, ipco[1] + 8, ipco[1] + ipco[2])) + 1
    clli = struct.pack(">I4sHH", 12, b"clli", max_cll, max_pall)
    # ipma: version 0 and flags 0 -- item IDs of 16 bits, one-byte associations.
    version, flags = data[ipma[1] + 8], int.from_bytes(data[ipma[1] + 9:ipma[1] + 12], "big")
    if version != 0 or flags & 1:
        raise SystemExit("ipma version or flags not handled")
    body = bytearray(data[ipma[1] + 16:ipma[1] + ipma[2]])
    entries = struct.unpack(">I", data[ipma[1] + 12:ipma[1] + 16])[0]
    at = 0
    for _ in range(entries):
        item, count = struct.unpack(">HB", body[at:at + 3])
        if item == primary:
            body[at + 2] = count + 1
            body[at + 3 + count:at + 3 + count] = bytes([index])  # not essential
            break
        at += 3 + count
    else:
        raise SystemExit("the primary item has no ipma entry")
    new_ipma = (struct.pack(">I4s", 16 + len(body), b"ipma") + data[ipma[1] + 8:ipma[1] + 16]
                + bytes(body))
    grow = len(clli) + len(new_ipma) - ipma[2]
    new_ipco = (struct.pack(">I4s", ipco[2] + len(clli), b"ipco")
                + data[ipco[1] + 8:ipco[1] + ipco[2]] + clli)
    new_iprp = struct.pack(">I4s", iprp[2] + grow, b"iprp") + new_ipco + new_ipma
    # iloc: version 0, base_offset of 0 bytes; each extent's offset moved.
    il = bytearray(data[iloc[1]:iloc[1] + iloc[2]])
    if il[8] != 0:
        raise SystemExit("iloc version not handled")
    offset_size, length_size = il[12] >> 4, il[12] & 15
    base_size = il[13] >> 4
    count = struct.unpack(">H", il[14:16])[0]
    at = 16
    for _ in range(count):
        at += 2 + 2  # item_ID, data_reference_index
        base = int.from_bytes(il[at:at + base_size], "big") if base_size else 0
        if base_size:
            il[at:at + base_size] = (base + grow).to_bytes(base_size, "big")
        at += base_size
        extents = struct.unpack(">H", il[at:at + 2])[0]
        at += 2
        for _ in range(extents):
            off = int.from_bytes(il[at:at + offset_size], "big")
            if not base_size:
                il[at:at + offset_size] = (off + grow).to_bytes(offset_size, "big")
            at += offset_size + length_size
    new_meta_body = b""
    for kind, off, size, _ in inner:
        if kind == b"iprp":
            new_meta_body += new_iprp
        elif kind == b"iloc":
            new_meta_body += bytes(il)
        else:
            new_meta_body += data[off:off + size]
    new_meta = struct.pack(">I4s", meta[2] + grow, b"meta") + data[meta[1] + 8:meta[1] + 12] + new_meta_body
    return data[:meta[1]] + new_meta + data[meta[1] + meta[2]:]


# --- Chrome -----------------------------------------------------------------------

def read_png(path):
    data = open(path, "rb").read()
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
        rows.append([line[i * bpp:i * bpp + 3] for i in range(width)])
        prev = line
    return rows


def write_png(path, rows):
    """8-bit RGB, no other chunks: no gamma, no profile."""
    def chunk(kind, data):
        return (struct.pack(">I", len(data)) + kind + data
                + struct.pack(">I", zlib.crc32(kind + data) & 0xffffffff))
    height, width = len(rows), len(rows[0])
    raw = b"".join(b"\0" + b"".join(bytes(px) for px in row) for row in rows)
    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)))
        f.write(chunk(b"IDAT", zlib.compress(raw, 9)))
        f.write(chunk(b"IEND", b""))


def chrome_pixels(chrome, avif, width, height):
    """`avif` as headless Chrome shows it, at one pixel to a pixel, on an sRGB
    screen: its screenshot, cut to the picture."""
    html = os.path.join(HERE, "tmp_avif_hdr.html")
    shot = os.path.join(HERE, "tmp_avif_hdr.png")
    with open(html, "w", encoding="utf-8") as f:
        f.write(f'<html><body style="margin:0;background:#000">'
                f'<img src="{os.path.basename(avif)}" width="{width}" height="{height}" '
                f'style="image-rendering:pixelated;display:block"></body></html>')
    if os.path.exists(shot):
        os.remove(shot)
    subprocess.run([chrome, "--headless=new", f"--screenshot={shot}",
                    f"--window-size={width},{height + 40}", "--hide-scrollbars",
                    "--force-device-scale-factor=1", "--force-color-profile=srgb",
                    "--allow-file-access-from-files", "file:///" + html.replace(os.sep, "/")],
                   check=True, capture_output=True, timeout=120)
    rows = read_png(shot)[:height]
    os.remove(shot)
    os.remove(html)
    return [row[:width] for row in rows]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--chrome", default=CHROME)
    a = ap.parse_args()
    version = subprocess.run(["powershell", "-NoProfile", "-Command",
                              f"(Get-Item '{a.chrome}').VersionInfo.ProductVersion"],
                             capture_output=True, text=True).stdout.strip()
    print("Chrome", version)

    made = []
    for name, transfer in (("avifhdr_pq", 16), ("avifhdr_hlg", 18)):
        planes, width, height = patch_planes(transfer)
        encode(planes, width, height, "yuv444p10le", transfer, f"{name}.avif")
        made.append((name, width, height))

    planes, width, height = patch_planes(16)
    with open("avifhdr_pq.avif", "rb") as f:
        data = f.read()
    with open("avifhdr_pq_clli.avif", "wb") as f:
        f.write(add_clli(data, 4000, 1000))
    made.append(("avifhdr_pq_clli", width, height))

    rnd = random.Random(7)
    w, h = 64, 32
    y = [[rnd.randrange(300, 800) for _ in range(w)] for _ in range(h)]
    u = [[rnd.randrange(350, 680) for _ in range(w // 2)] for _ in range(h // 2)]
    v = [[rnd.randrange(350, 680) for _ in range(w // 2)] for _ in range(h // 2)]
    encode([y, u, v], w, h, "yuv420p10le", 16, "avifhdr_pq_420.avif")
    made.append(("avifhdr_pq_420", w, h))

    for name, width, height in made:
        rows = chrome_pixels(a.chrome, f"{name}.avif", width, height)
        write_png(f"{name}.chrome.png", rows)
        print("wrote", name)

    # Chrome read the clli: the patches are the 4000 cd/m2 tone map's.
    rows = read_png("avifhdr_pq_clli.chrome.png")
    for peak, want_match in ((4000, True), (1000, False)):
        points = chrome_hdr.rwtmo_alt0(chrome_hdr.baseline_headroom(peak), half=True)
        same = 0
        for k, (_, rgb) in enumerate(patches(16)):
            yuv = to_yuv10(rgb)
            kr, kb = 0.2627, 0.0593
            yv, cb, cr = (yuv[0] - 64) / 876, (yuv[1] - 512) / 896, (yuv[2] - 512) / 896
            r = yv + 2 * (1 - kr) * cr
            b = yv + 2 * (1 - kb) * cb
            g = yv - 2 * (kr * (1 - kr) * cr + kb * (1 - kb) * cb) / (1 - kr - kb)
            want = chrome_hdr.pixel([min(max(c, 0.0), 1.0) for c in (r, g, b)], 16, points)
            same += list(rows[PATCH // 2][k * PATCH + PATCH // 2]) == want
        print(f"clli check: {same} of {len(patches(16))} patches are the {peak} cd/m2 map's")
        if (same == len(patches(16))) != want_match:
            raise SystemExit("Chrome did not read the clli box as written")


if __name__ == "__main__":
    os.chdir(HERE)
    main()
