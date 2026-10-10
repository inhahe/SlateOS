#!/usr/bin/env python3
"""Generate videocodec's HDR fixtures and their answers.

Each fixture NAME gets NAME.colour: what its video says of its light, frame
by frame, as FFmpeg reads it -- the four H.273 numbers (matrix, primaries,
transfer, range), the mastering display (SMPTE ST 2086: its primaries and
white point, and its peak and black) and the content light level
(CTA-861.3: MaxCLL and MaxFALL). `tests/hdr.rs` holds videocodec's
`Picture::colour` and `Picture::light` to those lines.

And NAME.chrome.png: its first frame as Chrome shows it on an sRGB screen
(design-decisions 1378), which `tests/hdr.rs` holds videocodec's pixels to,
within one. Made from ffmpeg's decoding of the frame (libdav1d's and
FFmpeg's VP9, which videocodec's decoders match bit for bit), its chroma
upsampled as Chrome's GPU samples it (centred bilinear, in floating point),
Y'CbCr to R'G'B' as libavif's floating point computes it, and from there
Chrome's handling of the light, in double precision (`chrome_hdr.py`), by
the light ffprobe says the frame has.

Where the answers come from -- none of it from the crate itself: ffprobe's
`-show_frames`, whose frames carry what the bitstream says (AV1's sequence
header and metadata OBUs, through libdav1d) and, where it says nothing,
what the file says of the track (Matroska's `Colour`, MP4's `colr`, `mdcv`,
`clli`, `SmDm` and `CoLL`), each kind of metadata on its own. A stream
whose colour nothing says has ffprobe's "unknown", and its answer is the
guess `src/colour.rs` makes (as `generate_fixtures.py`'s table states it).

The fixtures:

- **hdr10_av1.mkv**: AV1 (SVT-AV1) in PQ over BT.2020, with HDR10's
  metadata in the bitstream -- and other metadata in Matroska's `Colour`,
  which the bitstream's outranks, kind by kind.
- **cll_av1.mkv**: the same, the bitstream giving only its content light
  level: the file's mastering display stands beside the bitstream's level.
- **tagged_by_file_av1.mkv**: AV1 that says nothing of its colour, the
  file's `Colour` saying PQ over BT.2020.
- **hdr10_vp9.webm**: VP9 profile 2 in PQ; VP9 says nothing of its transfer
  or light, so Matroska's `Colour` says all of it.
- **hdr10_vp9.mp4**: the same in MP4, the light in VP9's own boxes (`SmDm`,
  `CoLL`), written here into the sample entry.
- **hdr10_av1.mp4**: AV1 without metadata OBUs in MP4, the light in `mdcv`
  and `clli`, written here.
- **hlg_vp9.webm** and **hlg_av1.mp4**: HLG, which carries no metadata.
- **partial_vp9.webm**: a `Colour` FFmpeg reads only some of -- a mastering
  luminance without primaries, and a MaxCLL without a MaxFALL (which FFmpeg
  then leaves unsaid).

Run from this directory, on Windows, with gyan.dev's ffmpeg (2026-03-09, git
9b7439c31b) and MKVToolNix 99.0's mkvmerge:

    python generate_hdr_fixtures.py [--ffmpeg DIR] [--mkvmerge PATH]
"""

import argparse
import json
import os
import struct
import subprocess
import sys
import zlib

import chrome_hdr
from generate_fixtures import insert_in_entry

FRAMES = 4
RATE = 25
SIZE = "96x64"
SRC = f"testsrc2=size={SIZE}:rate={RATE}:duration={FRAMES / RATE}"

PQ = "setparams=color_trc=smpte2084:color_primaries=bt2020:colorspace=bt2020nc:range=tv"
HLG = "setparams=color_trc=arib-std-b67:color_primaries=bt2020:colorspace=bt2020nc:range=tv"

SVT = ["-c:v", "libsvtav1", "-preset", "10", "-crf", "40", "-g", "2", "-fflags", "+bitexact"]
VP9 = ["-c:v", "libvpx-vp9", "-profile:v", "2", "-deadline", "good", "-cpu-used", "4",
       "-crf", "40", "-b:v", "0", "-g", "2", "-keyint_min", "2", "-threads", "1",
       "-row-mt", "0", "-tile-columns", "0", "-fflags", "+bitexact"]

