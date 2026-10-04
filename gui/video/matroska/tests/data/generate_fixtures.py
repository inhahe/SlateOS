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

Three kinds of fixture:

- **Written by ffmpeg** from its test sources: VP9 with Opus (Opus's codec
  delay, the last block's DiscardPadding), AV1, VP8 with Vorbis, VP9 with
  an alpha channel (BlockAdditions), a live-style file (a Segment of unknown
  size, no Cues), and subtitles in BlockGroups with durations.
- **Written here**, byte by byte, for what no muxer writes by default: the
  three lacings, header-stripping and zlib compression, Clusters of unknown
  size, BlockGroups with references, a Cluster with no Timestamp, negative
  block timestamps, a track FFmpeg ignores, and damage that FFmpeg reads past.
- **mediaprobe's**: the files lane E's `apps/mediaprobe` tested its own
  Matroska demuxer on (`src/mkv/demux/tests.rs`), laid out as its writer
  laid them out, from before this crate became the tree's one demuxer
  (`lane_e_layouts`).

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
    # Frames of no bytes, which FFmpeg makes no packet of -- unless the block
    # has additions, when it makes one of no bytes, even if the additions are
    # empty too; and a SimpleBlock with no bytes at all, which it passes
    # over. The codec ID is padded with zeros, as EBML allows a string to be.
    padded = el("AE", uint("D7", 1) + uint("73C5", 1) + uint("83", 1) + el("86", b"V_SNOW\0\0\0")
                + el("E0", uint("B0", 16) + uint("BA", 16)) + uint("23E383", 10_000_000))
    with_more = lambda rel, add_id, extra: el(  # noqa: E731
        "A0",
        el("A1", block(1, rel, 0, b""))
        + el("75A1", el("A6", uint("EE", add_id) + el("A5", extra))),
    )
    out["empty_frames.mkv"] = segment(
        INFO,
        el("1654AE6B", padded),
        cluster(
            0,
            simple(1, 0, 0x80, b""),
            simple(1, 10, 0x80 | 0x02, xiph([frame(1, 3), b"", frame(2, 4)])),
            with_more(40, 1, b"alpha"),
            with_more(50, 2, b""),
            el("A3", b""),
            simple(1, 60, 0x80, frame(3, 5)),
        ),
    )
    # A laced block of a zlib track whose second frame does not inflate:
    # FFmpeg gives the first, which it had queued, drops the rest of the
    # Cluster, and reads on from the next.
    deflated = el("6D80", el("6240", uint("5031", 0) + uint("5032", 1) + uint("5033", 0) + el("5034", uint("4254", 0))))
    out["zlib_laces.mkv"] = segment(
        INFO,
        el("1654AE6B", snow_track(1, 10_000_000, deflated)),
        cluster(
            0,
            simple(1, 0, 0x80 | 0x02, xiph([zlib.compress(frame(1, 10)), b"not zlib", zlib.compress(frame(2, 10))])),
            simple(1, 30, 0x80, zlib.compress(frame(3, 10))),
        ),
        cluster(100, simple(1, 0, 0x80, zlib.compress(frame(4, 10)))),
    )
    # A block claiming three bytes more than its Cluster has left -- bytes the
    # file does have, the next Cluster's first: FFmpeg drops the rest of the
    # Cluster and reads on from the next.
    overrunning = block(1, 10, 0x80, frame(2, 6))
    out["overrun.mkv"] = segment(
        INFO,
        one,
        cluster(0, simple(1, 0, 0x80, frame(1, 4)), bytes.fromhex("A3") + vint(len(overrunning) + 3) + overrunning),
        cluster(100, simple(1, 0, 0x80, frame(3, 4))),
    )
    # The tracks FFmpeg reads and those it passes over: a video track whose
    # codec ID is a sound's, one with no codec ID, and a second entry with
    # the first's number, whose blocks go to the first. A block of a track
    # no entry declares is damage.
    out["ignored_tracks.mkv"] = segment(
        INFO,
        el(
            "1654AE6B",
            snow_track(1, 10_000_000)
            + el("AE", uint("D7", 2) + uint("73C5", 2) + uint("83", 1) + string("86", "A_OPUS"))
            + el("AE", uint("D7", 3) + uint("73C5", 3) + uint("83", 1))
            + el("AE", uint("D7", 1) + uint("73C5", 4) + uint("83", 1) + string("86", "V_SNOW")
                 + el("E0", uint("B0", 8) + uint("BA", 8)) + uint("23E383", 20_000_000)),
        ),
        cluster(
            0,
            simple(1, 0, 0x80, frame(1, 4)),
            simple(2, 0, 0x80, frame(2, 4)),
            simple(3, 0, 0x80, frame(3, 4)),
            simple(1, 10, 0x00, frame(4, 4)),
            simple(7, 20, 0x80, frame(5, 4)),
            simple(1, 30, 0x00, frame(6, 4)),
        ),
        cluster(100, simple(1, 0, 0x80, frame(7, 4))),
    )
    # Info and Tracks after the Clusters, found through the SeekHead before
    # them (its positions eight bytes wide, so that its length does not
    # depend on them).
    clusters_first = cluster(0, simple(1, 0, 0x80, frame(1, 4)), simple(1, 10, 0x00, frame(2, 4)))
    seek_to = lambda id_hex, pos: el("4DBB", el("53AB", bytes.fromhex(id_hex)) + el("53AC", pos.to_bytes(8, "big")))  # noqa: E731
    head_len = len(el("114D9B74", seek_to("1549A966", 0) + seek_to("1654AE6B", 0)))
    at_info = head_len + len(clusters_first)
    head = el("114D9B74", seek_to("1549A966", at_info) + seek_to("1654AE6B", at_info + len(INFO)))
    out["tracks_at_the_end.mkv"] = segment(head, clusters_first, INFO, one)
    # After a seek to 1.5 s, which lands on the second Cluster, the two levels
    # of FFmpeg's dropping: the demuxer's, of everything before the key
    # frame's time until a key frame -- or, as here, until a frame of another
    # track that is not one ("keyframes not correctly marked") -- and its
    # generic layer's, of the seek's track until its key frame, which here
    # comes after a later frame of the track.
    out["reordered.mkv"] = segment(
        INFO,
        el("1654AE6B", snow_track(1, 10_000_000) + snow_track(2, 10_000_000)),
        cluster(0, simple(1, 0, 0x80, frame(1, 4)), simple(2, 0, 0x80, frame(2, 4)), simple(1, 40, 0x00, frame(3, 4))),
        cluster(
            1000,
            simple(2, 20, 0x00, frame(4, 4)),
            simple(2, -10, 0x00, frame(5, 4)),
            simple(1, 40, 0x00, frame(6, 4)),
            simple(1, 0, 0x80, frame(7, 4)),
            simple(2, 60, 0x00, frame(8, 4)),
            simple(1, 80, 0x00, frame(9, 4)),
        ),
    )
    # Where a resync starts: one byte past the last element FFmpeg knew at
    # its level -- not past an element it does not know (here 0x4DFF, between
    # a block and the damage), which it passes over without counting it good.
    # The block's frame holds the bytes of a whole Cluster, so a resync
    # begun past the block's first byte finds that one first.
    inner = cluster(500, simple(1, 0, 0x80, frame(2, 4)))
    out["resync_point.mkv"] = segment(
        INFO,
        one,
        cluster(0, simple(1, 0, 0x80, inner), el("4DFF", frame(3, 6)), b"\x00\x00"),
        cluster(1000, simple(1, 0, 0x80, frame(4, 4))),
    )
    # Inside a BlockGroup, the last element FFmpeg knew is its last child
    # read, not the group: a Block of a track no entry declares, whose bytes
    # hold a whole Cluster, then a BlockDuration -- the resync starts past
    # the BlockDuration's first byte, beyond the Cluster in the Block.
    # A seek's walk for key frames in a file without Cues reads every block as
    # playing does, so damage costs it what it costs playing: the bad block
    # at 1 s, which is not a key frame, sends it to the next Cluster, past
    # the key frame at 1.5 s, so a seek to 1.7 s lands at the start. And a key
    # frame is taken before its laces are read: the bad one at 3 s is where
    # a seek to 3.5 s lands, though nothing of it plays.
    bad_lace = bytes([1, 255, 255, 9, 1])
    out["walk_damage.mkv"] = segment(
        INFO,
        one,
        cluster(0, simple(1, 0, 0x80, frame(1, 4))),
        cluster(1000, simple(1, 0, 0x02, bad_lace), simple(1, 500, 0x80, frame(2, 4))),
        cluster(2000, simple(1, 0, 0x80, frame(3, 4))),
        cluster(3000, simple(1, 0, 0x80 | 0x02, bad_lace), simple(1, 40, 0x00, frame(4, 4))),
        cluster(4000, simple(1, 0, 0x80, frame(5, 4))),
    )
    bad_group = el("A0", el("A1", block(7, 0, 0, inner)) + uint("9B", 10))
    out["resync_in_group.mkv"] = segment(
        INFO,
        one,
        cluster(0, simple(1, 0, 0x80, frame(1, 4)), bad_group, simple(1, 20, 0x80, frame(3, 4))),
        cluster(1000, simple(1, 0, 0x80, frame(4, 4))),
    )
    out.update(lane_e_layouts())
    return out


# --- mediaprobe's ------------------------------------------------------------
#
# Lane E's `apps/mediaprobe` carried a Matroska demuxer of its own, tested on
# files its tests laid out block by block (`src/mkv/demux/tests.rs`, as of
# 2026-10-04). When this crate became the tree's one demuxer, those cases came
# here, written as that writer wrote them -- every size eight bytes long,
# every unsigned number eight bytes wide, an EBML header of a DocType alone --
# and held, as everything here is, to what FFmpeg makes of them. Two things a
# muxer would have written were added, without which ffprobe does not read the
# files: a whole OpusHead for the Opus track (FFmpeg's decoder refuses the
# eight bytes "OpusHead" alone, and ffprobe with it), and a picture size for
# the video track (without one FFmpeg scores the sound its default stream, and
# ffprobe seeks that).


def e_size(n):
    """A size as an EBML integer of eight bytes."""
    return b"\x01" + n.to_bytes(7, "big")


def e_el(id_hex, payload, size=None):
    return bytes.fromhex(id_hex) + (e_size(len(payload)) if size is None else size) + payload


def e_uint(id_hex, v):
    return e_el(id_hex, v.to_bytes(8, "big"))


E_VIDEO, E_AUDIO = 1, 2
# RFC 7845's identification header: version 1, one channel, a pre-skip of
# 312, 48 kHz, no gain, channel mapping 0.
OPUS_HEAD = b"OpusHead" + bytes([1, 1]) + (312).to_bytes(2, "little") + (48000).to_bytes(4, "little") + bytes(3)


def e_track(number, kind, codec, *extra):
    body = e_uint("D7", number) + e_uint("83", kind) + e_el("86", codec.encode())
    if kind == 1:
        body += e_el("E0", e_uint("B0", 64) + e_uint("BA", 48))
    return e_el("AE", body + b"".join(extra))


def e_two_tracks():
    """VP9 video at 25 frames a second (1), and Opus (2)."""
    return e_el(
        "1654AE6B",
        e_track(E_VIDEO, 1, "V_VP9", e_uint("23E383", 40_000_000))
        + e_track(E_AUDIO, 2, "A_OPUS", e_el("63A2", OPUS_HEAD)),
    )


def e_block(track, relative, flags, rest):
    return bytes([0x80 | track]) + relative.to_bytes(2, "big", signed=True) + bytes([flags]) + rest


def e_simple(track, relative, key, data):
    return e_el("A3", e_block(track, relative, 0x80 if key else 0, data))


def e_cluster(time, *blocks, size=None):
    return e_el("1F43B675", e_uint("E7", time) + b"".join(blocks), size)


E_HEADER = e_el("1A45DFA3", e_el("4282", b"webm"))
# A millisecond a tick, and three seconds long.
E_INFO = e_el("1549A966", e_uint("2AD7B1", 1_000_000) + e_el("4489", struct.pack(">d", 3000.0)))


def e_file(tracks, *rest):
    return E_HEADER + e_el("18538067", E_INFO + tracks + b"".join(rest))


def e_three_seconds(cue_track=None, cue_times=(0, 1000, 2000), listed=True):
    """Three Clusters a second apart, a key frame first in each. With
    `cue_track`, Cues for it at `cue_times`, after the Clusters, found as a
    muxer has them found -- through a SeekHead before Info -- unless not
    `listed`."""
    clusters = [
        e_cluster(s * 1000, e_simple(E_VIDEO, 0, True, f"key{s}".encode()),
                  e_simple(E_VIDEO, 40, False, f"inter{s}".encode()))
        for s in range(3)
    ]
    tracks = e_two_tracks()
    seek_head = lambda at: e_el("114D9B74", e_el("4DBB", e_el("53AB", bytes.fromhex("1C53BB6B")) + e_uint("53AC", at)))  # noqa: E731
    with_head = cue_track is not None and listed
    head = len(seek_head(0)) if with_head else 0
    at = head + len(E_INFO) + len(tracks)
    points = b""
    for s, c in enumerate(clusters):
        if s * 1000 in cue_times:
            points += e_el("BB", e_uint("B3", s * 1000) + e_el("B7", e_uint("F7", cue_track or 0) + e_uint("F1", at)))
        at += len(c)
    body = b""
    if with_head:
        body += seek_head(at)
    body += E_INFO + tracks + b"".join(clusters)
    if cue_track is not None:
        body += e_el("1C53BB6B", points)
    return E_HEADER + e_el("18538067", body)


def e_rolled(pre_roll, delay):
    """Three Clusters a second apart of one Opus track, whose decoder needs
    `pre_roll` nanoseconds of sound before its output is right, and whose
    frames are played `delay` nanoseconds before their blocks' times."""
    tracks = e_el("1654AE6B", e_track(E_AUDIO, 2, "A_OPUS", e_uint("56BB", pre_roll), e_uint("56AA", delay)))
    return e_file(tracks, *[e_cluster(s * 1000, e_simple(E_AUDIO, 0, True, f"a{s}".encode())) for s in range(3)])


def lane_e_layouts():
    out = {}
    v, a = E_VIDEO, E_AUDIO
    # Two Clusters' blocks, in order, timed from their Clusters -- one before
    # its own.
    out["order.mkv"] = e_file(
        e_two_tracks(),
        e_cluster(100, e_simple(v, 0, True, b"key"), e_simple(a, 5, True, b"sound"), e_simple(v, 40, False, b"inter")),
        e_cluster(180, e_simple(v, -1, False, b"late")),
    )
    # Each lacing, of frames of 300, 254 and 7 bytes: 254 is one Xiph size
    # byte short of the 255 that says "more", and a difference of -46 in two
    # bytes of EBML lacing.
    big, short, rest = bytes([1]) * 300, bytes([2]) * 254, bytes([3]) * 7
    xiph_laced = bytes([2, 255, 45, 254]) + big + short + rest
    ebml_laced_ = bytes([2, 0x41, 0x2C]) + ((8191 - 46) | 0x4000).to_bytes(2, "big") + big + short + rest
    fixed = bytes([2]) + bytes([4]) * 4 + bytes([5]) * 4 + bytes([6]) * 4
    out["lacings.mkv"] = e_file(
        e_two_tracks(),
        e_cluster(
            0,
            e_el("A3", e_block(v, 0, 0x80 | 0x02, xiph_laced)),
            e_el("A3", e_block(v, 200, 0x06, ebml_laced_)),
            e_el("A3", e_block(v, 400, 0x04, fixed)),
        ),
    )
    # Laces that do not add up: ten bytes in three fixed laces, and Xiph sizes
    # past the block. mediaprobe dropped the block; FFmpeg drops the rest of
    # its Cluster too, and reads on from the next.
    out["bad_laces.mkv"] = e_file(
        e_two_tracks(),
        e_cluster(
            0,
            e_el("A3", e_block(v, 0, 0x04, bytes([2, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]))),
            e_el("A3", e_block(v, 1, 0x02, bytes([1, 255, 255, 9, 1]))),
            e_simple(v, 2, True, b"after"),
        ),
        e_cluster(1000, e_simple(v, 0, True, b"next"), e_el("A3", e_block(v, 1, 0x02, bytes([1, 255, 255, 9, 1]))),
                  e_simple(v, 2, True, b"lost")),
        e_cluster(2000, e_simple(v, 0, True, b"last")),
    )
    # BlockGroups: a key frame without a ReferenceBlock, not with one.
    group = lambda rel, reference, data: e_el(  # noqa: E731
        "A0",
        e_el("A1", e_block(v, rel, 0, data)) + e_uint("9B", 33) + (e_el("FB", b"\xff") if reference else b""),
    )
    out["groups.mkv"] = e_file(e_two_tracks(), e_cluster(0, group(0, False, b"i"), group(33, True, b"p")))
    # Clusters of unknown size, each ending where the next begins.
    out["unknown_clusters.mkv"] = e_file(
        e_two_tracks(),
        e_cluster(0, e_simple(v, 0, True, b"one"), e_simple(v, 40, False, b"two"), size=UNKNOWN),
        e_cluster(80, e_simple(v, 0, False, b"three"), size=UNKNOWN),
        e_cluster(120, e_simple(v, 0, True, b"four")),
    )
    out["cued.mkv"] = e_three_seconds(cue_track=v)
    out["uncued.mkv"] = e_three_seconds()
    # The middle Cluster not cued, as a muxer cues key frames every few
    # seconds. Not seeked by ffprobe: its answer here depends on what it has
    # read before the seek (tests/beyond_ffprobe.rs).
    out["sparse_cues.mkv"] = e_three_seconds(cue_track=v, cue_times=(0, 2000))
    # Cues of the sound alone, at its second and third Clusters: the video's
    # seek walks.
    out["cues_of_another_track.mkv"] = e_three_seconds(cue_track=a, cue_times=(1000, 2000))
    # One cue point, which FFmpeg does not use: a seek walks.
    out["one_cue_point.mkv"] = e_three_seconds(cue_track=v, cue_times=(2000,))
    # The sparse Cues after the Clusters, with no SeekHead to say where: FFmpeg
    # finds Cues only before the first Cluster or through the SeekHead, and
    # passes over these while reading, so a seek walks -- even after reading
    # past them (tests/beyond_ffprobe.rs).
    out["unlisted_cues.mkv"] = e_three_seconds(cue_track=v, cue_times=(0, 2000), listed=False)
    # A codec delay (6.5 ms) is taken off its own track's timestamps alone.
    out["codec_delay.mkv"] = e_file(
        e_el("1654AE6B", e_track(v, 1, "V_VP9") + e_track(a, 2, "A_OPUS", e_uint("56AA", 6_500_000))),
        e_cluster(0, e_simple(a, 0, True, b"first"), e_simple(v, 0, True, b"picture"), e_simple(a, 20, True, b"second")),
    )
    # An 80 ms SeekPreRoll, which FFmpeg's seek does not act on (the caller
    # seeks that much earlier: tests/beyond_ffprobe.rs); and a 960 ms CodecDelay, which
    # puts the second Cluster's frame 40 ms in.
    out["pre_roll.mkv"] = e_rolled(80_000_000, 0)
    out["delayed.mkv"] = e_rolled(0, 960_000_000)
    # A tick of 100 microseconds, and no Duration.
    out["tick_100us.mkv"] = E_HEADER + e_el(
        "18538067",
        e_el("1549A966", e_uint("2AD7B1", 100_000)) + e_two_tracks() + e_cluster(100, e_simple(v, 50, True, b"x")),
    )
    # Header stripping, undone; and zlib, whose frame here is not zlib -- so
    # FFmpeg drops the rest of the Cluster.
    encoding = lambda algo, settings: e_el(  # noqa: E731
        "6D80", e_el("6240", e_el("5034", e_uint("4254", algo) + e_el("4255", settings))))
    aac = e_el("63A2", bytes([0x11, 0x90])) + e_el("E1", e_el("B5", struct.pack(">d", 48000.0)) + e_uint("9F", 2))
    out["encodings.mkv"] = e_file(
        e_el("1654AE6B", e_track(v, 1, "V_MPEG4/ISO/AVC", encoding(3, bytes([0, 0, 1])))
             + e_track(a, 2, "A_AAC", encoding(0, b""), aac)),
        e_cluster(0, e_simple(v, 0, True, b"rest"), e_simple(a, 0, True, b"zlib")),
    )
    # A child whose ID is no EBML number (a zero byte), inside a Cluster whose
    # size covers it, and two bytes of rubbish between it and the next.
    damaged = e_uint("E7", 0) + e_simple(v, 0, True, b"before") + bytes(3) + e_simple(v, 40, False, b"lost")
    out["resync.mkv"] = e_file(
        e_two_tracks(), e_el("1F43B675", damaged), b"\xde\xad", e_cluster(1000, e_simple(v, 0, True, b"after"))
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
# after: ffprobe's -read_intervals seeks the file's default stream (its video,
# or its sound when it has none) backward to the time, as `Demuxer::seek` on
# that track does.
SEEKS = [0.0, 0.25, 0.55, 1.2, 99.0]
SEEK_PACKETS = 6
# mediaprobe's seeks: on each side of each Cluster's start, and past the end.
THREE_SECONDS = [0.0, 0.5, 0.999, 1.0, 1.5, 2.0, 9.0]
SEEKABLE = {
    "vp9_opus.webm": SEEKS,
    "av1.webm": SEEKS,
    "vp8_vorbis.webm": SEEKS,
    "live.webm": SEEKS,
    "unknown_sizes.mkv": SEEKS,
    "laced.mkv": SEEKS,
    "cued.mkv": THREE_SECONDS,
    "uncued.mkv": THREE_SECONDS,
    "cues_of_another_track.mkv": THREE_SECONDS,
    "one_cue_point.mkv": THREE_SECONDS,
    "pre_roll.mkv": [0.03, 0.1, 1.05, 1.1],
    "delayed.mkv": [0.03, 0.1, 1.05, 1.1],
    "reordered.mkv": [0.0, 1.5],
    "walk_damage.mkv": [1.7, 3.5],
    # The walk undoes each frame's zlib as playing does: the second lace of
    # the first block does not inflate, so the key frame at 30 ms is never
    # walked to, and a seek to 50 ms lands at the start.
    "zlib_laces.mkv": [0.05],
    "unlisted_cues.mkv": THREE_SECONDS,
}


def seek_answer(name):
    lines = [f"# {name}: ffprobe's first {SEEK_PACKETS} packets after each seek (generate_fixtures.py)."]
    for s in SEEKABLE[name]:
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
