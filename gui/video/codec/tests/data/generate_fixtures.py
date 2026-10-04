#!/usr/bin/env python3
"""Generate videocodec's fixtures and their answers.

Each fixture NAME.webm, NAME.mkv or NAME.mp4 gets NAME.txt: its video as a
player should be given it -- the track's description as `VideoInfo` says it, then
each frame's time and duration (nanoseconds), key-frame flag, size, and the
MD5 of its pixels (0xAARRGGBB words, little-endian: the bytes B G R A).
`tests/frames.rs` holds videocodec's frames to those lines.

Where the answers come from -- none of it from the crate itself:

- **Which frames, and when.** ffprobe's `-show_frames`: FFmpeg's demuxer and
  decoders (`-fflags +noparse+nofillin`, so that the times and durations are
  the file's own, as `gui/video/matroska`'s fixtures take them).
- **Their pixels.** ffmpeg decodes each frame to raw planes -- its own VP8
  and VP9 decoders, libvpx's where the video has alpha, libdav1d for AV1,
  each bit-exact to the reference decoder gui/video's decoders are held to --
  and
  libavif 1.3.0 converts each frame's planes exactly as it converts an AVIF
  still: `../../tools/libavif_reformat_reference.c`, built in WSL as its
  header says, with libyuv's C. The colour it converts with is the one this
  file's table says the stream should resolve to (matrix, primaries, range):
  the table states the rule `src/colour.rs` is meant to follow, and the test
  is whether the code agrees with it.
- **4:4:0**, which libavif does not convert: the chroma is doubled down its
  columns here, as `src/picture.rs`'s `rows_doubled` is meant to do it (the
  first row alone, 3:1 and 1:3 between, an even height's last row alone),
  and the result converted as 4:4:4.
- **A crop**, which ffmpeg does not write: `vp9_cropped.mkv` is written here,
  byte by byte, around VP9 frames ffmpeg encoded, with a crop, a display
  aspect and a `Colour` element (the bitstream says nothing of its colour, so
  the file's word is what decides it). Its answer is the whole picture's
  pixels with the crop cut away.
- **MP4**, as ffmpeg's muxer writes it: VP9 and AV1; a fragmented file; a
  pixel 4:3 wide (`pasp`); and two files cut from longer ones with `-ss` and
  stream copy, whose edit lists start between key frames -- the frames before
  the cut are decoded and not shown, which ffprobe and ffmpeg (whose decoders
  drop them) agree on. Two are then changed here, in place: `vp9_colr.mp4`'s
  `colr` box is made to say BT.2020 where the bitstream says nothing, so the
  file's word decides the matrix and primaries (and the bitstream's the
  range); and `vp9_clap.mp4` has a clean aperture (`clap`) put in its sample
  entry, whose crop the answer cuts away as for `vp9_cropped.mkv`.
  `VideoInfo`'s numbers for MP4: the frame rate's reciprocal where FFmpeg's
  demuxer finds one rate (none for a fragmented file), and the duration of
  the longest track, from ffprobe's `duration_ts`.

Every fixture is 8 frames at 25 a second, a key frame every 3, encoded with
one thread and `-fflags +bitexact`, so a run makes the same bytes as the last.

Run from this directory, on Windows, with gyan.dev's ffmpeg (2026-03-09, git
9b7439c31b) and the harness built in WSL:

    python generate_fixtures.py [--ffmpeg DIR] [--harness WSL-PATH]
"""

import argparse
import hashlib
import json
import os
import struct
import subprocess
import sys

FRAMES = 8
RATE = 25
SRC = f"testsrc2=size={{}}:rate={RATE}:duration={FRAMES / RATE}"

VP9 = ["-c:v", "libvpx-vp9", "-deadline", "good", "-cpu-used", "4", "-crf", "40",
       "-b:v", "0", "-g", "3", "-keyint_min", "3", "-threads", "1", "-row-mt", "0",
       "-tile-columns", "0"]
# libvpx's VP8 takes -crf only under a bitrate cap.
VP8 = ["-c:v", "libvpx", "-deadline", "good", "-cpu-used", "4", "-crf", "40",
       "-b:v", "1M", "-g", "3", "-keyint_min", "3", "-threads", "1", "-auto-alt-ref", "0"]
