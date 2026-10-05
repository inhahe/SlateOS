#!/usr/bin/env python3
"""Generate the Ogg demuxer's fixtures and their answers.

Each fixture NAME.EXT gets NAME.txt, what the demuxer must make of it:

    stream INDEX CODEC TIME_BASE
    packet STREAM PTS DURATION SIZE POS FLAGS MD5 SKIP DISCARD HEADERS

a line a stream, then a line a packet, in file order. PTS is N/A where the
file gives the packet none; POS is where the page the packet begins on
begins (FFmpeg's position); FLAGS is C for a packet whose length cannot be
read, else _; SKIP and DISCARD are the samples to drop from its start and
end; HEADERS is how many headers come with it, - where none do.

Where the answers come from -- none of it from the crate:

- **ffprobe's packets**, `-fflags +noparse+nofillin`: FFmpeg's demuxer
  alone, with nothing libavformat fills in afterwards, as the crate gives
  them.
- **Changed only where the crate means to differ** (`src/stream.rs`,
  `src/demux.rs`), and each change checked:
  - *A Vorbis packet FFmpeg mistimes.* FFmpeg forgets the block before at
    every packet but a page's first, so a short block in the middle of a page
    after a long one comes out late and short, its end right. Here such a
    packet's length is what Tremor decodes from it (gui/video/vorbis's
    `tools/reference.c`, in its `pcm` mode, built in WSL by its
    `tools/build_reference.sh`), and its time is its end less that. The
    generator checks that every other packet of every Vorbis fixture lasts
    exactly what Tremor decodes from it (the first, which decodes to
    nothing, and the last, which its page trims, aside), and that every
    packet it changes is that case: a short block after a long one, not the
    first to end on its page, on a page that is not the stream's last, and
    FFmpeg's length the short block's half.
  - *A Vorbis stream of one page* (its first page its last). FFmpeg takes it
    to start at granule 0 with its first packet, which decodes to nothing;
    the crate, as the Vorbis I specification does, with its first sound,
    the first packet its own length before. The first packet's time is
    moved back by its length, and the last packet's trimming and length
    reckoned again from there -- checked, as every last packet is, against
    what Tremor decodes from it.
  - *A chained file.* FFmpeg starts each link's times again; the crate
    moves them on. Each link is made as a file of its own first, and its
    answer is that file's, its positions moved by the bytes before it, its
    times moved so that its first sound follows the last link's last, and
    its streams' headers given with their first packets.

The fixtures are made by gyan.dev's ffmpeg (the same build as
gui/video/matroska's), one thread, `-fflags +bitexact`, so that a run writes
the bytes the last did; the few it cannot make are its packets re-muxed
here (`remux`).

Run from this directory:

    python generate_fixtures.py [--answers-only] [NAME...]
"""

import argparse
import json
import os
import shlex
import struct
import subprocess
import sys
import tempfile

FFMPEG = "ffmpeg"
FFPROBE = "ffprobe"
VORBIS_REFERENCE = "~/vorbisref/build/reference"

SINE = "{a}*sin(2*PI*{f}*t)"
# A 2 ms burst of 3 kHz every 210 ms, which makes libvorbis switch to short
# blocks (escaped for the filter graph).
CLICKS = "0.6*lt(mod(t\\,0.21)\\,0.002)*sin(2*PI*3000*t)"


def signal(channels, seconds, rate, clicks=False):
    exprs = [SINE.format(a=0.25, f=220 + 110 * c) + ("+" + CLICKS if clicks and c == 0 else "")
             for c in range(channels)]
    layout = {1: "mono", 2: "stereo", 6: "5.1"}[channels]
    return ["-f", "lavfi", "-i", f"aevalsrc={'|'.join(exprs)}:s={rate}:d={seconds}:c={layout}"]