# HDR10's usual mastering display: BT.2020's primaries, D65, 1000 cd/m2.
HDR10 = "G(0.17,0.797)B(0.131,0.046)R(0.708,0.292)WP(0.3127,0.329)L(1000,0.0001)"


def run(args, **kw):
    r = subprocess.run(args, capture_output=True, **kw)
    if r.returncode != 0:
        raise SystemExit(f"{args[0]} failed: {r.stderr.decode('utf-8', 'replace')[-2000:]}")
    return r.stdout


class Tools:
    def __init__(self, ffmpeg_dir, mkvmerge):
        exe = lambda n: os.path.join(ffmpeg_dir, n) if ffmpeg_dir else n
        self.ffmpeg, self.ffprobe, self.mkvmerge = exe("ffmpeg"), exe("ffprobe"), mkvmerge

    def encode(self, out, vf, codec_args, extra=()):
        run([self.ffmpeg, "-hide_banner", "-v", "error", "-y", "-f", "lavfi", "-i", SRC,
             "-vf", f"format=yuv420p10le,{vf}", *codec_args, *extra, out])

    def frames(self, name):
        out = run([self.ffprobe, "-v", "error", "-show_frames", "-show_entries",
                   "frame=key_frame,pts,color_range,color_space,color_transfer,"
                   "color_primaries:frame_side_data", "-of", "json", name])
        return json.loads(out)["frames"]


# --- MP4's light boxes, written into the visual sample entry ---------------

def box(kind, payload):
    return struct.pack(">I4s", 8 + len(payload), kind) + payload


def mdcv(primaries_gbr, white, max_lum, min_lum):
    """ISO/IEC 23001-8's mdcv: G, B, R in 0.00002 units, luminances in
    0.0001 cd/m2 (as FFmpeg reads it, `mov_read_mdcv`)."""
    p = b"".join(struct.pack(">HH", round(x / 0.00002), round(y / 0.00002))
                 for x, y in primaries_gbr)
    w = struct.pack(">HH", round(white[0] / 0.00002), round(white[1] / 0.00002))
    return box(b"mdcv", p + w + struct.pack(">II", round(max_lum / 0.0001), round(min_lum / 0.0001)))


def clli(max_cll, max_fall):
    return box(b"clli", struct.pack(">HH", max_cll, max_fall))


def smdm(primaries_rgb, white, max_lum, min_lum):
    """VP9's SmDm, a full box: R, G, B and the white in 0.16, the peak in
    24.8, the black in 18.14 (`mov_read_smdm`)."""
    p = b"".join(struct.pack(">HH", round(x * 65536), round(y * 65536)) for x, y in primaries_rgb)
    w = struct.pack(">HH", round(white[0] * 65536), round(white[1] * 65536))
    lum = struct.pack(">II", round(max_lum * 256), round(min_lum * 16384))
    return box(b"SmDm", b"\0\0\0\0" + p + w + lum)


def coll(max_cll, max_fall):
    return box(b"CoLL", b"\0\0\0\0" + struct.pack(">HH", max_cll, max_fall))


# --- the answers --------------------------------------------------------------

TRANSFERS = {"bt709": 1, "smpte2084": 16, "arib-std-b67": 18, "bt2020-10": 14}
PRIMARIES = {"bt709": 1, "bt2020": 9}
MATRICES = {"bt709": 1, "bt2020nc": 9}


def rational(s):
    num, den = s.split("/")
    return f"{int(num)}/{int(den)}"


def resolved(frame):
    """The frame's colour as src/colour.rs settles it from what ffprobe
    reads: a transfer of BT.2020's guesses BT.2020 for the rest (BT.709's
    where either says it), as Chrome does; otherwise mpv's guess, the matrix
    by the size and the primaries by the matrix."""
    width, height = (int(n) for n in SIZE.split("x"))
    hd = width >= 1280 or height > 576
    said_matrix = MATRICES.get(frame.get("color_space"))
    said_primaries = PRIMARIES.get(frame.get("color_primaries"))
    transfer = TRANSFERS.get(frame.get("color_transfer"), 1)
    if transfer in (14, 15, 16, 18):
        guess = 1 if 1 in (said_matrix, said_primaries) else 9
        matrix, primaries = said_matrix or guess, said_primaries or guess
    else:
        matrix = said_matrix or (1 if hd else 6)
        primaries = said_primaries or {9: 9, 10: 9, 1: 1}.get(
            matrix, 1 if hd else {576: 5, 480: 6, 486: 6}.get(height, 1))
    return matrix, primaries, transfer


