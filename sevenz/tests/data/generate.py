#!/usr/bin/env python3
"""Writes the sevenz crate's fixtures: archives 7-Zip 26.00 made, and 7-Zip's
own verdict on each -- and on every one-byte corruption of a few small ones.

The oracle is 7-Zip 26.00 for Windows (`C:/Program Files/7-Zip/7z.exe`),
the release the port's LZMA SDK 26.00 sources are from. Run from Windows:

    python sevenz/tests/data/generate.py

(`generate.py crafted` remakes only the crafted archives, below, and
`generate.py single` only the one-file archives.)

What it writes, beside itself:

- `input.txt`: the tree every archive is made of -- text, random bytes,
  machine-code-like bytes, an empty file, an empty folder, a name that is not
  ASCII -- one line an entry, with each file's size and FNV-1a hash. The tree
  is generated, built in `input/` while the archives are made, and removed.
- `made/`: archives of `input/` made by 7-Zip with each method, filter and
  option the reader is to handle, and `made.txt`: how each was made and what
  `7z t` said.
- `mutations.txt`: every byte of a few small archives XORed with 01, 80 and
  FF, and chosen bytes of two LZMA2 archives of several chunks -- every bit
  of every chunk header, and data bytes at each chunk's start, middle and
  end -- with `7z t`'s verdict on each: OK, or the errors it reported.
- `mutations-mmt-off.txt`: the same, tested by 7-Zip with one thread
  (`-mmt=off`). Its LZMA2 decoder works differently with several threads,
  and on some damaged streams the two give back different amounts of data.
- `made/crafted-*.7z` and `crafted.txt`: what no one-byte mutant makes,
  from the small archives by rewriting a packed stream and the header with
  it -- bytes after a coder's stream, two BZip2 streams in one coder, and
  Deflate streams zlib would refuse -- with `7z t`'s verdict on each.
- `made/<name>.7z` for each of `SINGLE`, and `single.txt`: archives of one
  file each, made for what the tree has too little of -- RISC-V code whose
  AUIPC pairs 7-Zip's encoder escapes, an LZMA coder with a property other
  than lc changed -- with the file's size and hash, which every one gives
  back whole (`7z t` says OK of each, or nothing is written).

7-Zip 26.00 sometimes crashes -- an access violation -- testing a damaged
LZMA2 archive of several dictionary-reset blocks with several threads; on the
worst of the mutants here about one run in twelve dies and the rest agree. A
crash is not a verdict, so a run that crashes is run again, and the crashes
are counted in the summary. (A verdict of `CRASH` would mean every attempt
died.)
"""

from __future__ import annotations

import bz2
import os
import pathlib
import shutil
import subprocess
import sys
import zlib

HERE = pathlib.Path(__file__).resolve().parent
SEVENZIP = pathlib.Path("C:/Program Files/7-Zip/7z.exe")
MASK64 = (1 << 64) - 1


class Rng:
    """`xz/tests/data/generate.py`'s generator."""

    def __init__(self, seed: int) -> None:
        self.s = seed & MASK64

    def next(self) -> int:
        self.s = (self.s * 6364136223846793005 + 1442695040888963407) & MASK64
        return self.s >> 33


WORDS = [b"alpha", b"beta", b"gamma", b"delta", b"epsilon", b"zeta", b"eta", b"theta"]


def text(n: int, seed: int) -> bytes:
    r = Rng(seed)
    out = bytearray()
    while len(out) < n:
        out += WORDS[r.next() % len(WORDS)]
        out += b"\n" if r.next() % 9 == 0 else b" "
    return bytes(out[:n])


def random_bytes(n: int, seed: int) -> bytes:
    r = Rng(seed)
    return bytes(r.next() & 0xFF for _ in range(n))


