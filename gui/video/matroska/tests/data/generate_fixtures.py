#!/usr/bin/env python3
"""Generate the matroska crate's fixtures and their answers.

Each fixture `NAME.webm` or `NAME.mkv` gets `NAME.txt`: what FFmpeg's
demuxer makes of it, as `ffprobe` prints it -- a line per stream (index,
codec, time base) and a line per packet (stream, pts, duration, size,
position, flags, the data's MD5, and the BlockAddIDs of its additions).
`tests/fixtures.rs` holds the crate's packets to those lines.

The packets are the demuxer's own: `-fflags +noparse+nofillin` keeps
FFmpeg's codec parsers (which would time an Opus packet from its contents)
and its generic layer (which marks every frame of an intra-only codec a key
frame, and fills in missing timestamps) out of them.

Two kinds of fixture:

- **Written by ffmpeg** from its test sources: VP9 with Opus (Opus's codec
  delay, the last block's DiscardPadding), AV1, VP8 with Vorbis, VP9 with
  an alpha channel (BlockAdditions), a live-style file (a Segment of unknown
  size, no Cues), and subtitles in BlockGroups with durations.
- **Written here**, byte by byte, for what no muxer writes by default: the
  three lacings, header-stripping and zlib compression, Clusters of unknown
  size, BlockGroups with references, a Cluster with no Timestamp, negative
  block timestamps, a track FFmpeg ignores, and damage that FFmpeg reads past.

The answers were made with the ffmpeg and ffprobe of gyan.dev's full build
of 2026-03-09 (git 9b7439c31b). Run from this directory:

    python generate_fixtures.py [path-to-ffmpeg-directory]
"""

import hashlib
import os
import struct
import subprocess
import sys
import zlib

FFDIR = sys.argv[1] if len(sys.argv) > 1 else "D:/utils"
FFMPEG = os.path.join(FFDIR, "ffmpeg.exe" if os.name == "nt" else "ffmpeg")
FFPROBE = os.path.join(FFDIR, "ffprobe.exe" if os.name == "nt" else "ffprobe")


def run(*args):
    subprocess.run(args, check=True, capture_output=True)


def ffmpeg(name, *args):
    # bitexact: no random UIDs or version strings, so a run makes the same
    # bytes as the last.
    run(FFMPEG, "-hide_banner", "-loglevel", "error", "-y", *args,
        "-fflags", "+bitexact", "-threads", "1", name)


# --- EBML, written by hand --------------------------------------------------


def vint(n, length=None):
    """n as an EBML size: the shortest length, or `length` bytes."""
    if length is None:
        length = 1
        while n >= (1 << (7 * length)) - 1:
            length += 1
    return ((1 << (7 * length)) | n).to_bytes(length, "big")


UNKNOWN = b"\x01\xff\xff\xff\xff\xff\xff\xff"


def el(id_hex, payload, size=None):
    """An element: its ID (hex, marker included), size and payload."""
    return bytes.fromhex(id_hex) + (vint(len(payload)) if size is None else size) + payload