# Fixtures ffmpeg makes: name, then ffmpeg's arguments after its input.
MADE = {
    "opus_stereo.opus": (signal(2, 2.51, 48000), ["-c:a", "libopus", "-b:a", "64k"]),
    "opus_mono_silk.opus": (signal(1, 1.37, 48000),
                            ["-c:a", "libopus", "-application", "voip", "-b:a", "12k", "-frame_duration", "60"]),
    "opus_short_frames.ogg": (signal(2, 0.83, 48000),
                              ["-c:a", "libopus", "-b:a", "96k", "-frame_duration", "2.5"]),
    "opus_51.opus": (signal(6, 1.21, 48000), ["-c:a", "libopus", "-b:a", "192k", "-mapping_family", "1"]),
    "opus_one_page.opus": (signal(1, 0.37, 48000), ["-c:a", "libopus", "-b:a", "32k"]),
    # Packets of some 1900 bytes on pages of 300: each runs over several.
    "opus_spanning.opus": (signal(2, 0.61, 48000),
                           ["-c:a", "libopus", "-b:a", "256k", "-frame_duration", "60", "-pagesize", "300"]),
    "vorbis_stereo.ogg": (signal(2, 1.73, 44100, clicks=True), ["-c:a", "libvorbis", "-q:a", "3"]),
    "vorbis_mono_22k.ogg": (signal(1, 1.29, 22050, clicks=True), ["-c:a", "libvorbis", "-q:a", "1"]),
    "vorbis_51.ogg": (signal(6, 0.91, 48000), ["-c:a", "libvorbis", "-q:a", "2"]),
    "vorbis_one_page.ogg": (signal(2, 0.11, 44100), ["-c:a", "libvorbis", "-q:a", "3"]),
    "vorbis_spanning.ogg": (signal(2, 0.97, 44100, clicks=True),
                            ["-c:a", "libvorbis", "-q:a", "8", "-pagesize", "2048"]),
    "vorbis_native.ogg": (signal(2, 0.77, 44100), ["-c:a", "vorbis", "-strict", "-2"]),
    "flac.oga": (signal(1, 0.53, 44100), ["-c:a", "flac"]),
    "speex.spx": (signal(1, 0.47, 16000), ["-c:a", "libspeex"]),
    "theora_vorbis.ogv": (["-f", "lavfi", "-i", "testsrc=size=64x48:rate=10:duration=0.9"]
                          + signal(2, 0.9, 44100, clicks=True),
                          ["-c:v", "libtheora", "-q:v", "5", "-c:a", "libvorbis", "-q:a", "3"]),
}

# Chained files: their links, each made as a fixture of its own first (and
# kept, a fixture too, whose answer the chain's is made from).
CHAINS = {
    "opus_chained.opus": [("opus_chained_link0.opus", signal(2, 0.81, 48000), ["-c:a", "libopus", "-b:a", "64k"]),
                          ("opus_chained_link1.opus", signal(2, 0.67, 48000), ["-c:a", "libopus", "-b:a", "48k"])],
    "vorbis_chained.ogg": [("vorbis_chained_link0.ogg", signal(2, 0.71, 44100, clicks=True),
                            ["-c:a", "libvorbis", "-q:a", "3"]),
                           ("vorbis_chained_link1.ogg", signal(2, 0.63, 44100),
                            ["-c:a", "libvorbis", "-q:a", "6"])],
    # A second link of one channel cannot follow a first of two: the file
    # ends with the first.
    "opus_chained_unlike.opus": [("opus_unlike_link0.opus", signal(2, 0.43, 48000), ["-c:a", "libopus"]),
                                 ("opus_unlike_link1.opus", signal(1, 0.39, 48000), ["-c:a", "libopus"])],
}

# Fixtures re-muxed here from another's packets: a layout FFmpeg's muxer
# never writes. Name: (source, how).
REMUXED = {
    # The comment and setup headers and the first audio packets on one page.
    "vorbis_headers_with_data.ogg": ("vorbis_stereo.ogg", "headers_with_data"),
    # OpusTags and the first audio packets on one page.
    "opus_tags_with_data.opus": ("opus_stereo.opus", "headers_with_data"),
}