def code(n: int, seed: int) -> bytes:
    """Calls (E8) to a few targets and recurring instruction runs: work for
    the x86 converters."""
    r = Rng(seed)
    snippets = [bytes((i * 37 + j * 11) & 0xFF for j in range(6 + i % 5)) for i in range(12)]
    out = bytearray()
    while len(out) < n:
        k = r.next() % 8
        if k == 0:
            out += bytes([0xE8]) + (0x400 * (r.next() % 9)).to_bytes(4, "little")
        elif k == 1:
            out += bytes([0x90]) * (1 + r.next() % 4)
        else:
            out += snippets[r.next() % len(snippets)]
    return bytes(out[:n])


def riscv_code(n: int, seed: int) -> bytes:
    """RISC-V-like code for the RISC-V converter: AUIPC pairs -- an AUIPC,
    then an instruction that may read the register it set -- with x0 and x2
    for that register as often as all the others together, JALs, 16-bit
    instructions between them to move the alignment, and noise."""
    r = Rng(seed)
    out = bytearray()

    def word(w: int) -> None:
        out.extend((w & 0xFFFF_FFFF).to_bytes(4, "little"))

    while len(out) < n:
        k = r.next() % 8
        if k <= 2:
            rd = (0, 2, r.next() % 32)[k]
            word(((r.next() & 0xFFFFF) << 12) | (rd << 7) | 0x17)  # AUIPC
            rs1 = rd if r.next() % 2 else r.next() % 32
            # ADDI, LW, JALR, LD
            op, f3 = ((0x13, 0), (0x03, 2), (0x67, 0), (0x03, 3))[r.next() % 4]
            imm = r.next() & 0xFFF
            word((imm << 20) | (rs1 << 15) | (f3 << 12) | ((r.next() % 32) << 7) | op)
        elif k == 3:
            word(((r.next() & 0xFFFFF) << 12) | ((r.next() % 32) << 7) | 0x6F)  # JAL
        elif k == 4:
            c = r.next() & 0xFFFF
            if c & 3 == 3:
                c ^= 1
            out.extend(c.to_bytes(2, "little"))  # a 16-bit instruction
        else:
            word(r.next() | (r.next() << 16))
    return bytes(out[:n])


# The tree: (path, bytes), or (path, None) for an empty folder.
TREE = [
    ("docs/readme.txt", text(5_000, 1)),
    ("docs/empty.txt", b""),
    ("docs/sub", None),
    ("data/random.bin", random_bytes(20_000, 2)),
    ("data/code.bin", code(30_000, 3)),
    ("top.txt", text(50_000, 4)),
    ("na\u00efve \u2013 \u00fcn\u00efc\u00f8d\u00e9.txt", text(700, 5)),
]

# (name, 7z arguments): each exercises something.
MADE = [
    ("lzma2-solid", ["-m0=LZMA2", "-mx=5", "-ms=on"]),
    ("lzma2-files", ["-m0=LZMA2", "-mx=5", "-ms=off"]),
    ("lzma", ["-m0=LZMA", "-mx=5"]),
    ("lzma-fast", ["-m0=LZMA", "-mx=1"]),
    ("bzip2", ["-m0=BZip2"]),
    ("deflate", ["-m0=Deflate"]),
    ("deflate64", ["-m0=Deflate64"]),
    ("copy", ["-m0=Copy"]),
    ("ppmd", ["-m0=PPMd"]),
    # 64 KiB of model for about 105 KB of input: the model fills and starts
    # again several times, which is where an allocator that is not exactly
    # 7-Zip's would show.
    ("ppmd-small-mem", ["-m0=PPMd:mem=64k:o=32"]),
    ("bcj-lzma2", ["-mf=BCJ", "-m0=LZMA2"]),
    ("bcj2", ["-mf=BCJ2", "-m0=LZMA"]),
    ("delta", ["-mf=Delta:4", "-m0=LZMA2"]),
    ("arm", ["-mf=ARM", "-m0=LZMA2"]),
    ("arm64", ["-mf=ARM64", "-m0=LZMA2"]),
    ("riscv", ["-mf=RISCV", "-m0=LZMA2"]),
    ("headers-plain", ["-m0=LZMA2", "-mhc=off"]),
    ("times", ["-m0=LZMA2", "-mtc=on", "-mta=on"]),
    ("encrypted", ["-m0=LZMA2", "-psecret"]),
    ("encrypted-headers", ["-m0=LZMA2", "-psecret", "-mhe=on"]),
]

