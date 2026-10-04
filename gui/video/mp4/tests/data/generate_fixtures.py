#!/usr/bin/env python3
"""Generate the mp4 crate's fixtures and their answers.

Each fixture `NAME.mp4` gets `NAME.txt`: what FFmpeg's demuxer makes of it,
as `ffprobe` prints it -- a line per stream (index, codec tag, time base)
and a line per packet (stream, pts, dts, duration, size, position, flags,
the data's MD5, and the samples to skip from its start). `tests/fixtures.rs`
holds the crate's packets to those lines. Fixtures it seeks in get
`NAME.seek.txt` too: the first packets after each seek.

The packets are the demuxer's own: `-fflags +noparse+nofillin` keeps
FFmpeg's codec parsers and its generic layer's filling-in out of them. (The
skip-samples side data is the generic layer's -- it hands on the demuxer's
count with the first packet of the stream read -- and stays.)

Two kinds of fixture:

- **Written by ffmpeg**, as a player meets MP4: AV1 with its index after the
  media and before it (faststart); VP9 with Opus, whose priming the edit list
  trims; H.264 with B-frames, as ffmpeg writes their times by default and
  with negative offsets; MPEG-4 Part 2 with B-frames; AAC, whose priming
  makes the first packet's skip; a sound track starting later than the
  picture (an empty edit); and fragmented files, as a live stream or DASH
  writes them.
- **Written here**, byte by byte, for what muxers seldom write.

The answers were made with the ffmpeg and ffprobe of gyan.dev's full build
of 2026-03-09 (git 9b7439c31b). Run from this directory:

    python generate_fixtures.py [path-to-ffmpeg-directory]
"""

import hashlib
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


MATRIX = b"".join(u32(v) for v in (0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x40000000))


def mvhd(timescale, duration):
    return full(b"mvhd", 0, 0, u32(0), u32(0), u32(timescale), u32(duration), u32(0x10000), u16(0x100),
                bytes(10), MATRIX, bytes(24), u32(9))


def tkhd(track_id, duration, width=0, height=0):
    return full(b"tkhd", 0, 3, u32(0), u32(0), u32(track_id), u32(0), u32(duration), bytes(8), u16(0), u16(0),
                u16(0 if width else 0x100), u16(0), MATRIX, u32(width << 16), u32(height << 16))


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


class Trak:
    """A track to lay out: its samples (bytes, duration, key, composition
    offset), how many go in each chunk, and what its tables say."""

    def __init__(self, handler, entry, timescale, samples, chunks, *, edits=None, stss_keys="auto",
                 ctts_version=None, extra_stbl=b"", stsc_override=None, stsd_entries=None,
                 sizes="stsz", offsets="stco", stts_override=None, ctts_override=None, width=64, height=48):
        self.handler, self.entry, self.timescale = handler, entry, timescale
        self.samples, self.chunks = samples, chunks
        self.edits, self.stss_keys, self.ctts_version = edits, stss_keys, ctts_version
        self.extra_stbl, self.stsc_override = extra_stbl, stsc_override
        self.stsd_entries = stsd_entries
        self.sizes, self.offsets = sizes, offsets
        self.stts_override, self.ctts_override = stts_override, ctts_override
        self.width, self.height = width, height

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
        return box(b"trak", tkhd(track_id, movie_duration, w, h), edts, mdia)


def mp4(traks, movie_scale=1000, brand=b"isom", moov_kind=b"moov", mdat_header=None, cut=None):
    """A file: ftyp, moov, then mdat with each track's chunks interleaved
    chunk by chunk. `mdat_header` writes the mdat's header itself (a 64-bit
    size, or 0 for one running to the end)."""
    ftyp = box(b"ftyp", brand, u32(0x200), b"isom", b"iso2", b"mp41")
    chunks = [t.chunk_data() for t in traks]
    order = []
    for k in range(max(len(c) for c in chunks)):
        for ti, c in enumerate(chunks):
            if k < len(c):
                order.append((ti, k))

    def moov_with(offsets):
        duration = max(sum(s[1] for s in t.samples) * movie_scale // t.timescale for t in traks)
        body = mvhd(movie_scale, duration) + b"".join(
            t.trak(i + 1, offsets[i], movie_scale) for i, t in enumerate(traks))
        return box(moov_kind, body)

    zero = [[0] * len(c) for c in chunks]
    moov_len = len(moov_with(zero))
    payload_len = sum(len(chunks[ti][k]) for ti, k in order)
    header = mdat_header(payload_len) if mdat_header else u32(8 + payload_len) + b"mdat"
    at = len(ftyp) + moov_len + len(header)
    offsets = [[0] * len(c) for c in chunks]
    payload = b""
    for ti, k in order:
        offsets[ti][k] = at
        at += len(chunks[ti][k])
        payload += chunks[ti][k]
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
    out["largesize.mp4"] = mp4([Trak(b"vide", av01, 10240, v12, [12])],
                               mdat_header=lambda n: u32(1) + b"mdat" + u64(16 + n))
    out["mdat_to_end.mp4"] = mp4([Trak(b"vide", av01, 10240, v12, [12])],
                                 mdat_header=lambda n: u32(0) + b"mdat")
    # A moov written as 'hoov', which FFmpeg reads as one.
    out["hoov.mp4"] = mp4([Trak(b"vide", av01, 10240, v12, [12])], moov_kind=b"hoov")
    # The mdat cut short: the samples past the end.
    out["truncated.mp4"] = mp4([Trak(b"vide", av01, 10240, v12, [12])], cut=-25)
    return out


# --- the answers ---------------------------------------------------------------


def probe(*args):
    return subprocess.run(
        [FFPROBE, "-v", "error", *args],
        check=True, capture_output=True, text=True, encoding="utf-8",
    ).stdout.splitlines()


def fields(line):
    return dict(kv.split("=", 1) for kv in line.split("|") if "=" in kv)


def answer(name):
    lines = [f"# {name}: what ffprobe makes of it (generate_fixtures.py)."]
    for s in probe("-show_entries", "stream=index,codec_type,codec_tag_string,time_base,width,height,sample_rate,channels",
                   "-of", "compact=p=0", name):
        f = fields(s)
        if f.get("codec_type") == "video":
            shape = f"{f.get('width', '0')}x{f.get('height', '0')}"
        elif f.get("codec_type") == "audio":
            shape = f"{f.get('sample_rate', '0')}Hz/{f.get('channels', '0')}"
        else:
            shape = "-"
        lines.append(f"stream {f['index']} {f['codec_tag_string']} {f['time_base']} {shape}")
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
        if name in SEEKABLE:
            with open(f"{base}.seek.txt", "w", encoding="utf-8", newline="\n") as f:
                f.write(seek_answer(name))


if __name__ == "__main__":
    main()