AV1 = ["-c:v", "libaom-av1", "-cpu-used", "8", "-crf", "50", "-b:v", "0", "-g", "3",
       "-keyint_min", "3", "-threads", "1", "-row-mt", "0", "-tiles", "1x1"]

# A varying alpha for VP9's transparency: a ramp across, down and in time.
ALPHA = (",format=yuva420p,geq=lum='lum(X,Y)':cb='cb(X,Y)':cr='cr(X,Y)'"
         ":a='mod(3*X+5*Y+40*N,256)'")

# name: (source, encoder arguments, raw pixel format, (format, depth, alpha)
#        for the harness, colour the stream resolves to (matrix, primaries,
#        range), the decoder ffmpeg reads the frames with)
FIXTURES = {
    # Nothing said, at a small size: BT.601 matrix, BT.709 primaries (mpv's
    # guess for a height that is neither 576 nor 480), studio range.
    "vp9_sd_untagged.webm": (
        SRC.format("176x144"), VP9 + ["-pix_fmt", "yuv420p"], "yuv420p",
        ("420", 8, 0), (6, 1, "limited"), None),
    # Nothing said, 1280 wide: BT.709.
    "vp9_hd_untagged.webm": (
        SRC.format("1280x48"), VP9 + ["-pix_fmt", "yuv420p"], "yuv420p",
        ("420", 8, 0), (1, 1, "limited"), None),
    # BT.709 said, at a small size: the word stands over the guess.
    "vp9_bt709_sd.webm": (
        SRC.format("176x144"), VP9 + ["-pix_fmt", "yuv420p", "-colorspace", "bt709",
                                      "-color_range", "tv"],
        "yuv420p", ("420", 8, 0), (1, 1, "limited"), None),
    # Full range: libyuv's JPEG constants.
    "vp9_full_range.webm": (
        SRC.format("176x144"), VP9 + ["-pix_fmt", "yuv420p", "-colorspace", "smpte170m",
                                      "-color_range", "pc"],
        "yuv420p", ("420", 8, 0), (6, 1, "full"), None),
    # Profile 2: 10 bits, BT.2020 -- the primaries from the file's Colour.
    "vp9_10bit_bt2020.webm": (
        SRC.format("176x144"), VP9 + ["-pix_fmt", "yuv420p10le", "-colorspace", "bt2020nc",
                                      "-color_primaries", "bt2020", "-color_range", "tv"],
        "yuv420p10le", ("420", 10, 0), (9, 9, "limited"), None),
    # Profile 1: 4:4:4.
    "vp9_444.webm": (
        SRC.format("176x144"), VP9 + ["-pix_fmt", "yuv444p", "-colorspace", "smpte170m"],
        "yuv444p", ("444", 8, 0), (6, 1, "limited"), None),
    # Profile 1: 4:4:0, doubled to 4:4:4 here (see above).
    "vp9_440.webm": (
        SRC.format("176x144"), VP9 + ["-pix_fmt", "yuv440p", "-colorspace", "bt709"],
        "yuv440p", ("440", 8, 0), (1, 1, "limited"), None),
    # Profile 3: 12-bit 4:2:2, which libavif cuts to 8 bits first.
    "vp9_422_12bit.webm": (
        SRC.format("176x144"), VP9 + ["-pix_fmt", "yuv422p12le", "-colorspace", "bt709"],
        "yuv422p12le", ("422", 12, 0), (1, 1, "limited"), None),
    # RGB, stored as G, B, R: the identity matrix, full range.
    "vp9_gbr.webm": (
        SRC.format("176x144"), VP9 + ["-pix_fmt", "gbrp"], "gbrp",
        ("444", 8, 0), (0, 1, "full"), None),
    # SMPTE 240M: libavif's own floating point, upsampling as it goes.
    "vp9_smpte240.webm": (
        SRC.format("176x144"), VP9 + ["-pix_fmt", "yuv420p", "-colorspace", "smpte240m"],
        "yuv420p", ("420", 8, 0), (7, 1, "limited"), None),
    # WebM's transparency: a second VP9 stream in each block's additions.
    "vp9_alpha.webm": (
        SRC.format("176x144") + ALPHA, VP9 + ["-pix_fmt", "yuva420p"], "yuva420p",
        ("420", 8, 1), (6, 1, "limited"), "libvpx-vp9"),
    # VP8's key frames say YUV, which FFmpeg reads as BT.601 at any size:
    # 1280 wide is BT.601 here, where untagged VP9 is guessed BT.709.
    "vp8_sd.webm": (
        SRC.format("176x144"), VP8 + ["-pix_fmt", "yuv420p"], "yuv420p",
        ("420", 8, 0), (5, 1, "limited"), None),
    "vp8_hd.webm": (
        SRC.format("1280x48"), VP8 + ["-pix_fmt", "yuv420p"], "yuv420p",
        ("420", 8, 0), (5, 1, "limited"), None),
    # An odd size: chroma rounded up, the last macroblocks partly outside.
    "vp8_odd.webm": (
        SRC.format("175x143"), VP8 + ["-pix_fmt", "yuv420p"], "yuv420p",
        ("420", 8, 0), (5, 1, "limited"), None),
    # The file says BT.709 and SMPTE 170M primaries: the bitstream's BT.601
    # stands over the file's matrix, and the primaries are the file's.
    "vp8_tagged.webm": (
        SRC.format("176x144"), VP8 + ["-pix_fmt", "yuv420p", "-colorspace", "bt709",
                                      "-color_primaries", "smpte170m", "-color_range", "tv"],
        "yuv420p", ("420", 8, 0), (5, 6, "limited"), None),
    # WebM's transparency over VP8, decoded by libvpx (FFmpeg's own VP8
    # decoder does not read the alpha).
    "vp8_alpha.webm": (
        SRC.format("176x144") + ALPHA, VP8 + ["-pix_fmt", "yuva420p"], "yuva420p",
        ("420", 8, 1), (5, 1, "limited"), "libvpx"),
    "av1_bt709.webm": (
        SRC.format("176x144"), AV1 + ["-pix_fmt", "yuv420p", "-colorspace", "bt709",
                                      "-color_primaries", "bt709", "-color_trc", "bt709",
                                      "-color_range", "tv"],
        "yuv420p", ("420", 8, 0), (1, 1, "limited"), "libdav1d"),
    # Monochrome: grey, through libyuv's I400. ffmpeg's grey is full range,
    # and libaom says so in the sequence header.
    "av1_mono.webm": (
        SRC.format("176x144"), AV1 + ["-pix_fmt", "gray"], "gray",
        ("400", 8, 0), (6, 1, "full"), "libdav1d"),
    "av1_444_10bit.webm": (
        SRC.format("176x144"), AV1 + ["-pix_fmt", "yuv444p10le", "-colorspace", "bt2020nc",
                                      "-color_primaries", "bt2020", "-color_range", "pc"],
        "yuv444p10le", ("444", 10, 0), (9, 9, "full"), "libdav1d"),
}