# Mutated whole: small archives with something in every part of the format.
MUTATED = [
    "small-lzma2",
    "small-headers-plain",
    "small-copy",
    "small-ppmd",
    "small-bzip2",
    "small-deflate",
    "small-deflate64",
    "small-bcj-lzma2",
    "small-bcj2",
    "small-arm64",
]
SMALL = {
    "small-lzma2": ["-m0=LZMA2", "-mx=5"],
    "small-headers-plain": ["-m0=LZMA", "-mhc=off"],
    "small-copy": ["-m0=Copy", "-mhc=off"],
    "small-ppmd": ["-m0=PPMd", "-mhc=off"],
    "small-bzip2": ["-m0=BZip2", "-mhc=off"],
    "small-deflate": ["-m0=Deflate", "-mhc=off"],
    "small-deflate64": ["-m0=Deflate64", "-mhc=off"],
    # A filter before a coder: two coders, one bond.
    "small-bcj-lzma2": ["-mf=BCJ", "-m0=LZMA2", "-mhc=off"],
    # Four packed streams, three of them through LZMA.
    "small-bcj2": ["-mf=BCJ2", "-m0=LZMA", "-mhc=off"],
    # A filter 7-Zip added in 23.00, over Copy: every byte it converted is
    # in the archive as it stands.
    "small-arm64": ["-mf=ARM64", "-m0=Copy", "-mhc=off"],
}
# The tree they are made of: text, random bytes, machine-code-like bytes for
# the branch converters, an empty file and an empty folder.
SMALL_TREE = [
    ("a.txt", text(300, 6)),
    ("dir", None),
    ("b.bin", random_bytes(40, 7)),
    ("c.bin", code(240, 8)),
    ("e.txt", b""),
]

# Archives of one file each, made for something the input tree has too little
# of: (archive name, 7z arguments, file name, bytes). Listed in `single.txt`
# with the file's size and hash, which the reader must give back.
SINGLE = [
    # An AUIPC that sets x0 or x2 is rare in real code and next to absent
    # from text, noise and bytes made for x86. 7-Zip's encoder escapes such a
    # pair -- it would read as one the encoder converted -- and only this
    # file's decoding undoes the escape. Over Copy, so every byte the encoder
    # wrote is in the archive as it stands.
    ("riscv-pairs", ["-mf=RISCV", "-m0=Copy"], "pairs.bin", riscv_code(16_384, 31)),
    # LZMA with one property other than lc changed: 7-Zip names each
    # property that differs from the defaults, and lc only when it does.
    ("lzma-pb0", ["-m0=LZMA:pb=0"], "a.txt", text(3_000, 32)),
]


# Mutated at chosen bytes: archives too big to mutate whole, whose LZMA2
# chunk structure is the point. One block of two LZMA chunks under one
# dictionary; and nine blocks of one chunk each, each its own dictionary
# (`c=64k`: a 64 KiB block size), which 7-Zip's threads decode apart.
TARGETED = {
    "chunks-lzma2": ["-m0=LZMA2", "-mx=5"],
    "blocks-lzma2": ["-m0=LZMA2:d=64k:c=64k"],
}
CHUNKY_TREE = [
    ("a.txt", text(240_000, 21)),
    ("b.bin", random_bytes(30_000, 22)),
    ("c.txt", text(240_000, 23)),
]
# Every bit of a chunk header, and FF; 01, 80 and FF elsewhere.
HEADER_XORS = (0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0xFF)
XORS = (0x01, 0x80, 0xFF)


def lzma2_chunks(data: bytes, start: int) -> list[tuple[int, int, int]]:
    """The chunks of the LZMA2 stream at `start`, as (offset, header
    length, data length), the end marker last as (offset, 1, 0)."""
    out = []
    i = start
    while True:
        c = data[i]
        if c == 0:
            out.append((i, 1, 0))
            return out
        unpack = (data[i + 1] << 8 | data[i + 2]) + 1
        if c & 0x80:
            pack = (data[i + 3] << 8 | data[i + 4]) + 1
            header = 6 if c >= 0xC0 else 5
            out.append((i, header, pack))
            i += header + pack
        else:
            out.append((i, 3, unpack))
            i += 3 + unpack


