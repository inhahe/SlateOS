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

And the **projections** (`proj_*.mkv`, answered together in
`projections.txt`): a video track's `Projection` -- its pose, which turns
and mirrors the picture, and the spherical kinds -- each answered with the
display matrix ffprobe shows for it, `none`, or `refused` where FFmpeg will
not open the file. `tests/projection.rs` holds the crate to them.

The answers were made with the ffmpeg and ffprobe of gyan.dev's full build
of 2026-03-09 (git 9b7439c31b). Run from this directory:

    python generate_fixtures.py [path-to-ffmpeg-directory]
"""

import hashlib
import json
import os
import struct
import subprocess
import sys
import zlib
from decimal import Decimal

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
    # Elements of IDs the specification reserves -- value bits all ones
    # (0xFF, 0x7FFF) or all zeros (0x80) -- between the Tracks and the first
    # Cluster and inside it: FFmpeg reads each as an element it does not know
    # and passes over it, so the file opens and the frame after them comes.
    reserved = b"\xff\x82xx" + b"\x7f\xff\x82yy" + b"\x80\x82zz"
    out["reserved_ids.mkv"] = segment(
        INFO,
        one,
        reserved,
        cluster(0, simple(1, 0, 0x80, frame(1, 6)), reserved, simple(1, 40, 0x00, frame(2, 6))),
        cluster(100, simple(1, 0, 0x80, frame(3, 6))),
    )
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


# --- metadata -----------------------------------------------------------------
#
# Chapters, tags and attachments, and the metadata FFmpeg makes of them and
# of the Info and the tracks: each `meta_*.mkv` answered in `meta_*.meta.txt`
# (`meta_answer`), which `tests/metadata.rs` holds the crate to. Each file
# puts one group of FFmpeg's rules to the test; the comments say which.


def m_simple_tag(name=None, value=None, lang=None, default=None, bogus_default=None, subs=(), raw=b""):
    body = b""
    if name is not None:
        body += el("45A3", name if isinstance(name, bytes) else name.encode())
    if lang is not None:
        body += el("447A", lang if isinstance(lang, bytes) else lang.encode())
    if default is not None:
        body += uint("4484", default)
    if bogus_default is not None:
        body += uint("44B4", bogus_default)
    if value is not None:
        body += el("4487", value if isinstance(value, bytes) else value.encode())
    return el("67C8", body + b"".join(subs) + raw)


def m_targets(kind=None, type_value=None, track=None, chapter=None, attachment=None):
    body = b""
    if type_value is not None:
        body += uint("68CA", type_value)
    if kind is not None:
        body += el("63CA", kind.encode())
    if track is not None:
        body += uint("63C5", track)
    if chapter is not None:
        body += uint("63C4", chapter)
    if attachment is not None:
        body += uint("63C6", attachment)
    return el("63C0", body)


def m_tag(*children):
    return el("7373", b"".join(children))


def m_tags(*tags):
    return el("1254C367", b"".join(tags))


def m_atom(uid=None, start=None, end=None, titles=(), extra=b""):
    body = b""
    if uid is not None:
        body += uint("73C4", uid)
    if start is not None:
        body += uint("91", start)
    if end is not None:
        body += uint("92", end)
    for t in titles:
        body += el("80", (el("85", t.encode()) if t is not None else b"") + el("437C", b"eng"))
    return el("B6", body + extra)


def m_chapters(*editions):
    return el("1043A770", b"".join(el("45B9", uint("45BC", 1) + b"".join(atoms)) for atoms in editions))


def m_attached(uid=None, name=None, mime=None, data=None, description=None):
    body = b""
    if description is not None:
        body += el("467E", description.encode())
    if name is not None:
        body += el("466E", name.encode())
    if mime is not None:
        body += el("4660", mime.encode())
    if data is not None:
        body += el("465C", data)
    if uid is not None:
        body += uint("46AE", uid)
    return el("61A7", body)


def m_attachments(*files):
    return el("1941A469", b"".join(files))


def m_info(scale=1_000_000, title=None, muxer=None, date=None, duration=None):
    body = b""
    if scale is not None:
        body += uint("2AD7B1", scale)
    if title is not None:
        body += el("7BA9", title.encode())
    if muxer is not None:
        body += el("4D80", muxer.encode())
    body += el("5741", b"writer")
    if date is not None:
        body += el("4461", date)
    if duration is not None:
        body += el("4489", struct.pack(">d", duration))
    return el("1549A966", body)


def m_seek(id_hex, pos):
    # Positions four bytes wide, so that a layout's sizes do not depend on them.
    return el("4DBB", el("53AB", bytes.fromhex(id_hex)) + el("53AC", pos.to_bytes(4, "big")))


def m_offsets(entries, parts):
    """Where each of `parts` begins in the Segment's data, after the
    SeekHead `m_laid_out` puts first."""
    head = el("114D9B74", b"".join(m_seek(i, 0) for i, _ in entries))
    offsets, pos = {}, len(head)
    for name, b in parts:
        offsets[name] = pos
        pos += len(b)
    return offsets


def m_laid_out(entries, parts):
    """A Segment's body: a SeekHead (`entries`: (ID, part name) pairs) and
    then `parts` ((name, bytes) pairs) in order."""
    offsets = m_offsets(entries, parts)
    head = el("114D9B74", b"".join(m_seek(i, offsets[n]) for i, n in entries))
    return head + b"".join(b for _, b in parts)


def m_file(*children, size=None):
    return EBML_HEADER + el("18538067", b"".join(children), size)


def m_clusters():
    return cluster(0, simple(1, 0, 0x80, frame(1, 12)), simple(1, 40, 0x00, frame(2, 12)))


def m_snow(number, uid, extra=b"", video=b""):
    """A Snow track, as `snow_track`, with its UID apart from its number."""
    body = (
        uint("D7", number)
        + uint("73C5", uid)
        + uint("83", 1)
        + string("86", "V_SNOW")
        + el("E0", uint("B0", 16) + uint("BA", 16) + video)
    )
    return el("AE", body + extra)


def m_pictures():
    """A PNG and a JPEG, as ffmpeg writes them: attached pictures FFmpeg
    decodes when it probes them."""
    out = {}
    for ext in ("png", "jpg"):
        name = f"m_picture.{ext}"
        run(FFMPEG, "-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=16x16:rate=1",
            "-frames:v", "1", "-flags", "+bitexact", "-fflags", "+bitexact", name)
        with open(name, "rb") as f:
            out[ext] = f.read()
        os.remove(name)
    return out


# The metadata fixtures whose packets are answered too: those whose streams
# are their tracks alone (an attachment is a stream to ffprobe).
PACKETS_TOO = {"meta_tags.mkv", "meta_chapters.mkv", "meta_info.mkv", "meta_date.mkv", "meta_two_tracks.mkv",
               "meta_damaged_info.mkv"}


def metadata_fixtures():
    """The metadata fixtures: name -> bytes."""
    out = {}
    pics = m_pictures()

    # Written by ffmpeg: the file's metadata, a stream's, chapters from an
    # ffmetadata file (one with a key besides its title, which the muxer
    # writes as a tag naming the chapter), and attachments of four kinds.
    with open("m_meta.txt", "w", encoding="utf-8", newline="\n") as f:
        f.write(";FFMETADATA1\ntitle=A film\nartist=Someone\ncomment=Café ☕\n\n"
                "[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=150\ntitle=Opening\n\n"
                "[CHAPTER]\nTIMEBASE=1/1000\nSTART=150\nEND=400\ntitle=Second part\nartist=Guest\n")
    for ext, data in pics.items():
        with open(f"m_cover.{ext}", "wb") as f:
            f.write(data)
    with open("m_font.ttf", "wb") as f:
        f.write(b"not really a font")
    with open("m_blob.bin", "wb") as f:
        f.write(b"some bytes")
    lavfi = ["-f", "lavfi", "-i"]
    ffmpeg(
        "meta_ffmpeg.mkv",
        *lavfi, "testsrc=size=32x24:rate=10:duration=0.4",
        *lavfi, "sine=frequency=440:sample_rate=48000:duration=0.4",
        "-i", "m_meta.txt", "-map", "0", "-map", "1", "-map_metadata", "2", "-map_chapters", "2",
        "-c:v", "libvpx-vp9", "-b:v", "30k", "-c:a", "libopus", "-b:a", "24k", "-ac", "1",
        "-metadata:s:v:0", "title=The picture", "-metadata:s:a:0", "language=fre",
        "-metadata:s:a:0", "title=Sound",
        "-attach", "m_cover.png", "-metadata:s:2", "mimetype=image/png",
        "-attach", "m_font.ttf", "-metadata:s:3", "mimetype=application/x-truetype-font",
        "-attach", "m_blob.bin", "-metadata:s:4", "mimetype=application/octet-stream",
        "-attach", "m_cover.jpg", "-metadata:s:5", "mimetype=image/jpeg", "-metadata:s:5", "title=A cover",
    )
    for n in ("m_meta.txt", "m_cover.png", "m_cover.jpg", "m_font.ttf", "m_blob.bin"):
        os.remove(n)
    with open("meta_ffmpeg.mkv", "rb") as f:
        out["meta_ffmpeg.mkv"] = f.read()

    # Tags: a TargetType's prefix, languages with and without the default
    # flag (and its misspelt ID), nesting, a tag with no string removing its
    # key, a key replaced whatever its case, the renamed keys -- PART_NUMBER
    # beside a "track" already there, kept twice -- and tags for tracks: two
    # tracks of one UID both take a tag, a tag for a UID no track has is
    # dropped, a tag naming a chapter and a track goes to the (missing)
    # chapter, and a second Targets starts its UIDs afresh. And the tracks'
    # own metadata: a name, an empty one and none, a language, `und` and
    # none, a stereo mode, an alpha mode, and an encrypted track's key ID.
    encrypted = el("6D80", el("6240", uint("5031", 0) + uint("5032", 1) + uint("5033", 1)
                              + el("5035", uint("47E1", 5) + el("47E2", b"\x01\x02\x03\x04\x05"))))
    tracks = el("1654AE6B",
                m_snow(1, 11, el("536E", b"Picture") + el("22B59C", b"fre"))
                + m_snow(2, 22, el("536E", b"") + el("22B59C", b"und"))
                + m_snow(3, 22, video=uint("53B8", 1) + uint("53C0", 1))
                + m_snow(4, 44, encrypted)
                + m_snow(5, 55, video=uint("53B8", 15))
                + m_snow(6, 66, video=uint("53B8", 0)))
    deep = m_simple_tag("A", "a", subs=[m_simple_tag("B", "b", lang="fre", subs=[m_simple_tag("C", "c")])])
    tags = m_tags(
        m_tag(m_targets(kind="ALBUM", type_value=50),
              m_simple_tag("ARTIST", "Someone", subs=[m_simple_tag("SORT_WITH", "One, Some")]),
              m_simple_tag("TITLE", "Titre", lang="fre", default=0),
              m_simple_tag("TITLE", "Title", lang="eng", default=1),
              m_simple_tag("TITLE", "Titel", lang="ger", bogus_default=1)),
        m_tag(m_simple_tag("COMMENT", "first"),
              m_simple_tag("GONE", "here"),
              m_simple_tag("comment", "second"),
              # A key replaced in the middle: FFmpeg moves its last entry
              # (ORDER_C) into the gap and adds the new one at the end.
              m_simple_tag("ORDER_A", "1"),
              m_simple_tag("ORDER_B", "2"),
              m_simple_tag("ORDER_C", "3"),
              m_simple_tag("order_a", "4"),
              m_simple_tag("Gone"),
              m_simple_tag("track", "5"),
              m_simple_tag("PART_NUMBER", "7"),
              m_simple_tag("LEAD_PERFORMER", "Lead"),
              m_simple_tag(value="no name"),
              m_simple_tag("", "empty name"),
              m_simple_tag("EMPTY_LANG", "x", lang=""),
              m_simple_tag("NUL_LANG", "y", lang=b"\0\0\0"),
              m_simple_tag("NUL_VALUE", b"cut\0here"),
              m_simple_tag("K" * 1100, "long", lang="fre"),
              m_simple_tag("L" * 1100, "plain long"),
              deep),
        m_tag(m_simple_tag("PART_NUMBER", "5"), m_simple_tag("part_number", "9")),
        m_tag(m_targets(kind=""), m_simple_tag("SLASHED", "s")),
        m_tag(m_targets(track=22), m_simple_tag("TITLE", "Shared"), m_simple_tag("TAGGED", "yes")),
        m_tag(m_targets(track=99), m_simple_tag("LOST", "track")),
        m_tag(m_targets(track=11, chapter=5), m_simple_tag("LOST", "chapter")),
        m_tag(m_targets(track=11), m_targets(kind="SECOND"), m_simple_tag("AFRESH", "global")),
        m_tag(m_targets(track=44), m_simple_tag("ENCRYPTED", "track")),
    )
    out["meta_tags.mkv"] = m_file(m_info(title="Tagged", muxer="hand"), tracks, tags,
                                  cluster(0, simple(1, 0, 0x80, frame(1, 8))))

    # Chapters: two editions' top-level atoms, a nested atom left out, two
    # displays (the last title kept), atoms without a UID or a start left
    # out, a second start of 0 kept, a start before the last dropped, one
    # ending before it starts dropped but counted as started, a UID seen
    # again replacing its chapter in place (a display without a string
    # removing its title), a UID of 2^63 and more (FFmpeg's ID negative), and
    # ends filled in from the next start and the duration; tags for a
    # chapter whose UID two atoms share, and for a chapter dropped.
    big = (1 << 63) + 5
    chapters = m_chapters(
        [m_atom(1, 0, 1_000_000_000, ["One"], extra=m_atom(50, 100, None, ["Nested"])),
         m_atom(2, 0, None, ["Two", "Deux"]),
         m_atom(None, 1_500_000_000, None, ["No UID"]),
         m_atom(3, None, None, ["No start"]),
         m_atom(4, 2_000_000_000, 1_800_000_000, ["Backwards"]),
         m_atom(5, 1_900_000_000, None, ["Too early"]),
         m_atom(6, 2_500_000_000, None, ["Sechs", "Six"])],
        [m_atom(2, 2_600_000_000, None, [None]),
         m_atom(big, 2_800_000_000, None, ["Big"]),
         m_atom(7, 2_900_000_000, 2_950_000_000, [])],
    )
    tags = m_tags(m_tag(m_targets(chapter=2), m_simple_tag("ARTIST", "Chap"), m_simple_tag("TWICE", "x")),
                  m_tag(m_targets(chapter=4), m_simple_tag("LOST", "chapter")),
                  m_tag(m_targets(chapter=big), m_simple_tag("BIG", "yes")))
    out["meta_chapters.mkv"] = m_file(m_info(duration=3000.0), el("1654AE6B", snow_track(1)), chapters, tags,
                                      m_clusters())

    # Attachments: pictures (PNG; JPEG by a media type that only starts with
    # its name), fonts (one by a media type starting with another's name),
    # `binary`, one FFmpeg names nothing, one of an empty name; ones without
    # a media type, without data and with empty data, which FFmpeg leaves
    # out; two of one UID, which a tag for it names both of.
    files = m_attachments(
        m_attached(1, "cover.png", "image/png", pics["png"]),
        m_attached(2, "font.ttf", "application/x-font-ttf", b"font bytes"),
        m_attached(3, "", "binary", b"bin"),
        m_attached(4, "no-type", None, b"x"),
        m_attached(5, "no-data", "text/plain", None),
        m_attached(6, "empty", "text/plain", b""),
        m_attached(7, "photo.jpg", "image/jpeg; q=1", pics["jpg"], description="A photo"),
        m_attached(8, "open.otf", "application/vnd.ms-opentype", b"otf bytes"),
        m_attached(2, "font2.ttf", "application/x-truetype-font", b"second font"),
        m_attached(9, "notes.txt", "text/plain", b"plain text"),
    )
    tags = m_tags(m_tag(m_targets(attachment=2), m_simple_tag("NOTE", "font tag")),
                  m_tag(m_targets(attachment=4), m_simple_tag("LOST", "attachment")),
                  m_tag(m_targets(attachment=7), m_simple_tag("TITLE", "Retitled")))
    out["meta_attachments.mkv"] = m_file(m_info(), el("1654AE6B", snow_track(1)), files, tags, m_clusters())

    # Two Infos before the first Cluster, both read: the second starts the
    # timestamp scale and the duration afresh, and its empty MuxingApp and
    # its four-byte DateUTC replace the first's -- so there is no
    # creation_time -- while the title it lacks stays the first's. And a
    # third file whose date FFmpeg shows.
    date = struct.pack(">q", (1_577_934_245 - 978_307_200) * 1_000_000_000 + 678_901_234)
    out["meta_info.mkv"] = m_file(
        m_info(scale=500_000, title="first", muxer="muxer one", date=date, duration=4000.0),
        m_info(scale=None, muxer="", date=b"\0\0\0\0", duration=2000.0),
        el("1654AE6B", snow_track(1)), m_clusters())
    out["meta_date.mkv"] = m_file(m_info(title="", muxer="dated", date=date), el("1654AE6B", snow_track(1)),
                                  m_clusters())

    # An Info damaged partway, before the first Cluster -- its MuxingApp runs
    # past it: FFmpeg keeps the fields read before the damage (a tick of half
    # a millisecond, the title) and reads on, the Tracks once.
    bad_child = bytes.fromhex("4D80") + vint(100) + b"lost"
    damaged = el("1549A966", uint("2AD7B1", 500_000) + string("7BA9", "kept") + bad_child)
    out["meta_damaged_info.mkv"] = m_file(damaged, el("1654AE6B", snow_track(1)), m_clusters())

    # Two Tracks before the first Cluster: FFmpeg reads both, and a tag names
    # the second's track.
    out["meta_two_tracks.mkv"] = m_file(
        m_info(), el("1654AE6B", m_snow(1, 11)), el("1654AE6B", m_snow(2, 22, el("536E", b"Second"))),
        m_tags(m_tag(m_targets(track=22), m_simple_tag("FOUND", "yes"))),
        cluster(0, simple(1, 0, 0x80, frame(1, 8)), simple(2, 0, 0x80, frame(2, 8))))

    # Through the SeekHead: Tags at two positions after the Clusters, both
    # read; Chapters there too, not read, as some were met before the
    # Clusters; Attachments; an entry pointing at a Void, which reads as
    # nothing; and a chained SeekHead, whose Tags are read too.
    ch_linear = m_chapters([m_atom(1, 0, None, ["Linear"])])
    ch_late = m_chapters([m_atom(2, 500_000_000, None, ["Late"])])
    t1 = m_tags(m_tag(m_simple_tag("FIRST", "1")))
    t2 = m_tags(m_tag(m_simple_tag("SECOND", "2")))
    t3 = m_tags(m_tag(m_simple_tag("THIRD", "3")))
    att = m_attachments(m_attached(1, "a.bin", "binary", b"late"))
    void = el("EC", b"\0" * 4)
    # The chained SeekHead, written once t3's position is known: its size
    # does not depend on it.
    stub = el("114D9B74", m_seek("1254C367", 0))
    parts = [("ch1", ch_linear), ("info", m_info(duration=1000.0)), ("tracks", el("1654AE6B", snow_track(1))),
             ("c", m_clusters()), ("t1", t1), ("ch2", ch_late), ("att", att), ("void", void), ("t2", t2),
             ("sh2", stub), ("t3", t3)]
    entries = [("1549A966", "info"), ("1654AE6B", "tracks"), ("1254C367", "t1"), ("1043A770", "ch2"),
               ("1941A469", "att"), ("1254C367", "void"), ("1254C367", "t2"), ("114D9B74", "sh2")]
    t3_at = m_offsets(entries, parts)["t3"]
    parts[parts.index(("sh2", stub))] = ("sh2", el("114D9B74", m_seek("1254C367", t3_at)))
    out["meta_seek_head.mkv"] = m_file(m_laid_out(entries, parts))

    # A SeekHead entry naming Attachments that points at Tags: FFmpeg reads
    # the Tags there, and takes the Attachments for read -- so the entry
    # after it, at the real Attachments, is not followed.
    t4 = m_tags(m_tag(m_simple_tag("READ_AS", "tags")))
    att = m_attachments(m_attached(1, "never.bin", "binary", b"unread"))
    parts = [("info", m_info()), ("tracks", el("1654AE6B", snow_track(1))), ("c", m_clusters()),
             ("t4", t4), ("att", att)]
    out["meta_seek_mismatch.mkv"] = m_file(m_laid_out(
        [("1941A469", "t4"), ("1941A469", "att")], parts))

    return out


def e_cue_layouts():
    """Cues the SeekHead points at twice, laid out as `e_three_seconds`:
    three Clusters a second apart, then Cues -- 'bad' ones, every time at the
    last Cluster, and good ones. FFmpeg takes the Cues from where the last
    entry naming them points; and none at all once following the SeekHead
    has failed, which marks its index broken -- so a seek walks."""
    v = E_VIDEO
    clusters = [
        e_cluster(s * 1000, e_simple(v, 0, True, f"key{s}".encode()),
                  e_simple(v, 40, False, f"inter{s}".encode()))
        for s in range(3)
    ]
    tracks = e_two_tracks()

    def head(entries):
        return e_el("114D9B74", b"".join(
            e_el("4DBB", e_el("53AB", bytes.fromhex(i)) + e_uint("53AC", p)) for i, p in entries))

    def cues(points):
        return e_el("1C53BB6B", b"".join(
            e_el("BB", e_uint("B3", t) + e_el("B7", e_uint("F7", v) + e_uint("F1", p))) for t, p in points))

    def build(kinds):
        """`kinds`, the SeekHead's entries in order: 'bad' or 'good' Cues, or
        'broken' Tags (a tag running past its parent)."""
        at = len(head([("1C53BB6B", 0)] * len(kinds))) + len(E_INFO) + len(tracks)
        starts = []
        for c in clusters:
            starts.append(at)
            at += len(c)
        made = {
            "good": cues([(0, starts[0]), (1000, starts[1]), (2000, starts[2])]),
            "bad": cues([(0, starts[2]), (1000, starts[2]), (2000, starts[2])]),
            # A TagString of 8 bytes, with 2 left in its SimpleTag.
            "broken": e_el("1254C367", e_el("7373", e_el("67C8", e_el("45A3", b"X") + b"\x44\x87\x88ab"))),
        }
        entries, tail = [], b""
        for k in kinds:
            entries.append(("1254C367" if k == "broken" else "1C53BB6B", at + len(tail)))
            tail += made[k]
        return E_HEADER + e_el("18538067", head(entries) + E_INFO + tracks + b"".join(clusters) + tail)

    out = {
        "cues_last_entry.mkv": build(["bad", "good"]),
        "cues_broken.mkv": build(["bad", "broken"]),
    }

    # Four Clusters ten seconds apart, for Cues FFmpeg's probing does not
    # hide (`SEEK_PROBING`).
    far = [
        e_cluster(s * 10_000, e_simple(v, 0, True, f"key{s}".encode()),
                  e_simple(v, 40, False, f"inter{s}".encode()))
        for s in range(4)
    ]

    def starts_after(at):
        out = []
        for c in far:
            out.append(at)
            at += len(c)
        return out

    # Two Cues elements before the first Cluster: FFmpeg's index is both --
    # the second's point at 20 s is where a seek to 25 s lands.
    first = lambda s: cues([(0, s[0]), (30_000, s[3])])  # noqa: E731
    second = lambda s: cues([(20_000, s[2])])  # noqa: E731
    stub = [0, 0, 0, 0]
    s = starts_after(len(E_INFO) + len(tracks) + len(first(stub)) + len(second(stub)))
    out["two_cues.mkv"] = E_HEADER + e_el("18538067", E_INFO + tracks + first(s) + second(s) + b"".join(far))

    # Cues after the Clusters, through the SeekHead, whose third point is
    # damaged (its CueTime runs past it): FFmpeg keeps the two before -- and
    # the second sends 20 s to the last Cluster, so a seek to 25 s lands at
    # 30 s, where a walk would land at 20.
    def damaged(s):
        # The two good points (the Cues element's own 12-byte header cut
        # off), then a CuePoint whose CueTime claims 8 bytes of its 1.
        points = cues([(0, s[0]), (20_000, s[3])])[12:]
        return e_el("1C53BB6B", points + e_el("BB", b"\xb3\x88\x00"))

    at = len(head([("1C53BB6B", 0)])) + len(E_INFO) + len(tracks)
    s = starts_after(at)
    tail_at = s[3] + len(far[3])
    out["damaged_cues.mkv"] = E_HEADER + e_el(
        "18538067", head([("1C53BB6B", tail_at)]) + E_INFO + tracks + b"".join(far) + damaged(s))
    return out


def m_trailing():
    """The file the truncation sweep cuts: metadata after the Clusters, found
    through the SeekHead -- chapters, attachments (a picture among them) and
    tags of the file, a track, a chapter and an attachment."""
    pics = m_pictures()
    chapters = m_chapters([m_atom(1, 0, 400_000_000, ["Start"]),
                           m_atom(2, 400_000_000, None, ["Middle", "Milieu"]),
                           m_atom(3, 700_000_000, 900_000_000, ["End"])])
    files = m_attachments(m_attached(1, "cover.png", "image/png", pics["png"], description="Front"),
                          m_attached(2, "f.ttf", "application/x-truetype-font", b"glyphs" * 5))
    tags = m_tags(
        m_tag(m_targets(kind="ALBUM"), m_simple_tag("ARTIST", "Someone", subs=[m_simple_tag("URL", "u")]),
              m_simple_tag("TITLE", "Titre", lang="fre", default=1)),
        m_tag(m_targets(track=1), m_simple_tag("PART_NUMBER", "3"), m_simple_tag("ENCODER", "x")),
        m_tag(m_targets(chapter=2), m_simple_tag("TITLE", "Tagged middle")),
        m_tag(m_targets(attachment=2), m_simple_tag("NOTE", "a font")),
    )
    parts = [("info", m_info(title="Trailing", muxer="hand", duration=1000.0)),
             ("tracks", el("1654AE6B", snow_track(1))), ("c", m_clusters()),
             ("ch", chapters), ("att", files), ("tags", tags)]
    entries = [("1549A966", "info"), ("1654AE6B", "tracks"), ("1043A770", "ch"), ("1941A469", "att"),
               ("1254C367", "tags")]
    data = m_file(m_laid_out(entries, parts))
    # Where the metadata begins: the sweep cuts from there to the end.
    return data, data.index(chapters)


def esc(b):
    """Bytes as the answers write them: printable ASCII as it is, the rest
    (and the backslash) as \\x and two hex digits."""
    return "".join(chr(c) if 0x20 <= c < 0x7F and c != 0x5C else f"\\x{c:02x}" for c in b)


def m_get(pairs, key, default=None):
    for k, v in pairs:
        if k == key:
            return v
    return default


def meta_lines(name):
    """What ffprobe shows of a file's metadata, a line each: `start` (the
    file's start in microseconds, which FFmpeg ends chapters by), the file's
    tags, each stream -- a track, an attachment or a picture, the last two
    with their size and MD5 -- and its tags, then each chapter (its ID, start
    and end in nanoseconds) and its tags. `refused` if ffprobe will not open
    it."""
    r = subprocess.run(
        [FFPROBE, "-v", "quiet", "-show_format", "-show_streams", "-show_chapters", "-show_packets",
         "-show_data_hash", "MD5", "-of", "json", name],
        capture_output=True, text=True, encoding="utf-8",
    )
    if r.returncode != 0:
        return ["refused"]
    # Pairs, not dicts: a key may come twice in FFmpeg's metadata.
    j = json.loads(r.stdout, object_pairs_hook=list)
    fmt = m_get(j, "format", [])
    start = m_get(fmt, "start_time")
    lines = ["start " + ("none" if start in (None, "N/A") else str(int(Decimal(start) * 1_000_000)))]

    def tag_lines(obj):
        for k, v in m_get(obj, "tags", []):
            lines.append("tag " + esc(k.encode()) + "=" + esc(v.encode()))

    lines.append("format")
    tag_lines(fmt)
    packets = m_get(j, "packets", [])
    for s in m_get(j, "streams", []):
        index = m_get(s, "index")
        codec = m_get(s, "codec_name") or "none"
        if m_get(m_get(s, "disposition", []), "attached_pic", 0) == 1:
            p = next(p for p in packets if m_get(p, "stream_index") == index)
            lines.append(f"stream {index} picture {codec} {m_get(p, 'size')} "
                         f"{m_get(p, 'data_hash').removeprefix('MD5:')}")
        elif m_get(s, "codec_type") == "attachment":
            lines.append(f"stream {index} attachment {codec} {m_get(s, 'extradata_size')} "
                         f"{m_get(s, 'extradata_hash').removeprefix('MD5:')}")
        else:
            lines.append(f"stream {index} track")
        tag_lines(s)
    for c in m_get(j, "chapters", []):
        lines.append(f"chapter {m_get(c, 'id')} {m_get(c, 'start')} {m_get(c, 'end')}")
        tag_lines(c)
    return lines


def meta_answer(name):
    lines = [f"# {name}: the metadata ffprobe shows (generate_fixtures.py)."] + meta_lines(name)
    return "\n".join(lines) + "\n"


def sweep_answer(name, data, first):
    """Each cut of `data` from `first` to its end: the MD5 of `meta_lines`,
    without its `start` line -- given apart, as the test needs it."""
    lines = [f"# {name} cut short at each length from {first}: `start`, and the MD5 of the rest "
             "of what ffprobe shows (generate_fixtures.py)."]
    cut = "m_cut.mkv"
    for n in range(first, len(data)):
        with open(cut, "wb") as f:
            f.write(data[:n])
        got = meta_lines(cut)
        if got == ["refused"]:
            lines.append(f"cut {n} refused")
            continue
        text = "\n".join(got[1:]) + "\n"
        lines.append(f"cut {n} {got[0].split(' ', 1)[1]} {hashlib.md5(text.encode()).hexdigest()}")
    os.remove(cut)
    return "\n".join(lines) + "\n"


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
    "cues_last_entry.mkv": THREE_SECONDS,
    "cues_broken.mkv": THREE_SECONDS,
    "two_cues.mkv": [5.0, 25.0, 35.0],
    "damaged_cues.mkv": [5.0, 25.0],
}
# Files whose seeks ffprobe makes having probed as little as it can: FFmpeg
# indexes every key frame it reads, and its probing of a file this small
# reads them all, which hides what the Cues say. Probing 32 bytes, it reads
# the first two Clusters' key frames (0 and 10 s), so these files' Cues are
# about later ones, and their seeks avoid the times those two would answer.
SEEK_PROBING = {
    "two_cues.mkv": ["-probesize", "32", "-analyzeduration", "1"],
    "damaged_cues.mkv": ["-probesize", "32", "-analyzeduration", "1"],
}


def seek_answer(name):
    lines = [f"# {name}: ffprobe's first {SEEK_PACKETS} packets after each seek (generate_fixtures.py)."]
    for s in SEEKABLE[name]:
        out = subprocess.run(
            [FFPROBE, "-v", "error", "-fflags", "+noparse+nofillin", *SEEK_PROBING.get(name, []),
             "-read_intervals", f"{s}%+#{SEEK_PACKETS}",
             "-show_entries", "packet=stream_index,pts,flags", "-of", "compact=p=0", name],
            check=True, capture_output=True, text=True, encoding="utf-8",
        ).stdout.splitlines()
        lines.append(f"seek {round(s * 1000)}")
        for p in out:
            f = dict(kv.split("=", 1) for kv in p.split("|") if "=" in kv)
            lines.append(f"packet {f['stream_index']} {f['pts']} {f['flags']}")
    return "\n".join(lines) + "\n"


# --- projections ------------------------------------------------------------


def double(id_hex, v):
    return el(id_hex, struct.pack(">d", v))


def projection(kind=None, private=None, yaw=None, pitch=None, roll=None):
    body = b""
    if kind is not None:
        body += uint("7671", kind)
    if private is not None:
        body += el("7672", private)
    for id_hex, v in (("7673", yaw), ("7674", pitch), ("7675", roll)):
        if v is not None:
            body += double(id_hex, v)
    return el("7670", body)


def projected(proj, kind=1, codec="V_SNOW"):
    """A file of one track -- video unless `kind` says sound -- whose Video
    element holds `proj`, and a frame."""
    video = el("E0", uint("B0", 16) + uint("BA", 16) + proj)
    track = el("AE", uint("D7", 1) + uint("73C5", 1) + uint("83", kind) + string("86", codec) + video)
    return EBML_HEADER + el("18538067", INFO + el("1654AE6B", track)
                            + cluster(0, simple(1, 0, 0x80, frame(1, 16))))


def be32(*words):
    return b"".join(w.to_bytes(4, "big") for w in words)


def projections():
    """name -> bytes, each a pose or a spherical projection."""
    nan = struct.unpack(">d", bytes.fromhex("7ff8000000000000"))[0]
    return {
        # Rectangular, the type said or not: a roll turns, a yaw of 180
        # mirrors (applied first, as the specification has it).
        "proj_roll_90.mkv": projected(projection(kind=0, roll=90.0)),
        "proj_roll_minus_90.mkv": projected(projection(roll=-90.0)),
        "proj_roll_180.mkv": projected(projection(kind=0, roll=180.0)),
        "proj_roll_30.mkv": projected(projection(kind=0, roll=30.0)),
        "proj_mirror.mkv": projected(projection(kind=0, yaw=180.0)),
        "proj_mirror_roll.mkv": projected(projection(kind=0, yaw=180.0, roll=-90.0)),
        "proj_mirror_minus_180.mkv": projected(projection(kind=0, yaw=-180.0, roll=90.0)),
        # Poses FFmpeg does not apply: none at all, a pitch, a yaw but a
        # mirror's, a roll that is not a number; and spherical metadata of a
        # version it does not know.
        "proj_still.mkv": projected(projection(kind=0)),
        "proj_pitch.mkv": projected(projection(kind=0, pitch=10.0, roll=90.0)),
        "proj_yaw_90.mkv": projected(projection(kind=0, yaw=90.0)),
        "proj_roll_nan.mkv": projected(projection(kind=0, roll=nan)),
        "proj_private_version.mkv": projected(projection(kind=0, private=bytes([1, 0, 0, 0]), roll=90.0)),
        # Spherical: equirectangular with its 20 bytes or none, and with
        # bounds that overflow or the wrong size; cubemap with its 12 bytes,
        # a layout FFmpeg does not know, too few bytes, or the wrong size; a
        # mesh. And the same faults in a sound track's Video element, which
        # FFmpeg does not look at.
        "proj_equirect.mkv": projected(projection(kind=1, private=be32(0, 1, 2, 3, 4))),
        "proj_equirect_empty.mkv": projected(projection(kind=1)),
        "proj_equirect_bounds.mkv": projected(projection(kind=1, private=be32(0, 0xFFFFFFF0, 0x20, 0, 0))),
        "proj_equirect_size.mkv": projected(projection(kind=1, private=bytes(7))),
        "proj_cubemap.mkv": projected(projection(kind=2, private=be32(0, 0, 8))),
        "proj_cubemap_layout.mkv": projected(projection(kind=2, private=be32(0, 1, 0))),
        "proj_cubemap_short.mkv": projected(projection(kind=2, private=bytes(3))),
        "proj_cubemap_size.mkv": projected(projection(kind=2, private=bytes(8))),
        "proj_mesh.mkv": projected(projection(kind=3, roll=90.0)),
        "proj_sound_track.mkv": projected(projection(kind=1, private=bytes(7)), kind=2, codec="A_OPUS"),
    }


def projection_answer(name):
    """The display matrix ffprobe shows for the file's first stream, nine
    numbers; `none`; or `refused`."""
    r = subprocess.run([FFPROBE, "-v", "error", "-show_entries", "stream_side_data=displaymatrix",
                        "-of", "compact=p=0", name], capture_output=True, text=True, encoding="utf-8")
    if r.returncode != 0:
        return "refused"
    for line in r.stdout.splitlines():
        if "displaymatrix=" in line:
            rows = line.split("displaymatrix=", 1)[1].replace("\\n", "\n").split("\n")
            words = [w for row in rows if ":" in row for w in row.split(":", 1)[1].split()]
            return ",".join(words)
    return "none"


def main():
    looks = projections()
    lines = ["# Each proj_*.mkv: the display matrix ffprobe shows for its projection, `none`, or",
             "# `refused` where ffprobe will not open it (generate_fixtures.py)."]
    for name, data in sorted(looks.items()):
        with open(name, "wb") as f:
            f.write(data)
        lines.append(f"{name} {projection_answer(name)}")
    with open("projections.txt", "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines) + "\n")
    files = {**made_by_ffmpeg(), **synthetic(), **e_cue_layouts()}
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
    meta = metadata_fixtures()
    for name, data in sorted(meta.items()):
        with open(name, "wb") as f:
            f.write(data)
        base = name.rsplit(".", 1)[0]
        with open(f"{base}.meta.txt", "w", encoding="utf-8", newline="\n") as f:
            f.write(meta_answer(name))
        # Their packets too, where FFmpeg's streams are the tracks alone.
        if name in PACKETS_TOO:
            with open(f"{base}.txt", "w", encoding="utf-8", newline="\n") as f:
                f.write(answer(name))
        print(f"{name}: {len(data)} bytes, sha256 {hashlib.sha256(data).hexdigest()[:16]}")
    trailing, first = m_trailing()
    with open("meta_trailing.mkv", "wb") as f:
        f.write(trailing)
    with open("meta_trailing.meta.txt", "w", encoding="utf-8", newline="\n") as f:
        f.write(meta_answer("meta_trailing.mkv"))
    with open("meta_trailing.cut.txt", "w", encoding="utf-8", newline="\n") as f:
        f.write(sweep_answer("meta_trailing.mkv", trailing, first))
    print(f"meta_trailing.mkv: {len(trailing)} bytes, cut from {first}")


if __name__ == "__main__":
    main()
