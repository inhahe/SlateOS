#!/usr/bin/env python3
"""Generate the MP3 decoder's fixtures and their answers.

Each fixture NAME.mp1/.mp2/.mp3 gets NAME.txt: what minimp3 makes of it
(gui/video/mp3's `tools/reference.c`, built against minimp3 with the crate's
two documented differences applied by `tools/patch_minimp3.py`) -- a line a
`mp3dec_decode_frame` call, each given the rest of the file: where it
started, the bytes it took, the samples and the frame's facts, and a hash of
the samples -- to which the Rust decoder is held line for line
(`tests/fixtures.rs`).

The streams are made from signals made here, by the encoders a desktop's
files come from, across their settings, so that every part of the format a
decoder reads is reached:

- Layer III by LAME (through FFmpeg's libmp3lame, and LAME 3.100's own
  command line for what FFmpeg cannot ask of it): MPEG-1, -2 and -2.5 at all
  nine sample rates, mono, stereo, joint (mid/side) stereo and dual channel,
  constant and variable bit rates, short blocks from sharp attacks, no bit
  reservoir, free format, CRC; and by shine, a fixed-point encoder.
- Layer II by FFmpeg's own encoder (MPEG-1 and MPEG-2 rates) and by twolame
  (joint stereo's intensity bands, dual channel, CRC).
- Layer I, which none of them write, by `layer1()` below: valid frames
  whose allocations, scale factors and samples are drawn by a seeded
  generator -- noise, but every field of the syntax at every value it may
  take, in stereo, joint stereo (each intensity bound), mono, with CRC and
  in free format.
- Damage: those streams with their bytes changed by a seeded generator --
  bits flipped, garbage put in, a range cut, the file cut short, ID3 and APE
  tags around the frames -- so that what minimp3 does with damage is what
  the decoder does.

Layer III's intensity stereo and mixed blocks are written by no encoder
here; minimp3's own vectors (the ISO conformance streams among them) reach
them, read by the ignored test `tests/vectors.rs`.

Run from this directory, with these in WSL:

- FFmpeg with libmp3lame, libshine and libtwolame (Ubuntu's has all three);
- LAME 3.100 built at ~/lameref/install (./configure --prefix=$HOME/lameref/install
  --disable-shared && make install);
- the reference, built as `tools/reference.c` says, at ~/mp3ref/reference.

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

FFMPEG = "ffmpeg -y -hide_banner -loglevel error"
LAME = "~/lameref/install/bin/lame"
REFERENCE = "~/mp3ref/reference"


def run(args):
    r = subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if r.returncode != 0:
        sys.exit(f"{' '.join(args)}: {r.stderr.decode('utf-8', 'replace')}")
    return r.stdout


def wsl_path(p):
    p = os.path.abspath(p).replace(os.sep, "/")
    return "/mnt/" + p[0].lower() + p[2:]


def wsl(command):
    return run(["wsl", "-d", "Ubuntu", "--", "bash", "-c", command])


class Lcg:
    """A seeded generator, the same on every machine."""

    def __init__(self, seed):
        self.state = seed & 0xFFFFFFFFFFFFFFFF

    def next(self):
        self.state = (self.state * 6364136223846793005 + 1442695040888963407) & 0xFFFFFFFFFFFFFFFF
        return self.state >> 33

    def below(self, n):
        return self.next() % n

    def unit(self):
        return self.next() / float(1 << 31) * 2.0 - 1.0


def signal(rate, channels, seconds, style, seed=1):
    """Interleaved 16-bit samples: `music` (partials that glide and swell,
    the channels alike but not the same, a little noise, a quiet stretch),
    or `attacks` (silence broken by clicks and bursts, for short blocks)."""
    lcg = Lcg(seed)
    n = int(rate * seconds)
    out = bytearray()
    for i in range(n):
        t = i / rate
        frame = []
        for ch in range(channels):
            if style == "music":
                v = 0.0
                for k, (f, a) in enumerate(((220.0, 0.30), (330.0, 0.18), (523.25, 0.12), (1760.0, 0.06), (5000.0, 0.03))):
                    f *= 1.0 + 0.02 * math.sin(2 * math.pi * 0.7 * t + k + ch)
                    swell = 0.6 + 0.4 * math.sin(2 * math.pi * (0.5 + 0.3 * k) * t + ch * 0.9)
                    v += a * swell * math.sin(2 * math.pi * f * t + ch * 0.4 * k)
                v += 0.02 * lcg.unit()
                if 0.42 < (t % 1.0) < 0.47:
                    v *= 0.0005
            else:
                period = 0.11 + 0.03 * ch
                phase = t % period
                v = 0.0
                if phase < 0.004:
                    v = (0.9 if ch == 0 else -0.8) * math.exp(-phase * 900.0) * lcg.unit()
                elif phase < 0.03:
                    v = 0.25 * math.exp(-phase * 120.0) * math.sin(2 * math.pi * 3000.0 * t)
            frame.append(max(-32768, min(32767, int(round(v * 32767.0)))))
        out += struct.pack("<%dh" % channels, *frame)
    return bytes(out)


def encode(name, ext, rate, channels, seconds, style, how, args):
    raw = tempfile.NamedTemporaryFile(suffix=".raw", delete=False, dir=".")
    raw.write(signal(rate, channels, seconds, style))
    raw.close()
    src = shlex.quote(wsl_path(raw.name))
    dst = shlex.quote(wsl_path(f"{name}.{ext}"))
    a = " ".join(shlex.quote(x) for x in args)
    try:
        if how == "ffmpeg":
            wsl(f"{FFMPEG} -f s16le -ar {rate} -ac {channels} -i {src} -flags +bitexact {a} {dst}")
        else:
            mode = [] if "-m" in args else ["-m", "m" if channels == 1 else "j"]
            m = " ".join(mode)
            wsl(f"{LAME} --quiet -r --signed --little-endian --bitwidth 16 -s {rate / 1000:g} {m} {a} {src} {dst}")
    finally:
        os.unlink(raw.name)


class Bits:
    def __init__(self):
        self.out = bytearray()
        self.acc = 0
        self.n = 0

    def put(self, value, width):
        for k in range(width - 1, -1, -1):
            self.acc = (self.acc << 1) | ((value >> k) & 1)
            self.n += 1
            if self.n == 8:
                self.out.append(self.acc)
                self.acc = 0
                self.n = 0

    def bits(self):
        return len(self.out) * 8 + self.n

    def pad_to(self, total_bits):
        while self.bits() < total_bits:
            self.put(0, 1)
        return bytes(self.out)


L1_KBPS = [0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448]


def layer1(name, rate_idx, bitrate_idx, mode, frames, seed, crc=False, free_bytes=0):
    """Valid MPEG-1 Layer I frames, every field drawn by a seeded generator:
    allocations of every width (0 to 14, never the forbidden 15), scale
    factors of every index but the reserved 63, samples of every code but
    the forbidden all-ones -- fitted to the frame -- in `mode` (joint stereo
    cycling through the four intensity bounds), with a CRC field (not
    checked by minimp3; zeros here), or in free format at `free_bytes` a
    frame."""
    lcg = Lcg(seed)
    hz = [44100, 48000, 32000][rate_idx]
    nch = 1 if mode == 3 else 2
    out = bytearray()
    for f in range(frames):
        mode_ext = f % 4 if mode == 1 else 0
        if free_bytes:
            frame_bytes, padding = free_bytes, 0
        else:
            padding = lcg.below(2) if hz == 44100 else 0
            frame_bytes = (12 * L1_KBPS[bitrate_idx] * 1000 // hz + padding) * 4
        b = Bits()
        b.put(0xFFF, 12)
        b.put(1, 1)  # MPEG-1
        b.put(3, 2)  # Layer I
        b.put(0 if crc else 1, 1)
        b.put(0 if free_bytes else bitrate_idx, 4)
        b.put(rate_idx, 2)
        b.put(padding, 1)
        b.put(0, 1)  # private
        b.put(mode, 2)
        b.put(mode_ext, 2)
        b.put(0, 1)  # copyright
        b.put(1, 1)  # original
        b.put(0, 2)  # emphasis
        if crc:
            b.put(0, 16)
        bound = (mode_ext + 1) * 4 if mode == 1 else 32
        # alloc[ch][sb]; in the intensity bands the second channel shares
        # the first's.
        alloc = [[lcg.below(15) if lcg.below(4) else 0 for _ in range(32)] for _ in range(nch)]
        if nch == 2:
            for sb in range(bound, 32):
                alloc[1][sb] = alloc[0][sb]

        def cost():
            c = b.bits() + 4 * (bound * nch + (32 - bound) if nch == 2 else 32)
            for sb in range(32):
                for ch in range(nch):
                    if alloc[ch][sb]:
                        c += 6
                        if sb < bound or ch == 0:
                            c += 12 * (alloc[ch][sb] + 1)
            return c

        while cost() > frame_bytes * 8:
            ch, sb = lcg.below(nch), lcg.below(32)
            if alloc[ch][sb]:
                alloc[ch][sb] -= 1
                if nch == 2 and sb >= bound:
                    alloc[1 - ch][sb] = alloc[ch][sb]
        for sb in range(32):
            if nch == 2 and sb < bound:
                b.put(alloc[0][sb], 4)
                b.put(alloc[1][sb], 4)
            else:
                b.put(alloc[0][sb], 4)
        for sb in range(32):
            for ch in range(nch):
                if alloc[ch][sb]:
                    b.put(8 + lcg.below(40) if lcg.below(8) else lcg.below(63), 6)
        for _ in range(12):
            for sb in range(32):
                chans = range(nch) if sb < bound else range(1)
                for ch in chans:
                    a = alloc[ch][sb]
                    if a:
                        b.put(lcg.below((1 << (a + 1)) - 1), a + 1)
        out += b.pad_to(frame_bytes * 8)
    with open(f"{name}.mp1", "wb") as fh:
        fh.write(bytes(out))


def damage(name, source, ext, how, seed):
    """`source`'s bytes, changed by a seeded generator."""
    with open(source, "rb") as fh:
        data = bytearray(fh.read())
    lcg = Lcg(seed)
    if how == "bitflips":
        for _ in range(60):
            p = 200 + lcg.below(len(data) - 200)
            data[p] ^= 1 << lcg.below(8)
    elif how == "headers":
        # Flips in headers and side information only: just after sync words.
        syncs = [k for k in range(len(data) - 1) if data[k] == 0xFF and data[k + 1] & 0xE0 == 0xE0]
        for _ in range(25):
            p = syncs[lcg.below(len(syncs))] + 1 + lcg.below(20)
            data[p] ^= 1 << lcg.below(8)
    elif how == "garbage":
        for _ in range(6):
            p = lcg.below(len(data))
            junk = bytes(lcg.below(256) for _ in range(50 + lcg.below(400)))
            # With false sync words in it.
            junk = junk[:10] + b"\xff\xfb\x90\x64" + junk[14:]
            data[p:p] = junk
    elif how == "cut":
        p = len(data) // 3 + lcg.below(len(data) // 3)
        del data[p:p + 1000 + lcg.below(3000)]
    elif how == "truncated":
        del data[len(data) - 1 - lcg.below(300):]
    elif how == "tags":
        # An ID3v2.3 tag of 256 bytes in front, its title holding a false
        # sync word as real tags' pictures and texts do; an APEv2 tag (with
        # its header) and an ID3v1 tag behind.
        body = (b"\x00" + b"A title \xff\xfb\x90\x00 in Latin-1").ljust(245, b"\x00")
        frame = b"TIT2" + struct.pack(">I", len(body)) + b"\x00\x00" + body
        id3v2 = b"ID3\x03\x00\x00" + bytes([0, 0, 2, 0]) + frame.ljust(256, b"\x00")
        item = struct.pack("<II", 5, 0) + b"Title\x00" + b"hello"
        size = len(item) + 32
        ape = (b"APETAGEX" + struct.pack("<IIII", 2000, size, 1, 0xA0000000) + bytes(8)
               + item
               + b"APETAGEX" + struct.pack("<IIII", 2000, size, 1, 0x80000000) + bytes(8))
        id3v1 = b"TAG" + b"A title".ljust(30, b"\x00") + bytes(95)
        assert len(id3v1) == 128
        data = bytearray(id3v2) + data + ape + id3v1
    with open(f"{name}.{ext}", "wb") as fh:
        fh.write(bytes(data))