def targeted(data: bytes) -> list[tuple[int, int]]:
    """The (position, xor) pairs a TARGETED archive is mutated at: its one
    folder's LZMA2 stream starts right after the start header."""
    out = []
    for at, header, size in lzma2_chunks(data, 32):
        for pos in range(at, at + header):
            out.extend((pos, x) for x in HEADER_XORS)
        if size:
            first = at + header
            for pos in sorted({first, first + 1, first + size // 2, first + size - 2, first + size - 1}):
                out.extend((pos, x) for x in XORS)
    return out


def read_number(data: bytes, i: int) -> tuple[int, int]:
    """A 7z NUMBER at `i`: the first byte's leading 1-bits count the bytes
    after it, which are the value's low bytes; its other bits, the high
    part. Returns the value and where it ends."""
    first = data[i]
    i += 1
    value = 0
    mask = 0x80
    for k in range(8):
        if first & mask == 0:
            return value | (first & (mask - 1)) << (8 * k), i
        value |= data[i] << (8 * k)
        i += 1
        mask >>= 1
    return value, i


def write_number(v: int) -> bytes:
    """`v` as the shortest 7z NUMBER."""
    for k in range(9):
        if k == 8 or v < 1 << (8 * k + 7 - k):
            high = v >> (8 * k) if k < 8 else 0
            first = (0xFF00 >> k) & 0xFF | high
            return bytes([first]) + (v & ((1 << (8 * k)) - 1)).to_bytes(k, "little")
    raise AssertionError


def with_pack_stream(data: bytes, j: int, new: bytes) -> bytes:
    """The plain-header archive `data` with its packed stream `j` replaced by
    `new`: the header's size for it rewritten, the streams after it moved,
    and the start header's offsets and CRCs made good."""
    next_offset = int.from_bytes(data[12:20], "little")
    next_size = int.from_bytes(data[20:28], "little")
    header = data[32 + next_offset:32 + next_offset + next_size]
    # kHeader, kMainStreamsInfo, kPackInfo: the pack position and count,
    # then kSize and a size a stream.
    assert header[:3] == b"\x01\x04\x06", "a plain header"
    pack_pos, i = read_number(header, 3)
    count, i = read_number(header, i)
    assert header[i] == 0x09
    i += 1
    spans, sizes = [], []
    for _ in range(count):
        start = i
        size, i = read_number(header, i)
        spans.append((start, i))
        sizes.append(size)
    begin = 32 + pack_pos + sum(sizes[:j])
    end = begin + sizes[j]
    s, e = spans[j]
    header = header[:s] + write_number(len(new)) + header[e:]
    packed = data[32:begin] + new + data[end:32 + next_offset]
    start = (
        len(packed).to_bytes(8, "little")
        + len(header).to_bytes(8, "little")
        + zlib.crc32(header).to_bytes(4, "little")
    )
    return data[:8] + zlib.crc32(start).to_bytes(4, "little") + start + packed + header


def with_header_bytes(data: bytes, old: bytes, new: bytes) -> bytes:
    """The plain-header archive `data` with `old`, found once in its header,
    made `new`, and the start header's size and CRCs made good."""
    next_offset = int.from_bytes(data[12:20], "little")
    next_size = int.from_bytes(data[20:28], "little")
    header = data[32 + next_offset:32 + next_offset + next_size]
    assert header.count(old) == 1, f"{old.hex()} is not once in the header"
    header = header.replace(old, new)
    start = (
        next_offset.to_bytes(8, "little")
        + len(header).to_bytes(8, "little")
        + zlib.crc32(header).to_bytes(4, "little")
    )
    return data[:8] + zlib.crc32(start).to_bytes(4, "little") + start + data[32:32 + next_offset] + header


def pack_stream(data: bytes, j: int = 0) -> bytes:
    """Packed stream `j` of the plain-header archive `data`."""
    next_offset = int.from_bytes(data[12:20], "little")
    header = data[32 + next_offset:]
    pack_pos, i = read_number(header, 3)
    count, i = read_number(header, i)
    i += 1
    sizes = []
    for _ in range(count):
        size, i = read_number(header, i)
        sizes.append(size)
    begin = 32 + pack_pos + sum(sizes[:j])
    return data[begin:begin + sizes[j]]


class Bits:
    """Bits least significant first, as Deflate packs them."""

    def __init__(self) -> None:
        self.out = bytearray()
        self.n = 0

    def put(self, v: int, n: int) -> None:
        for k in range(n):
            if self.n % 8 == 0:
                self.out.append(0)
            self.out[-1] |= ((v >> k) & 1) << (self.n % 8)
            self.n += 1

    def code(self, c: int, n: int) -> None:
        """A Huffman code, its first bit the most significant."""
        for k in reversed(range(n)):
            self.put((c >> k) & 1, 1)


LEVEL_ORDER = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15]


def incomplete_deflate(data: bytes) -> bytes:
    """`data` as one final dynamic block whose literal/length code gives every
    byte and the end of block 9 bits and nothing else a code: 257 codes of 9
    bits fill 257/512 of the code space. RFC 1951 does not say such a code
    is wrong; zlib refuses it, 7-Zip need not."""
    w = Bits()
    w.put(1, 1)
    w.put(2, 2)
    w.put(0, 5)  # 257 literal/length lengths
    w.put(0, 5)  # 1 distance length
    w.put(15, 4)  # all 19 code-length lengths
    # The code-length code: 9 in 1 bit ("0"), 0 in 2 ("10"), 16 and 18 in
    # 3 ("110", "111").
    cl = [0] * 19
    cl[9], cl[0], cl[16], cl[18] = 1, 2, 3, 3
    for k in LEVEL_ORDER:
        w.put(cl[k], 3)
    w.code(0, 1)  # a 9
    left = 256
    while left:
        run = min(left, 6)
        w.code(6, 3)  # 16: the last length again, 3 to 6 times
        w.put(run - 3, 2)
        left -= run
    w.code(2, 2)  # the one distance length: 0
    for b in data:
        w.code(b, 9)
    w.code(256, 9)
    return bytes(w.out)


# RFC 1951's fixed literal/length code and its distance symbols.
def fixed_code(w: Bits, sym: int) -> None:
    if sym < 144:
        w.code(0x30 + sym, 8)
    elif sym < 256:
        w.code(0x190 + sym - 144, 9)
    elif sym < 280:
        w.code(sym - 256, 7)
    else:
        w.code(0xC0 + sym - 280, 8)


DIST_BASE = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769,
             1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577]