def uint(id_hex, v):
    n = max(1, (v.bit_length() + 7) // 8)
    return el(id_hex, v.to_bytes(n, "big"))


def sint(id_hex, v):
    n = 1
    while not -(1 << (8 * n - 1)) <= v < (1 << (8 * n - 1)):
        n += 1
    return el(id_hex, v.to_bytes(n, "big", signed=True))


def string(id_hex, s):
    return el(id_hex, s.encode())


EBML_HEADER = el(
    "1A45DFA3",
    uint("4286", 1)
    + uint("42F7", 1)
    + uint("42F2", 4)
    + uint("42F3", 8)
    + string("4282", "matroska")
    + uint("4287", 4)
    + uint("4285", 2),
)
INFO = el("1549A966", uint("2AD7B1", 1_000_000) + string("4D80", "fixture") + string("5741", "fixture"))


def snow_track(number, default_duration=None, encoding=b""):
    """A video track of Snow, whose frames FFmpeg neither parses nor marks
    as key frames itself (Snow has inter frames and no parser), so that the
    key-frame flags ffprobe prints are the demuxer's. The bytes are not
    Snow; nothing decodes them."""
    body = (
        uint("D7", number)
        + uint("73C5", number)
        + uint("83", 1)
        + string("86", "V_SNOW")
        + el("E0", uint("B0", 16) + uint("BA", 16))
    )
    if default_duration is not None:
        body += uint("23E383", default_duration)
    return el("AE", body + encoding)


def block(track, relative, flags, body):
    return vint(track) + relative.to_bytes(2, "big", signed=True) + bytes([flags]) + body


def simple(track, relative, flags, body):
    return el("A3", block(track, relative, flags, body))


def cluster(timestamp, *children, size=None):
    payload = (uint("E7", timestamp) if timestamp is not None else b"") + b"".join(children)
    return el("1F43B675", payload, size)


def segment(*children, size=None):
    return EBML_HEADER + el("18538067", b"".join(children), size)


def xiph(frames):
    out = bytes([len(frames) - 1])
    for f in frames[:-1]:
        n = len(f)
        out += b"\xff" * (n // 255) + bytes([n % 255])
    return out + b"".join(frames)


def ebml_laced(frames):
    sizes = [len(f) for f in frames]
    out = bytes([len(frames) - 1]) + vint(sizes[0])
    for prev, cur in zip(sizes, sizes[1:-1]):
        d = cur - prev
        # A signed EBML number: the value plus half the range, one byte when
        # it fits.
        n = 1
        while not -((1 << (7 * n - 1)) - 1) <= d <= (1 << (7 * n - 1)) - 1:
            n += 1
        out += vint(d + (1 << (7 * n - 1)) - 1, n)
    return out + b"".join(frames)


def frame(seed, n):
    return bytes((seed * 31 + i * 7) & 0xFF for i in range(n))


def synthetic():
    """The hand-written fixtures: name -> bytes."""
    out = {}
    one = el("1654AE6B", snow_track(1, default_duration=10_000_000))
    out["laced.mkv"] = segment(
        INFO,
        one,
        cluster(
            0,
            simple(1, 0, 0x80 | 0x02, xiph([frame(1, 3), frame(2, 300), frame(3, 5)])),
            simple(1, 30, 0x80 | 0x04, bytes([2]) + frame(4, 6) + frame(5, 6) + frame(6, 6)),
            simple(1, 60, 0x80 | 0x06, ebml_laced([frame(7, 4), frame(8, 9), frame(9, 2), frame(10, 7)])),
            simple(1, 100, 0x80, frame(11, 8)),
        ),
    )
    # No default duration: a laced block's later frames have no timestamp.
    out["laced_untimed.mkv"] = segment(
        INFO,
        el("1654AE6B", snow_track(1)),
        cluster(5, simple(1, 0, 0x82, xiph([frame(1, 4), frame(2, 4), frame(3, 4)]))),
    )
    stripped = el("6D80", el("6240", uint("5031", 0) + uint("5032", 1) + uint("5033", 0) + el("5034", uint("4254", 3) + el("4255", b"HDR!"))))
    zlibbed = el("6D80", el("6240", uint("5031", 0) + uint("5032", 1) + uint("5033", 0) + el("5034", uint("4254", 0))))
    out["compressed.mkv"] = segment(
        INFO,
        el("1654AE6B", snow_track(1, 20_000_000, stripped) + snow_track(2, 20_000_000, zlibbed)),
        cluster(
            0,
            simple(1, 0, 0x80, frame(1, 10)),
            simple(2, 0, 0x80, zlib.compress(frame(2, 40))),
            simple(1, 20, 0x80, frame(3, 12)),
            simple(2, 20, 0x80, zlib.compress(frame(4, 33))),
        ),
    )
    # Clusters of unknown size, in a Segment of unknown size; the first has
    # no Timestamp; BlockGroups with durations and references; a negative
    # block timestamp; a track of a kind FFmpeg ignores (complex, 3).
    ignored = el("AE", uint("D7", 9) + uint("73C5", 9) + uint("83", 3) + string("86", "V_X"))
    group = lambda t, rel, body, dur=None, refs=(): el(  # noqa: E731
        "A0",
        el("A1", block(t, rel, 0, body))
        + (uint("9B", dur) if dur is not None else b"")
        + b"".join(sint("FB", r) for r in refs),
    )
    out["unknown_sizes.mkv"] = segment(
        INFO,
        el("1654AE6B", snow_track(1, 10_000_000) + ignored),
        cluster(None, simple(1, 3, 0x80, frame(1, 4)), simple(9, 4, 0x80, frame(2, 4)), size=UNKNOWN),
        cluster(
            100,
            group(1, -20, frame(3, 6), dur=7),
            group(1, 0, frame(4, 6), refs=(-10,)),
            group(1, 15, frame(5, 6), refs=(-5, 5)),
            simple(1, 40, 0x00, frame(6, 6)),
            size=UNKNOWN,
        ),
        cluster(200, simple(1, 0, 0x80, frame(7, 5))),
        size=UNKNOWN,
    )
    # Damage: a laced block whose sizes overrun it, inside the second
    # Cluster -- FFmpeg drops the rest of that Cluster and reads on from the
    # next.
    out["damaged.mkv"] = segment(
        INFO,
        one,
        cluster(0, simple(1, 0, 0x80, frame(1, 4)), simple(1, 10, 0x80, frame(2, 4))),
        cluster(
            50,
            simple(1, 0, 0x80, frame(3, 4)),
            simple(1, 10, 0x82, bytes([2, 200, 200]) + frame(4, 5)),
            simple(1, 20, 0x80, frame(5, 4)),
        ),
        cluster(100, simple(1, 0, 0x80, frame(6, 4))),
    )
    return out


# --- ffmpeg's ----------------------------------------------------------------


def made_by_ffmpeg():
    """The fixtures ffmpeg writes: name -> bytes."""
    lavfi = ["-f", "lavfi", "-i"]
    ffmpeg(
        "vp9_opus.webm",
        *lavfi, "testsrc=size=64x48:rate=10:duration=1.5",
        *lavfi, "sine=frequency=440:sample_rate=48000:duration=1.5",
        "-c:v", "libvpx-vp9", "-pix_fmt", "yuv420p", "-g", "5", "-b:v", "50k",
        "-c:a", "libopus", "-b:a", "24k", "-ac", "1",
    )
    ffmpeg(
        "av1.webm",
        *lavfi, "testsrc=size=64x48:rate=10:duration=1",
        "-c:v", "libaom-av1", "-pix_fmt", "yuv420p", "-cpu-used", "8", "-g", "4", "-b:v", "40k",
    )
    ffmpeg(
        "vp8_vorbis.webm",
        *lavfi, "testsrc=size=48x32:rate=8:duration=1",
        *lavfi, "sine=frequency=300:sample_rate=22050:duration=1",
        "-c:v", "libvpx", "-g", "3", "-b:v", "40k",
        "-c:a", "libvorbis", "-ac", "1",
    )
    ffmpeg(
        "vp9_alpha.webm",
        *lavfi, "testsrc=size=32x24:rate=10:duration=0.6",
        "-c:v", "libvpx-vp9", "-pix_fmt", "yuva420p", "-b:v", "30k",
    )
    ffmpeg(
        "live.webm",
        *lavfi, "testsrc=size=32x24:rate=10:duration=1",
        "-c:v", "libvpx-vp9", "-g", "4", "-b:v", "30k", "-live", "1", "-f", "webm",
    )
    with open("subs.srt", "w", encoding="utf-8", newline="\n") as f:
        f.write("1\n00:00:00,100 --> 00:00:00,600\nOne\n\n2\n00:00:00,400 --> 00:00:01,000\nTwo\n\n")
    ffmpeg(
        "subtitles.mkv",
        *lavfi, "testsrc=size=32x24:rate=10:duration=1",
        "-i", "subs.srt",
        "-map", "0", "-map", "1",
        "-c:v", "libvpx-vp9", "-b:v", "30k", "-c:s", "srt",
    )
    os.remove("subs.srt")
    names = ["vp9_opus.webm", "av1.webm", "vp8_vorbis.webm", "vp9_alpha.webm", "live.webm", "subtitles.mkv"]
    out = {}
    for n in names:
        with open(n, "rb") as f:
            out[n] = f.read()
    return out


# --- the answers ---------------------------------------------------------------


def answer(name):
    streams = subprocess.run(
        [FFPROBE, "-v", "error", "-show_entries", "stream=index,codec_name,time_base", "-of", "compact=p=0", name],
        check=True, capture_output=True, text=True, encoding="utf-8",
    ).stdout.splitlines()
    packets = subprocess.run(
        [FFPROBE, "-v", "error", "-fflags", "+noparse+nofillin", "-show_entries",
         "packet=stream_index,pts,duration,size,pos,flags,data_hash:packet_side_data=block_additional_id",
         "-show_data_hash", "MD5", "-of", "compact=p=0", name],
        check=True, capture_output=True, text=True, encoding="utf-8",
    ).stdout.splitlines()
    lines = [f"# {name}: what ffprobe makes of it (generate_fixtures.py)."]
    for s in streams:
        f = dict(kv.split("=", 1) for kv in s.split("|") if "=" in kv)
        lines.append(f"stream {f['index']} {f.get('codec_name', 'none')} {f['time_base']}")
    for p in packets:
        f = dict(kv.split("=", 1) for kv in p.split("|") if "=" in kv)
        adds = [v for k, v in f.items() if k.endswith("block_additional_id")]
        lines.append(
            "packet {} {} {} {} {} {} {} {}".format(
                f["stream_index"], f["pts"], f["duration"], f["size"], f["pos"], f["flags"],
                f["data_hash"].removeprefix("MD5:"), ",".join(adds) or "-",
            )
        )
    return "\n".join(lines) + "\n"


# The pictures, as FFmpeg decodes them: each frame's MD5 over its planes, row
# by row (`-f framemd5`). VP9 by FFmpeg's own decoder (bit-exact with
# libvpx's), AV1 by dav1d, and VP9 with alpha by libvpx, which decodes the
# BlockAdditional's second stream into the alpha plane.
DECODED = {"vp9_opus.webm": "vp9", "av1.webm": "libdav1d", "vp9_alpha.webm": "libvpx-vp9"}


def frames_answer(name, decoder):
    out = subprocess.run(
        [FFMPEG, "-hide_banner", "-loglevel", "error", "-flags", "+bitexact", "-c:v", decoder,
         "-i", name, "-map", "0:v", "-f", "framemd5", "-"],
        check=True, capture_output=True, text=True, encoding="utf-8",
    ).stdout.splitlines()
    lines = [f"# {name}: each frame's MD5 as ffmpeg's {decoder} decodes it (generate_fixtures.py)."]
    for line in out:
        if line.startswith("#"):
            continue
        fields = [f.strip() for f in line.split(",")]
        lines.append(f"frame {fields[4]} {fields[5]}")
    return "\n".join(lines) + "\n"


# Where each seek test seeks, in seconds, and how many packets it reads
# after: ffprobe's -read_intervals seeks the file's default stream (its video)
# backward to the time, as `Demuxer::seek` on that track does.
SEEKS = [0.0, 0.25, 0.55, 1.2, 99.0]
SEEK_PACKETS = 6
SEEKABLE = ["vp9_opus.webm", "av1.webm", "vp8_vorbis.webm", "live.webm", "unknown_sizes.mkv", "laced.mkv"]


def seek_answer(name):
    lines = [f"# {name}: ffprobe's first {SEEK_PACKETS} packets after each seek (generate_fixtures.py)."]
    for s in SEEKS:
        out = subprocess.run(
            [FFPROBE, "-v", "error", "-fflags", "+noparse+nofillin",
             "-read_intervals", f"{s}%+#{SEEK_PACKETS}",
             "-show_entries", "packet=stream_index,pts,flags", "-of", "compact=p=0", name],
            check=True, capture_output=True, text=True, encoding="utf-8",
        ).stdout.splitlines()
        lines.append(f"seek {round(s * 1000)}")
        for p in out:
            f = dict(kv.split("=", 1) for kv in p.split("|") if "=" in kv)
            lines.append(f"packet {f['stream_index']} {f['pts']} {f['flags']}")
    return "\n".join(lines) + "\n"


def main():
    files = {**made_by_ffmpeg(), **synthetic()}
    for name, data in sorted(files.items()):
        with open(name, "wb") as f:
            f.write(data)
        text = answer(name)
        base = name.rsplit(".", 1)[0]
        with open(f"{base}.txt", "w", encoding="utf-8", newline="\n") as f:
            f.write(text)
        print(f"{name}: {len(data)} bytes, {text.count('packet ')} packets, sha256 {hashlib.sha256(data).hexdigest()[:16]}")
        if name in DECODED:
            with open(f"{base}.frames.txt", "w", encoding="utf-8", newline="\n") as f:
                f.write(frames_answer(name, DECODED[name]))
        if name in SEEKABLE:
            with open(f"{base}.seek.txt", "w", encoding="utf-8", newline="\n") as f:
                f.write(seek_answer(name))


if __name__ == "__main__":
    main()