# name: (extension, maker, its arguments)
FIXTURES = {
    # Layer III, LAME through FFmpeg: no ID3v2 tag, no Xing frame.
    "lame_cbr128_joint_44100": ("mp3", "ffmpeg", 44100, 2, 1.0, "music", ["-c:a", "libmp3lame", "-b:a", "128k"]),
    "lame_cbr320_stereo_48000": ("mp3", "ffmpeg", 48000, 2, 0.5, "music", ["-c:a", "libmp3lame", "-b:a", "320k", "-joint_stereo", "0"]),
    "lame_vbr_mono_32000": ("mp3", "ffmpeg", 32000, 1, 1.0, "music", ["-c:a", "libmp3lame", "-q:a", "4"]),
    "lame_mpeg2_24000_joint": ("mp3", "ffmpeg", 24000, 2, 1.0, "music", ["-c:a", "libmp3lame", "-b:a", "64k"]),
    "lame_mpeg2_22050_stereo": ("mp3", "ffmpeg", 22050, 2, 1.0, "music", ["-c:a", "libmp3lame", "-b:a", "96k", "-joint_stereo", "0"]),
    "lame_mpeg2_16000_mono": ("mp3", "ffmpeg", 16000, 1, 1.0, "music", ["-c:a", "libmp3lame", "-b:a", "32k"]),
    "lame_mpeg25_12000_stereo": ("mp3", "ffmpeg", 12000, 2, 1.0, "music", ["-c:a", "libmp3lame", "-b:a", "48k", "-joint_stereo", "0"]),
    "lame_mpeg25_11025_joint": ("mp3", "ffmpeg", 11025, 2, 1.0, "music", ["-c:a", "libmp3lame", "-b:a", "32k"]),
    "lame_mpeg25_8000_mono": ("mp3", "ffmpeg", 8000, 1, 1.5, "music", ["-c:a", "libmp3lame", "-b:a", "16k"]),
    "lame_attacks_44100": ("mp3", "ffmpeg", 44100, 2, 1.0, "attacks", ["-c:a", "libmp3lame", "-b:a", "192k"]),
    "lame_attacks_mpeg2_22050": ("mp3", "ffmpeg", 22050, 1, 1.0, "attacks", ["-c:a", "libmp3lame", "-b:a", "48k"]),
    "lame_noreservoir_44100": ("mp3", "ffmpeg", 44100, 2, 0.5, "music", ["-c:a", "libmp3lame", "-b:a", "128k", "-reservoir", "0"]),
    # With FFmpeg's ID3v2 tag and LAME's Xing/Info frame in front, as an
    # .mp3 from FFmpeg comes.
    "lame_tagged_44100": ("mp3", "ffmpeg", 44100, 2, 0.5, "music", ["-c:a", "libmp3lame", "-b:a", "128k", "-write_xing", "1"]),
    # Layer III, LAME 3.100's command line: free format, CRC, dual channel.
    "lame_freeformat_44100": ("mp3", "lame", 44100, 2, 0.5, "music", ["-t", "--freeformat", "-b", "400"]),
    "lame_freeformat_mpeg2_24000": ("mp3", "lame", 24000, 1, 0.5, "music", ["-t", "--freeformat", "-b", "200"]),
    "lame_crc_dual_48000": ("mp3", "lame", 48000, 2, 0.5, "music", ["-t", "-p", "-m", "d", "-b", "160"]),
    "lame_crc_mpeg25_11025": ("mp3", "lame", 11025, 1, 0.5, "attacks", ["-t", "-p", "-b", "24"]),
    # Layer III, shine.
    "shine_128_44100": ("mp3", "ffmpeg", 44100, 2, 1.0, "music", ["-c:a", "libshine", "-b:a", "128k"]),
    "shine_mono_32000": ("mp3", "ffmpeg", 32000, 1, 1.0, "attacks", ["-c:a", "libshine", "-b:a", "64k"]),
    # Layer II.
    "mp2_192_stereo_48000": ("mp2", "ffmpeg", 48000, 2, 1.0, "music", ["-c:a", "mp2", "-b:a", "192k"]),
    "mp2_56_mono_44100": ("mp2", "ffmpeg", 44100, 1, 1.0, "music", ["-c:a", "mp2", "-b:a", "56k"]),
    "mp2_32_stereo_32000": ("mp2", "ffmpeg", 32000, 2, 1.0, "music", ["-c:a", "mp2", "-b:a", "64k"]),
    "mp2_mpeg2_22050_mono": ("mp2", "ffmpeg", 22050, 1, 1.0, "music", ["-c:a", "mp2", "-b:a", "64k"]),
    "mp2_mpeg2_16000_stereo": ("mp2", "ffmpeg", 16000, 2, 1.0, "attacks", ["-c:a", "mp2", "-b:a", "96k"]),
    "twolame_joint_44100": ("mp2", "ffmpeg", 44100, 2, 1.0, "music", ["-c:a", "libtwolame", "-b:a", "128k", "-mode", "joint_stereo"]),
    "twolame_crc_dual_32000": ("mp2", "ffmpeg", 32000, 2, 1.0, "music", ["-c:a", "libtwolame", "-b:a", "192k", "-mode", "dual_channel", "-error_protection", "1"]),
    "twolame_mpeg2_24000_joint": ("mp2", "ffmpeg", 24000, 2, 1.0, "attacks", ["-c:a", "libtwolame", "-b:a", "80k", "-mode", "joint_stereo"]),
}