DIST_EXTRA = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8,
              9, 9, 10, 10, 11, 11, 12, 12, 13, 13]


def deflate_with_code(data: bytes, sym: int) -> bytes:
    """`data` as one final fixed block of literals but one match of length
    3, coded with literal/length code `sym` -- 286 or 287, which RFC 1951
    leaves without a meaning."""
    for i in range(3, len(data) - 2):
        j = data.rfind(data[i:i + 3], 0, i)
        if j >= 0 and j + 3 <= i:
            break
    else:
        raise AssertionError("no three bytes repeat")
    d = i - j
    w = Bits()
    w.put(1, 1)
    w.put(1, 2)
    for b in data[:i]:
        fixed_code(w, b)
    fixed_code(w, sym)
    k = max(k for k in range(30) if DIST_BASE[k] <= d)
    w.code(k, 5)
    w.put(d - DIST_BASE[k], DIST_EXTRA[k])
    for b in data[i + 3:]:
        fixed_code(w, b)
    fixed_code(w, 256)
    return bytes(w.out)


def crafted(made: pathlib.Path) -> list[tuple[str, bytes, str]]:
    """Archives no one-byte mutant makes, from the small plain-header ones:
    (name, archive, what was done to it)."""
    small = {name: (made / f"{name}.7z").read_bytes() for name in MUTATED}
    out = []
    # Two bytes after the stream: data after the end, if the coder says how
    # much it read (BZip2, Deflate, Copy) -- or its own error (LZMA, PPMd).
    for name, label in (("small-deflate", "deflate"), ("small-deflate64", "deflate64"),
                        ("small-bzip2", "bzip2"), ("small-copy", "copy"),
                        ("small-headers-plain", "lzma"), ("small-ppmd", "ppmd")):
        data = small[name]
        tail = with_pack_stream(data, 0, pack_stream(data) + b"\x00\x7f")
        out.append((f"after-end-{label}", tail, f"{name} with 2 bytes after its first packed stream"))
    folder = bz2.decompress(pack_stream(small["small-bzip2"]))
    half = len(folder) // 2
    two = bz2.compress(folder[:half]) + bz2.compress(folder[half:])
    out.append(("two-streams-bzip2", with_pack_stream(small["small-bzip2"], 0, two),
                "small-bzip2 packed as two bzip2 streams, as pbzip2 does"))
    folder = zlib.decompressobj(-15).decompress(pack_stream(small["small-deflate"]))
    out.append(("incomplete-code-deflate",
                with_pack_stream(small["small-deflate"], 0, incomplete_deflate(folder)),
                "small-deflate with a literal/length code of 257 9-bit codes"))
    for sym in (286, 287):
        out.append((f"code-{sym}-deflate",
                    with_pack_stream(small["small-deflate"], 0, deflate_with_code(folder, sym)),
                    f"small-deflate with a match of 3 coded as {sym}"))
    # A coder given a property byte it does not take -- its record's flags
    # gain "has properties" (20) and one byte, 00, follows the method id.
    for name, method in (("small-deflate", "040108"), ("small-bzip2", "040202")):
        mid = bytes.fromhex(method)
        given = with_header_bytes(small[name], b"\x03" + mid, b"\x23" + mid + b"\x01\x00")
        out.append((f"props-{name[6:]}", given, f"{name} with a property byte on its coder"))
    return out


