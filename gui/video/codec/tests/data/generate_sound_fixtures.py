#!/usr/bin/env python3
"""Generate videocodec's sound fixtures and their answers.

Each fixture NAME.EXT (.webm, .mka, .mp4, .opus or .ogg; no two of the same
NAME) gets NAME.sound.txt: its sound as a player should be given it --

    track N channels C rate R
    block TIME_NS SAMPLES          one a decoded block, in order
    pcm BYTES FNV1A64              the blocks' samples, 16-bit little-endian

`tests/sound.rs` holds `videocodec::Sound` to those lines.

Where the answers come from -- none of it from the crate itself:

- **Which blocks, and when.** ffprobe's `-show_frames`: FFmpeg's demuxer and
  decoder, which drop the codec delay's samples from the stream's start and
  each packet's discard padding from its end, and time each frame as its
  packet's time plus the samples dropped from its start, rounded to the
  file's tick (libavcodec's `decode.c`). Two exceptions, where FFmpeg's Ogg
  demuxer is wrong and `videocodec` does not follow it (gui/video/ogg's
  `src/stream.rs` and `src/demux.rs` say why): a short Vorbis block in the
  middle of an Ogg page after a long one, which FFmpeg times (long - short)
  / 4 samples late, is timed by the samples Tremor decodes from it -- each
  such packet checked to be that case and no other (`retime_vorbis`); a
  Vorbis stream of one Ogg page, which FFmpeg starts a packet late and cuts
  wrongly at its end, is Tremor's samples from 0 cut at the page's granule
  position, as the Vorbis I specification has it (`one_page_vorbis`), FFmpeg's
  frames checked to be those a packet late; and a chained Ogg file, whose
  times FFmpeg starts again at every link, is each link's answer made from
  that link alone, its times moved on to follow the last link's end
  (`chain_answer`).
- **Their samples.** For Opus, libopus 1.5.2's fixed-point decoder
  (gui/video/opus's `tools/reference.c`, built in WSL as its header says,
  the decoder `gui/video/opus` is held to bit for bit) decodes the packets
  ffprobe dumps (`-show_packets -show_data`), and the samples FFmpeg drops
  are dropped here -- the decoder's own pre-skip until a packet's skip side
  data replaces it, the packets an MP4 edit list marks to be dropped, the
  discard padding -- checked against FFmpeg's own count of what is left
  (`ffmpeg_trim` says how). For Vorbis, Tremor (gui/video/vorbis's
  `tools/reference.c` in its `pcm` mode, built by its
  `tools/build_reference.sh`; the decoder `gui/video/vorbis` is held to bit
  for bit) decodes the same packets, copied into an Ogg file, as ov_read
  gives them; FFmpeg drops nothing of a Vorbis stream (its first packet
  decodes to nothing, for Tremor too), and its count of samples must be
  Tremor's. (FFmpeg's own decoders are libopus's floating-point build and
  its native float decoders, a unit or so off fixed point's.)

The signals are made by ffmpeg's `aevalsrc` and encoded by its libopus or
libvorbis, one thread, `-fflags +bitexact`, so that a run makes the same
bytes as the last.

Run from this directory, on Windows, with gyan.dev's ffmpeg and the
reference decoders built in WSL:

    python generate_sound_fixtures.py [--ffmpeg DIR] [--reference WSL-PATH]
                                      [--vorbis-reference WSL-PATH] [NAME...]
"""

import argparse
import json
import os
import shlex
import struct
import subprocess
import sys
import tempfile