# vp9_cropped.mkv: the crop (left, top, right, bottom), the display aspect
# (DisplayUnit 3), and the Colour element (matrix, range, primaries).
CROPPED = "vp9_cropped.mkv"
CROPPED_SIZE = (176, 144)
CROP = (8, 4, 16, 6)
ASPECT = (16, 9)
CROPPED_COLOUR = (1, 1, 1)


def run(args, stdin=None):
    r = subprocess.run(args, input=stdin, capture_output=True)
    if r.returncode != 0:
        tail = r.stderr.decode("utf-8", "replace")[-3000:]
        raise SystemExit(f"{args[0]} failed ({r.returncode}):\n{tail}")
    return r.stdout


class Tools:
    def __init__(self, ffdir, harness):
        exe = ".exe" if os.name == "nt" else ""
        self.ffmpeg = os.path.join(ffdir, "ffmpeg" + exe)
        self.ffprobe = os.path.join(ffdir, "ffprobe" + exe)
        self.harness = harness

    def encode(self, name, source, args, fmt=None):
        fmt = fmt or ("mp4" if name.endswith(".mp4") else "webm")
        run([self.ffmpeg, "-hide_banner", "-loglevel", "error", "-y",
             "-f", "lavfi", "-i", source, *args, "-fflags", "+bitexact",
             "-f", fmt, name])

    def frames(self, name, decoder):
        """ffprobe's frames of the first video stream, and the stream's time
        base, frame rate and the file's duration."""
        dec = ["-c:v", decoder] if decoder else []
        out = run([self.ffprobe, "-v", "error", "-fflags", "+noparse+nofillin", *dec,
                   "-select_streams", "v:0", "-show_frames", "-show_streams",
                   "-show_format", "-of", "json", name])
        return json.loads(out.decode("utf-8"))

    def raw(self, name, decoder, pix_fmt):
        """Every frame's planes, whole: a crop the file asks for is the
        answer's to apply, after the conversion, not ffmpeg's before it."""
        dec = ["-c:v", decoder] if decoder else []
        return run([self.ffmpeg, "-hide_banner", "-loglevel", "error", *dec, "-apply_cropping", "none",
                    "-i", name,
                    "-map", "0:v:0", "-fps_mode", "passthrough", "-f", "rawvideo",
                    "-pix_fmt", pix_fmt, "-"])

    def convert(self, raw, width, height, fmt, depth, alpha, colour):
        matrix, primaries, rng = colour
        return run(["wsl", "-d", "Ubuntu", "--", self.harness, str(width), str(height), fmt,
                    str(depth), str(matrix), str(primaries), rng, str(alpha)], stdin=raw)