def write_crafted(made: pathlib.Path) -> None:
    """The crafted archives, from the small ones in `made`, and their
    verdicts in `crafted.txt`."""
    lines = ["# archive verdict -- what was done to it; verdicts with several threads"]
    for name, data, how in crafted(made):
        archive = made / f"crafted-{name}.7z"
        archive.write_bytes(data)
        lines.append(f"{archive.name} {verdict(archive)} -- {how}")
    (HERE / "crafted.txt").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


def write_single(made: pathlib.Path) -> None:
    """The SINGLE archives into `made`, and `single.txt`: each archive's one
    file, by size and hash -- and 7-Zip's verdict, which must be OK."""
    lines = ["# archive size fnv64 file -- one file each, as it went in"]
    src = HERE / "single-input"
    for name, args, file, data in SINGLE:
        write_tree(src, [(file, data)])
        archive = make(name, args, src, made)
        v = verdict(archive)
        if v != "OK":
            sys.exit(f"7z t {archive.name}: {v}")
        lines.append(f"{archive.name} {len(data)} {fnv(data):016x} {file}")
    shutil.rmtree(src)
    (HERE / "single.txt").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


def sevenzip(*args: str, cwd: pathlib.Path | None = None) -> subprocess.CompletedProcess:
    return subprocess.run([str(SEVENZIP), *args], capture_output=True, cwd=cwd)


# 7-Zip's own exit codes: OK, warning, fatal error, command line error, out
# of memory, stopped by the user. Anything else is a crash.
SEVENZIP_EXIT_CODES = (0, 1, 2, 7, 8, 255)
CRASH_RETRIES = 40
# Every crash seen, for the summary.
CRASHES: list[str] = []


def kind(text: str) -> str:
    """A 7-Zip message as one token: `Data Error` -> `Data_Error`."""
    return text.strip().replace(" ", "_")


