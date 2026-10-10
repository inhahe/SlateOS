#!/usr/bin/env python3
"""Generate the FLAC decoder's fixtures and their answers.

Each fixture NAME.flac gets NAME.txt: what libFLAC 1.5.0's stream decoder
makes of it (gui/video/flac's `tools/reference.c`, built in WSL as its header
says) -- a line a metadata block, a frame (its first sample, size, and a hash
of its samples), an error, a seek -- to which the Rust reader is held line
for line (`tests/reference.rs`).

The streams are encoded by libFLAC's own `flac` 1.5.0 from signals made here
(raw PCM at any bit depth, channel count and rate), across its settings, so
that every part of the format a decoder reads is reached: every bit depth
from 4 to 32 (and with it the 33-bit side channel), one to eight channels and
each way of coding two, every block-size and sample-rate code, constant,
verbatim, fixed and LPC subframes up to order 32, wasted bits, Rice and
Rice2 partitions; with tags, a picture, a seek table, no padding, an ID3v2
tag in front, no metadata at all. The damaged ones are those streams with
their bytes changed by a seeded generator -- bits flipped, a frame taken out
whole, a range cut, garbage put in, the file cut short, a header's or a
metadata block's bytes broken -- so that what libFLAC does with damage is
what the reader does.

Run from this directory, with libFLAC 1.5.0 built in WSL at ~/flacref
(./configure --disable-ogg --disable-shared && make) and the reference
built from tools/reference.c:

    python generate_fixtures.py [--answers-only] [NAME...]
"""

import argparse
import math
import os
import shlex
import struct
import subprocess
import sys
import tempfile

FLAC = "~/flacref/flac-1.5.0/src/flac/flac"
REFERENCE = "~/flacref/build/reference"


def run(args, **kw):
    r = subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, **kw)
    if r.returncode != 0:
        sys.exit(f"{' '.join(args)}: {r.stderr.decode('utf-8', 'replace')}")
    return r.stdout


def wsl_path(p):
    p = os.path.abspath(p).replace(os.sep, "/")
    return "/mnt/" + p[0].lower() + p[2:]


def wsl(command):
    return run(["wsl", "-d", "Ubuntu", "--", "bash", "-c", command])


class Lcg:
    def __init__(self, seed):
        self.state = seed & 0xFFFFFFFF

    def next(self):
        self.state = (self.state * 1103515245 + 12345) & 0xFFFFFFFF
        return self.state >> 16


def signal(kind, channels, bits, rate, seconds, seed=1):
    """Interleaved samples, `bits` wide: tones and noise in proportions by
    `kind`, so the encoder chooses among its predictors."""
    n = int(rate * seconds)
    peak = (1 << (bits - 1)) - 1
    rng = Lcg(seed)
    out = []
    for i in range(n):
        t = i / rate
        for c in range(channels):
            if kind == "silence":
                v = 0
            elif kind == "noise":
                v = (rng.next() / 32768.0 - 1.0) * peak
            elif kind == "wasted":
                v = 0.6 * peak * math.sin(2 * math.pi * (220 + 55 * c) * t)
                v = int(v) & ~7
            elif kind == "correlated":
                # Two near-identical channels: the encoder codes them as a
                # side channel, 33 bits wide at 32 bits a sample.
                base = 0.8 * peak * math.sin(2 * math.pi * 330 * t) + 0.1 * peak * math.sin(2 * math.pi * 3100 * t)
                v = base + (0.001 * peak if c else 0)
            else:
                v = (0.5 * math.sin(2 * math.pi * (220 * (c + 1)) * t) + 0.2 * math.sin(2 * math.pi * 3300 * t)
                     + 0.02 * (rng.next() / 32768.0 - 1.0)) * peak
            out.append(max(-peak - 1, min(peak, int(v))))
    return out


def raw_bytes(samples, bits):
    width = (bits + 7) // 8
    out = bytearray()
    for s in samples:
        out += (s & ((1 << (8 * width)) - 1)).to_bytes(width, "little")
    return bytes(out)


