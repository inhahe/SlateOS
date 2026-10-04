"""The digests the decoder's tests hold it to: what libopus's own decoders
make of each test stream, at every output rate and channel count -- intact,
with packets lost, and damaged -- and with each decoder setting.

    python3 tools/references.py OPUS_DEMO REFERENCE STREAMS OUT [--vectors DIR OUT2] [--keep DIR]

OPUS_DEMO is libopus 1.5.2's opus_demo and REFERENCE tools/reference.c, both
built against its fixed-point library (configure --enable-fixed-point
--disable-intrinsics, CFLAGS="-O2 -ffp-contract=off"). STREAMS holds the
streams tools/make_streams.c wrote (NAME.bit, and NAME.head for a
multistream one); OUT gets a line for each decode:

    NAME RATE CHANNELS VARIANT BYTES FNV1A64 ERRORS ERRORS_FNV1A32 STATE_FNV1A32

-- the output's length and digest; how many decode calls failed or made
nothing, with a digest of `PACKET:CODE;` for each (libopus's error codes);
and a digest of the decoder's state after each packet, `BANDWIDTH:PITCH:
DURATION:RANGE;` (REFERENCE's `S` lines: what OPUS_GET_BANDWIDTH,
OPUS_GET_PITCH -- a single stream's -- OPUS_GET_LAST_PACKET_DURATION and
OPUS_GET_FINAL_RANGE say).

VARIANT is `+`-joined: `plain`, `lossy` (packets lost in the pattern lossy()
makes) or `damaged` (packets damaged as damaged() damages them), then any of
`gain=N`, `noinv`, `float`, `reset=N` (the decoder reset before every Nth
packet) and `odd` (odd calls between packets, drawn from odd_seed(); see
REFERENCE). The tests make the same losses, damage and calls. Every
decode of a single stream without settings is made twice, by opus_demo and by
REFERENCE, and the two must agree, so that REFERENCE's loop is known to be
opus_demo's. `@random` is not a file but random_packets()'s made-up stream.

With --vectors, the RFC 8251 test vectors in DIR (testvectorNN.bit) are done
the same way, plain, lossy and damaged, into OUT2. --keep DIR keeps every
output, as NAME_RATE_CHANNELS_VARIANT.pcm, for a test to compare sample by
sample (OPUS_REFERENCE).
"""

import os
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

RATES = [48000, 24000, 16000, 12000, 8000]

# What every stream is decoded as, at every rate and channel count.
VARIANTS = ["plain", "lossy", "damaged"]

# Decoder settings and calls opus_demo has no option for, and the streams
# they are tried on: (stream, rate, channels, variant). `odd` goes over every
# single stream (SINGLE_STREAMS: whatever is in STREAMS without a head).
def single_variants(single_streams):
    return (
        [(s, r, c, f"plain+gain={g}")
         for s in ["switching_speech", "celt_loud", "silk_nb"]
         for (r, c) in [(48000, 2), (16000, 1)]
         for g in [-32768, -1536, 1536, 6000, 32767]]
        + [(s, r, c, "plain+noinv")
           for s in ["celt_fb_stereo", "celt_voice_stereo", "celt_nb_5ms", "celt_loud",
                     "switching_music", "switching_speech", "hybrid_swb", "celt_tone_stereo"]
           for (r, c) in [(48000, 2), (24000, 2), (48000, 1)]]
        + [(s, r, c, v)
           for s in ["switching_speech", "frame_sizes_cbr", "silk_mb_stereo", "celt_2_5ms"]
           for (r, c) in [(48000, 2), (8000, 1)]
           for v in ["plain+float", "lossy+float", "damaged+float"]]
        + [(s, r, c, v)
           for s in ["switching_speech", "frame_sizes_cbr", "silk_wb_60ms", "hybrid_fb", "celt_voice"]
           for (r, c) in [(48000, 2), (16000, 1)]
           for v in ["plain+reset=50", "lossy+reset=37"]]
        + [(s, r, c, v)
           for s in single_streams
           for (r, c) in [(48000, 2), (12000, 1)]
           for v in ["plain+odd", "lossy+odd", "damaged+odd"]]
    )


MULTI_VARIANTS = (
    [(s, r, v)
     for s in ["surround", "surround_voice", "ambisonics", "ambisonics_2nd", "discrete"]
     for r in [48000]
     for v in ["plain+float", "lossy+float", "damaged+float", "plain+gain=1536", "plain+noinv",
               "plain+reset=40", "lossy+odd", "damaged+odd"]]
    + [(s, 16000, "lossy+odd")
       for s in ["surround_voice", "ambisonics_family2", "discrete"]]
)