def run(args, **kw):
    r = subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, **kw)
    if r.returncode != 0:
        sys.exit(f"{' '.join(args)}: {r.stderr.decode('utf-8', 'replace')}")
    return r.stdout


def wsl_path(p):
    p = os.path.abspath(p).replace(os.sep, "/")
    return "/mnt/" + p[0].lower() + p[2:]


# --- Ogg, read and written: an implementation of RFC 3533 of the
# generator's own, so that the answers depend on nothing in the crate.

def crc(data):
    c = 0
    for b in data:
        c ^= b << 24
        for _ in range(8):
            c = ((c << 1) ^ 0x04C11DB7) if c & 0x80000000 else c << 1
            c &= 0xFFFFFFFF
    return c


def pages(data):
    """Every page of a well-formed file: (position, flags, granule, serial,
    sequence, lacing, body)."""
    out, at = [], 0
    while at < len(data):
        if data[at:at + 4] != b"OggS":
            sys.exit(f"no page at {at}")
        flags, granule, serial, seq, _crc, n = struct.unpack("<BqIIIB", data[at + 5:at + 27])
        lacing = list(data[at + 27:at + 27 + n])
        size = sum(lacing)
        body = data[at + 27 + n:at + 27 + n + size]
        whole = bytearray(data[at:at + 27 + n + size])
        whole[22:26] = b"\0\0\0\0"
        if crc(whole) != _crc:
            sys.exit(f"bad CRC at {at}")
        out.append((at, flags, granule, serial, seq, lacing, body))
        at += 27 + n + size
    return out


def packets(data, serial):
    """A stream's packets, each with the index of the page it ends on and
    whether it is the first to end there: (bytes, page index, first)."""
    out, partial = [], b""
    for i, (_pos, _flags, _g, s, _seq, lacing, body) in enumerate(pages(data)):
        if s != serial:
            continue
        at, first = 0, True
        for n in lacing:
            partial += body[at:at + n]
            at += n
            if n < 255:
                out.append((partial, i, first))
                partial, first = b"", False
    return out


