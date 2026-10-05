#!/usr/bin/env python3
"""Generate the mp4 crate's fixtures and their answers.

Each fixture `NAME.mp4` gets `NAME.txt`: what FFmpeg's demuxer makes of it,
as `ffprobe` prints it -- a line per stream (index, codec tag, time base,
picture size or sound, duration in ticks, and a picture's frame rate) and a
line per packet (stream, pts, dts, duration, size, position, flags, the
data's MD5, and the samples to skip from its start). `tests/fixtures.rs`
holds the crate's tracks and packets to those lines. Fixtures it seeks in
get `NAME.seek.txt` too: the first packets after each seek. A fixture that
describes its picture gets a `look` line per stream as well (the pixel's
shape, the colour, the display matrix, the crop), and one FFmpeg refuses to
open is answered `refused`.

The packets are the demuxer's own: `-fflags +noparse+nofillin` keeps
FFmpeg's codec parsers and its generic layer's filling-in out of them. (The
skip-samples side data is the generic layer's -- it hands on the demuxer's
count with the first packet of the stream read -- and stays.)

Three kinds of fixture:

- **Written by ffmpeg**, as a player meets MP4: AV1 with its index after the
  media and before it (faststart); VP9 with Opus, whose priming the edit list
  trims; H.264 with B-frames, as ffmpeg writes their times by default and
  with negative offsets; MPEG-4 Part 2 with B-frames; AAC, whose priming
  makes the first packet's skip; a sound track starting later than the
  picture (an empty edit); and fragmented files, as a live stream or DASH
  writes them.
- **Written here**, byte by byte, for what muxers seldom write.
- **Described here**, byte by byte, for what a track says of its picture
  (`colr`, `vpcC`, `pasp`, `clap`, the display matrices) -- each in a code
  no decoder reads, so that what ffprobe prints is the demuxer's word, not a
  decoder's -- and two FFmpeg refuses.

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

FFDIR = sys.argv[1] if len(sys.argv) > 1 else "D:/utils"
FFMPEG = os.path.join(FFDIR, "ffmpeg.exe" if os.name == "nt" else "ffmpeg")
FFPROBE = os.path.join(FFDIR, "ffprobe.exe" if os.name == "nt" else "ffprobe")


def run(*args):
    subprocess.run(args, check=True, capture_output=True)


def ffmpeg(name, *args):
    # bitexact: no random UIDs or version strings, so a run makes the same
    # bytes as the last.
    run(FFMPEG, "-hide_banner", "-loglevel", "error", "-y", *args,
        "-fflags", "+bitexact", "-flags", "+bitexact", "-threads", "1", name)


LAVFI = ["-f", "lavfi", "-i"]
PICTURE = "testsrc=size=64x48:rate=10:duration=1.2"
SOUND = "sine=frequency=440:sample_rate=48000:duration=1.2"


def made_by_ffmpeg():
    """The fixtures ffmpeg writes: name -> bytes."""
    av1 = ["-c:v", "libaom-av1", "-pix_fmt", "yuv420p", "-cpu-used", "8", "-g", "4", "-b:v", "40k"]
    vp9 = ["-c:v", "libvpx-vp9", "-pix_fmt", "yuv420p", "-g", "5", "-b:v", "50k"]
    x264 = ["-c:v", "libx264", "-pix_fmt", "yuv420p", "-g", "6", "-bf", "2", "-preset", "veryfast"]
    opus = ["-c:a", "libopus", "-b:a", "24k", "-ac", "1"]
    files = {
        "av1.mp4": [*LAVFI, PICTURE, *av1],
        "faststart.mp4": [*LAVFI, PICTURE, *av1, "-movflags", "+faststart"],
        "vp9_opus.mp4": [*LAVFI, PICTURE, *LAVFI, SOUND, *vp9, *opus],
        "h264_bframes.mp4": [*LAVFI, PICTURE, *x264],
        "h264_negative_cts.mp4": [*LAVFI, PICTURE, *x264, "-movflags", "+negative_cts_offsets"],
        "mpeg4_bframes.mp4": [*LAVFI, PICTURE, "-c:v", "mpeg4", "-bf", "2", "-g", "6", "-q:v", "5"],
        "aac.mp4": [*LAVFI, PICTURE, *LAVFI, SOUND, *av1, "-c:a", "aac", "-b:a", "48k", "-ac", "1"],
        "delayed_audio.mp4": [*LAVFI, PICTURE, "-itsoffset", "0.2", *LAVFI, SOUND, *av1, *opus],
        "fragmented.mp4": [*LAVFI, PICTURE, *LAVFI, SOUND, *vp9, *opus,
                           "-movflags", "frag_keyframe+empty_moov"],
        "fragmented_moof_base.mp4": [*LAVFI, PICTURE, *LAVFI, SOUND, *av1, *opus,
                                     "-movflags", "frag_keyframe+empty_moov+default_base_moof"],
        # QuickTime with uncompressed sound, a sample a tick: FFmpeg reads it
        # in packets of up to 1024 samples a chunk.
        "pcm.mov": [*LAVFI, PICTURE, *LAVFI, SOUND, "-c:v", "mpeg4", "-q:v", "5", "-c:a", "pcm_s16le", "-f", "mov"],
    }
    out = {}
    for name, args in files.items():
        ffmpeg(name, *args)
        with open(name, "rb") as f:
            out[name] = f.read()
    return out


# --- MP4, written by hand --------------------------------------------------


def u8(v):
    return struct.pack(">B", v)


def u16(v):
    return struct.pack(">H", v & 0xFFFF)


def u32(v):
    return struct.pack(">I", v & 0xFFFFFFFF)


def i32(v):
    return struct.pack(">i", v)


def u64(v):
    return struct.pack(">Q", v)


def box(kind, *parts):
    payload = b"".join(parts)
    return u32(8 + len(payload)) + kind + payload


def full(kind, version, flags, *parts):
    return box(kind, u8(version), flags.to_bytes(3, "big"), *parts)


def matrix(*values):
    """A display matrix: nine words, row by row (16.16, 16.16, 2.30)."""
    return b"".join(u32(v) for v in values)


MATRIX = matrix(0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x40000000)


def mvhd(timescale, duration, display=MATRIX):
    return full(b"mvhd", 0, 0, u32(0), u32(0), u32(timescale), u32(duration), u32(0x10000), u16(0x100),
                bytes(10), display, bytes(24), u32(9))


def tkhd(track_id, duration, width=0, height=0, display=MATRIX):
    return full(b"tkhd", 0, 3, u32(0), u32(0), u32(track_id), u32(0), u32(duration), bytes(8), u16(0), u16(0),
                u16(0 if width else 0x100), u16(0), display, u32(width << 16), u32(height << 16))


def mdhd(timescale, duration):
    return full(b"mdhd", 0, 0, u32(0), u32(0), u32(timescale), u32(duration), u16(0x55C4), u16(0))


def hdlr(handler):
    return full(b"hdlr", 0, 0, u32(0), handler, bytes(12), b"fixture\0")


def visual_entry(fourcc, width=64, height=48, *children):
    return box(fourcc, bytes(6), u16(1), bytes(16), u16(width), u16(height), u32(0x480000), u32(0x480000),
               u32(0), u16(1), bytes(32), u16(0x18), u16(0xFFFF), *children)


def sound_entry(fourcc, channels=1, rate=48000, *children, bits=16):
    return box(fourcc, bytes(6), u16(1), u16(0), bytes(6), u16(channels), u16(bits), u16(0), u16(0),
               u32(rate << 16), *children)


def stsd(*entries):
    return full(b"stsd", 0, 0, u32(len(entries)), *entries)


def stts(entries):
    return full(b"stts", 0, 0, u32(len(entries)), *[u32(c) + u32(d) for c, d in entries])


def ctts(entries, version=0):
    return full(b"ctts", version, 0, u32(len(entries)), *[u32(c) + i32(o) for c, o in entries])


def stsc(entries):
    return full(b"stsc", 0, 0, u32(len(entries)), *[u32(f) + u32(c) + u32(i) for f, c, i in entries])


def stsz(sizes):
    return full(b"stsz", 0, 0, u32(0), u32(len(sizes)), *[u32(s) for s in sizes])


def stz2(sizes, field):
    bits = "".join(format(s, f"0{field}b") for s in sizes)
    bits += "0" * (-len(bits) % 8)
    raw = int(bits, 2).to_bytes(len(bits) // 8, "big") if bits else b""
    return full(b"stz2", 0, 0, bytes(3), u8(field), u32(len(sizes)), raw)


def stco(offsets):
    return full(b"stco", 0, 0, u32(len(offsets)), *[u32(o) for o in offsets])


def co64(offsets):
    return full(b"co64", 0, 0, u32(len(offsets)), *[u64(o) for o in offsets])


def stss(keys):
    return full(b"stss", 0, 0, u32(len(keys)), *[u32(k) for k in keys])


def stps(keys):
    return full(b"stps", 0, 0, u32(len(keys)), *[u32(k) for k in keys])


def elst(edits):
    return full(b"elst", 0, 0, u32(len(edits)), *[u32(d) + i32(t) + u32(0x10000) for d, t in edits])


def sbgp_rap(runs):
    return full(b"sbgp", 0, 0, b"rap ", u32(len(runs)), *[u32(c) + u32(i) for c, i in runs])


def colr(kind, primaries, transfer, space, full_range=None):
    """`nclx` with its range flag, or QuickTime's `nclc` without one."""
    flag = b"" if full_range is None else u8(0x80 if full_range else 0)
    return box(b"colr", kind, u16(primaries), u16(transfer), u16(space), flag)