def fnv1a64(data):
    h = 0xCBF29CE484222325
    for b in data:
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def fnv1a32(data):
    h = 0x811C9DC5
    for b in data:
        h ^= b
        h = (h * 0x01000193) & 0xFFFFFFFF
    return h


def loss_seed(name):
    """The loss pattern's seed: an RFC vector's number times 7919, as
    tools/make_lossy.py had it; for any other stream, its name's FNV-1a."""
    if name.startswith("testvector"):
        return int(name[len("testvector"):]) * 7919
    return fnv1a32(name.encode())


class Lcg:
    """The damage pattern's generator: 15 bits a draw."""

    def __init__(self, seed):
        self.seed = seed

    def next(self):
        self.seed = (self.seed * 1103515245 + 12345) & 0x7FFFFFFF
        return self.seed >> 16


def damage_seed(name):
    return fnv1a32((name + "/damage").encode())


def odd_seed(name):
    """The seed of `odd`'s calls: its draws are the damage pattern's
    generator's (REFERENCE's odd_next), from this."""
    return fnv1a32((name + "/odd").encode())


def damaged(bit, seed):
    """`bit` with packets damaged -- bits flipped, cut short (to nothing:
    lost; or to a few bytes), the payload made random, the TOC changed,
    random bytes added -- and every final range 0, so that nothing holds the
    decoder to the encoder's."""
    rng = Lcg(seed)
    out = bytearray()
    at = 0
    while at + 8 <= len(bit):
        (length,) = struct.unpack(">I", bit[at:at + 4])
        p = bytearray(bit[at + 8:at + 8 + length])
        at += 8 + length
        action = rng.next() % 12
        if action < 3 and p:
            for _ in range(1 + rng.next() % 4):
                i = rng.next() % len(p)
                p[i] ^= 1 << (rng.next() % 8)
        elif action == 3:
            del p[rng.next() % (len(p) + 1):]
        elif action == 4 and p:
            for i in range(1, len(p)):
                p[i] = rng.next() & 0xFF
        elif action == 5 and p:
            p[0] = rng.next() & 0xFF
        elif action == 6:
            p += bytes(rng.next() & 0xFF for _ in range(1 + rng.next() % 16))
            del p[1500:]
        elif action == 7:
            # Cut to 1 to 8 bytes: shorter than a multistream packet's
            # self-delimited lengths can be.
            del p[1 + rng.next() % 8:]
        out += struct.pack(">II", len(p), 0) + p
    return bytes(out)


def random_packets():
    """A stream of made-up packets: every TOC, with random payloads of 0 to
    23 bytes and a few longer -- the `@random` stream, which is not a file:
    the tests make it as this does."""
    rng = Lcg(1)
    out = bytearray()
    for toc in range(256):
        for length in list(range(24)) + [40, 64, 100, 300, 1000, 1275]:
            p = bytes([toc]) + bytes(rng.next() & 0xFF for _ in range(length))
            out += struct.pack(">II", len(p), 0) + p
    return bytes(out)


def lossy(bit, seed):
    """`bit` with packets marked lost (a length of 0, the final range kept),
    in bursts of 1 to 8 so that both the short and the long concealment paths
    run, none of the first three."""
    out = bytearray()
    burst = 0
    total = 0
    at = 0
    while at + 8 <= len(bit):
        length, rng = struct.unpack(">II", bit[at:at + 8])
        payload = bit[at + 8:at + 8 + length]
        at += 8 + length
        total += 1
        seed = (seed * 1103515245 + 12345) & 0x7FFFFFFF
        if burst == 0 and seed % 100 < 6:
            burst = 1 + (seed >> 8) % 8
        if burst > 0 and total > 3:
            burst -= 1
            out += struct.pack(">II", 0, rng)
        else:
            burst = max(0, burst - 1) if total <= 3 else burst
            out += struct.pack(">II", length, rng) + payload
    return bytes(out)


