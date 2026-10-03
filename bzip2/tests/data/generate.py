#!/usr/bin/env python3
"""Writes the bzip2 crate's fixtures, every byte of them from libbzip2 1.0.8.

The oracle is Python's `bz2` module, which links libbzip2 1.0.8 (13 July 2019)
-- the release `src/` is ported from; the version string is checked below,
not assumed. Run from anywhere:

    python bzip2/tests/data/generate.py [path/to/bzip2-1.0.8]

The optional path is the bzip2 source tree (default: the copy the
`bzip2-sys` 0.1.13 crate vendors, in the cargo registry), used for its three
sample files and its randomisation table.

What it writes, beside itself:

- `<name>.l<level>.bz2`: libbzip2's compression of a generated input, for
  every row of CASES. `cases.txt` lists them with the generator that makes
  the input (the Rust tests regenerate it -- the generators below are
  mirrored in `tests/libbzip2.rs`) and the input's length and FNV-1a hash.
  The tests require `bzip2::compress` to reproduce each file exactly.
- `sample1.bz2`, `sample3.bz2`: from the bzip2 distribution, with the
  length and hash of what they decompress to (checked here against the
  distribution's own `.ref` files).
- `randomised.l4.bz2`: one *randomised* block, which nothing has written
  since bzip2 0.9.5 and libbzip2 still reads. Built here by a small encoder
  of its own, long enough that the randomisation table is used to the end
  and wraps -- and accepted by libbzip2 before it is written, so the file is
  valid whatever the encoder below gets wrong.
- `mutations.txt`: every byte of two small streams changed three ways, and
  what `bzip2 -d` makes of each: an error, or a length and hash.
"""

from __future__ import annotations

import bz2
import heapq
import pathlib
import re
import sys

HERE = pathlib.Path(__file__).resolve().parent
DEFAULT_SRC = pathlib.Path.home() / (
    ".cargo/registry/src/index.crates.io-1949cf8c6b5b557f/"
    "bzip2-sys-0.1.13+1.0.8/bzip2-1.0.8"
)
MASK64 = (1 << 64) - 1


# --- the input generators (mirrored in tests/libbzip2.rs) -----------------

class Rng:
    """A 64-bit LCG (Knuth's MMIX constants), top bits out."""

    def __init__(self, seed: int) -> None:
        self.s = seed & MASK64

    def next(self) -> int:
        self.s = (self.s * 6364136223846793005 + 1442695040888963407) & MASK64
        return self.s >> 33


VOCAB = [
    b"the", b"of", b"and", b"a", b"to", b"in", b"is", b"you", b"that", b"it",
    b"he", b"was", b"for", b"on", b"are", b"as", b"with", b"his", b"they",
    b"I", b"at", b"be", b"this", b"have", b"from", b"or", b"one", b"had",
    b"by", b"word", b"but", b"not", b"what", b"all", b"were", b"we", b"when",
    b"your", b"can", b"said", b"there", b"use", b"an", b"each", b"which",
    b"she", b"do", b"how",
]