def wav_bytes(samples, channels, bits, rate):
    """A WAVE_FORMAT_EXTENSIBLE file: each sample left-justified in a
    container of whole bytes, its valid bits `bits` -- how flac takes a bit
    depth its raw input does not (only 8, 16, 24 and 32 there)."""
    container = 8 * ((bits + 7) // 8)
    shift = container - bits
    data = raw_bytes([s << shift for s in samples], container)
    block = channels * container // 8
    fmt = struct.pack("<HHIIHHHHIH14s", 0xFFFE, channels, rate, rate * block, block, container, 22, bits,
                      (1 << channels) - 1 if channels <= 8 else 0, 1,
                      bytes.fromhex("000000001000800000aa00389b71"))
    riff = b"WAVE" + b"fmt " + struct.pack("<I", len(fmt)) + fmt + b"data" + struct.pack("<I", len(data)) + data
    return b"RIFF" + struct.pack("<I", len(riff)) + riff


# Name: (signal kind, channels, bits, rate, seconds, flac options).
MADE = {
    "s16_stereo_l5": ("tones", 2, 16, 44100, 0.6, ["-5"]),
    "s16_stereo_l0": ("tones", 2, 16, 44100, 0.4, ["-0"]),
    "s16_stereo_l8": ("tones", 2, 16, 44100, 0.4, ["-8"]),
    "s16_stereo_exhaustive": ("tones", 2, 16, 44100, 0.3, ["-8", "-e", "-p"]),
    "s16_mono_order32": ("tones", 1, 16, 48000, 0.3, ["--lax", "-l", "32", "-b", "4096"]),
    "s16_order_fixed_only": ("tones", 1, 16, 32000, 0.3, ["-l", "0"]),
    "s16_block_192": ("tones", 2, 16, 44100, 0.1, ["-b", "192"]),
    "s16_block_576": ("tones", 1, 16, 44100, 0.1, ["-b", "576"]),
    "s16_block_100": ("tones", 1, 16, 44100, 0.05, ["--lax", "-b", "100"]),
    "s16_block_1000": ("tones", 1, 16, 44100, 0.1, ["--lax", "-b", "1000"]),
    "s16_block_16384": ("tones", 2, 16, 44100, 0.8, ["--lax", "-b", "16384"]),
    "s16_block_65535": ("tones", 1, 16, 8000, 9.0, ["--lax", "-b", "65535"]),
    "s16_rice_order15": ("tones", 1, 16, 44100, 0.4, ["--lax", "-b", "32768", "-r", "15"]),
    "s4_mono": ("tones", 1, 4, 8000, 0.2, ["--lax"]),
    "s7_rate_11025": ("tones", 1, 7, 11025, 0.2, ["--lax"]),
    "s8_mono": ("tones", 1, 8, 8000, 0.3, []),
    "s12_stereo": ("tones", 2, 12, 16000, 0.3, []),
    "s20_stereo": ("tones", 2, 20, 48000, 0.2, []),
    "s24_stereo": ("tones", 2, 24, 96000, 0.2, ["-8"]),
    "s24_noise_rice2": ("noise", 2, 24, 44100, 0.1, []),
    "s32_stereo": ("tones", 2, 32, 44100, 0.2, ["-8"]),
    "s32_side_33bit": ("correlated", 2, 32, 44100, 0.2, ["-8"]),
    "s16_rate_22060": ("tones", 1, 16, 22060, 0.2, []),
    "s16_rate_12k": ("tones", 1, 16, 12000, 0.2, []),
    "s16_rate_768k": ("tones", 1, 16, 768000, 0.02, ["--lax"]),
    "s16_rate_655350": ("tones", 1, 16, 655350, 0.02, ["--lax"]),
    "s16_3ch": ("tones", 3, 16, 44100, 0.2, []),
    "s16_4ch": ("tones", 4, 16, 44100, 0.2, []),
    "s16_51": ("tones", 6, 16, 48000, 0.2, []),
    "s16_8ch": ("tones", 8, 16, 48000, 0.15, []),
    "s16_wasted_bits": ("wasted", 2, 16, 44100, 0.3, []),
    "s16_silence_constant": ("silence", 2, 16, 44100, 0.3, []),
    "s16_noise_verbatim": ("noise", 2, 16, 44100, 0.1, []),
    "s16_tags_picture_seektable": ("tones", 2, 16, 44100, 0.5, [
        "-T", "TITLE=Fixture", "-T", "ARTIST=SlateOS", "-S", "4096s", "--picture", "PICTURE"]),
    "s16_no_padding_no_md5": ("tones", 1, 16, 44100, 0.2, ["--no-padding", "--no-md5-sum"]),
    # The damaged fixtures' source: fourteen frames.
    "s16_stereo_b1152": ("tones", 2, 16, 44100, 0.35, ["-b", "1152"]),
}


def make(name, kind, channels, bits, rate, seconds, opts, tmp):
    samples = signal(kind, channels, bits, rate, seconds)
    if bits in (8, 16, 24, 32):
        source = os.path.join(tmp, name + ".raw")
        with open(source, "wb") as f:
            f.write(raw_bytes(samples, bits))
        form = (f"--force-raw-format --endian=little --sign=signed --channels={channels} "
                f"--bps={bits} --sample-rate={rate}")
    else:
        source = os.path.join(tmp, name + ".wav")
        with open(source, "wb") as f:
            f.write(wav_bytes(samples, channels, bits, rate))
        form = ""
    opts = list(opts)
    if "PICTURE" in opts:
        # A one-pixel PNG, made here.
        png = os.path.join(tmp, "cover.png")
        with open(png, "wb") as f:
            f.write(bytes.fromhex("89504e470d0a1a0a0000000d4948445200000001000000010806000000"
                                  "1f15c4890000000d4944415478da63f8cfc0f01f0005000201a3d1a2bb0000000049454e44ae426082"))
        opts[opts.index("PICTURE")] = "3||front||" + wsl_path(png)
    out = name + ".flac"
    wsl(f"{FLAC} -s -f {form} {' '.join(shlex.quote(o) for o in opts)} "
        f"-o {shlex.quote(wsl_path(out))} {shlex.quote(wsl_path(source))}")


def crc8(data):
    c = 0
    for b in data:
        c ^= b
        for _ in range(8):
            c = ((c << 1) ^ 0x07) & 0xFF if c & 0x80 else (c << 1) & 0xFF
    return c


def frame_starts(data):
    """Where each frame starts: a sync code whose header's CRC-8 is right
    (headers here are made by flac: no hints past 16 bits)."""
    starts = []
    i = 0
    while i + 4 < len(data):
        if data[i] == 0xFF and data[i + 1] >> 1 == 0x7C:
            # The header: 4 bytes, the coded number, the hints, the CRC.
            j = i + 4
            first = data[j]
            extra = 0 if first < 0x80 else 1 if first < 0xE0 else 2 if first < 0xF0 else 3 if first < 0xF8 else 4 if first < 0xFC else 5
            j += 1 + extra
            bs, sr = data[i + 2] >> 4, data[i + 2] & 15
            j += {6: 1, 7: 2}.get(bs, 0) + {12: 1, 13: 2, 14: 2}.get(sr, 0)
            if j < len(data) and crc8(data[i:j]) == data[j]:
                starts.append(i)
                i = j
                continue
        i += 1
    return starts


def metadata_end(data):
    """Where the metadata ends: past fLaC and its last block."""
    at = 4
    while True:
        last = data[at] & 0x80
        length = int.from_bytes(data[at + 1:at + 4], "big")
        at += 4 + length
        if last:
            return at


def damage(source, how, seed):
    with open(source + ".flac", "rb") as f:
        data = bytearray(f.read())
    rng = Lcg(seed)
    starts = frame_starts(data)
    body = metadata_end(data)
    if how == "bitflips":
        for k in (2, 5, 9):
            at = starts[k] + 20 + rng.next() % (starts[k + 1] - starts[k] - 24)
            data[at] ^= 1 << (rng.next() % 8)
    elif how == "frame_removed":
        del data[starts[4]:starts[5]]
    elif how == "range_cut":
        a = starts[3] + 100
        b = starts[6] - 50
        del data[a:b]
    elif how == "garbage":
        junk = bytes(rng.next() & 0xFF for _ in range(300)) + b"\xff\xf8\x00"
        data[starts[5]:starts[5]] = junk
    elif how == "truncated":
        del data[starts[7] + (starts[8] - starts[7]) // 2:]
    elif how == "header_crc":
        data[starts[3] + 4] ^= 0x01
    elif how == "header_sync_inside":
        data[starts[3] + 2] = 0xFF
    elif how == "tags_length":
        # The Vorbis comment block's first comment's length made too long.
        at = 4
        while True:
            kind = data[at] & 0x7F
            length = int.from_bytes(data[at + 1:at + 4], "big")
            if kind == 4:
                vendor = int.from_bytes(data[at + 4:at + 8], "little")
                c = at + 4 + 4 + vendor + 4
                data[c:c + 4] = (100000).to_bytes(4, "little")
                break
            at += 4 + length
    elif how == "id3v2_in_front":
        tag = b"ID3\x04\x00\x00" + bytes([0, 0, 1, 0]) + bytes(128)
        data[0:0] = tag
    elif how == "no_metadata":
        del data[:body]
    else:
        sys.exit(how)
    return bytes(data)


# Name: (the fixture it is made from, how).
DAMAGED = {
    "damaged_bitflips": ("s16_stereo_b1152", "bitflips"),
    "damaged_frame_removed": ("s16_stereo_b1152", "frame_removed"),
    "damaged_range_cut": ("s16_stereo_b1152", "range_cut"),
    "damaged_garbage": ("s16_stereo_b1152", "garbage"),
    "damaged_truncated": ("s16_stereo_b1152", "truncated"),
    "damaged_header_crc": ("s16_stereo_b1152", "header_crc"),
    "damaged_header_sync_inside": ("s16_stereo_b1152", "header_sync_inside"),
    "damaged_tags_length": ("s16_tags_picture_seektable", "tags_length"),
    "id3v2_in_front": ("s16_stereo_l0", "id3v2_in_front"),
    "no_metadata": ("s16_stereo_l0", "no_metadata"),
}


def total_samples(data):
    """STREAMINFO's total, where the file has one."""
    if data[:4] != b"fLaC":
        return 0
    packed = int.from_bytes(data[18:26], "big")
    return packed & 0xF_FFFF_FFFF


def answer(name, seeks):
    """The reference read through the file (with its MD5 check), then its
    seeks, each from a decoder that has read the file through: the second
    run's lines from its first seek on."""
    path = shlex.quote(wsl_path(name + ".flac"))
    lines = wsl(f"{REFERENCE} {path}").decode("ascii").splitlines()
    if seeks:
        again = wsl(f"{REFERENCE} {path} {' '.join(str(s) for s in seeks)}").decode("ascii").splitlines()
        first = next(i for i, l in enumerate(again) if l.startswith("seek "))
        lines += [l for l in again[first:] if not l.startswith("end ")]
    with open(name + ".txt", "w", encoding="utf-8", newline="\n") as f:
        f.write(f"# {name}.flac: what libFLAC 1.5.0 makes of it (generate_fixtures.py)\n")
        f.write("\n".join(lines) + "\n")
    errors = sum(1 for l in lines if l.startswith("error"))
    frames = sum(1 for l in lines if l.startswith("frame"))
    print(f"{name}: {frames} frames, {errors} errors, {lines[-1]}")


def seeks_for(data):
    total = total_samples(data)
    if total == 0:
        return []
    return [0, total // 3, total // 2 + 1, total - 1, total + 5]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--answers-only", action="store_true")
    ap.add_argument("names", nargs="*")
    args = ap.parse_args()
    want = lambda n: not args.names or n in args.names
    with tempfile.TemporaryDirectory(dir=".") as tmp:
        for name, spec in MADE.items():
            if not want(name):
                continue
            if not args.answers_only:
                make(name, *spec, tmp)
            with open(name + ".flac", "rb") as f:
                answer(name, seeks_for(f.read()))
    for name, (source, how) in DAMAGED.items():
        if not want(name):
            continue
        if not args.answers_only:
            with open(name + ".flac", "wb") as f:
                f.write(damage(source, how, seed=sum(map(ord, name))))
        answer(name, [])


main()