def verdict(archive: pathlib.Path, one_thread: bool = False) -> str:
    """`7z t`'s verdict -- with `-mmt=off` if `one_thread` -- as one line:

    - `OK`: opened, every item tested good, nothing to say;
    - `OPENFAIL kinds`: not opened, with the reasons 7-Zip gave (its `ERRORS:`
      list after `Open ERROR`, e.g. `Is_not_archive`, `Headers_Error`,
      `Unexpected_end_of_archive`);
    - `OPEN errors=kinds warnings=kinds items=path:kind|...`: opened, with
      what it reported about the archive and, item by item, what failed --
      `#N` for a failure belonging to no file (a folder that failed after
      its last file).
    """
    threads = ["-mmt=off"] if one_thread else []
    # `-t7z`: 7-Zip's 7z handler and no other. Without it, an archive whose
    # 7z signature is damaged is tried as every other format 7-Zip knows --
    # and a BZip2 folder inside one opens as a .bz2 at offset 32.
    args = ["t", "-t7z", *threads, "-bso1", "-bse1", "-bsp0", "-sccUTF-8", "-psecret", str(archive)]
    # 7-Zip 26.00 itself sometimes dies -- an access violation, exit
    # 0xC0000005 -- testing a damaged LZMA2 archive of several blocks with
    # several threads: about one run in twelve on the worst mutants here,
    # the others agreeing. A crash is no verdict; run it again.
    for _ in range(CRASH_RETRIES):
        r = sevenzip(*args)
        if r.returncode in SEVENZIP_EXIT_CODES:
            break
        CRASHES.append(f"{archive.name}: exit {r.returncode:#x}")
    else:
        return "CRASH"
    lines = (r.stdout.decode("utf-8", "replace") + r.stderr.decode("utf-8", "replace")).splitlines()
    open_failed = any(line.startswith("Open ERROR") for line in lines)
    errors, warnings, items = [], [], []
    section = None
    for line in lines:
        s = line.strip()
        if s in ("ERRORS:", "WARNINGS:"):
            section = s
            continue
        if not s:
            section = None
            continue
        if section == "ERRORS:":
            errors.append(kind(s))
        elif section == "WARNINGS:":
            warnings.append(kind(s))
        elif s.startswith("ERROR: ") and " : " in s:
            what, item = s[len("ERROR: "):].rsplit(" : ", 1)
            items.append(f"{item.replace(chr(92), '/')}:{kind(what)}")
    if open_failed:
        return "OPENFAIL " + (",".join(sorted(set(errors))) or "-")
    if r.returncode == 0 and not errors and not warnings and not items:
        return "OK"
    return (
        f"OPEN errors={','.join(sorted(set(errors))) or '-'}"
        f" warnings={','.join(sorted(set(warnings))) or '-'}"
        f" items={'|'.join(items) or '-'}"
    )


def fnv(data: bytes) -> int:
    h = 0xCBF29CE484222325
    for b in data:
        h = ((h ^ b) * 0x100000001B3) & MASK64
    return h


# Every input's time, so that 7-Zip, which records them, writes the same
# archive every run: 2026-01-01 00:00:00 UTC.
FIXED_TIME = 1767225600


def write_tree(root: pathlib.Path, tree) -> None:
    if root.exists():
        shutil.rmtree(root)
    for path, data in tree:
        p = root / path
        if data is None:
            p.mkdir(parents=True, exist_ok=True)
        else:
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(data)
    # Folders last, as writing into one changes its time.
    for p in sorted(root.rglob("*"), key=lambda q: len(q.parts), reverse=True):
        os.utime(p, (FIXED_TIME, FIXED_TIME))


def make(name: str, args: list[str], src: pathlib.Path, out_dir: pathlib.Path) -> pathlib.Path:
    archive = out_dir / f"{name}.7z"
    if archive.exists():
        archive.unlink()
    # -mtm- etc. would leave the times out; the default records mtime. The
    # file order is 7-Zip's own (it sorts by extension and name).
    r = sevenzip("a", "-t7z", "-bso0", "-bsp0", *args, str(archive), ".", cwd=src)
    if r.returncode != 0:
        sys.exit(f"7z a {name} failed: {r.stderr.decode('utf-8', 'replace')}")
    return archive