def page_bytes(flags, granule, serial, seq, segments):
    """A page holding `segments`, each laced whole (and so each ending a
    packet, a 255-byte multiple getting its empty final segment)."""
    lacing = []
    for s in segments:
        lacing += [255] * (len(s) // 255) + [len(s) % 255]
    if len(lacing) > 255:
        sys.exit("a page of more than 255 segments")
    head = struct.pack("<4sBBqIIIB", b"OggS", 0, flags, granule, serial, seq, 0, len(lacing))
    p = bytearray(head + bytes(lacing) + b"".join(segments))
    p[22:26] = struct.pack("<I", crc(p))
    return bytes(p)


# --- FFmpeg's answers, and Tremor's.

def ffprobe_streams(name):
    out = json.loads(run([FFPROBE, "-v", "error", "-show_entries", "stream=index,codec_name,time_base",
                          "-of", "json", name]))
    return [(s["index"], s.get("codec_name", "none"), s["time_base"]) for s in out["streams"]]


def ffprobe_packets(name):
    out = json.loads(run([FFPROBE, "-v", "error", "-fflags", "+noparse+nofillin", "-show_entries",
                          "packet=stream_index,pts,duration,size,pos,flags,data_hash:packet_side_data",
                          "-show_data_hash", "MD5", "-of", "json", name]))
    result = []
    for p in out["packets"]:
        skip = discard = 0
        for sd in p.get("side_data_list", []):
            if sd.get("side_data_type") == "Skip Samples":
                skip, discard = int(sd["skip_samples"]), int(sd["discard_padding"])
        number = lambda v: None if v in (None, "N/A") else int(v)
        result.append({
            "stream": int(p["stream_index"]),
            "pts": number(p.get("pts")),
            "duration": number(p.get("duration")) or 0,
            "size": int(p["size"]),
            "pos": int(p["pos"]),
            "flags": "C" if "C" in p.get("flags", "") else "_",
            "md5": p["data_hash"].removeprefix("MD5:"),
            "skip": skip,
            "discard": discard,
            "headers": "-",
        })
    return result


def serials(data):
    """Each stream's serial number, in the order its first page comes."""
    out = []
    for _pos, flags, _g, s, _seq, _l, _b in pages(data):
        if flags & 2 and s not in out:
            out.append(s)
    return out


def tremor_counts(data, serial):
    """The samples Tremor decodes from each of a Vorbis stream's audio
    packets, from a file of that stream's pages alone."""
    alone = b"".join(data[pos:pos + 27 + len(lacing) + len(body)]
                     for pos, _f, _g, s, _q, lacing, body in pages(data) if s == serial)
    with tempfile.TemporaryDirectory(dir=".") as tmp:
        path = os.path.join(tmp, "alone.ogg")
        with open(path, "wb") as f:
            f.write(alone)
        out = run(["wsl", "-d", "Ubuntu", "--", "bash", "-c",
                   f"{VORBIS_REFERENCE} pcm {shlex.quote(wsl_path(path))} "
                   f"{shlex.quote(wsl_path(os.path.join(tmp, 'out.pcm')))}"])
    return [int(line) for line in out.split()]


def correct_vorbis(data, answer, stream, serial):
    """FFmpeg's times of a Vorbis stream's packets, with the packets it
    mistimes put right (see the module docs), every other packet checked."""
    page_list = pages(data)
    ours = [p for p in packets(data, serial) if not (p[0] and p[0][0] & 1)]
    theirs = [p for p in answer if p["stream"] == stream]
    counts = tremor_counts(data, serial)
    if not (len(ours) == len(theirs) == len(counts)):
        sys.exit(f"stream {stream}: {len(ours)} packets here, {len(theirs)} in ffprobe, {len(counts)} in Tremor")
    ident = next(p[0] for p in packets(data, serial) if p[0][:1] == b"\x01")
    short, long = 1 << (ident[28] & 15), 1 << (ident[28] >> 4)
    changed = 0
    # A stream of one page: its first data packet ends on its last page.
    one_page = bool(page_list[ours[0][1]][1] & 4)
    shift = 0
    if one_page:
        if theirs[0]["pts"] != 0:
            sys.exit(f"stream {stream}: one page, and FFmpeg's first packet at {theirs[0]['pts']}, not 0")
        shift = theirs[0]["duration"]
        theirs[0]["pts"] = -shift
        changed += 1
    for k, ((_bytes, page_index, first), p, count) in enumerate(zip(ours, theirs, counts)):
        last = k == len(theirs) - 1
        if last and (p["discard"] or one_page):
            # FFmpeg's length of the last packet is what its page's granule
            # leaves of it -- negative where the granule ends before the
            # packet starts, which FFmpeg's unsigned length wraps to 2^32
            # less it (the crate gives 0) -- and what runs past the granule,
            # Tremor's samples less that, is its trimming.
            length = p["duration"] - (1 << 32) if p["duration"] >= 1 << 31 else p["duration"]
            past = count - length
            if p["discard"] != max(past, 0):
                sys.exit(f"stream {stream}: the last packet lasts {length}, trimmed {p['discard']}, Tremor {count}")
            # A stream of one page, its start moved back: its end comes
            # that much later in its last packet.
            length, past = length + shift, past - shift
            p["duration"], p["discard"] = max(length, 0), max(past, 0)
            continue
        if k == 0 or p["duration"] == count:
            continue
        eos = page_list[page_index][1] & 4
        if (count, p["duration"]) != ((short + long) // 4, short // 2) or first or eos or p["pts"] is None:
            sys.exit(f"stream {stream} packet {k}: FFmpeg {p['duration']}, Tremor {count}, "
                     f"first on its page {first}, last page {bool(eos)}: not the mistiming the crate corrects")
        p["pts"] += p["duration"] - count
        p["duration"] = count
        changed += 1
    return changed


# The codecs the crate times. FFmpeg times FLAC, Speex and Theora packets
# too; the crate gives them untimed, nothing here decoding them yet.
TIMED = ("opus", "vorbis")


def answer_of(name):
    """A fixture's streams and packets, as the crate must give them."""
    with open(name, "rb") as f:
        data = f.read()
    streams = ffprobe_streams(name)
    answer = ffprobe_packets(name)
    changed = 0
    for index, s in enumerate(serials(data)):
        if index < len(streams) and streams[index][1] == "vorbis":
            changed += correct_vorbis(data, answer, index, s)
    for p in answer:
        if streams[p["stream"]][1] not in TIMED:
            p.update(pts=None, duration=0, skip=0, discard=0)
    return streams, answer, changed


def write_answer(name, streams, answer, note=""):
    lines = [f"# {name}: what the Ogg demuxer makes of it (generate_fixtures.py){note}."]
    for index, codec, tb in streams:
        lines.append(f"stream {index} {codec} {tb}")
    for p in answer:
        pts = "N/A" if p["pts"] is None else p["pts"]
        lines.append(f"packet {p['stream']} {pts} {p['duration']} {p['size']} {p['pos']} {p['flags']} "
                     f"{p['md5']} {p['skip']} {p['discard']} {p['headers']}")
    with open(os.path.splitext(name)[0] + ".txt", "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines) + "\n")
    print(f"{name}: {len(streams)} streams, {len(answer)} packets{note}")


def make(name, inputs, args):
    run([FFMPEG, "-hide_banner", "-loglevel", "error", "-y", "-fflags", "+bitexact", *inputs,
         *args, "-threads", "1", "-flags", "+bitexact", "-fflags", "+bitexact", name])


def make_chain(name, links):
    """A chained file: its links made one by one, each with serial numbers
    of its own, then joined; its answer the links' moved on."""
    blobs = []
    for k, (link, inputs, args) in enumerate(links):
        make(link, inputs, [*args, "-serial_offset", str(1000 * k)])
        with open(link, "rb") as f:
            blobs.append(f.read())
    with open(name, "wb") as f:
        f.write(b"".join(blobs))


def chain_answer(name, links):
    """The chained file's answer: each link's moved on, while its streams
    are like the first link's."""
    blobs, answers = [], []
    for link, _inputs, _args in links:
        with open(link, "rb") as f:
            blobs.append(f.read())
        answers.append(answer_of(link))
        # Each link is a fixture too.
        write_answer(link, answers[-1][0], answers[-1][1])
    first_streams = answers[0][0]
    out = list(answers[0][1])
    moved_bytes = len(blobs[0])
    previous_end = link_ends(blobs[0], answers[0][1], {i: 0 for i in range(len(first_streams))})
    for k in range(1, len(links)):
        streams, packets_k, _ = answers[k]
        if not alike(blobs[0], blobs[k]):
            break
        offsets = {}
        for i, (_index, codec, _tb) in enumerate(streams):
            mine = [p for p in packets_k if p["stream"] == i]
            first_sound = mine[0]["pts"] + (mine[0]["duration"] if codec == "vorbis" else mine[0]["skip"])
            offsets[i] = previous_end[i] - first_sound
        seen = set()
        for p in packets_k:
            q = dict(p)
            q["pos"] += moved_bytes
            if q["pts"] is not None:
                q["pts"] += offsets[q["stream"]]
            if q["stream"] not in seen:
                seen.add(q["stream"])
                q["headers"] = str(header_count(blobs[k], q["stream"]))
            out.append(q)
        previous_end = link_ends(blobs[k], packets_k, offsets)
        moved_bytes += len(blobs[k])
    note = f"; {len(blobs)} links" if len(out) > len(answers[0][1]) else "; its second link unlike its first"
    write_answer(name, first_streams, out, note)


def alike(a, b):
    """Whether two links hold the same streams: codecs, rates, channels."""
    def describe(data):
        out = []
        for s in serials(data):
            head = packets(data, s)[0][0]
            if head.startswith(b"OpusHead"):
                out.append(("opus", head[9], 48000))
            elif head.startswith(b"\x01vorbis"):
                out.append(("vorbis", head[11], struct.unpack("<I", head[12:16])[0]))
            else:
                out.append((head[:8], 0, 0))
        return out
    return describe(a) == describe(b)


def header_count(data, stream):
    s = serials(data)[stream]
    head = packets(data, s)[0][0]
    return 2 if head.startswith(b"OpusHead") else 3


def link_ends(data, answer, offsets):
    """Where each stream's sound ends: its last page's granule position,
    less Opus's pre-skip, moved on."""
    ends = {}
    for i, s in enumerate(serials(data)):
        head = packets(data, s)[0][0]
        pre_skip = struct.unpack("<H", head[10:12])[0] if head.startswith(b"OpusHead") else 0
        last = [g for _p, _f, g, ser, _q, _l, _b in pages(data) if ser == s and g != -1][-1]
        ends[i] = last - pre_skip + offsets[i]
    return ends


def toc_samples(packet):
    """An Opus packet's samples at 48 kHz, from its TOC (RFC 6716 3.1)."""
    toc = packet[0]
    config = toc >> 3
    frame = [480, 960, 1920, 2880][config & 3] if config < 12 else (480 << (config & 1)) if config < 16 \
        else (120 << (config & 3))
    count = 1 if toc & 3 == 0 else 2 if toc & 3 != 3 else packet[1] & 0x3F
    return frame * count


def remux(name, source, how):
    """`name` from `source`'s first stream's packets, laid out as `how`
    says: the first header alone, then the rest of the headers with the first
    four audio packets, then the audio eight packets a page, each page's
    granule position the samples decoded to its end (checked against the
    source's where a source page ends at the same packet), the last page the
    source's."""
    if how != "headers_with_data":
        sys.exit(how)
    with open(source, "rb") as f:
        data = f.read()
    s = serials(data)[0]
    page_list = pages(data)
    every = packets(data, s)
    heads = 2 if every[0][0].startswith(b"OpusHead") else 3
    audio = every[heads:]
    lengths = tremor_counts(data, s) if heads == 3 else [toc_samples(p[0]) for p in audio]
    ends, total = [], 0
    for n in lengths:
        total += n
        ends.append(total)
    # The source's pages agree, where a page of theirs ends where one of ours
    # will.
    for k, (_b, page_index, _f) in enumerate(audio):
        last_on_page = k + 1 == len(audio) or audio[k + 1][1] != page_index
        if last_on_page and not page_list[page_index][1] & 4 and page_list[page_index][2] != ends[k]:
            sys.exit(f"{source}: page {page_index} ends at {page_list[page_index][2]}, the packets at {ends[k]}")
    out = [page_bytes(2, 0, s, 0, [every[0][0]])]
    out.append(page_bytes(0, ends[3], s, 1, [p[0] for p in every[1:heads]] + [p[0] for p in audio[:4]]))
    seq = 2
    for k in range(4, len(audio), 8):
        run_ = audio[k:k + 8]
        last = k + len(run_) == len(audio)
        granule = page_list[-1][2] if last else ends[k + len(run_) - 1]
        out.append(page_bytes(4 if last else 0, granule, s, seq, [p[0] for p in run_]))
        seq += 1
    with open(name, "wb") as f:
        f.write(b"".join(out))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--answers-only", action="store_true",
                    help="work out the answers of the files there are, making nothing")
    ap.add_argument("names", nargs="*")
    args = ap.parse_args()
    want = lambda n: not args.names or n in args.names
    for name, (inputs, opts) in MADE.items():
        if want(name):
            if not args.answers_only:
                make(name, inputs, opts)
            streams, answer, changed = answer_of(name)
            write_answer(name, streams, answer, f"; {changed} Vorbis packets retimed" if changed else "")
    for name, links in CHAINS.items():
        if want(name):
            if not args.answers_only:
                make_chain(name, links)
            chain_answer(name, links)
    for name, (source, how) in REMUXED.items():
        if want(name):
            if not args.answers_only:
                remux(name, source, how)
            streams, answer, changed = answer_of(name)
            write_answer(name, streams, answer, f"; {changed} Vorbis packets retimed" if changed else "")


main()
