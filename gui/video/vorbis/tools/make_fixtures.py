"""Writes the test streams in tests/data: synthetic signals, encoded by
libvorbis (and by FFmpeg's own Vorbis encoder, whose setups differ) through
FFmpeg, at the rates, channel counts and qualities the tests should cover.

    python3 tools/make_fixtures.py

Run where ffmpeg has libvorbis (WSL's Ubuntu ffmpeg does). The signals are
generated here from fixed seeds -- tones with vibrato, a sweep, noise, and
clicks that make the encoder switch to short blocks -- so the streams are
the same each run for the same encoder. The streams themselves, not this
script, are what the tests are held to; regenerating them means
regenerating tests/data/references.txt (tools/references.py) too.
"""

import math
import os
import pathlib
import struct
import subprocess
import tempfile

OUT = pathlib.Path(__file__).resolve().parent.parent / "tests" / "data"


class Lcg:
    def __init__(self, seed):
        self.state = seed & 0xFFFFFFFF

    def next(self):
        self.state = (self.state * 1103515245 + 12345) & 0xFFFFFFFF
        return ((self.state >> 16) & 0x7FFF) / 32768.0 - 0.5


def signal(channels, rate, seconds, kind, seed=1):
    """Interleaved float samples."""
    n = int(rate * seconds)
    noise = [Lcg(seed * 977 + c) for c in range(channels)]
    clicks = [Lcg(seed * 131 + 50 + c) for c in range(channels)]
    out = []
    for i in range(n):
        t = i / rate
        for c in range(channels):
            if kind == "silence":
                v = 0.0
            elif kind == "clicks":
                # A click every 20 ms (offset by channel), on a quiet tone.
                phase = (t + 0.003 * c) % 0.02
                v = 0.1 * math.sin(2 * math.pi * 440 * (c + 1) * t)
                if phase < 0.0005:
                    v += 0.9 * clicks[c].next() * 2
            else:
                f0 = 110.0 * (c + 1) * (1 + 0.01 * math.sin(2 * math.pi * 5 * t))
                sweep = 200 + (rate / 2.5 - 200) * (t / seconds)
                v = (
                    0.25 * math.sin(2 * math.pi * f0 * t)
                    + 0.12 * math.sin(2 * math.pi * 3 * f0 * t + c)
                    + 0.1 * math.sin(2 * math.pi * sweep * t)
                    + 0.05 * noise[c].next()
                )
                if (t + 0.07 * c) % 0.37 < 0.002:
                    v += 0.7 * clicks[c].next()
            out.append(max(-1.0, min(1.0, v)))
    return out


def encode(name, channels, rate, seconds, args, kind="music", seed=1):
    samples = signal(channels, rate, seconds, kind, seed)
    with tempfile.NamedTemporaryFile(suffix=".f32", delete=False) as raw:
        raw.write(struct.pack(f"<{len(samples)}f", *samples))
        path = raw.name
    try:
        subprocess.run(
            ["ffmpeg", "-y", "-hide_banner", "-loglevel", "error",
             "-f", "f32le", "-ar", str(rate), "-ac", str(channels), "-i", path,
             "-fflags", "+bitexact", "-flags:a", "+bitexact", *args,
             "-f", "ogg", str(OUT / name)],
            check=True,
        )
    finally:
        os.unlink(path)
    print(f"{name}: {(OUT / name).stat().st_size} bytes")


LIB = ["-c:a", "libvorbis"]
# ffmpeg ignores a negative -q:a (it means "unset"); libvorbis's lowest
# quality, -0.1, is a global quality of -1 in qscale units.
LOWEST = LIB + ["-global_quality:a", "-118", "-flags:a", "+qscale+bitexact"]
# FFmpeg's own encoder: stereo only.
NATIVE = ["-c:a", "vorbis", "-strict", "-2"]

STREAMS = [
    # name, channels, rate, seconds, encoder arguments, signal
    ("stereo_q3.ogg", 2, 44100, 2.0, LIB + ["-q:a", "3"], "music"),
    ("mono_q3.ogg", 1, 44100, 2.0, LIB + ["-q:a", "3"], "music"),
    ("stereo_q10_48k.ogg", 2, 48000, 1.0, LIB + ["-q:a", "10"], "music"),
    ("stereo_qm1.ogg", 2, 44100, 2.0, LOWEST, "music"),
    ("stereo_q0.ogg", 2, 44100, 1.0, LIB + ["-q:a", "0"], "music"),
    ("stereo_q6.ogg", 2, 44100, 1.0, LIB + ["-q:a", "6"], "music"),
    ("mono_8k_q0.ogg", 1, 8000, 3.0, LIB + ["-q:a", "0"], "music"),
    ("mono_11k_qm1.ogg", 1, 11025, 2.0, LOWEST, "music"),
    ("stereo_22k_q5.ogg", 2, 22050, 2.0, LIB + ["-q:a", "5"], "music"),
    ("stereo_96k_q4.ogg", 2, 96000, 0.5, LIB + ["-q:a", "4"], "music"),
    ("three_ch.ogg", 3, 44100, 1.0, LIB + ["-q:a", "3"], "music"),
    ("quad.ogg", 4, 44100, 1.0, LIB + ["-q:a", "2"], "music"),
    ("surround51.ogg", 6, 48000, 1.0, LIB + ["-q:a", "4"], "music"),
    ("surround71.ogg", 8, 48000, 0.5, LIB + ["-q:a", "1"], "music"),
    ("abr_64k.ogg", 2, 44100, 2.0, LIB + ["-b:a", "64k"], "music"),
    ("clicks_q3.ogg", 2, 44100, 1.5, LIB + ["-q:a", "3"], "clicks"),
    ("tiny.ogg", 2, 44100, 0.01, LIB + ["-q:a", "3"], "music"),
    ("silence.ogg", 2, 44100, 1.0, LIB + ["-q:a", "3"], "silence"),
    ("native_stereo.ogg", 2, 44100, 1.5, NATIVE, "music"),
    ("native_stereo_22k.ogg", 2, 22050, 1.5, NATIVE, "clicks"),
]

if __name__ == "__main__":
    OUT.mkdir(parents=True, exist_ok=True)
    for name, channels, rate, seconds, args, kind in STREAMS:
        encode(name, channels, rate, seconds, args, kind)