def gen(kind: str, args: list[int]) -> bytes:
    if kind == "fill":
        n, byte = args
        return bytes([byte]) * n
    if kind == "text":
        n, seed = args
        r = Rng(seed)
        out = bytearray()
        while len(out) < n:
            out += VOCAB[r.next() % len(VOCAB)]
            out += b"\n" if r.next() % 12 == 0 else b" "
        return bytes(out[:n])
    if kind == "random":
        n, seed = args
        r = Rng(seed)
        return bytes(r.next() & 0xFF for _ in range(n))
    if kind == "runs":
        n, seed = args
        r = Rng(seed)
        out = bytearray()
        while len(out) < n:
            b = r.next() & 0xFF
            out += bytes([b]) * (1 + r.next() % 300)
        return bytes(out[:n])
    if kind == "periodic":
        n, period, seed = args
        pat = gen("random", [period, seed])
        return (pat * (n // period + 1))[:n]
    if kind == "aaaab":
        (n,) = args
        return (b"AAAAB" * (n // 5 + 1))[:n]
    if kind == "count":
        (n,) = args
        return bytes(i & 0xFF for i in range(n))
    if kind == "ab":
        n, seed = args
        r = Rng(seed)
        return bytes(b"ab"[r.next() & 1] for _ in range(n))
    raise ValueError(kind)


def fnv(data: bytes) -> int:
    h = 0xCBF29CE484222325
    for b in data:
        h = ((h ^ b) * 0x100000001B3) & MASK64
    return h


# (name, level, generator, args). Each row exercises something named.
CASES = [
    ("empty", 9, "fill", [0, 0]),          # header and trailer, no block
    ("empty", 1, "fill", [0, 0]),
    ("one", 9, "fill", [1, 0x58]),         # a block of one byte
    ("run4", 9, "fill", [4, 0x41]),        # four equal bytes and a count of 0
    ("run5", 9, "fill", [5, 0x41]),
    ("run255", 9, "fill", [255, 0x41]),    # the longest run
    ("run256", 9, "fill", [256, 0x41]),    # ...and one past it
    ("run260", 9, "fill", [260, 0x41]),
    ("short", 9, "text", [3_000, 1]),      # a block under 10 000: fallback sort
    ("text", 1, "text", [160_000, 2]),     # two blocks: the main sort
    ("text", 9, "text", [160_000, 2]),     # one block
    ("random", 9, "random", [20_000, 3]),  # all 256 values, six tables
    ("random", 2, "random", [3_000, 4]),
    ("runs", 1, "runs", [150_000, 5]),     # runs crossing block boundaries
    ("periodic", 9, "periodic", [60_000, 10, 6]),  # main sort gives up
    ("periodic", 5, "periodic", [7_000, 7, 7]),     # fallback only
    ("zeros", 9, "fill", [1_000_000, 0]),
    ("aaaab", 1, "aaaab", [400_000]),      # run-length coding grows the input
    ("count", 9, "count", [100_000]),      # period 256
    ("ab", 4, "ab", [60_000, 8]),          # two symbols, long zero runs
]

# Streams to mutate: (name in CASES, level).
MUTATED = [("text", 9), ("runs", 1)]
MUTATION_INPUTS = {
    ("text", 9): ("text", [3_000, 1]),
    ("runs", 1): ("runs", [2_000, 9]),
}


# --- bzip2(1)'s verdict on a whole file ---------------------------------

def could_begin_stream(data: bytes) -> bool:
    checks = [
        lambda c: c == 0x42,
        lambda c: c == 0x5A,
        lambda c: c == 0x68,
        lambda c: 0x31 <= c <= 0x39,
    ]
    return all(ok(c) for c, ok in zip(data[:4], checks))


def cli_verdict(data: bytes) -> bytes | None:
    """What `bzip2 -d` makes of `data` (`uncompressStream` in bzip2.c):
    the output, or None for an error. After the first stream, bytes that
    cannot begin a stream are ignored; bytes that can must be one."""
    out = bytearray()
    rest = data
    first = True
    while True:
        if not first:
            if not rest:
                break
            if not could_begin_stream(rest):
                break
        d = bz2.BZ2Decompressor()
        try:
            out += d.decompress(rest)
        except OSError:
            return None
        if not d.eof:
            return None
        rest = d.unused_data
        first = False
    return bytes(out)


# --- a randomised block, built by hand ----------------------------------

def crc32_bzip2(data: bytes) -> int:
    table = []
    for i in range(256):
        c = i << 24
        for _ in range(8):
            c = ((c << 1) ^ 0x04C11DB7) & 0xFFFFFFFF if c & 0x80000000 else (c << 1) & 0xFFFFFFFF
        table.append(c)
    crc = 0xFFFFFFFF
    for b in data:
        crc = ((crc << 8) & 0xFFFFFFFF) ^ table[(crc >> 24) ^ b]
    return crc ^ 0xFFFFFFFF


def rle1(data: bytes) -> bytes:
    """libbzip2's initial run-length coding: runs of 4 to 255 become four
    copies and a count of the rest."""
    out = bytearray()
    i = 0
    while i < len(data):
        j = i
        while j < len(data) and data[j] == data[i] and j - i < 255:
            j += 1
        run = j - i
        if run < 4:
            out += data[i:j]
        else:
            out += data[i:i + 4] + bytes([run - 4])
        i = j
    return bytes(out)


def randomise(block: bytes, rnums: list[int]) -> bytes:
    """XOR the low bit of the byte ending each run of `rnums`, as bzip2
    0.9.0 did and `BZ_RAND_UPD_MASK`/`BZ_RAND_MASK` undo."""
    out = bytearray(block)
    n_to_go, t_pos = 0, 0
    for i in range(len(out)):
        if n_to_go == 0:
            n_to_go = rnums[t_pos]
            t_pos = (t_pos + 1) % 512
        n_to_go -= 1
        if n_to_go == 1:
            out[i] ^= 1
    return bytes(out)


class Bits:
    def __init__(self) -> None:
        self.out = bytearray()
        self.acc = 0
        self.n = 0

    def put(self, n: int, v: int) -> None:
        for k in range(n - 1, -1, -1):
            self.acc = (self.acc << 1) | ((v >> k) & 1)
            self.n += 1
            if self.n == 8:
                self.out.append(self.acc)
                self.acc, self.n = 0, 0

    def finish(self) -> bytes:
        if self.n:
            self.out.append(self.acc << (8 - self.n))
        return bytes(self.out)


def huffman_lengths(freq: list[int]) -> list[int]:
    heap = [(max(f, 1), i, (i,)) for i, f in enumerate(freq)]
    heapq.heapify(heap)
    depth = [0] * len(freq)
    tie = len(freq)
    while len(heap) > 1:
        w1, _, s1 = heapq.heappop(heap)
        w2, _, s2 = heapq.heappop(heap)
        for s in s1 + s2:
            depth[s] += 1
        heapq.heappush(heap, (w1 + w2, tie, s1 + s2))
        tie += 1
    assert max(depth) <= 20, "code too long for bzip2"
    return depth


def randomised_stream(data: bytes, level: int, rnums: list[int]) -> bytes:
    block = rle1(data)
    n = len(block)
    assert n <= 100_000 * level
    rblock = randomise(block, rnums)
    key = 96
    ext = rblock + rblock[:key]
    order = sorted(range(n), key=lambda i: ext[i:i + key])
    for a, b in zip(order, order[1:]):
        assert ext[a:a + key] < ext[b:b + key], "ambiguous sort: lengthen the key"
    orig_ptr = order.index(0)
    last = [rblock[(i - 1) % n] for i in order]

    in_use = sorted(set(rblock))
    seq = {b: i for i, b in enumerate(in_use)}
    yy = list(range(len(in_use)))
    syms: list[int] = []
    z = 0

    def flush(z: int) -> None:
        if z == 0:
            return
        z -= 1
        while True:
            syms.append(1 if z & 1 else 0)
            if z < 2:
                break
            z = (z - 2) // 2

    for c in last:
        s = seq[c]
        if yy[0] == s:
            z += 1
        else:
            flush(z)
            z = 0
            j = yy.index(s)
            yy.pop(j)
            yy.insert(0, s)
            syms.append(j + 1)
    flush(z)
    eob = len(in_use) + 1
    syms.append(eob)
    alpha = len(in_use) + 2

    freq = [0] * alpha
    for s in syms:
        freq[s] += 1
    lengths = huffman_lengths(freq)
    codes = [0] * alpha
    code = 0
    for ln in range(1, 21):
        for s in range(alpha):
            if lengths[s] == ln:
                codes[s] = code
                code += 1
        code <<= 1

    w = Bits()
    for c in b"BZh" + bytes([0x30 + level]):
        w.put(8, c)
    for c in bytes.fromhex("314159265359"):
        w.put(8, c)
    crc = crc32_bzip2(data)
    w.put(32, crc)
    w.put(1, 1)  # randomised
    w.put(24, orig_ptr)
    used = set(in_use)
    used16 = [any(16 * i + j in used for j in range(16)) for i in range(16)]
    for u in used16:
        w.put(1, int(u))
    for i in range(16):
        if used16[i]:
            for j in range(16):
                w.put(1, int(16 * i + j in used))
    n_sel = (len(syms) + 49) // 50
    w.put(3, 2)
    w.put(15, n_sel)
    for _ in range(n_sel):
        w.put(1, 0)  # every group uses table 0
    for _ in range(2):
        curr = lengths[0]
        w.put(5, curr)
        for ln in lengths:
            while curr < ln:
                w.put(2, 2)
                curr += 1
            while curr > ln:
                w.put(2, 3)
                curr -= 1
            w.put(1, 0)
    for s in syms:
        w.put(lengths[s], codes[s])
    for c in bytes.fromhex("177245385090"):
        w.put(8, c)
    w.put(32, crc)  # one block: the combined CRC is the block's
    return w.finish()


# --- main ------------------------------------------------------------------

def main() -> None:
    src = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_SRC
    import _bz2

    pyd = pathlib.Path(_bz2.__file__).read_bytes()
    assert b"1.0.8, 13-Jul-2019" in pyd, "the oracle must be libbzip2 1.0.8"

    for old in HERE.glob("*.bz2"):
        old.unlink()

    names = [f"{n}.l{l}" for n, l, _, _ in CASES]
    assert len(set(names)) == len(names), "two cases write one file"
    lines = ["# name level generator args... input-length input-fnv64"]
    for name, level, kind, args in CASES:
        data = gen(kind, args)
        packed = bz2.compress(data, level)
        (HERE / f"{name}.l{level}.bz2").write_bytes(packed)
        assert cli_verdict(packed) == data
        lines.append(
            " ".join([name, str(level), kind, *map(str, args), str(len(data)), f"{fnv(data):016x}"])
        )

    rnums_src = (src / "randtable.c").read_text(encoding="utf-8")
    body = rnums_src[rnums_src.index("{") + 1: rnums_src.index("}")]
    rnums = [int(x) for x in re.findall(r"\d+", body)]
    assert len(rnums) == 512
    data = gen("ab", [330_000, 12])
    assert len(rle1(data)) > sum(rnums), "the block must use the whole table"
    stream = randomised_stream(data, 4, rnums)
    assert bz2.decompress(stream) == data, "libbzip2 refuses the randomised block"
    (HERE / "randomised.l4.bz2").write_bytes(stream)
    lines.append(f"randomised 4 ab 330000 12 {len(data)} {fnv(data):016x}")

    for i in (1, 3):
        packed = (src / f"sample{i}.bz2").read_bytes()
        ref = (src / f"sample{i}.ref").read_bytes()
        assert bz2.decompress(packed) == ref
        (HERE / f"sample{i}.bz2").write_bytes(packed)
        lines.append(f"sample{i} {i} sample - {len(ref)} {fnv(ref):016x}")

    (HERE / "cases.txt").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")

    out = ["# file position xor verdict [length fnv64]"]
    for name, level in MUTATED:
        kind, args = MUTATION_INPUTS[(name, level)]
        good = bz2.compress(gen(kind, args), level)
        fname = f"mut-{name}.l{level}.bz2"
        (HERE / fname).write_bytes(good)
        for pos in range(len(good)):
            for delta in (0x01, 0x80, 0xFF):
                bad = bytearray(good)
                bad[pos] ^= delta
                v = cli_verdict(bytes(bad))
                if v is None:
                    out.append(f"{fname} {pos} {delta:02x} ERR")
                else:
                    out.append(f"{fname} {pos} {delta:02x} OK {len(v)} {fnv(v):016x}")
        lines_written = len(good)
        print(f"{fname}: {lines_written} bytes mutated")
    (HERE / "mutations.txt").write_text("\n".join(out) + "\n", encoding="utf-8", newline="\n")

    total = sum(p.stat().st_size for p in HERE.iterdir() if p.is_file())
    print(f"wrote {len(CASES)} cases, {len(out) - 1} mutations; {total} bytes in {HERE}")


if __name__ == "__main__":
    main()