# The fixtures: name, the signal (aevalsrc expressions, one a channel), its
# seconds, the encoder, its rate, and its options. Opus lengths that are not
# a whole number of frames, so that the last packet carries discard padding;
# Vorbis signals with clicks, so that the encoder switches to short blocks.
SINE = "{a}*sin(2*PI*{f}*t)"
# A 2 ms burst of 3 kHz every 210 ms. The commas are escaped for the
# filter graph (aevalsrc's arguments are split at them otherwise).
CLICKS = "0.6*lt(mod(t\\,0.21)\\,0.002)*sin(2*PI*3000*t)"
FIXTURES = [
    ("opus_stereo.webm", [SINE.format(a=0.3, f=330) + "+0.05*sin(2*PI*2900*t)",
                          SINE.format(a=0.3, f=550)], 2.51,
     "libopus", 48000, ["-b:a", "64k"]),
    ("opus_mono_voip.webm", ["0.4*sin(2*PI*180*t)*(0.5+0.5*sin(2*PI*3*t))"], 1.37,
     "libopus", 48000, ["-application", "voip", "-b:a", "12k", "-frame_duration", "60"]),
    ("opus_short_frames.mka", [SINE.format(a=0.2, f=440), SINE.format(a=0.25, f=660)], 0.83,
     "libopus", 48000, ["-b:a", "96k", "-frame_duration", "2.5"]),
    ("opus_51.webm", [SINE.format(a=0.2, f=200 + 110 * c) for c in range(6)], 1.21,
     "libopus", 48000, ["-b:a", "192k", "-mapping_family", "1"]),
    ("vorbis_stereo.webm", [SINE.format(a=0.3, f=330) + "+" + CLICKS,
                            SINE.format(a=0.25, f=495) + "+0.05*sin(2*PI*5100*t)"], 1.73,
     "libvorbis", 44100, ["-q:a", "3"]),
    ("vorbis_mono_22k.mka", ["0.4*sin(2*PI*180*t)*(0.5+0.5*sin(2*PI*3*t))+" + CLICKS], 1.29,
     "libvorbis", 22050, ["-q:a", "1"]),
    ("vorbis_51.webm", [SINE.format(a=0.2, f=200 + 110 * c) for c in range(6)], 0.91,
     "libvorbis", 48000, ["-q:a", "2"]),
    # Opus in MP4: an edit list leaving out the pre-skip, as ffmpeg's muxer
    # writes it; none (the decoder's own pre-skip drops it); 5.1; and an
    # edit list patched to leave out more than a packet (PATCHES).
    ("opus_mp4_stereo.mp4", [SINE.format(a=0.3, f=330), SINE.format(a=0.3, f=550) + "+" + CLICKS], 1.37,
     "libopus", 48000, ["-b:a", "64k"]),
    ("opus_mp4_no_edit_list.mp4", ["0.4*sin(2*PI*180*t)*(0.5+0.5*sin(2*PI*3*t))"], 0.97,
     "libopus", 48000, ["-b:a", "32k", "-use_editlist", "0"]),
    ("opus_mp4_51.mp4", [SINE.format(a=0.2, f=230 + 90 * c) for c in range(6)], 0.83,
     "libopus", 48000, ["-b:a", "192k", "-mapping_family", "1"]),
    ("opus_mp4_long_priming.mp4", [SINE.format(a=0.3, f=440), SINE.format(a=0.25, f=660)], 1.11,
     "libopus", 48000, ["-b:a", "64k"]),
    # Ogg: Opus files (.opus), the commonest Vorbis kind (.ogg), with clicks
    # for short blocks in the middle of pages -- which FFmpeg mistimes --,
    # and a chained file (CHAINS).
    ("opus_ogg_stereo.opus", [SINE.format(a=0.3, f=330) + "+0.05*sin(2*PI*2900*t)",
                              SINE.format(a=0.3, f=550)], 1.93, "libopus", 48000, ["-b:a", "64k"]),
    ("opus_ogg_51.opus", [SINE.format(a=0.2, f=200 + 110 * c) for c in range(6)], 0.87,
     "libopus", 48000, ["-b:a", "192k", "-mapping_family", "1"]),
    ("vorbis_ogg_stereo.ogg", [SINE.format(a=0.3, f=330) + "+" + CLICKS,
                               SINE.format(a=0.25, f=495) + "+0.05*sin(2*PI*5100*t)"], 1.73,
     "libvorbis", 44100, ["-q:a", "3"]),
    ("vorbis_ogg_mono_22k.ogg", ["0.4*sin(2*PI*180*t)*(0.5+0.5*sin(2*PI*3*t))+" + CLICKS], 1.29,
     "libvorbis", 22050, ["-q:a", "1"]),
    # A stream of one page, whose end FFmpeg does not cut at all: the cut it
    # reckons from a start a packet late is more than the last packet holds.
    ("vorbis_ogg_one_page.ogg", [SINE.format(a=0.3, f=330), SINE.format(a=0.3, f=495)], 0.71,
     "libvorbis", 44100, ["-q:a", "3"]),
]