class Runner:
    def __init__(self, opus_demo, reference, tmp, keep):
        self.opus_demo = opus_demo
        self.reference = reference
        self.tmp = Path(tmp)
        self.keep = keep
        self.lines = []

    def run(self, argv):
        r = subprocess.run(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        err = r.stderr.decode("utf-8", "replace")
        if r.returncode != 0 or "mismatch" in err:
            sys.exit(f"{' '.join(map(str, argv))} failed ({r.returncode}):\n{err}")
        return err

    def decode(self, name, bit, rate, channels, variant, head=None):
        tokens = variant.split("+")
        source = self.tmp / "in.bit"
        if tokens[0] == "lossy":
            bit = lossy(bit, loss_seed(name))
        elif tokens[0] == "damaged":
            bit = damaged(bit, damage_seed(name))
        elif tokens[0] != "plain":
            sys.exit(f"unknown variant {variant}")
        source.write_bytes(bit)
        out = self.tmp / "out.pcm"
        options = []
        for t in tokens[1:]:
            if t.startswith("gain="):
                options += ["-gain", t[len("gain="):]]
            elif t.startswith("reset="):
                options += ["-reset", t[len("reset="):]]
            elif t == "odd":
                options += ["-odd", str(odd_seed(name))]
            elif t in ("noinv", "float"):
                options.append("-" + t)
            else:
                sys.exit(f"unknown variant {t}")
        if head is not None:
            options += ["-head", str(head)]
        err = self.run([self.reference, *options, str(rate), str(channels), str(source), str(out)])
        data = out.read_bytes()
        if head is None and len(tokens) == 1:
            # opus_demo's own decode must be the same.
            demo = self.tmp / "demo.pcm"
            self.run([self.opus_demo, "-d", str(rate), str(channels), str(source), str(demo)])
            if demo.read_bytes() != data:
                sys.exit(f"{name} {rate} {channels} {variant}: reference differs from opus_demo")
        # The decode calls that failed or made nothing: how many, and a digest
        # of `PACKET:CODE;` for each; and the state after each packet.
        fields = [l.split() for l in err.splitlines()]
        errors = "".join(f"{f[1]}:{f[2]};" for f in fields if f and f[0] == "E")
        state = "".join(f"{f[1]};" for f in fields if f and f[0] == "S")
        self.lines.append(
            f"{name} {rate} {channels} {variant} {len(data)} {fnv1a64(data):016x} "
            f"{errors.count(';')} {fnv1a32(errors.encode()):08x} {fnv1a32(state.encode()):08x}"
        )
        if self.keep:
            stem = Path(self.keep) / f"{name}_{rate}_{channels}_{variant}"
            stem.with_suffix(".pcm").write_bytes(data)
            stem.with_suffix(".state").write_text(state.replace(";", "\n"), encoding="utf-8", newline="\n")
            stem.with_suffix(".errors").write_text(errors.replace(";", "\n"), encoding="utf-8", newline="\n")

    def write(self, path, header):
        Path(path).write_text(header + "".join(line + "\n" for line in self.lines), encoding="utf-8", newline="\n")
        self.lines = []


HEADER = """\
# What libopus 1.5.2's fixed-point decoders make of each stream: written by
# tools/references.py, read by the tests. NAME RATE CHANNELS VARIANT BYTES
# FNV1A64 ERRORS ERRORS_FNV1A32 STATE_FNV1A32 -- see tools/references.py for
# what each is.
"""


def main():
    args = sys.argv[1:]
    keep = None
    vectors = None
    if "--keep" in args:
        i = args.index("--keep")
        keep = args[i + 1]
        del args[i:i + 2]
        os.makedirs(keep, exist_ok=True)
    if "--vectors" in args:
        i = args.index("--vectors")
        vectors = (Path(args[i + 1]), args[i + 2])
        del args[i:i + 3]
    if len(args) != 4:
        sys.exit(__doc__)
    opus_demo, reference, streams, out = args
    streams = Path(streams)
    with tempfile.TemporaryDirectory() as tmp:
        runner = Runner(opus_demo, reference, tmp, keep)
        names = sorted(p.stem for p in streams.glob("*.bit"))
        for name in names:
            bit = (streams / f"{name}.bit").read_bytes()
            head = streams / f"{name}.head"
            if head.exists():
                channels = head.read_bytes()[9]
                for rate in RATES:
                    for variant in VARIANTS:
                        runner.decode(name, bit, rate, channels, variant, head)
            else:
                for rate in RATES:
                    for channels in [1, 2]:
                        for variant in VARIANTS:
                            runner.decode(name, bit, rate, channels, variant)
            print(name, file=sys.stderr)
        for rate in RATES:
            for channels in [1, 2]:
                runner.decode("@random", random_packets(), rate, channels, "plain")
        singles = [n for n in names if not (streams / f"{n}.head").exists()]
        for (name, rate, channels, variant) in single_variants(singles):
            runner.decode(name, (streams / f"{name}.bit").read_bytes(), rate, channels, variant)
        for (name, rate, variant) in MULTI_VARIANTS:
            head = streams / f"{name}.head"
            runner.decode(name, (streams / f"{name}.bit").read_bytes(), rate, head.read_bytes()[9], variant, head)
        runner.write(out, HEADER)
        if vectors:
            vdir, vout = vectors
            for n in range(1, 13):
                name = f"testvector{n:02}"
                bit = (vdir / f"{name}.bit").read_bytes()
                for rate in RATES:
                    for channels in [1, 2]:
                        for variant in VARIANTS:
                            runner.decode(name, bit, rate, channels, variant)
                print(name, file=sys.stderr)
            runner.write(vout, HEADER.replace("each stream", "each RFC 8251 test vector"))


main()