# name: (rate index, bitrate index, mode, frames, seed, CRC, free-format bytes)
LAYER1 = {
    "layer1_stereo_48000": (1, 12, 0, 40, 11, False, 0),
    "layer1_joint_44100": (0, 8, 1, 40, 12, False, 0),
    "layer1_dual_32000": (2, 14, 2, 30, 13, True, 0),
    "layer1_mono_crc_44100": (0, 4, 3, 40, 14, True, 0),
    "layer1_free_48000": (1, 0, 0, 30, 15, False, 600),
}

# name: (source, extension, how, seed)
DAMAGED = {
    "damaged_bitflips": ("lame_cbr128_joint_44100.mp3", "mp3", "bitflips", 21),
    "damaged_headers": ("lame_cbr128_joint_44100.mp3", "mp3", "headers", 22),
    "damaged_garbage": ("lame_mpeg2_24000_joint.mp3", "mp3", "garbage", 23),
    "damaged_cut": ("lame_vbr_mono_32000.mp3", "mp3", "cut", 24),
    "damaged_truncated": ("lame_attacks_44100.mp3", "mp3", "truncated", 25),
    "damaged_tags": ("lame_cbr320_stereo_48000.mp3", "mp3", "tags", 26),
    "damaged_layer2_bitflips": ("twolame_joint_44100.mp2", "mp2", "bitflips", 27),
    "damaged_layer2_garbage": ("mp2_mpeg2_22050_mono.mp2", "mp2", "garbage", 28),
    "damaged_layer1_bitflips": ("layer1_joint_44100.mp1", "mp1", "bitflips", 29),
    "damaged_freeformat_cut": ("lame_freeformat_44100.mp3", "mp3", "cut", 30),
}