def answer(name, frames):
    lines = [f"# {name}: what its frames say of their light, as ffprobe reads them "
             "(generate_hdr_fixtures.py)."]
    for index, f in enumerate(frames):
        matrix, primaries, transfer = resolved(f)
        rng = "full" if f.get("color_range") == "pc" else "limited"
        lines.append(f"frame {index} matrix={matrix} primaries={primaries} "
                     f"transfer={transfer} range={rng}")
        for sd in f.get("side_data_list", []):
            kind = sd["side_data_type"]
            if kind == "Mastering display metadata":
                fields = []
                if "red_x" in sd:
                    fields.append("primaries=" + ",".join(
                        rational(sd[k]) for k in ("red_x", "red_y", "green_x", "green_y",
                                                  "blue_x", "blue_y", "white_point_x",
                                                  "white_point_y")))
                if "max_luminance" in sd:
                    fields.append(f"luminance={rational(sd['max_luminance'])},"
                                  f"{rational(sd['min_luminance'])}")
                lines.append("  mastering " + " ".join(fields))
            elif kind == "Content light level metadata":
                lines.append(f"  light {sd['max_content']},{sd['max_average']}")
    return "\n".join(lines) + "\n"


# --- the pixels, as Chrome shows them -------------------------------------------

def chroma_value(code):
    """A 10-bit studio-range chroma sample as a fraction of its range."""
    return (code - 512) / 896


def upsampled(plane, width, height):
    """4:2:0 chroma at every pixel, as Chrome brings it up on the GPU:
    bilinear sampling sited at the centre of the luma samples each chroma
    sample covers, clamped at the edges -- 9:3:3:1 of the four nearest
    samples' values, in floating point (libavif's slow path, to the letter):
    the nearest, the column and the row next to it towards the pixel, and the
    sample between those; at the picture's edges the nearest again."""
    out = []
    for j in range(height):
        uv_j = j >> 1
        if j == 0 or (j == height - 1 and j % 2 != 0):
            adj_j = uv_j
        else:
            adj_j = uv_j + 1 if j % 2 != 0 else uv_j - 1
        row = []
        for i in range(width):
            uv_i = i >> 1
            if i == 0 or (i == width - 1 and i % 2 != 0):
                adj_i = uv_i
            else:
                adj_i = uv_i + 1 if i % 2 != 0 else uv_i - 1
            row.append(chroma_value(plane[uv_j][uv_i]) * 9 / 16
                       + chroma_value(plane[uv_j][adj_i]) * 3 / 16
                       + chroma_value(plane[adj_j][uv_i]) * 3 / 16
                       + chroma_value(plane[adj_j][adj_i]) * 1 / 16)
        out.append(row)
    return out


def rgb_of(y, cb, cr):
    """A 10-bit studio-range luma sample and its pixel's chroma values, BT.2020
    NCL, to R'G'B' as libavif's floating point computes it, in double
    precision, clamped to [0, 1]."""
    kr, kb = 0.2627, 0.0593
    kg = 1.0 - kr - kb
    yv = (y - 64) / 876
    r = yv + 2 * (1 - kr) * cr
    b = yv + 2 * (1 - kb) * cb
    g = yv - 2 * (kr * (1 - kr) * cr + kb * (1 - kb) * cb) / kg
    return [min(max(c, 0.0), 1.0) for c in (r, g, b)]


def first_frame(t, name):
    """The first frame's 10-bit 4:2:0 planes, as ffmpeg's decoders make them."""
    width, height = (int(n) for n in SIZE.split("x"))
    raw = run([t.ffmpeg, "-hide_banner", "-v", "error", "-i", name, "-frames:v", "1",
               "-f", "rawvideo", "-pix_fmt", "yuv420p10le", "-"])
    cw, ch = (width + 1) // 2, (height + 1) // 2
    samples = struct.unpack(f"<{width * height + 2 * cw * ch}H", raw)
    y = [samples[r * width:(r + 1) * width] for r in range(height)]
    at = width * height
    u = [samples[at + r * cw:at + (r + 1) * cw] for r in range(ch)]
    at += cw * ch
    v = [samples[at + r * cw:at + (r + 1) * cw] for r in range(ch)]
    return width, height, y, u, v