def main() -> None:
    if not SEVENZIP.exists():
        sys.exit(f"{SEVENZIP} not found")
    version = sevenzip("i").stdout.decode("utf-8", "replace")
    if "7-Zip 26.00" not in version:
        sys.exit("expected 7-Zip 26.00")

    src = HERE / "input"
    write_tree(src, TREE)
    listing = ["# D path | F size fnv64 path -- the tree every archive in made/ holds"]
    for path, data in sorted(TREE):
        listing.append(f"D {path}" if data is None else f"F {len(data)} {fnv(data):016x} {path}")
    (HERE / "input.txt").write_text("\n".join(listing) + "\n", encoding="utf-8", newline="\n")

    made = HERE / "made"
    if made.exists():
        shutil.rmtree(made)
    made.mkdir()
    lines = ["# archive verdict -- made by 7-Zip 26.00 with: args"]
    for name, args in MADE:
        archive = make(name, args, src, made)
        lines.append(f"{archive.name} {verdict(archive)} -- {' '.join(args)}")

    small_src = HERE / "small-input"
    write_tree(small_src, SMALL_TREE)
    for name in MUTATED:
        archive = make(name, SMALL[name], small_src, made)
        lines.append(f"{archive.name} {verdict(archive)} -- {' '.join(SMALL[name])}")
    shutil.rmtree(small_src)
    chunky_src = HERE / "chunky-input"
    write_tree(chunky_src, CHUNKY_TREE)
    for name, args in TARGETED.items():
        archive = make(name, args, chunky_src, made)
        lines.append(f"{archive.name} {verdict(archive)} -- {' '.join(args)}")
    shutil.rmtree(chunky_src)
    shutil.rmtree(src)
    (HERE / "made.txt").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    write_crafted(made)
    write_single(made)

    header = "# == archive, then: position xor verdict -- 7z t's"
    out = {False: [header], True: [header + " -mmt=off"]}
    tmp = HERE / "mutant.7z"
    plan = []
    for name in MUTATED:
        data = (made / f"{name}.7z").read_bytes()
        plan.append((name, data, [(pos, x) for pos in range(len(data)) for x in XORS]))
    for name in TARGETED:
        data = (made / f"{name}.7z").read_bytes()
        plan.append((name, data, targeted(data)))
    for name, data, cases in plan:
        for one_thread in (False, True):
            out[one_thread].append(f"== {name}.7z")
        for pos, x in cases:
            bad = bytearray(data)
            bad[pos] ^= x
            tmp.write_bytes(bytes(bad))
            for one_thread in (False, True):
                out[one_thread].append(f"{pos} {x:02x} {verdict(tmp, one_thread)}")
    tmp.unlink()
    (HERE / "mutations.txt").write_text("\n".join(out[False]) + "\n", encoding="utf-8", newline="\n")
    (HERE / "mutations-mmt-off.txt").write_text(
        "\n".join(out[True]) + "\n", encoding="utf-8", newline="\n"
    )
    differ = sum(a != b for a, b in zip(out[False][1:], out[True][1:]))
    print(f"{len(MADE)} made, {len(out[False])} mutation lines, {differ} differ with one thread")
    if CRASHES:
        print(f"7-Zip crashed {len(CRASHES)} time(s), each run again:")
        for c in sorted(set(CRASHES)):
            print(f"  {c} x{CRASHES.count(c)}")


if __name__ == "__main__":
    # `generate.py crafted`: the crafted archives alone, from the small
    # archives already in made/ -- seconds, where everything is an hour.
    if sys.argv[1:] == ["crafted"]:
        write_crafted(HERE / "made")
    # `generate.py single`: the one-file archives alone, likewise.
    elif sys.argv[1:] == ["single"]:
        if "7-Zip 26.00" not in sevenzip("i").stdout.decode("utf-8", "replace"):
            sys.exit("expected 7-Zip 26.00")
        write_single(HERE / "made")
    else:
        main()