# Chained Ogg files: each link made as a file of its own (name, signal,
# seconds, encoder, rate, options, as above), with serial numbers of its own,
# then the links joined.
CHAINS = {
    "opus_ogg_chained.opus": [
        ("opus_ogg_chained_link0.opus", [SINE.format(a=0.3, f=440), SINE.format(a=0.3, f=660)], 0.81,
         "libopus", 48000, ["-b:a", "64k"]),
        ("opus_ogg_chained_link1.opus", [SINE.format(a=0.25, f=523), SINE.format(a=0.25, f=784) + "+" + CLICKS],
         0.67, "libopus", 48000, ["-b:a", "48k"]),
    ],
    "vorbis_ogg_chained.ogg": [
        ("vorbis_ogg_chained_link0.ogg", [SINE.format(a=0.3, f=330) + "+" + CLICKS, SINE.format(a=0.3, f=495)],
         0.71, "libvorbis", 44100, ["-q:a", "3"]),
        ("vorbis_ogg_chained_link1.ogg", [SINE.format(a=0.25, f=392), SINE.format(a=0.25, f=587)], 0.63,
         "libvorbis", 44100, ["-q:a", "6"]),
    ],
}


def patch_edit_list(name, media_time):
    """Rewrites the first `elst` entry's media time (version 0: a 32-bit
    segment duration, then the 32-bit media time): an edit list that leaves
    out `media_time` samples of a 48 kHz track."""
    with open(name, "rb") as f:
        data = bytearray(f.read())
    at = data.find(b"elst")
    if at < 0 or data[at + 4] != 0:
        sys.exit(f"{name}: no version-0 elst to patch")
    entry = at + 12
    data[entry + 4:entry + 8] = media_time.to_bytes(4, "big")
    with open(name, "wb") as f:
        f.write(data)


# Fixtures changed after encoding: the edit list leaving out 2000 samples,
# more than the first two 960-sample packets -- which FFmpeg's demuxer marks
# to be decoded and dropped -- and the start of the third.
PATCHES = {"opus_mp4_long_priming.mp4": lambda name: patch_edit_list(name, 2000)}

LAYOUT = {1: "mono", 2: "stereo", 6: "5.1"}


def run(args, **kw):
    r = subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, **kw)
    if r.returncode != 0:
        sys.exit(f"{' '.join(args)}: {r.stderr.decode('utf-8', 'replace')}")
    return r.stdout