def extension(name):
    if name in FIXTURES:
        return FIXTURES[name][0]
    if name in LAYER1:
        return "mp1"
    return DAMAGED[name][1]


def answer(name):
    path = shlex.quote(wsl_path(f"{name}.{extension(name)}"))
    out = wsl(f"{REFERENCE} {path}")
    with open(f"{name}.txt", "wb") as fh:
        fh.write(out)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--answers-only", action="store_true")
    ap.add_argument("names", nargs="*")
    args = ap.parse_args()
    os.chdir(os.path.dirname(os.path.abspath(__file__)))
    names = args.names or list(FIXTURES) + list(LAYER1) + list(DAMAGED)
    for name in names:
        if not args.answers_only:
            if name in FIXTURES:
                ext, how, rate, channels, seconds, style, extra = FIXTURES[name]
                if how == "ffmpeg" and "-write_xing" not in extra:
                    extra = extra + ["-write_xing", "0", "-id3v2_version", "0"]
                encode(name, ext, rate, channels, seconds, style, how, extra)
            elif name in LAYER1:
                layer1(name, *LAYER1[name])
            else:
                source, ext, how, seed = DAMAGED[name]
                damage(name, source, ext, how, seed)
        answer(name)
        print(name)


if __name__ == "__main__":
    main()