def plane_sizes(fmt, width, height):
    """Each plane's width and height, in the harness's plane order."""
    half = lambda n: (n + 1) // 2  # noqa: E731
    chroma = {
        "444": (width, height),
        "422": (half(width), height),
        "420": (half(width), half(height)),
        "440": (width, half(height)),
    }
    if fmt == "400":
        return [(width, height)]
    return [(width, height), chroma[fmt], chroma[fmt]]


def frame_bytes(fmt, width, height, depth, alpha):
    bps = 2 if depth > 8 else 1
    n = sum(w * h for w, h in plane_sizes(fmt, width, height))
    if alpha:
        n += width * height
    return n * bps


def doubled_rows(plane, width, height, bps):
    """4:4:0 chroma (width x ceil(height / 2)) brought to `height` rows: the
    first row alone; then each pair 3:1 and 1:3 of the two chroma rows around
    it; and for an even height, the last chroma row alone again."""
    fmt = "<" + ("H" if bps == 2 else "B") * width
    rows = [list(struct.unpack_from(fmt, plane, r * width * bps))
            for r in range(len(plane) // (width * bps))]
    out = [rows[0]]
    c = 0
    for _ in range((height - 1) // 2):
        a, b = rows[c], rows[c + 1]
        out.append([(3 * x + y + 2) >> 2 for x, y in zip(a, b)])
        out.append([(x + 3 * y + 2) >> 2 for x, y in zip(a, b)])
        c += 1
    if height % 2 == 0:
        out.append(rows[c])
    return b"".join(struct.pack(fmt, *r) for r in out)


def to_444(frame, width, height, bps):
    """A 4:4:0 frame's planes, its chroma doubled to 4:4:4."""
    y = width * height * bps
    c = width * ((height + 1) // 2) * bps
    luma, u, v = frame[:y], frame[y:y + c], frame[y + c:y + 2 * c]
    return luma + doubled_rows(u, width, height, bps) + doubled_rows(v, width, height, bps)


def pixels_md5(tools, raw, count, width, height, fmt, depth, alpha, colour, crop=None):
    """Each frame of `raw` converted by libavif, cropped if asked, hashed."""
    size = frame_bytes(fmt, width, height, depth, alpha)
    if len(raw) != size * count:
        raise SystemExit(f"{len(raw)} raw bytes, not {count} frames of {size}")
    frames = [raw[i * size:(i + 1) * size] for i in range(count)]
    if fmt == "440":
        frames = [to_444(f, width, height, 2 if depth > 8 else 1) for f in frames]
        fmt = "444"
    bgra = tools.convert(b"".join(frames), width, height, fmt, depth, alpha, colour)
    per = width * height * 4
    if len(bgra) != per * count:
        raise SystemExit(f"the harness gave {len(bgra)} bytes, not {count} frames of {per}")
    out = []
    for i in range(count):
        px = bgra[i * per:(i + 1) * per]
        w, h = width, height
        if crop:
            left, top, right, bottom = crop
            rows = [px[(top + r) * width * 4 + left * 4:(top + r) * width * 4 + (width - right) * 4]
                    for r in range(height - top - bottom)]
            px = b"".join(rows)
            w, h = width - left - right, height - top - bottom
        out.append((w, h, hashlib.md5(px).hexdigest()))
    return out


def ns(ticks, time_base):
    num, den = (int(x) for x in time_base.split("/"))
    return ticks * num * 1_000_000_000 // den


def answer(tools, name, decoder, pixels, info):
    """NAME.txt from ffprobe's frames and the pixels' hashes."""
    probe = tools.frames(name, decoder)
    frames = probe["frames"]
    if len(frames) != len(pixels):
        raise SystemExit(f"{name}: ffprobe shows {len(frames)} frames, ffmpeg decoded {len(pixels)}")
    tb = probe["streams"][0]["time_base"]
    lines = [
        f"# {name}: its video as videocodec should give it (generate_fixtures.py).",
        f"# sha256 {hashlib.sha256(open(name, 'rb').read()).hexdigest()}",
        "info " + " ".join(f"{k}={v}" for k, v in info.items()),
    ]
    first = frames[0] if frames else {}
    print(f"{name}: ffprobe reads its colour as space={first.get('color_space')} "
          f"primaries={first.get('color_primaries')} range={first.get('color_range')}",
          file=sys.stderr)
    for f, (w, h, md5) in zip(frames, pixels):
        dur = ns(int(f["duration"]), tb) if "duration" in f else 0
        key = "K" if f["key_frame"] == 1 else "_"
        lines.append(f"frame {ns(int(f['pts']), tb)} {dur} {key} {w}x{h} {md5}")
    with open(name.rsplit(".", 1)[0] + ".txt", "w", encoding="utf-8", newline="\n") as out:
        out.write("\n".join(lines) + "\n")
    print(f"{name}: {len(frames)} frames", file=sys.stderr)


# ffprobe's names for H.273's matrices.
MATRICES = {"gbr": 0, "bt709": 1, "fcc": 4, "bt470bg": 5, "smpte170m": 6, "smpte240m": 7,
            "ycgco": 8, "bt2020nc": 9, "bt2020c": 10}


def check_colour(name, colour, probe):
    """The row's matrix and range against what the stream says, as FFmpeg's
    decoders read it (for VP9 and AV1, the bitstream's own word): where the
    stream names a matrix, the row must have it; where it does not, the row
    must have the size's guess; and the range must be the stream's. A row
    that has drifted from its file stops the run here rather than producing
    answers the code is then wrongly held to."""
    first = probe["frames"][0]
    width, height = first["width"], first["height"]
    space = first.get("color_space")
    guess = 1 if width >= 1280 or height > 576 else 6
    said = MATRICES.get(space, guess) if space not in (None, "unknown") else guess
    stream_range = "full" if first.get("color_range") == "pc" else "limited"
    if (colour[0], colour[2]) != (said, stream_range):
        raise SystemExit(f"{name}: the table says matrix {colour[0]} {colour[2]}, the stream "
                         f"{said} {stream_range}")


def stream_info(tools, name, decoder, codec, width, height, alpha):
    probe = tools.frames(name, decoder)
    rate = probe["streams"][0]["r_frame_rate"]
    num, den = (int(x) for x in rate.split("/"))
    duration = round(float(probe["format"]["duration"]) * 1_000_000_000)
    return {
        "track": 1,
        "codec": codec,
        "size": f"{width}x{height}",
        "display": f"{width}x{height}",
        # matroskaenc's DefaultDuration: 1e9 / the frame rate, truncated.
        "frame_duration": int(1e9 / (num / den)),
        "duration": duration,
        "alpha": alpha,
    }


# --- vp9_cropped.mkv, written by hand ---------------------------------------


def vint(n):
    length = 1
    while n >= (1 << (7 * length)) - 1:
        length += 1
    return ((1 << (7 * length)) | n).to_bytes(length, "big")


def el(id_hex, payload):
    return bytes.fromhex(id_hex) + vint(len(payload)) + payload


def uint(id_hex, v):
    return el(id_hex, v.to_bytes(max(1, (v.bit_length() + 7) // 8), "big"))


def ivf_frames(data):
    """An IVF file's frames: (pts, bytes)."""
    if data[:4] != b"DKIF":
        raise SystemExit("not an IVF file")
    header = struct.unpack_from("<H", data, 6)[0]
    at, out = header, []
    while at + 12 <= len(data):
        size, pts = struct.unpack_from("<IQ", data, at)
        out.append((pts, data[at + 12:at + 12 + size]))
        at += 12 + size
    return out


def vp9_is_key(frame):
    """A profile 0 frame's first byte: marker, profile, show_existing_frame,
    frame_type (0 for a key frame)."""
    b = frame[0]
    return (b >> 6) == 2 and not (b >> 3) & 1 and not (b >> 2) & 1


def write_cropped(tools):
    ivf = "vp9_cropped.ivf"
    width, height = CROPPED_SIZE
    tools.encode(ivf, SRC.format(f"{width}x{height}"), VP9 + ["-pix_fmt", "yuv420p"], "ivf")
    frames = ivf_frames(open(ivf, "rb").read())
    left, top, right, bottom = CROP
    matrix, rng, primaries = CROPPED_COLOUR
    video = (uint("B0", width) + uint("BA", height)
             + uint("54AA", bottom) + uint("54BB", top) + uint("54CC", left) + uint("54DD", right)
             + uint("54B0", ASPECT[0]) + uint("54BA", ASPECT[1]) + uint("54B2", 3)
             + el("55B0", uint("55B1", matrix) + uint("55B9", rng) + uint("55BB", primaries)))
    track = el("AE", uint("D7", 1) + uint("73C5", 1) + uint("83", 1)
               + el("86", b"V_VP9") + uint("23E383", 40_000_000) + el("E0", video))
    info = el("1549A966", uint("2AD7B1", 1_000_000)
              + el("4489", struct.pack(">d", FRAMES * 40.0)))
    clusters = b""
    for i, (_, frame) in enumerate(frames):
        if vp9_is_key(frame) or i == 0:
            if i:
                clusters += el("1F43B675", cluster)
            cluster, start = uint("E7", i * 40), i * 40
        block = vint(1) + struct.pack(">hB", i * 40 - start, 0x80 if vp9_is_key(frame) else 0) + frame
        cluster += el("A3", block)
    clusters += el("1F43B675", cluster)
    header = el("1A45DFA3", uint("4286", 1) + uint("42F7", 1) + uint("42F2", 4)
                + uint("42F3", 8) + el("4282", b"webm") + uint("4287", 4) + uint("4285", 2))
    with open(CROPPED, "wb") as out:
        out.write(header + el("18538067", info + el("1654AE6B", track) + clusters))
    raw = tools.raw(ivf, None, "yuv420p")
    os.remove(ivf)
    pixels = pixels_md5(tools, raw, len(frames), width, height, "420", 8, 0,
                        (matrix, primaries, "limited"), crop=CROP)
    w, h = width - left - right, height - top - bottom
    shown = (2 * h * ASPECT[0] + ASPECT[1]) // (2 * ASPECT[1])
    answer(tools, CROPPED, None, pixels, {
        "track": 1, "codec": "vp9", "size": f"{w}x{h}", "display": f"{shown}x{h}",
        "frame_duration": 40_000_000, "duration": FRAMES * 40_000_000, "alpha": 0,
    })


# --- MP4 --------------------------------------------------------------------

# Boxes whose children follow their header directly, and how far into a
# visual sample entry its children begin.
MP4_CONTAINERS = {b"moov", b"trak", b"mdia", b"minf", b"stbl"}
VISUAL_ENTRY_FIELDS = 78


def insert_in_entry(data, box):
    """`box` put at the end of the first video sample entry's children, and
    every box around it grown to hold it. The moov must follow the media, as
    ffmpeg writes it by default, so that no chunk offset moves."""
    chain = []

    def walk(start, end):
        at = start
        while at + 8 <= end:
            size, kind = struct.unpack_from(">I4s", data, at)
            if size < 8:
                return None
            if kind in MP4_CONTAINERS:
                chain.append(at)
                found = walk(at + 8, at + size)
                if found is not None:
                    return found
                chain.pop()
            elif kind == b"stsd":
                chain.append(at)
                entry = at + 16
                entry_size = struct.unpack_from(">I", data, entry)[0]
                chain.append(entry)
                return entry + entry_size
            at += size
        return None

    end = walk(0, len(data))
    if end is None or data.index(b"moov") < data.index(b"mdat"):
        raise SystemExit("no sample entry, or a moov before the media")
    out = bytearray(data[:end] + box + data[end:])
    for at in chain:
        size = struct.unpack_from(">I", out, at)[0]
        struct.pack_into(">I", out, at, size + len(box))
    return bytes(out)


def patch_colr(data, primaries, transfer, matrix, full_range):
    """The `colr` box's `nclx` words, rewritten in place."""
    at = data.index(b"colrnclx") + 8
    return data[:at] + struct.pack(">HHHB", primaries, transfer, matrix,
                                   0x80 if full_range else 0) + data[at + 7:]


def clap_box(width, height, offset_x, offset_y):
    """A clean aperture: four fractions, each (numerator, denominator)."""
    body = b"".join(struct.pack(">i", v) for q in (width, height, offset_x, offset_y) for v in q)
    return struct.pack(">I", 8 + len(body)) + b"clap" + body


def mp4_info(tools, name, decoder, codec, width, height, crop, fragmented):
    """`VideoInfo` for an MP4 fixture, from ffprobe: the size less the crop;
    the display width the pixel's shape gives that size, rounded half up;
    the frame rate's reciprocal, exactly, where FFmpeg's demuxer finds one
    rate (not in a fragmented file, whose moov lists no samples); and the
    longest track's duration."""
    probe = tools.frames(name, decoder)
    st = probe["streams"][0]
    w, h = width - crop[0] - crop[2], height - crop[1] - crop[3]
    n, d = (int(x) for x in st.get("sample_aspect_ratio", "0:1").split(":"))
    shown = (2 * w * n + d) // (2 * d) if n > 0 and d > 0 else w
    num, den = (int(x) for x in st["r_frame_rate"].split("/"))
    tb_num, tb_den = (int(x) for x in st["time_base"].split("/"))
    return {
        "track": 1,
        "codec": codec,
        "size": f"{w}x{h}",
        "display": f"{max(shown, 1)}x{h}",
        "frame_duration": "none" if fragmented else 10**9 * den // num,
        "duration": int(st["duration_ts"]) * 10**9 * tb_num // tb_den,
        "alpha": 0,
    }


def mp4_fixture(tools, name, source, args, pix_fmt, planes, colour, decoder, *,
                cut=None, change=None, crop=(0, 0, 0, 0), fragmented=False, said_by_file=False):
    """One MP4 fixture: encoded (from `cut` seconds into a longer encode, by
    stream copy, if asked), `change`d in place if asked, and answered."""
    if cut is not None:
        whole = name.replace(".mp4", ".whole.mp4")
        tools.encode(whole, source, args)
        run([tools.ffmpeg, "-hide_banner", "-loglevel", "error", "-y", "-ss", str(cut), "-i", whole,
             "-c", "copy", "-fflags", "+bitexact", "-f", "mp4", name])
        os.remove(whole)
    else:
        tools.encode(name, source, args)
    if change:
        with open(name, "rb") as f:
            data = change(f.read())
        with open(name, "wb") as f:
            f.write(data)
    own = None if decoder in ("libvpx", "libvpx-vp9") else decoder
    if not said_by_file:
        check_colour(name, colour, tools.frames(name, own))
    probe = tools.frames(name, decoder)
    # The crop the table states must be the one FFmpeg's demuxer works out
    # of the file's clean aperture (left, top, right, bottom).
    said = (0, 0, 0, 0)
    for sd in probe["streams"][0].get("side_data_list", []):
        if sd.get("side_data_type") == "Frame Cropping":
            said = (sd["crop_left"], sd["crop_top"], sd["crop_right"], sd["crop_bottom"])
    if said != tuple(crop):
        raise SystemExit(f"{name}: the table crops {crop}, FFmpeg {said}")
    width, height = probe["streams"][0]["width"], probe["streams"][0]["height"]
    fmt, depth, alpha = planes
    raw = tools.raw(name, decoder, pix_fmt)
    pixels = pixels_md5(tools, raw, len(probe["frames"]), width, height, fmt, depth, alpha, colour,
                        crop=crop if crop != (0, 0, 0, 0) else None)
    codec = name.split("_", 1)[0].split(".", 1)[0]
    answer(tools, name, own, pixels,
           mp4_info(tools, name, decoder, codec, width, height, crop, fragmented))


# The crop vp9_clap.mp4's clean aperture makes (left, top, right, bottom):
# 160x136 of the 176x144 picture, its centre 4 left and 2 up of the
# picture's.
CLAP = ((160, 1), (136, 1), (-4, 1), (-2, 1))
CLAP_CROP = (4, 2, 12, 6)


def write_mp4s(tools):
    small = SRC.format("176x144")
    longer = f"testsrc2=size=176x144:rate={RATE}:duration={(FRAMES + 2) / RATE}"
    yuv420 = ["-pix_fmt", "yuv420p"]
    bt709 = ["-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709",
             "-color_range", "tv"]
    plain = ("yuv420p", ("420", 8, 0))
    # VP9 and AV1 as a player meets them in MP4.
    mp4_fixture(tools, "vp9.mp4", small, VP9 + yuv420, *plain, (6, 1, "limited"), None)
    mp4_fixture(tools, "av1.mp4", small, AV1 + yuv420 + bt709, *plain, (1, 1, "limited"), "libdav1d")
    # Fragmented, as a live stream writes it.
    mp4_fixture(tools, "av1_fragmented.mp4", small,
                AV1 + yuv420 + bt709 + ["-movflags", "frag_keyframe+empty_moov"], *plain,
                (1, 1, "limited"), "libdav1d", fragmented=True)
    # A pixel 4:3 wide: shown 235 wide.
    mp4_fixture(tools, "vp9_pasp.mp4", small + ",setsar=4/3", VP9 + yuv420, *plain,
                (6, 1, "limited"), None)
    # Cut 0.05 s in, between key frames: the edit list starts in the second
    # frame, and the first two are decoded for the third and not shown.
    mp4_fixture(tools, "vp9_cut.mp4", longer, VP9 + yuv420, *plain, (6, 1, "limited"), None, cut=0.05)
    mp4_fixture(tools, "av1_cut.mp4", longer, AV1 + yuv420 + bt709, *plain, (1, 1, "limited"),
                "libdav1d", cut=0.05)
    # The file's colr says BT.2020, full range, where the bitstream says
    # nothing of the matrix: the file's matrix and primaries, the
    # bitstream's studio range.
    mp4_fixture(tools, "vp9_colr.mp4", small, VP9 + yuv420 + ["-movflags", "+write_colr"], *plain,
                (9, 9, "limited"), None, change=lambda d: patch_colr(d, 9, 1, 9, True),
                said_by_file=True)
    # A clean aperture.
    mp4_fixture(tools, "vp9_clap.mp4", small, VP9 + yuv420, *plain, (6, 1, "limited"), None,
                change=lambda d: insert_in_entry(d, clap_box(*CLAP)), crop=CLAP_CROP)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--ffmpeg", default="D:/utils")
    parser.add_argument("--harness",
                        default="/home/inhahe/avifref/reformatharness/libavif_reformat_reference")
    args = parser.parse_args()
    os.chdir(os.path.dirname(os.path.abspath(__file__)))
    tools = Tools(args.ffmpeg, args.harness)
    for name, (source, encode, pix_fmt, (fmt, depth, alpha), colour, decoder) in FIXTURES.items():
        tools.encode(name, source, encode)
        # FFmpeg's own decoders where the pixels need libvpx's (for the
        # alpha): ffmpeg's libvpx wrapper does not mark key frames, nor read
        # VP8's colour bits, and the frames, times and durations are the same.
        own = None if decoder in ("libvpx", "libvpx-vp9") else decoder
        probe = tools.frames(name, decoder)
        check_colour(name, colour, tools.frames(name, own))
        width, height = probe["streams"][0]["width"], probe["streams"][0]["height"]
        raw = tools.raw(name, decoder, pix_fmt)
        pixels = pixels_md5(tools, raw, len(probe["frames"]), width, height, fmt, depth, alpha,
                            colour)
        codec = name.split("_", 1)[0]
        answer(tools, name, own, pixels,
               stream_info(tools, name, decoder, codec, width, height, alpha))
    write_cropped(tools)
    write_mp4s(tools)


if __name__ == "__main__":
    main()