def fnv1a64(data):
    h = 0xCBF29CE484222325
    for b in data:
        h = ((h ^ b) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def wsl_path(p):
    p = os.path.abspath(p).replace(os.sep, "/")
    return "/mnt/" + p[0].lower() + p[2:]


def toc_samples(packet):
    """A packet's samples a channel at 48 kHz, from its TOC
    (opus_packet_get_nb_samples)."""
    toc = packet[0]
    if toc & 0x80:
        per = (48000 << ((toc >> 3) & 3)) // 400
    elif toc & 0x60 == 0x60:
        per = 960 if toc & 0x08 else 480
    else:
        size = (toc >> 3) & 3
        per = 2880 if size == 3 else (480 << size)
    code = toc & 3
    count = 1 if code == 0 else 2 if code != 3 else packet[1] & 0x3F
    return per * count


def hexdump_bytes(dump):
    """ffprobe's `-show_data` dump: `OFFSET: HEX HEX ...  ASCII` lines."""
    out = bytearray()
    for line in dump.splitlines():
        if ":" not in line:
            continue
        hexpart = line.split(":", 1)[1][:41]
        out += bytes.fromhex(hexpart.replace(" ", ""))
    return bytes(out)


def packet_key(p):
    """What two ffprobes must agree on of a packet: when, and how long."""
    return (p.get("pts"), p.get("size"))


def write_answer(name, stream, frames, pcm):
    """NAME.sound.txt: the track, FFmpeg's blocks, and the samples' digest."""
    channels = int(stream["channels"])
    num, den = (int(x) for x in stream["time_base"].split("/"))
    lines = [f"track {stream['index'] + 1} channels {channels} rate {stream['sample_rate']}"]
    for f in frames:
        ticks = int(f["pts"])
        lines.append(f"block {ticks * 1_000_000_000 * num // den} {f['nb_samples']}")
    lines.append(f"pcm {len(pcm)} {fnv1a64(pcm):016x}")
    with open(os.path.splitext(name)[0] + ".sound.txt", "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines) + "\n")
    total = sum(int(f["nb_samples"]) for f in frames)
    print(f"{name}: {len(frames)} blocks, {total} samples a channel")


def packet_discarded(p):
    """Whether FFmpeg's demuxer marks the packet to be decoded and its sound
    dropped (`AV_PKT_FLAG_DISCARD`, ffprobe's flags `__D`)."""
    return "D" in p.get("flags", "")


def opus_answer(name, ffprobe, reference):
    """An Opus fixture's answer: FFmpeg's blocks, libopus's samples trimmed
    as FFmpeg trims them."""
    write_answer(name, *opus_blocks(name, ffprobe, reference))


def opus_blocks(name, ffprobe, reference):
    """An Opus fixture's stream, FFmpeg's blocks, and libopus's samples
    trimmed as FFmpeg trims them."""
    info = json.loads(run([ffprobe, "-hide_banner", "-loglevel", "error", "-select_streams", "a",
                           "-show_streams", "-show_packets", "-of", "json", name]))
    # The packets' bytes from WSL's ffprobe (6.1): this ffprobe's data
    # dump is broken (`-show_data`, "Invalid data dump type"). The two
    # must list the same packets; the skip data is this one's, as the
    # FFmpeg of today gives it (6.1 left the start's to its decoder).
    dump = json.loads(run(["wsl", "-d", "Ubuntu", "--", "ffprobe", "-hide_banner", "-loglevel", "error",
                           "-select_streams", "a", "-show_streams", "-show_packets", "-show_data",
                           "-of", "json", wsl_path(name)]))
    if [packet_key(p) for p in info["packets"]] != [packet_key(p) for p in dump["packets"]]:
        sys.exit(f"{name}: the two ffprobes list different packets")
    for ours, theirs in zip(info["packets"], dump["packets"]):
        ours["data"] = theirs["data"]
    info["streams"][0]["extradata"] = dump["streams"][0].get("extradata", "")
    stream = info["streams"][0]
    channels = int(stream["channels"])
    frames = json.loads(run([ffprobe, "-hide_banner", "-loglevel", "error", "-select_streams", "a",
                             "-show_frames", "-show_entries", "frame=pts,nb_samples",
                             "-of", "json", name]))["frames"]
    # The packets, for the reference decoder, and what FFmpeg drops of
    # each: its skip side data, its discard flag.
    bit = bytearray()
    packets = []
    for p in info["packets"]:
        data = hexdump_bytes(p["data"])
        bit += struct.pack(">II", len(data), 0) + data
        packets.append((toc_samples(data), skip_data(p), packet_discarded(p)))
    # The decoder's own skip, until side data replaces it: the OpusHead's
    # pre-skip (which FFmpeg's Opus decoder drops by itself in a file that
    # says nothing else -- an MP4 with no edit list).
    head_bytes = hexdump_bytes(stream["extradata"])
    pre_skip = int.from_bytes(head_bytes[10:12], "little") if len(head_bytes) >= 12 else 0
    with tempfile.TemporaryDirectory(dir=".") as tmp:
        with open(os.path.join(tmp, "in.bit"), "wb") as f:
            f.write(bit)
        head = "-head " + shlex.quote(wsl_path(os.path.join(tmp, "head"))) + " " if channels > 2 else ""
        if channels > 2:
            with open(os.path.join(tmp, "head"), "wb") as f:
                f.write(head_bytes)
        run(["wsl", "-d", "Ubuntu", "--", "bash", "-c",
             f"{reference} {head}48000 {channels} "
             f"{shlex.quote(wsl_path(os.path.join(tmp, 'in.bit')))} "
             f"{shlex.quote(wsl_path(os.path.join(tmp, 'out.pcm')))}"])
        with open(os.path.join(tmp, "out.pcm"), "rb") as f:
            pcm = f.read()
    out = ffmpeg_trim(pcm, packets, channels, pre_skip)
    total = sum(int(f["nb_samples"]) for f in frames)
    if total * 2 * channels != len(out):
        sys.exit(f"{name}: FFmpeg keeps {total} samples, the trimming here {len(out) // (2 * channels)}")
    return stream, frames, out


def skip_data(packet):
    """A packet's skip side data, if it has any: the samples to drop from
    its start and from its end."""
    for sd in packet.get("side_data_list", []):
        if sd.get("side_data_type") == "Skip Samples":
            return int(sd.get("skip_samples", 0)), int(sd.get("discard_padding", 0))
    return None


def ffmpeg_trim(pcm, packets, channels, initial_skip):
    """FFmpeg's trimming (libavcodec's `decode.c`), packet by packet --
    `packets` each (samples it decodes to, its skip side data or None,
    whether it is marked to be dropped): the decoder starts with
    `initial_skip` to drop; a packet's side data replaces what is left to
    skip, and without it the skip carries on until spent; a packet marked to
    be dropped is, and what is left to skip loses its samples; the discard
    padding comes off a packet's end, unless it is longer than what is left
    -- and a packet that decodes to nothing makes no frame, so its side data
    is never read (a Vorbis stream's first, which carries the codec delay:
    FFmpeg plays those samples)."""
    frame_bytes = 2 * channels
    out = bytearray()
    at = 0
    skip_left = initial_skip
    for samples, side, dropped in packets:
        whole = pcm[at:at + samples * frame_bytes]
        at += samples * frame_bytes
        if samples == 0:
            continue
        discard = 0
        if side is not None:
            skip_left, discard = side
        if dropped:
            skip_left = max(0, skip_left - samples)
            continue
        drop = min(samples, skip_left)
        skip_left -= drop
        keep = samples - drop
        if 0 < discard <= keep:
            keep -= discard
        out += whole[drop * frame_bytes:(drop + keep) * frame_bytes]
    if at != len(pcm):
        sys.exit(f"the reference decoded {len(pcm)} bytes, its packets hold {at}")
    return out


def vorbis_answer(name, ffmpeg, ffprobe, reference):
    """A Vorbis fixture's answer: FFmpeg's blocks, Tremor's samples trimmed
    as FFmpeg trims them."""
    write_answer(name, *vorbis_blocks(name, ffmpeg, ffprobe, reference))


def vorbis_blocks(name, ffmpeg, ffprobe, reference):
    """A Vorbis fixture's stream, FFmpeg's blocks (retimed where FFmpeg's Ogg
    demuxer mistimes them), and Tremor's samples trimmed as FFmpeg trims
    them."""
    info = json.loads(run([ffprobe, "-hide_banner", "-loglevel", "error", "-select_streams", "a",
                           "-show_streams", "-show_packets", "-of", "json", name]))
    stream = info["streams"][0]
    channels = int(stream["channels"])
    frames = json.loads(run([ffprobe, "-hide_banner", "-loglevel", "error", "-select_streams", "a",
                             "-show_frames", "-show_entries", "frame=pts,nb_samples",
                             "-of", "json", name]))["frames"]
    with tempfile.TemporaryDirectory(dir=".") as tmp:
        ogg = os.path.join(tmp, "in.ogg")
        out = os.path.join(tmp, "out.pcm")
        # The same packets, in Ogg, for Tremor's reference.
        run([ffmpeg, "-hide_banner", "-loglevel", "error", "-y", "-i", name, "-c:a", "copy",
             "-fflags", "+bitexact", "-f", "ogg", ogg])
        counts = [int(line) for line in run(["wsl", "-d", "Ubuntu", "--", "bash", "-c",
                                             f"{reference} pcm {shlex.quote(wsl_path(ogg))} "
                                             f"{shlex.quote(wsl_path(out))}"]).split()]
        with open(out, "rb") as f:
            pcm = f.read()
    if len(counts) != len(info["packets"]):
        sys.exit(f"{name}: {len(info['packets'])} packets in the file, {len(counts)} in its Ogg copy")
    packets = [(n, skip_data(p), packet_discarded(p)) for n, p in zip(counts, info["packets"])]
    kept = ffmpeg_trim(pcm, packets, channels, 0)
    total = sum(int(f["nb_samples"]) for f in frames)
    if total * 2 * channels != len(kept):
        sys.exit(f"{name}: FFmpeg keeps {total} samples a channel, the trimming here {len(kept) // (2 * channels)}")
    if name.endswith(".ogg"):
        with open(name, "rb") as f:
            data = f.read()
        if one_page(data):
            frames, kept = one_page_vorbis(name, data, counts, pcm, channels, frames, info["packets"])
        else:
            retime_vorbis(name, info["packets"], counts, frames)
    return stream, frames, kept


def one_page(data):
    """Whether an Ogg file's stream is of one data page: one page past its
    headers' (whose granule positions are 0), and that page its last."""
    timed, at = [], 0
    while True:
        at = data.find(b"OggS", at)
        if at < 0:
            break
        flags = data[at + 5]
        granule = int.from_bytes(data[at + 6:at + 14], "little", signed=True)
        if granule > 0:
            timed.append(flags)
        at += 4
    return len(timed) == 1 and timed[0] & 4 != 0


def one_page_vorbis(name, data, counts, pcm, channels, ff_frames, ff_packets):
    """A one-page Vorbis stream's blocks, as the Vorbis I specification
    (A.2) and libvorbis have it: the first sound at 0, each packet's samples
    after the last's, the end cut at the page's granule position. FFmpeg's
    are checked to be the same blocks a first packet's length late (its
    first packet started at 0), the last cut differently or not at all."""
    granule = last_granule(data)
    frame_bytes = 2 * channels
    blocks, out, t, at = [], bytearray(), 0, 0
    for n in counts:
        samples = pcm[at:at + n * frame_bytes]
        at += n * frame_bytes
        if n == 0:
            continue
        keep = min(n, granule - t)
        if 0 < keep < n and t + n != sum(counts):
            sys.exit(f"{name}: the granule cuts a packet before the last")
        if keep > 0:
            blocks.append({"pts": t, "nb_samples": keep})
            out += samples[:keep * frame_bytes]
        t += n
    late = int(ff_packets[0]["duration"])
    if len(ff_frames) != len(blocks) or any(int(f["pts"]) != b["pts"] + late
                                            for f, b in zip(ff_frames, blocks)):
        sys.exit(f"{name}: FFmpeg's blocks are not these, {late} samples late")
    ff_total = sum(int(f["nb_samples"]) for f in ff_frames)
    print(f"{name}: one page; {sum(b['nb_samples'] for b in blocks)} samples to its granule, "
          f"FFmpeg's {ff_total} from {late} late")
    return blocks, bytes(out)


def retime_vorbis(name, packets, counts, frames):
    """The frames of the Ogg Vorbis packets FFmpeg mistimes, retimed: a short
    block in the middle of a page after a long one, which FFmpeg's demuxer
    makes (long + short) / 4 - short / 2 samples late and as much short --
    its end right. Each packet whose length is not what Tremor decodes from
    it (the first, which decodes to nothing, and the last, which its page
    trims, aside) must be that case: the block sizes are the identification
    header's. Its frame takes its end less what Tremor decodes."""
    with open(name, "rb") as f:
        data = f.read()
    at = data.find(b"\x01vorbis")
    short, long = 1 << (data[at + 28] & 15), 1 << (data[at + 28] >> 4)
    # Which frame each packet makes: one for each that decodes to samples
    # (FFmpeg trims none of them away whole here).
    frame_of, k = {}, 0
    for i, n in enumerate(counts):
        if n > 0:
            frame_of[i] = k
            k += 1
    if k != len(frames):
        sys.exit(f"{name}: {k} packets decode to samples, ffprobe has {len(frames)} frames")
    retimed = 0
    for i, (p, n) in enumerate(zip(packets, counts)):
        if i == 0 or i == len(packets) - 1 or int(p["duration"]) == n:
            continue
        if (n, int(p["duration"])) != ((short + long) // 4, short // 2):
            sys.exit(f"{name} packet {i}: FFmpeg {p['duration']}, Tremor {n}: not the mistiming corrected")
        f = frames[frame_of[i]]
        f["pts"] = int(f["pts"]) + int(p["duration"]) - n
        retimed += 1
    print(f"{name}: {retimed} Vorbis blocks retimed")


def encode(ffmpeg, name, chans, seconds, encoder, rate, opts):
    """`name`, made by ffmpeg from `chans` (an aevalsrc expression each)."""
    expr = "|".join(chans)
    layout = LAYOUT[len(chans)]
    run([ffmpeg, "-hide_banner", "-loglevel", "error", "-y", "-fflags", "+bitexact",
         "-f", "lavfi", "-i", f"aevalsrc={expr}:s={rate}:d={seconds}:c={layout}",
         "-c:a", encoder, "-threads", "1", *opts, "-flags", "+bitexact",
         # The muxer's too (before `-i` it is the input's): no random
         # UIDs or dates, so that a run writes the same file as the last.
         "-fflags", "+bitexact", name])


def last_granule(data):
    """An Ogg file's last page's granule position."""
    at = data.rfind(b"OggS")
    return int.from_bytes(data[at + 6:at + 14], "little", signed=True)


def chain_answer(name, links, ffmpeg, ffprobe, args):
    """A chained Ogg file's answer: each link's blocks and samples as that
    link alone gives them, its times moved so that its first sound follows
    the last link's last -- the link's end its last granule, less Opus's
    pre-skip -- and its samples after the last link's."""
    stream, frames, pcm, offset = None, [], b"", 0
    previous_end = None
    for link, _chans, _seconds, encoder, _rate, _opts in links:
        if encoder == "libvorbis":
            s, f, out = vorbis_blocks(link, ffmpeg, ffprobe, args.vorbis_reference)
        else:
            s, f, out = opus_blocks(link, ffprobe, args.reference)
        with open(link, "rb") as fh:
            data = fh.read()
        pre_skip = 0
        if data.find(b"OpusHead") >= 0:
            head = data.find(b"OpusHead")
            pre_skip = int.from_bytes(data[head + 10:head + 12], "little")
        # Where this link's sound starts, alone: its first frame.
        first = int(f[0]["pts"])
        if previous_end is not None:
            offset = previous_end - first
        for fr in f:
            fr["pts"] = int(fr["pts"]) + offset
        stream = stream or s
        frames += f
        pcm += out
        previous_end = last_granule(data) - pre_skip + offset
    write_answer(name, stream, frames, pcm)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ffmpeg", default="")
    ap.add_argument("--reference", default="~/opusref/reference")
    ap.add_argument("--vorbis-reference", default="~/vorbisref/build/reference")
    ap.add_argument("--answers-only", action="store_true",
                    help="work out the answers of the files there are, encoding nothing")
    ap.add_argument("names", nargs="*", help="the fixtures to make (all, if none)")
    args = ap.parse_args()
    stems = [os.path.splitext(f[0])[0] for f in FIXTURES]
    stems += [os.path.splitext(n)[0] for n in CHAINS]
    stems += [os.path.splitext(link[0])[0] for links in CHAINS.values() for link in links]
    if len(set(stems)) != len(stems):
        sys.exit("two fixtures share a name, and so would an answer file")
    exe = ".exe" if os.name == "nt" else ""
    ffmpeg = os.path.join(args.ffmpeg, "ffmpeg" + exe) if args.ffmpeg else "ffmpeg"
    ffprobe = os.path.join(args.ffmpeg, "ffprobe" + exe) if args.ffmpeg else "ffprobe"
    for name, chans, seconds, encoder, rate, opts in FIXTURES:
        if args.names and name not in args.names:
            continue
        if not args.answers_only:
            encode(ffmpeg, name, chans, seconds, encoder, rate, opts)
            if name in PATCHES:
                PATCHES[name](name)
        if encoder == "libvorbis":
            vorbis_answer(name, ffmpeg, ffprobe, args.vorbis_reference)
        else:
            opus_answer(name, ffprobe, args.reference)
    for name, links in CHAINS.items():
        if args.names and name not in args.names:
            continue
        if not args.answers_only:
            for k, (link, chans, seconds, encoder, rate, opts) in enumerate(links):
                encode(ffmpeg, link, chans, seconds, encoder, rate, [*opts, "-serial_offset", str(1000 * k)])
            with open(name, "wb") as out:
                for link, *_ in links:
                    with open(link, "rb") as f:
                        out.write(f.read())
        chain_answer(name, links, ffmpeg, ffprobe, args)


main()