def frame_light(frame):
    """MaxCLL and the mastering display's peak, as the frame's side data
    gives them (0 where it does not)."""
    max_cll, peak = 0, 0.0
    for sd in frame.get("side_data_list", []):
        if sd["side_data_type"] == "Content light level metadata":
            max_cll = int(sd["max_content"])
        elif sd["side_data_type"] == "Mastering display metadata" and "max_luminance" in sd:
            num, den = sd["max_luminance"].split("/")
            peak = int(num) / int(den)
    return max_cll, peak


def png(path, width, height, rgb_rows):
    """An 8-bit RGB PNG, no other chunks: no gamma, no profile."""
    def chunk(kind, data):
        return (struct.pack(">I", len(data)) + kind + data
                + struct.pack(">I", zlib.crc32(kind + data) & 0xffffffff))
    raw = b"".join(b"\0" + bytes(row) for row in rgb_rows)
    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)))
        f.write(chunk(b"IDAT", zlib.compress(raw, 9)))
        f.write(chunk(b"IEND", b""))


def chrome_frame(t, name):
    """NAME.chrome.png: the first frame as Chrome shows it."""
    frame = t.frames(name)[0]
    matrix, primaries, transfer = resolved(frame)
    if (matrix, primaries) != (9, 9) or transfer not in (16, 18):
        raise SystemExit(f"{name}: chrome_hdr.py has only BT.2020's PQ and HLG")
    width, height, y, u, v = first_frame(t, name)
    us, vs = upsampled(u, width, height), upsampled(v, width, height)
    max_cll, mastering_peak = frame_light(frame)
    headroom = chrome_hdr.baseline_headroom(chrome_hdr.peak_luminance(max_cll, mastering_peak))
    points = chrome_hdr.rwtmo_alt0(headroom, half=True)
    rows = []
    for r in range(height):
        row = []
        for c in range(width):
            row += chrome_hdr.pixel(rgb_of(y[r][c], us[r][c], vs[r][c]), transfer, points)
        rows.append(row)
    png(f"{name}.chrome.png", width, height, rows)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ffmpeg", default="")
    ap.add_argument("--mkvmerge", default="D:/utils/mkvtoolnix-99.0/mkvmerge.exe")
    a = ap.parse_args()
    t = Tools(a.ffmpeg, a.mkvmerge)
    made = []

    # hdr10_av1.mkv: the bitstream's light, and the file's other light.
    t.encode("tmp_av1_meta.mkv", PQ, SVT,
             ["-svtav1-params", f"mastering-display={HDR10}:content-light=1000,400"])
    run([t.mkvmerge, "-q", "-o", "hdr10_av1.mkv",
         # A transfer the bitstream's PQ outranks.
         "--colour-transfer-characteristics", "0:1",
         "--max-content-light", "0:2000", "--max-frame-light", "0:500",
         "--chromaticity-coordinates", "0:0.68,0.32,0.265,0.69,0.15,0.06",
         "--white-colour-coordinates", "0:0.3127,0.3290",
         "--max-luminance", "0:4000", "--min-luminance", "0:0.005", "tmp_av1_meta.mkv"])
    made.append("hdr10_av1.mkv")

    # cll_av1.mkv: the bitstream says only its content light level, so the
    # file's mastering display stands beside it.
    t.encode("tmp_av1_cll.mkv", PQ, SVT, ["-svtav1-params", "content-light=1000,400"])
    run([t.mkvmerge, "-q", "-o", "cll_av1.mkv",
         "--max-content-light", "0:2000", "--max-frame-light", "0:500",
         "--chromaticity-coordinates", "0:0.68,0.32,0.265,0.69,0.15,0.06",
         "--white-colour-coordinates", "0:0.3127,0.3290",
         "--max-luminance", "0:4000", "--min-luminance", "0:0.005", "tmp_av1_cll.mkv"])
    made.append("cll_av1.mkv")

    # tagged_by_file_av1.mkv: AV1 that says nothing of its colour, the file
    # saying all of it.
    t.encode("tmp_av1_untagged.mkv", "null", SVT)
    run([t.mkvmerge, "-q", "-o", "tagged_by_file_av1.mkv",
         "--colour-transfer-characteristics", "0:16", "--colour-primaries", "0:9",
         "--colour-matrix-coefficients", "0:9", "--colour-range", "0:1",
         "tmp_av1_untagged.mkv"])
    made.append("tagged_by_file_av1.mkv")

    # hdr10_vp9.webm: all of it the file's.
    t.encode("tmp_vp9_pq.webm", PQ, VP9)
    run([t.mkvmerge, "-q", "--webm", "-o", "hdr10_vp9.webm",
         "--colour-transfer-characteristics", "0:16", "--colour-primaries", "0:9",
         "--max-content-light", "0:1000", "--max-frame-light", "0:400",
         "--chromaticity-coordinates", "0:0.708,0.292,0.170,0.797,0.131,0.046",
         "--white-colour-coordinates", "0:0.3127,0.3290",
         "--max-luminance", "0:1000", "--min-luminance", "0:0.0001", "tmp_vp9_pq.webm"])
    made.append("hdr10_vp9.webm")

    # hdr10_vp9.mp4: VP9's own boxes.
    run([t.ffmpeg, "-hide_banner", "-v", "error", "-y", "-i", "tmp_vp9_pq.webm", "-c", "copy",
         "-fflags", "+bitexact", "tmp_vp9_pq.mp4"])
    with open("tmp_vp9_pq.mp4", "rb") as f:
        data = f.read()
    data = insert_in_entry(data, smdm([(0.708, 0.292), (0.170, 0.797), (0.131, 0.046)],
                                               (0.3127, 0.3290), 1000, 0.0001))
    data = insert_in_entry(data, coll(1000, 400))
    with open("hdr10_vp9.mp4", "wb") as f:
        f.write(data)
    made.append("hdr10_vp9.mp4")

    # hdr10_av1.mp4: no metadata OBUs; mdcv and clli say it.
    t.encode("tmp_av1_pq.mp4", PQ, SVT)
    with open("tmp_av1_pq.mp4", "rb") as f:
        data = f.read()
    data = insert_in_entry(data, mdcv([(0.170, 0.797), (0.131, 0.046), (0.708, 0.292)],
                                               (0.3127, 0.3290), 1000, 0.0001))
    data = insert_in_entry(data, clli(1000, 400))
    with open("hdr10_av1.mp4", "wb") as f:
        f.write(data)
    made.append("hdr10_av1.mp4")

    # HLG.
    t.encode("tmp_vp9_hlg.webm", HLG, VP9)
    run([t.mkvmerge, "-q", "--webm", "-o", "hlg_vp9.webm",
         "--colour-transfer-characteristics", "0:18", "--colour-primaries", "0:9",
         "tmp_vp9_hlg.webm"])
    made.append("hlg_vp9.webm")
    t.encode("hlg_av1.mp4", HLG, SVT)
    made.append("hlg_av1.mp4")

    # partial_vp9.webm: a luminance without primaries, a MaxCLL without a
    # MaxFALL.
    run([t.mkvmerge, "-q", "--webm", "-o", "partial_vp9.webm",
         "--colour-transfer-characteristics", "0:16",
         "--max-content-light", "0:1000",
         "--max-luminance", "0:600", "--min-luminance", "0:0.05", "tmp_vp9_pq.webm"])
    made.append("partial_vp9.webm")

    answers(t, made)
    for tmp in [n for n in os.listdir(".") if n.startswith("tmp_")]:
        os.remove(tmp)


def answers(t, names):
    for name in names:
        with open(f"{name}.colour", "w", encoding="utf-8", newline="\n") as f:
            f.write(answer(name, t.frames(name)))
        chrome_frame(t, name)
        print("wrote", name)


FIXTURES = ["hdr10_av1.mkv", "cll_av1.mkv", "tagged_by_file_av1.mkv", "hdr10_vp9.webm",
            "hdr10_vp9.mp4", "hdr10_av1.mp4", "hlg_vp9.webm", "hlg_av1.mp4", "partial_vp9.webm"]


if __name__ == "__main__":
    if "--answers-only" in sys.argv:
        # The answers of the fixtures already made, without encoding anew:
        # encoders are not bit-exact from build to build, the answers are.
        sys.argv.remove("--answers-only")
        ap = argparse.ArgumentParser()
        ap.add_argument("--ffmpeg", default="")
        ap.add_argument("--mkvmerge", default="D:/utils/mkvtoolnix-99.0/mkvmerge.exe")
        a = ap.parse_args()
        answers(Tools(a.ffmpeg, a.mkvmerge), FIXTURES)
    else:
        main()