def vpcc(version=1, packed=0x80, primaries=2, transfer=2, space=2, init=0, body=None):
    """VP9's configuration: `packed` is the bit depth (high four bits),
    chroma subsampling and the full-range flag (bit 0)."""
    if body is None:
        body = (u8(version) + bytes(3) + u8(0) + u8(10) + u8(packed) + u8(primaries) + u8(transfer)
                + u8(space) + u16(init) + bytes(init))
    return box(b"vpcC", body)


def pasp(h, v):
    return box(b"pasp", u32(h), u32(v))


def clap(width, height, offset_x, offset_y):
    """The clean aperture: four fractions, each (numerator, denominator)."""
    return box(b"clap", *[u32(v) for q in (width, height, offset_x, offset_y) for v in q])


class Trak:
    """A track to lay out: its samples (bytes, duration, key, composition
    offset), how many go in each chunk, and what its tables say."""

    def __init__(self, handler, entry, timescale, samples, chunks, *, edits=None, stss_keys="auto",
                 ctts_version=None, extra_stbl=b"", stsc_override=None, stsd_entries=None,
                 sizes="stsz", offsets="stco", stts_override=None, ctts_override=None, width=64, height=48,
                 display=MATRIX):
        self.handler, self.entry, self.timescale = handler, entry, timescale
        self.samples, self.chunks = samples, chunks
        self.edits, self.stss_keys, self.ctts_version = edits, stss_keys, ctts_version
        self.extra_stbl, self.stsc_override = extra_stbl, stsc_override
        self.stsd_entries = stsd_entries
        self.sizes, self.offsets = sizes, offsets
        self.stts_override, self.ctts_override = stts_override, ctts_override
        self.width, self.height = width, height
        self.display = display

    def chunk_data(self):
        out, at = [], 0
        for n in self.chunks:
            out.append(b"".join(s[0] for s in self.samples[at:at + n]))
            at += n
        return out

    def trak(self, track_id, offsets, movie_scale):
        durations = [s[1] for s in self.samples]
        media = sum(durations)
        runs = []
        for d in durations:
            if runs and runs[-1][1] == d:
                runs[-1][0] += 1
            else:
                runs.append([d and 1, d] if False else [1, d])
        stbl = [stsd(*(self.stsd_entries or [self.entry])),
                stts(self.stts_override if self.stts_override is not None else [tuple(r) for r in runs])]
        if self.ctts_override is not None:
            stbl.append(ctts(self.ctts_override, self.ctts_version or 0))
        elif self.ctts_version is not None:
            stbl.append(ctts([(1, s[3]) for s in self.samples], self.ctts_version))
        if self.stss_keys == "auto":
            stbl.append(stss([i + 1 for i, s in enumerate(self.samples) if s[2]]))
        elif self.stss_keys is not None:
            stbl.append(stss(self.stss_keys))
        if self.stsc_override is not None:
            stbl.append(stsc(self.stsc_override))
        else:
            entries, prev = [], None
            for i, n in enumerate(self.chunks):
                if n != prev:
                    entries.append((i + 1, n, 1))
                    prev = n
            stbl.append(stsc(entries))
        sizes = [len(s[0]) for s in self.samples]
        stbl.append(stsz(sizes) if self.sizes == "stsz" else stz2(sizes, self.sizes))
        stbl.append(stco(offsets) if self.offsets == "stco" else co64(offsets))
        stbl.append(self.extra_stbl)
        movie_duration = media * movie_scale // self.timescale
        header = vmhd = box(b"vmhd", bytes(12)) if self.handler == b"vide" else box(b"smhd", bytes(8))
        minf = box(b"minf", header, box(b"dinf", full(b"dref", 0, 0, u32(1), full(b"url ", 0, 1))),
                   box(b"stbl", *stbl))
        mdia = box(b"mdia", mdhd(self.timescale, media), hdlr(self.handler), minf)
        edts = box(b"edts", elst(self.edits)) if self.edits else b""
        w, h = (self.width, self.height) if self.handler == b"vide" else (0, 0)
        return box(b"trak", tkhd(track_id, movie_duration, w, h, self.display), edts, mdia)


