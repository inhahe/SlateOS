#!/usr/bin/env python3
"""Generate videocodec's sound fixtures and their answers.

Each fixture NAME (.webm or .mka) gets NAME.sound.txt: its sound as a player
should be given it --

    track N channels C rate R
    block TIME_NS SAMPLES          one a decoded block, in order
    pcm BYTES FNV1A64              the blocks' samples, 16-bit little-endian

`tests/sound.rs` holds `videocodec::Sound` to those lines.

Where the answers come from -- none of it from the crate itself:

- **Which blocks, and when.** ffprobe's `-show_frames`: FFmpeg's demuxer and
  decoder, which drop the codec delay's samples from the stream's start and
  each packet's discard padding from its end, and time each frame as its
  packet's time plus the samples dropped from its start, rounded to the
  file's tick (libavcodec's `decode.c`).
- **Their samples.** For Opus, libopus 1.5.2's fixed-point decoder
  (gui/video/opus's `tools/reference.c`, built in WSL as its header says,
  the decoder `gui/video/opus` is held to bit for bit) decodes the packets
  ffprobe dumps (`-show_packets -show_data`), and the samples FFmpeg drops
  -- its packets' skip side data -- are dropped here; checked against
  FFmpeg's own count of what is left. For Vorbis, Tremor (gui/video/vorbis's
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
]

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


def opus_answer(name, ffprobe, reference):
    """An Opus fixture's answer: FFmpeg's blocks, libopus's samples trimmed
    as FFmpeg trims them."""
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
    # each: its skip side data.
    bit = bytearray()
    trims = []
    for p in info["packets"]:
        data = hexdump_bytes(p["data"])
        bit += struct.pack(">II", len(data), 0) + data
        skip = discard = 0
        for sd in p.get("side_data_list", []):
            if sd.get("side_data_type") == "Skip Samples":
                skip = int(sd.get("skip_samples", 0))
                discard = int(sd.get("discard_padding", 0))
        trims.append((toc_samples(data), skip, discard))
    with tempfile.TemporaryDirectory(dir=".") as tmp:
        with open(os.path.join(tmp, "in.bit"), "wb") as f:
            f.write(bit)
        head = "-head " + shlex.quote(wsl_path(os.path.join(tmp, "head"))) + " " if channels > 2 else ""
        if channels > 2:
            with open(os.path.join(tmp, "head"), "wb") as f:
                f.write(hexdump_bytes(stream["extradata"]))
        run(["wsl", "-d", "Ubuntu", "--", "bash", "-c",
             f"{reference} {head}48000 {channels} "
             f"{shlex.quote(wsl_path(os.path.join(tmp, 'in.bit')))} "
             f"{shlex.quote(wsl_path(os.path.join(tmp, 'out.pcm')))}"])
        with open(os.path.join(tmp, "out.pcm"), "rb") as f:
            pcm = f.read()
    # FFmpeg's trimming: a skip carries over into the packets after it
    # until spent; the discard padding comes off the packet's end.
    frame_bytes = 2 * channels
    out = bytearray()
    at = 0
    carry = 0
    for samples, skip, discard in trims:
        whole = pcm[at:at + samples * frame_bytes]
        at += samples * frame_bytes
        drop = min(samples, carry + skip)
        carry = carry + skip - drop
        keep = max(0, samples - drop - discard)
        out += whole[drop * frame_bytes:(drop + keep) * frame_bytes]
    if at != len(pcm):
        sys.exit(f"{name}: the reference decoded {len(pcm)} bytes, the packets hold {at}")
    total = sum(int(f["nb_samples"]) for f in frames)
    if total * frame_bytes != len(out):
        sys.exit(f"{name}: FFmpeg keeps {total} samples, the trimming here {len(out) // frame_bytes}")
    write_answer(name, stream, frames, out)


def skip_data(packet):
    """A packet's skip side data, if it has any: the samples to drop from
    its start and from its end."""
    for sd in packet.get("side_data_list", []):
        if sd.get("side_data_type") == "Skip Samples":
            return int(sd.get("skip_samples", 0)), int(sd.get("discard_padding", 0))
    return None


def ffmpeg_trim(pcm, counts, sides, channels):
    """FFmpeg's trimming (libavcodec's `decode.c`), packet by packet: a skip
    carries over into the packets after it until spent, the discard padding
    comes off the packet's end -- and a packet that decodes to nothing makes
    no frame, so its side data is never read (a Vorbis stream's first, which
    carries the codec delay: FFmpeg plays those samples)."""
    frame_bytes = 2 * channels
    out = bytearray()
    at = 0
    skip_left = 0
    for samples, side in zip(counts, sides):
        whole = pcm[at:at + samples * frame_bytes]
        at += samples * frame_bytes
        if samples == 0:
            continue
        # Side data replaces what is left to skip; without it, the skip
        # carries on.
        discard = 0
        if side is not None:
            skip_left, discard = side
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
    kept = ffmpeg_trim(pcm, counts, [skip_data(p) for p in info["packets"]], channels)
    total = sum(int(f["nb_samples"]) for f in frames)
    if total * 2 * channels != len(kept):
        sys.exit(f"{name}: FFmpeg keeps {total} samples a channel, the trimming here {len(kept) // (2 * channels)}")
    write_answer(name, stream, frames, kept)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ffmpeg", default="")
    ap.add_argument("--reference", default="~/opusref/reference")
    ap.add_argument("--vorbis-reference", default="~/vorbisref/build/reference")
    ap.add_argument("names", nargs="*", help="the fixtures to make (all, if none)")
    args = ap.parse_args()
    exe = ".exe" if os.name == "nt" else ""
    ffmpeg = os.path.join(args.ffmpeg, "ffmpeg" + exe) if args.ffmpeg else "ffmpeg"
    ffprobe = os.path.join(args.ffmpeg, "ffprobe" + exe) if args.ffmpeg else "ffprobe"
    for name, chans, seconds, encoder, rate, opts in FIXTURES:
        if args.names and name not in args.names:
            continue
        expr = "|".join(chans)
        layout = LAYOUT[len(chans)]
        run([ffmpeg, "-hide_banner", "-loglevel", "error", "-y", "-fflags", "+bitexact",
             "-f", "lavfi", "-i", f"aevalsrc={expr}:s={rate}:d={seconds}:c={layout}",
             "-c:a", encoder, "-threads", "1", *opts, "-flags", "+bitexact",
             # The muxer's too (before `-i` it is the input's): no random
             # UIDs or dates, so that a run writes the same file as the last.
             "-fflags", "+bitexact", name])
        if encoder == "libvorbis":
            vorbis_answer(name, ffmpeg, ffprobe, args.vorbis_reference)
        else:
            opus_answer(name, ffprobe, args.reference)


main()