def mp4(traks, movie_scale=1000, brand=b"isom", moov_kind=b"moov", mdat_header=None, cut=None,
        movie_display=MATRIX, moov_last=False, moov_to_end=False, last_trak_to_end=False):
    """A file: ftyp, moov, then mdat with each track's chunks interleaved
    chunk by chunk -- or, `moov_last`, ftyp, mdat, moov. `mdat_header` writes
    the mdat's header itself (a 64-bit size, or 0 for one running to the
    end); `moov_to_end` gives the moov a size of 0, running to the end of the
    file (so it must be last), and `last_trak_to_end` the moov's last trak,
    running to the end of the moov."""
    if moov_to_end and not moov_last:
        raise SystemExit("a moov running to the end of the file must be its last box")
    ftyp = box(b"ftyp", brand, u32(0x200), b"isom", b"iso2", b"mp41")
    chunks = [t.chunk_data() for t in traks]
    order = []
    for k in range(max(len(c) for c in chunks)):
        for ti, c in enumerate(chunks):
            if k < len(c):
                order.append((ti, k))

    def moov_with(offsets):
        duration = max(sum(s[1] for s in t.samples) * movie_scale // t.timescale for t in traks)
        boxes = [t.trak(i + 1, offsets[i], movie_scale) for i, t in enumerate(traks)]
        if last_trak_to_end:
            boxes[-1] = u32(0) + boxes[-1][4:]
        moov = box(moov_kind, mvhd(movie_scale, duration, movie_display) + b"".join(boxes))
        return u32(0) + moov[4:] if moov_to_end else moov

    zero = [[0] * len(c) for c in chunks]
    moov_len = len(moov_with(zero))
    payload_len = sum(len(chunks[ti][k]) for ti, k in order)
    header = mdat_header(payload_len) if mdat_header else u32(8 + payload_len) + b"mdat"
    at = len(ftyp) + (0 if moov_last else moov_len) + len(header)
    offsets = [[0] * len(c) for c in chunks]
    payload = b""
    for ti, k in order:
        offsets[ti][k] = at
        at += len(chunks[ti][k])
        payload += chunks[ti][k]
    if moov_last:
        data = ftyp + header + payload + moov_with(offsets)
    else:
        data = ftyp + moov_with(offsets) + header + payload
    return data[:cut] if cut else data


def sample(i, size=10):
    return bytes((i * 37 + j * 11) & 0xFF for j in range(size))


def video_samples(n, duration=1024, keys=(0,), cts=None, size=10):
    """`n` samples of 0.1 s at 10240 a second; key frames at `keys`."""
    return [(sample(i, size), duration, i in keys, (cts[i] if cts else 0)) for i in range(n)]


def synthetic():
    """The hand-written fixtures: name -> bytes."""
    out = {}
    # Codes FFmpeg knows no codec for, so that ffprobe decodes nothing of
    # these samples, which are not media; the handler makes each track
    # pictures or sound.
    av01 = visual_entry(b"vfx0")
    opus = sound_entry(b"afx0")
    v12 = video_samples(12, keys=(0, 4, 8))
    # Two edits: the first 0.3 s, then 0.3 s from 0.6 s in -- FFmpeg's
    # index replays the second from the key frame before it, marked to be
    # dropped.
    out["two_edits.mp4"] = mp4([Trak(b"vide", av01, 10240, v12, [4, 4, 4], edits=[(300, 0), (300, 6144)])])
    # An empty first edit: the picture starts 0.2 s late.
    out["empty_edit.mp4"] = mp4([Trak(b"vide", av01, 10240, v12, [6, 6], edits=[(200, -1), (1200, 0)])])
    # An edit from the middle of a group of pictures.
    out["edit_mid_gop.mp4"] = mp4([Trak(b"vide", av01, 10240, v12, [12], edits=[(900, 2560)])])
    # Sound whose edit starts inside its second frame: the frame is kept,
    # its sound before the edit skipped.
    s20 = [(sample(i, 8), 960, True, 0) for i in range(20)]
    out["sound_edit_mid_frame.mp4"] = mp4([Trak(b"soun", opus, 48000, s20, [5, 5, 5, 5], edits=[(300, 1500)])])
    # An stsc repaired as FFmpeg repairs it: its second entry repeats the
    # first's chunk.
    out["stsc_repair.mp4"] = mp4([Trak(b"vide", av01, 10240, v12, [2, 3, 3, 2, 2],
                                       stsc_override=[(1, 2, 1), (1, 3, 1), (4, 2, 1)])])
    # A delta past FFmpeg's limit, read as a negative correction.
    out["stts_negative.mp4"] = mp4([Trak(b"vide", av01, 10240, v12[:7], [7],
                                         stts_override=[(3, 1024), (1, 0xFFFFF000), (3, 1024)])])
    # No stss: every sample a key frame. An stss of no entries: only the
    # first.
    out["no_stss.mp4"] = mp4([Trak(b"vide", av01, 10240, v12, [12], stss_keys=None)])
    out["empty_stss.mp4"] = mp4([Trak(b"vide", av01, 10240, v12, [12], stss_keys=[])])
    # Partial sync samples (stps), and a 'rap ' sample group.
    out["stps.mp4"] = mp4([Trak(b"vide", av01, 10240, v12, [12], stss_keys=[1], extra_stbl=stps([6]))])
    out["rap_group.mp4"] = mp4([Trak(b"vide", av01, 10240, v12, [12], stss_keys=[1],
                                     extra_stbl=sbgp_rap([(3, 0), (1, 1), (8, 0)]))])
    # Composition offsets whose only negative ones are in the last two
    # entries, which FFmpeg's shift does not see.
    out["ctts_tail.mp4"] = mp4([Trak(b"vide", av01, 10240, video_samples(6, keys=(0,)), [6],
                                     ctts_version=1,
                                     ctts_override=[(1, 1024), (1, 2048), (1, 0), (1, 1024), (1, -1024), (1, 0)])])
    # Two sample entries of one codec, the chunks naming them in turn; and an
    # entry of another codec, whose samples FFmpeg leaves out.
    two = [visual_entry(b"vfx0"), visual_entry(b"vfx0")]
    out["two_entries.mp4"] = mp4([Trak(b"vide", av01, 10240, v12, [4, 4, 4], stsd_entries=two,
                                       stsc_override=[(1, 4, 1), (2, 4, 2), (3, 4, 1)])])
    other = [visual_entry(b"vfx0"), visual_entry(b"vfx1")]
    out["two_codecs.mp4"] = mp4([Trak(b"vide", av01, 10240, v12, [4, 4, 4], stsd_entries=other,
                                      stsc_override=[(1, 4, 1), (2, 4, 2), (3, 4, 1)])])
    # 64-bit chunk offsets, compact sample sizes, and a 64-bit mdat size.
    out["co64_stz2.mp4"] = mp4([Trak(b"vide", av01, 10240, v12, [6, 6], sizes=16, offsets="co64"),
                                Trak(b"soun", opus, 48000, s20, [10, 10], sizes=8)])
    out["stz2_4bit.mp4"] = mp4([Trak(b"vide", av01, 10240, video_samples(6, size=9), [6], sizes=4)])
    # A box's size in 64 bits -- an mdat's, before the moov, which is found
    # only by reading it -- and sizes of 0, running a box to its parent's end:
    # an mdat's, the file's last; a moov's, after the mdat; and a trak's, the
    # last in its moov.
    out["largesize.mp4"] = mp4([Trak(b"vide", av01, 10240, v12, [12])],
                               mdat_header=lambda n: u32(1) + b"mdat" + u64(16 + n), moov_last=True)
    out["mdat_to_end.mp4"] = mp4([Trak(b"vide", av01, 10240, v12, [12])],
                                 mdat_header=lambda n: u32(0) + b"mdat")
    out["moov_to_end.mp4"] = mp4([Trak(b"vide", av01, 10240, v12, [12])], moov_last=True,
                                 moov_to_end=True)
    out["trak_to_end.mp4"] = mp4([Trak(b"vide", av01, 10240, v12, [6, 6]),
                                  Trak(b"soun", opus, 48000, s20, [10, 10])], last_trak_to_end=True)
    # B-frames' composition offsets with an edit starting between a key
    # frame's decoding and its showing -- FFmpeg goes back a group of
    # pictures, to a key frame shown before the edit -- and one starting as a
    # key frame is shown, inside a run of the offsets, from which the rest
    # of the edit's offsets are counted.
    bframes = video_samples(12, keys=(0, 4, 8))
    runs = [(1, 1024), (1, 3072), (1, 0), (2, 2048), (2, 0), (2, 1024), (1, 3072), (2, 0)]
    out["edit_before_key_shows.mp4"] = mp4([Trak(b"vide", av01, 10240, bframes, [12], ctts_version=0,
                                                 ctts_override=runs, edits=[(300, 4608)])])
    out["edit_at_shown_key.mp4"] = mp4([Trak(b"vide", av01, 10240, bframes, [12], ctts_version=0,
                                             ctts_override=runs, edits=[(300, 6144)])])
    # A moov written as 'hoov', which FFmpeg reads as one.
    out["hoov.mp4"] = mp4([Trak(b"vide", av01, 10240, v12, [12])], moov_kind=b"hoov")
    # The mdat cut short: the samples past the end.
    out["truncated.mp4"] = mp4([Trak(b"vide", av01, 10240, v12, [12])], cut=-25)
    return out


def described():
    """Hand-written fixtures for what a track says of its picture -- its
    colour, its pixel's shape, its display matrix, its clean aperture -- each
    a picture track of a code no decoder has, so that what ffprobe prints of
    it is the demuxer's word alone: name -> bytes."""
    out = {}
    v8 = video_samples(8, keys=(0, 4))

    def one(name, *children, movie_display=MATRIX, **trak):
        entry = visual_entry(b"vfx0", 64, 48, *children)
        out[name] = mp4([Trak(b"vide", entry, 10240, v8, [8], **trak)], movie_display=movie_display)

    # Colour: nclx's three code points and its range; nclc's, which has no
    # range, over vpcC's; code points FFmpeg has no name for, which it reads
    # as unspecified; the rarest it has names for; vpcC alone, and in a
    # version FFmpeg passes over; an ICC profile, which says nothing here.
    one("colr_nclx.mp4", colr(b"nclx", 1, 1, 1, True))
    one("colr_nclc_after_vpcc.mp4", vpcc(packed=0x83, primaries=9, transfer=16, space=9),
        colr(b"nclc", 5, 6, 6))
    one("colr_unknown_codes.mp4", colr(b"nclx", 13, 19, 18, False))
    one("colr_rare_codes.mp4", colr(b"nclx", 22, 256, 17, True))
    one("vpcc.mp4", vpcc(packed=0x80, primaries=1, transfer=1, space=1))
    one("vpcc_version_0.mp4", vpcc(version=0, packed=0x81, primaries=9, transfer=16, space=9))
    one("colr_prof.mp4", box(b"colr", b"prof", bytes(16)))
    # The pixel's shape: pasp's, in lowest terms; a pasp with no vertical
    # spacing, passed over; one with no horizontal spacing, kept but not
    # used, so the track's size against the picture's decides; that alone;
    # and a display matrix that stretches one axis.
    one("pasp.mp4", pasp(4, 3))
    one("pasp_reduced.mp4", pasp(64, 48))
    one("pasp_no_vertical.mp4", pasp(5, 0))
    one("pasp_no_horizontal.mp4", pasp(0, 5), width=128)
    one("tkhd_size.mp4", width=96, height=48)
    one("matrix_stretch.mp4", display=matrix(0x20000, 0, 0, 0, 0x10000, 0, 0, 0, 0x40000000))
    # Rotation and mirroring: the track's matrix, the movie's, and both (the
    # track's, then the movie's).
    quarter = matrix(0, 0x10000, 0, -0x10000, 0, 0, 0, 0, 0x40000000)
    one("rotate_90.mp4", display=quarter)
    one("rotate_180.mp4", display=matrix(-0x10000, 0, 0, 0, -0x10000, 0, 0, 0, 0x40000000))
    one("rotate_270.mp4", display=matrix(0, -0x10000, 0, 0x10000, 0, 0, 0, 0, 0x40000000))
    one("mirror.mp4", display=matrix(-0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x40000000))
    one("movie_rotation.mp4", movie_display=quarter)
    one("both_rotations.mp4", display=quarter, movie_display=quarter)
    # The clean aperture: centred; moved off centre; a fractional width; one
    # wider than the picture, passed over; one reaching outside it, whose
    # negative left edge FFmpeg's C wraps past its own check, keeping a crop
    # no frame can take; and a valid one followed by an invalid one, the
    # first standing.
    one("clap.mp4", clap((48, 1), (40, 1), (0, 1), (0, 1)))
    one("clap_offset.mp4", clap((48, 1), (40, 1), (-4, 1), (2, 1)))
    one("clap_fraction.mp4", clap((95, 2), (40, 1), (0, 1), (0, 1)))
    one("clap_too_wide.mp4", clap((80, 1), (40, 1), (0, 1), (0, 1)))
    # Half a pixel too wide: only the width check refuses it -- its edges,
    # rounded toward zero, fit the picture.
    one("clap_wider_by_half.mp4", clap((129, 2), (40, 1), (0, 1), (0, 1)))
    one("clap_outside.mp4", clap((48, 1), (40, 1), (-40, 1), (0, 1)))
    one("clap_twice.mp4", clap((48, 1), (40, 1), (0, 1), (0, 1)),
        clap((80, 1), (40, 1), (0, 1), (0, 1)))
    # An offset of x/0, which FFmpeg's rationals make an infinite centre and
    # its C conversions make edges of: the crop comes out of how GCC turns
    # infinity into an unsigned 64-bit number.
    one("clap_infinite.mp4", clap((48, 1), (40, 1), (1, 0), (0, 1)))
    return out


def refused():
    """Hand-written files FFmpeg refuses to open: name -> bytes."""
    v8 = video_samples(8, keys=(0, 4))
    children = {
        # VP9 has no codec initialization data, and FFmpeg refuses a vpcC
        # that says it has some.
        "vpcc_init_data.mp4": vpcc(init=2),
        # A vpcC too short to hold its version and flags.
        "vpcc_short.mp4": vpcc(body=bytes(4)),
    }
    return {name: mp4([Trak(b"vide", visual_entry(b"vfx0", 64, 48, child), 10240, v8, [8])])
            for name, child in children.items()}


# --- the answers ---------------------------------------------------------------


def probe(*args):
    return subprocess.run(
        [FFPROBE, "-v", "error", *args],
        check=True, capture_output=True, text=True, encoding="utf-8",
    ).stdout.splitlines()


def fields(line):
    return dict(kv.split("=", 1) for kv in line.split("|") if "=" in kv)


def look(name):
    """A `look` line per stream: what the track says of its picture, as
    ffprobe prints it -- the pixel's shape, the colour (primaries, transfer,
    matrix, range, by FFmpeg's names), the display matrix and the crop."""
    out = subprocess.run([FFPROBE, "-v", "error", "-show_streams", "-of", "json", name],
                         check=True, capture_output=True, text=True, encoding="utf-8").stdout
    lines = []
    for st in json.loads(out)["streams"]:
        display, crop = "none", "0,0,0,0"
        for sd in st.get("side_data_list", []):
            if sd["side_data_type"] == "Display Matrix":
                rows = sd["displaymatrix"].strip().splitlines()
                display = ",".join(w for row in rows for w in row.split(":", 1)[1].split())
            elif sd["side_data_type"] == "Frame Cropping":
                crop = f"{sd['crop_left']},{sd['crop_top']},{sd['crop_right']},{sd['crop_bottom']}"
        colour = "/".join(st.get(k, "unknown") for k in
                          ("color_primaries", "color_transfer", "color_space", "color_range"))
        lines.append(f"look {st['index']} sar={st.get('sample_aspect_ratio', 'N/A')} colour={colour} "
                     f"matrix={display} crop={crop}")
    return lines


def answer(name, described=False):
    lines = [f"# {name}: what ffprobe makes of it (generate_fixtures.py)."]
    for s in probe("-show_entries", "stream=index,codec_type,codec_tag_string,time_base,width,height,"
                   "sample_rate,channels,duration_ts,r_frame_rate", "-of", "compact=p=0", name):
        f = fields(s)
        if f.get("codec_type") == "video":
            shape = f"{f.get('width', '0')}x{f.get('height', '0')}"
        elif f.get("codec_type") == "audio":
            shape = f"{f.get('sample_rate', '0')}Hz/{f.get('channels', '0')}"
        else:
            shape = "-"
        # The frame rate is the demuxer's where its rule finds one (every
        # sample but the last as long as the first) and ffprobe's own
        # estimate where not: tests/fixtures.rs holds the crate to it only
        # where the crate's rule finds one too.
        rate = f.get("r_frame_rate", "0/0") if f.get("codec_type") == "video" else "-"
        lines.append(f"stream {f['index']} {f['codec_tag_string']} {f['time_base']} {shape} "
                     f"{f.get('duration_ts', 'N/A')} {rate}")
    if described:
        lines += look(name)
    for p in probe("-fflags", "+noparse+nofillin", "-show_entries",
                   "packet=stream_index,pts,dts,duration,size,pos,flags,data_hash:packet_side_data",
                   "-show_data_hash", "MD5", "-of", "compact=p=0", name):
        f = fields(p)
        skip = f.get("side_datum/skip_samples:skip_samples", "0")
        lines.append(
            "packet {} {} {} {} {} {} {} {} {}".format(
                f["stream_index"], f["pts"], f["dts"], f["duration"], f["size"], f["pos"], f["flags"],
                f["data_hash"].removeprefix("MD5:"), skip,
            )
        )
    return "\n".join(lines) + "\n"


# Where each seek test seeks, in seconds, and how many packets it reads
# after: ffprobe's -read_intervals seeks the file's default stream (its video,
# or its sound when it has none) backward to the time.
SEEKS = [0.0, 0.25, 0.55, 0.9, 99.0]
SEEK_PACKETS = 6
SEEKABLE = {
    "av1.mp4": SEEKS,
    "vp9_opus.mp4": SEEKS,
    "h264_bframes.mp4": SEEKS,
    "h264_negative_cts.mp4": SEEKS,
    "mpeg4_bframes.mp4": SEEKS,
    "aac.mp4": SEEKS,
    "delayed_audio.mp4": SEEKS,
    "pcm.mov": SEEKS,
    "two_edits.mp4": [0.0, 0.15, 0.35, 0.5, 9.0],
    "empty_edit.mp4": SEEKS,
    "edit_mid_gop.mp4": SEEKS,
    "sound_edit_mid_frame.mp4": [0.0, 0.05, 0.12, 0.25],
    "stps.mp4": SEEKS,
    "rap_group.mp4": SEEKS,
    "ctts_tail.mp4": [0.0, 0.25, 0.4],
    "edit_before_key_shows.mp4": [0.0, 0.1, 0.2],
    "edit_at_shown_key.mp4": [0.0, 0.1, 0.2],
}


def seek_answer(name):
    lines = [f"# {name}: ffprobe's first {SEEK_PACKETS} packets after each seek (generate_fixtures.py)."]
    for s in SEEKABLE[name]:
        out = probe("-fflags", "+noparse+nofillin", "-read_intervals", f"{s}%+#{SEEK_PACKETS}",
                    "-show_entries", "packet=stream_index,pts,dts,flags:packet_side_data",
                    "-of", "compact=p=0", name)
        lines.append(f"seek {round(s * 1000)}")
        for p in out:
            f = fields(p)
            skip = f.get("side_datum/skip_samples:skip_samples", "0")
            lines.append(f"packet {f['stream_index']} {f['pts']} {f['dts']} {f['flags']} {skip}")
    return "\n".join(lines) + "\n"


def refusal(name):
    """A file FFmpeg refuses: ffprobe must fail on it."""
    r = subprocess.run([FFPROBE, "-v", "error", "-show_streams", name], capture_output=True, text=True,
                       encoding="utf-8")
    if r.returncode == 0:
        raise SystemExit(f"{name}: ffprobe opened it, and it was meant to be refused")
    why = r.stderr.strip().splitlines()[-1] if r.stderr.strip() else "no message"
    return f"# {name}: ffprobe refuses it ({why}) (generate_fixtures.py).\nrefused\n"


def main():
    files = {**made_by_ffmpeg(), **synthetic()}
    looks = described()
    refusals = refused()
    for name, data in sorted({**files, **looks, **refusals}.items()):
        with open(name, "wb") as f:
            f.write(data)
        text = refusal(name) if name in refusals else answer(name, described=name in looks)
        base = name.rsplit(".", 1)[0]
        with open(f"{base}.txt", "w", encoding="utf-8", newline="\n") as f:
            f.write(text)
        print(f"{name}: {len(data)} bytes, {text.count('packet ')} packets, sha256 {hashlib.sha256(data).hexdigest()[:16]}")
        if name in SEEKABLE:
            with open(f"{base}.seek.txt", "w", encoding="utf-8", newline="\n") as f:
                f.write(seek_answer(name))


if __name__ == "__main__":
    main()
