"""Regenerate the fixed-buffer inflate corpus: vectors.bin and expected.txt.

`deflate::zlib_inflate_into` must stop where zlib 1.3's `inflate()` stops, as
libtiff 4.7.1's PixarLogDecode drives it; `deflate::zlib_decompress_into` where
libdeflate 1.24's `libdeflate_zlib_decompress` (no size out-parameter) does, as
libtiff's tif_zip.c calls it. This writes streams at the places those two
disagree with each other and with a plain decoder -- hand-built edge cases,
mutated zlib output, and dynamic blocks with deliberately long codewords -- and
records what the two libraries themselves answer, built here from pinned,
hash-checked sources. deflate/tests/fixed_buffer.rs replays the answers.

    python deflate/tests/data/fixed_buffer/generate.py

On Windows the oracle is built and run in WSL (like
gui/imagecodec/tests/data/generate_tiff.py); elsewhere, natively. Needs gcc,
cmake and make there.

Output, per vector, two lines:
  Z FULL <n> <hash> | Z ENDED <n> <hash> | Z ERR
  D COMPLETE <n> <hash> | D FULL <hash 0x00-filled> <hash 0xFF-filled> | D SHORT | D BAD
<hash> is FNV-1a 64 of the bytes named. libdeflate's FULL hashes the whole
buffer twice, pre-filled two ways, because its fastloop copies a match a word
at a time and leaves up to 39 bytes of overrun that can depend on what the
buffer held before -- and libtiff shows that buffer as it is.
"""
import hashlib
import heapq
import os
import pathlib
import random
import struct
import subprocess
import sys
import zlib

HERE = pathlib.Path(__file__).resolve().parent
SEED = 20260926

SOURCES = {
    "zlib-1.3.tar.gz": (
        "https://zlib.net/fossils/zlib-1.3.tar.gz",
        "ff0ba4c292013dbc27530b3a81e1f9a813cd39de01ca5e0f8bf355702efa593e",
    ),
    "libdeflate-1.24.tar.gz": (
        "https://github.com/ebiggers/libdeflate/archive/refs/tags/v1.24.tar.gz",
        "ad8d3723d0065c4723ab738be9723f2ff1cb0f1571e8bfcf0301ff9661f475e8",
    ),
}
ORACLE_TAG = "zlib13-deflate124-1"

ORACLE_C = r"""
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "zlib.h"
#include "libdeflate.h"

static uint64_t fnv(const unsigned char *p, size_t n)
{
    uint64_t h = 1469598103934665603ULL;
    for (size_t i = 0; i < n; i++) {
        h ^= p[i];
        h *= 1099511628211ULL;
    }
    return h;
}

static int rd32(FILE *f, uint32_t *v)
{
    unsigned char b[4];
    if (fread(b, 1, 4, f) != 4)
        return 0;
    *v = (uint32_t)b[0] | (uint32_t)b[1] << 8 | (uint32_t)b[2] << 16 | (uint32_t)b[3] << 24;
    return 1;
}

int main(int argc, char **argv)
{
    FILE *f = fopen(argv[1], "rb");
    if (!f)
        return 2;
    printf("# zlib %s, libdeflate %s\n", zlibVersion(), LIBDEFLATE_VERSION_STRING);
    struct libdeflate_decompressor *d = libdeflate_alloc_decompressor();
    uint32_t out_len, in_len;
    while (rd32(f, &out_len) && rd32(f, &in_len)) {
        unsigned char *in = malloc(in_len ? in_len : 1);
        if (in_len && fread(in, 1, in_len, f) != in_len)
            return 2;
        unsigned char *out = malloc(out_len ? out_len : 1);
        unsigned char *out2 = malloc(out_len ? out_len : 1);

        /* zlib, exactly as PixarLogDecode drives it */
        z_stream s;
        memset(&s, 0, sizeof s);
        if (inflateInit(&s) != Z_OK)
            return 2;
        s.next_in = in;
        s.avail_in = in_len;
        s.next_out = out;
        s.avail_out = out_len;
        int state;
        do {
            state = inflate(&s, Z_PARTIAL_FLUSH);
            if (state == Z_STREAM_END || state != Z_OK)
                break;
        } while (s.avail_out > 0);
        size_t n = out_len - s.avail_out;
        if (state == Z_STREAM_END)
            printf("Z ENDED %zu %016llx\n", n, (unsigned long long)fnv(out, n));
        else if (state != Z_OK)
            printf("Z ERR\n");
        else
            printf("Z FULL %zu %016llx\n", n, (unsigned long long)fnv(out, n));
        inflateEnd(&s);

        /* libdeflate, exactly as tif_zip.c calls it */
        memset(out, 0x00, out_len);
        memset(out2, 0xFF, out_len);
        enum libdeflate_result r1 = libdeflate_zlib_decompress(d, in, in_len, out, out_len, NULL);
        enum libdeflate_result r2 = libdeflate_zlib_decompress(d, in, in_len, out2, out_len, NULL);
        if (r1 != r2)
            return 3;
        if (r1 == LIBDEFLATE_SUCCESS)
            printf("D COMPLETE %u %016llx\n", out_len, (unsigned long long)fnv(out, out_len));
        else if (r1 == LIBDEFLATE_INSUFFICIENT_SPACE)
            printf("D FULL %016llx %016llx\n", (unsigned long long)fnv(out, out_len),
                   (unsigned long long)fnv(out2, out_len));
        else if (r1 == LIBDEFLATE_SHORT_OUTPUT)
            printf("D SHORT\n");
        else
            printf("D BAD\n");
        free(in);
        free(out);
        free(out2);
    }
    libdeflate_free_decompressor(d);
    return 0;
}
"""

BUILD_SH = r"""
set -euo pipefail
SRC="$1"
ROOT="$2"
mkdir -p "$ROOT"
cd "$ROOT"
for t in zlib-1.3 libdeflate-1.24; do
  rm -rf "$t"
  tar xzf "$SRC/$t.tar.gz"
done
(cd zlib-1.3 && ./configure --static > /dev/null && make -j2 libz.a > /dev/null)
cmake -S libdeflate-1.24 -B build-deflate -DCMAKE_BUILD_TYPE=Release \
  -DLIBDEFLATE_BUILD_SHARED_LIB=OFF -DLIBDEFLATE_BUILD_GZIP=OFF > /dev/null
cmake --build build-deflate -j 2 > /dev/null
cp "$SRC/oracle.c" .
gcc -O2 -Izlib-1.3 -Ilibdeflate-1.24 -o oracle oracle.c \
  build-deflate/libdeflate.a zlib-1.3/libz.a
"""


def on_windows():
    return sys.platform == "win32"


def to_wsl(path):
    p = str(pathlib.Path(path).resolve()).replace("\\", "/")
    return f"/mnt/{p[0].lower()}{p[2:]}"


def run(args, **kw):
    if on_windows():
        return subprocess.run(["wsl", "-e"] + args, check=True, capture_output=True, **kw)
    return subprocess.run(args, check=True, capture_output=True, **kw)


def fetch(url, sha256):
    import urllib.request

    with urllib.request.urlopen(url) as response:
        data = response.read()
    got = hashlib.sha256(data).hexdigest()
    if got != sha256:
        sys.exit(f"{url}: SHA-256 {got}, expected {sha256}")
    return data


def build_oracle():
    home = run(["sh", "-c", "echo $HOME"]).stdout.decode().strip()
    root = f"{home}/.cache/slateos-deflate-oracle-{ORACLE_TAG}"
    exe = f"{root}/oracle"
    if run(["sh", "-c", f"test -x '{exe}' && echo yes || echo no"]).stdout.decode().strip() == "yes":
        return exe
    staging = pathlib.Path(os.environ.get("TEMP", "/tmp")) / f"slateos-deflate-oracle-src-{ORACLE_TAG}"
    staging.mkdir(parents=True, exist_ok=True)
    for name, (url, sha256) in SOURCES.items():
        target = staging / name
        if not target.exists() or hashlib.sha256(target.read_bytes()).hexdigest() != sha256:
            target.write_bytes(fetch(url, sha256))
    (staging / "oracle.c").write_text(ORACLE_C, newline="\n")
    (staging / "build.sh").write_text(BUILD_SH, newline="\n")
    src = to_wsl(staging) if on_windows() else str(staging)
    run(["bash", f"{src}/build.sh", src, root])
    return exe


# ---------------------------------------------------------------------------
# Streams
# ---------------------------------------------------------------------------

class BW:
    """DEFLATE bit writer (fields LSB-first; Huffman codes MSB-first)."""

    def __init__(self):
        self.bits = []

    def put(self, v, n):
        for i in range(n):
            self.bits.append((v >> i) & 1)

    def code(self, c, n):
        for i in range(n - 1, -1, -1):
            self.bits.append((c >> i) & 1)

    def data(self):
        b = list(self.bits)
        while len(b) % 8:
            b.append(0)
        return bytes(sum(b[i + j] << j for j in range(8)) for i in range(0, len(b), 8))


def fixed_lit(bw, sym):
    if sym < 144:
        bw.code(0x30 + sym, 8)
    elif sym < 256:
        bw.code(0x190 + sym - 144, 9)
    elif sym < 280:
        bw.code(sym - 256, 7)
    else:
        bw.code(0xC0 + sym - 280, 8)


def canonical(lengths):
    maxl = max(lengths) if lengths else 0
    count = [0] * (maxl + 2)
    for length in lengths:
        if length:
            count[length] += 1
    code, nxt = 0, [0] * (maxl + 2)
    for bits in range(1, maxl + 1):
        code = (code + count[bits - 1]) << 1
        nxt[bits] = code
    out = {}
    for s, length in enumerate(lengths):
        if length:
            out[s] = (nxt[length], length)
            nxt[length] += 1
    return out


ORDER = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15]
# A complete precode: 13 symbols of length 4 and 6 of length 5.
PRECODE = [4] * 13 + [5] * 6


def dynamic_header(bw, final, lit, dist, lens_syms=None):
    """Header for a dynamic block. `lens_syms`, if given, is the precode symbol
    stream [(sym, extra)] -- to write repeats or malformed runs."""
    if lens_syms is None:
        lens_syms = [(length, 0) for length in lit + dist]
    bw.put(final, 1)
    bw.put(2, 2)
    bw.put(len(lit) - 257, 5)
    bw.put(len(dist) - 1, 5)
    bw.put(19 - 4, 4)
    for i in range(19):
        bw.put(PRECODE[ORDER[i]], 3)
    pc = canonical(PRECODE)
    for s, extra in lens_syms:
        bw.code(*pc[s])
        bw.put(extra, {16: 2, 17: 3, 18: 7}.get(s, 0))
    return canonical(lit), canonical(dist)


def zwrap(deflate, content=b"", adler=None, header=b"\x78\x9c"):
    a = zlib.adler32(content) if adler is None else adler
    return header + deflate + struct.pack(">I", a & 0xFFFFFFFF)


def complete_litlen(nsyms=286, without=None):
    """A complete litlen code over `nsyms` symbols (lengths 8 and 9)."""
    syms = [s for s in range(nsyms) if s != without]
    lit = [0] * nsyms
    nine = 2 * len(syms) - 512  # n8 * 2 + n9 = 512, n8 + n9 = len
    for i, s in enumerate(syms):
        lit[s] = 9 if i >= len(syms) - nine else 8
    return lit


def crafted():
    out = []

    def add(desc, deflate, content=b"", sizes=None, **kw):
        stream = zwrap(deflate, content, **kw)
        for size in sizes if sizes is not None else [len(content), len(content) + 1, max(0, len(content) - 1)]:
            out.append((f"{desc}:out={size}", size, stream))
            for cut in (1, 2, 3, 5):
                if len(stream) > cut:
                    out.append((f"{desc}:out={size}:cut{cut}", size, stream[:-cut]))

    for sym in (285, 286, 287):
        bw = BW()
        bw.put(1, 1)
        bw.put(1, 2)
        for c in b"ab":
            fixed_lit(bw, c)
        fixed_lit(bw, sym)
        bw.code(0, 5)
        fixed_lit(bw, 256)
        add(f"fixed-litlen-{sym}", bw.data(), b"ab" + b"b" * 258, [2, 259, 260, 261, 100])
    for dsym in (29, 30, 31):
        bw = BW()
        bw.put(1, 1)
        bw.put(1, 2)
        fixed_lit(bw, 97)
        written = 1
        while written < 25000:
            fixed_lit(bw, 285)
            bw.code(0, 5)
            written += 258
        fixed_lit(bw, 257)
        bw.code(dsym, 5)
        bw.put(0, 13)
        fixed_lit(bw, 256)
        add(f"fixed-dist-{dsym}", bw.data(), b"a" * (written + 3), [written + 3, written, written + 10])
    for nlit, ndist in ((286, 30), (287, 30), (288, 30), (286, 31), (286, 32), (288, 32)):
        lit = complete_litlen(286) + [0] * (nlit - 286)
        dist = [4, 4] + [5] * 28 + [0] * (ndist - 30)
        bw = BW()
        lc, _ = dynamic_header(bw, 1, lit, dist)
        for c in b"hi":
            bw.code(*lc[c])
        bw.code(*lc[256])
        add(f"dyn-hlit{nlit}-hdist{ndist}", bw.data(), b"hi")
    for use_match in (False, True):
        bw = BW()
        lc, _ = dynamic_header(bw, 1, complete_litlen(286), [0])
        for c in b"xy":
            bw.code(*lc[c])
        content = b"xy"
        if use_match:
            bw.code(*lc[257])
            bw.put(0, 1)
            content += b"yyy"
        bw.code(*lc[256])
        add(f"dyn-empty-dist-match{int(use_match)}", bw.data(), content)
    for bit in (0, 1):
        bw = BW()
        lc, _ = dynamic_header(bw, 1, complete_litlen(286), [1])
        bw.code(*lc[ord("z")])
        bw.code(*lc[257])
        bw.put(bit, 1)
        bw.code(*lc[256])
        add(f"dyn-single-dist-bit{bit}", bw.data(), b"zzzz", [4, 5, 3, 1])
    for bit in (0, 1):
        lit = [0] * 257
        lit[256] = 1
        bw = BW()
        dynamic_header(bw, 1, lit, [0])
        bw.put(bit, 1)
        bw.put(0, 1)
        add(f"dyn-eob-only-bit{bit}", bw.data(), b"", [0, 1, 5])
    bw = BW()
    lc, _ = dynamic_header(bw, 1, complete_litlen(286, without=256), [1, 1])
    for c in b"no eob":
        bw.code(*lc[c])
    add("dyn-missing-eob", bw.data(), b"no eob", [6, 3, 7, 100])
    # An incomplete precode: one code of length 1.
    bw = BW()
    bw.put(1, 1)
    bw.put(2, 2)
    bw.put(0, 5)
    bw.put(0, 5)
    bw.put(15, 4)
    for i in range(19):
        bw.put(1 if ORDER[i] == 8 else 0, 3)
    bw.put(0, 300)
    add("dyn-incomplete-precode", bw.data(), b"", [0, 10, 300])
    # An empty precode.
    rng = random.Random(1)
    bw = BW()
    bw.put(1, 1)
    bw.put(2, 2)
    bw.put(0, 5)
    bw.put(0, 5)
    bw.put(0, 4)
    bw.put(0, 12)
    for _ in range(400):
        bw.put(rng.getrandbits(1), 1)
    add("dyn-empty-precode", bw.data(), b"", [0, 10, 100, 1000])
    lit = complete_litlen(286)
    dist = [4, 4] + [5] * 28
    bw = BW()
    dynamic_header(bw, 1, lit, dist, [(16, 0)] + [(length, 0) for length in lit + dist])
    add("dyn-repeat16-first", bw.data(), b"", [0, 10])
    bw = BW()
    dynamic_header(bw, 1, lit, dist, [(length, 0) for length in (lit + dist)[:-3]] + [(18, 10)])
    add("dyn-repeat-overrun", bw.data(), b"", [0, 10])
    payload = bytes(random.Random(2).getrandbits(8) for _ in range(100))
    stored = b"\x01" + struct.pack("<HH", 100, 0xFFFF ^ 100) + payload
    add("stored-100", stored, payload, [0, 50, 99, 100, 101, 200])
    add("stored-nlen-bad", b"\x01" + struct.pack("<HH", 100, 0x1234) + payload, payload, [100, 50])
    two = (b"\x00" + struct.pack("<HH", 60, 0xFFFF ^ 60) + payload[:60]
           + b"\x01" + struct.pack("<HH", 40, 0xFFFF ^ 40) + payload[60:])
    add("stored-two-blocks", two, payload, [100, 60, 59, 61, 30])
    bw = BW()
    bw.put(1, 1)
    bw.put(3, 2)
    add("btype3", bw.data() + b"\x00" * 8, b"", [0, 5])
    body = b"hello hello hello hello"
    comp = zlib.compress(body)[2:-4]
    for hdr in (b"\x78\x9c", b"\x78\x01", b"\x78\xda", b"\x88\x98", b"\x08\x1d", b"\x79\x9c", b"\x78\x9d"):
        add(f"header-{hdr.hex()}", comp, body, [len(body)], header=hdr)
    add("fdict", b"\x00\x00\x00\x01" + comp, body, [len(body), 0], header=b"\x78\xbb")
    for tiny in (b"", b"\x78", b"\x78\x9c", b"\x78\x9c\x03", b"\x78\x9c\x03\x00",
                 b"\x78\x9c\x03\x00\x00\x00", b"\x78\x9c\x03\x00\x00\x00\x00\x01"):
        for size in (0, 1):
            out.append((f"tiny-{tiny.hex()}:out={size}", size, tiny))
    return out


def sample_data(rng):
    kind = rng.randrange(7)
    n = rng.choice([0, 1, 2, 3, 10, 50, 200, 1000, 2500, rng.randrange(1, 2500)])
    if kind == 0:
        return bytes(rng.getrandbits(8) for _ in range(n))
    if kind == 1:
        words = [b"the", b"quick", b"brown", b"fox", b"jumps", b"over", b"lazy", b"dog", b"\n"]
        s = b""
        while len(s) < n:
            s += rng.choice(words) + b" "
        return s[:n]
    if kind == 2:
        return bytes([rng.getrandbits(8)]) * n
    if kind == 3:
        pat = bytes(rng.getrandbits(8) for _ in range(rng.randrange(1, 20)))
        return (pat * (n // max(1, len(pat)) + 1))[:n]
    if kind == 4:
        return bytes(rng.randrange(4) for _ in range(n))
    if kind == 5:
        v, s = rng.randrange(65536), bytearray()
        for _ in range(n // 2):
            v = (v + rng.randrange(-40, 41)) % 65536
            s += struct.pack("<H", v)
        return bytes(s)
    return bytes(rng.getrandbits(8) & rng.getrandbits(8) for _ in range(n))


def compress(rng, data):
    level = rng.randrange(-1, 10)
    strategy = rng.choice([zlib.Z_DEFAULT_STRATEGY, zlib.Z_FILTERED, zlib.Z_HUFFMAN_ONLY, zlib.Z_RLE, zlib.Z_FIXED])
    c = zlib.compressobj(level, zlib.DEFLATED, rng.choice([15, 15, 9, 12]), 9, strategy)
    out, at = b"", 0
    while at < len(data):
        step = rng.randrange(1, max(2, len(data) // 3 + 2))
        out += c.compress(data[at:at + step])
        at += step
        r = rng.random()
        if r < 0.1:
            out += c.flush(zlib.Z_FULL_FLUSH)
        elif r < 0.2:
            out += c.flush(zlib.Z_SYNC_FLUSH)
    return out + c.flush()


def mutate(rng, s):
    s = bytearray(s)
    if not s:
        return bytes(s), "empty"
    kind = rng.randrange(8)
    if kind == 0:
        cut = rng.randrange(0, len(s) + 1)
        return bytes(s[:cut]), f"truncate@{cut}"
    if kind == 1:
        for _ in range(rng.randrange(1, 4)):
            s[rng.randrange(len(s))] ^= 1 << rng.randrange(8)
        return bytes(s), "flip"
    if kind == 2:
        i = rng.randrange(len(s))
        s[i] = rng.getrandbits(8)
        return bytes(s), f"byte@{i}"
    if kind == 3:
        i = rng.randrange(len(s))
        del s[i]
        return bytes(s), f"del@{i}"
    if kind == 4:
        i = rng.randrange(len(s) + 1)
        s[i:i] = bytes([rng.getrandbits(8)])
        return bytes(s), f"ins@{i}"
    if kind == 5:
        if len(s) >= 4:
            s[-rng.randrange(1, 5)] ^= 0xFF
        return bytes(s), "adler"
    if kind == 6:
        i = rng.randrange(min(len(s), 40))
        s[i] ^= 1 << rng.randrange(8)
        return bytes(s), f"hdrflip@{i}"
    return bytes(s + bytes(rng.getrandbits(8) for _ in range(rng.randrange(1, 8)))), "append"


LEN_BASE = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
            131, 163, 195, 227, 258]
LEN_EXTRA = [0] * 8 + [1] * 4 + [2] * 4 + [3] * 4 + [4] * 4 + [5] * 4 + [0]
DIST_BASE = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
             2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577]
DIST_EXTRA = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13]


def huffman_lengths(freqs, limit):
    freqs = list(freqs)
    while True:
        heap = [(f, s, (s,)) for s, f in enumerate(freqs) if f]
        if len(heap) == 1:
            lens = [0] * len(freqs)
            lens[heap[0][1]] = 1
            return lens
        heapq.heapify(heap)
        depth, uid = [0] * len(freqs), len(freqs)
        while len(heap) > 1:
            f1, _, a = heapq.heappop(heap)
            f2, _, b = heapq.heappop(heap)
            for s in a + b:
                depth[s] += 1
            heapq.heappush(heap, (f1 + f2, uid, a + b))
            uid += 1
        if max(depth) <= limit:
            return depth
        freqs = [(f + 1) // 2 if f else 0 for f in freqs]


def synthetic(rng):
    """A dynamic block with skewed codes: litlen codewords up to 15 bits and
    offset codewords past 8, which is what reaches libdeflate's subtables."""
    lit_f = [0] * 286
    for s in range(256):
        if rng.random() < 0.6:
            lit_f[s] = max(1, int(100000 * 0.5 ** rng.uniform(0, 17)))
    lit_f[256] = 1
    for s in range(257, 286):
        if rng.random() < 0.5:
            lit_f[s] = max(1, int(50000 * 0.5 ** rng.uniform(0, 16)))
    if not any(lit_f[:256]):
        lit_f[0] = 1
    dist_f = [0] * 30
    for s in range(rng.choice([4, 10, 20, 26, 29]) + 1):
        if rng.random() < 0.8:
            dist_f[s] = max(1, int(50000 * 0.5 ** rng.uniform(0, 16)))
    if not any(dist_f):
        dist_f[0] = 1
    lit_l, dist_l = huffman_lengths(lit_f, 15), huffman_lengths(dist_f, 15)
    if sum(1 for length in dist_l if length) == 1:
        dist_l[(dist_l.index(1) + 1) % 30] = 1
    bw = BW()
    lc, dc = dynamic_header(bw, 1, lit_l, dist_l)
    lits = [s for s in range(256) if lit_l[s]]
    lens = [s for s in range(257, 286) if lit_l[s]]
    dists = [s for s in range(30) if dist_l[s]]
    out = bytearray()
    target = rng.choice([300, 1000, 3000, 6000])
    while len(out) < target:
        if lens and out and rng.random() < 0.4:
            ok = [s for s in dists if DIST_BASE[s] <= len(out)]
            if ok:
                ls, ds = rng.choice(lens), rng.choice(ok)
                li = ls - 257
                lx = rng.getrandbits(LEN_EXTRA[li]) if LEN_EXTRA[li] else 0
                dmax = min(len(out) - DIST_BASE[ds], (1 << DIST_EXTRA[ds]) - 1)
                dx = rng.randrange(0, dmax + 1) if DIST_EXTRA[ds] else 0
                bw.code(*lc[ls])
                bw.put(lx, LEN_EXTRA[li])
                bw.code(*dc[ds])
                bw.put(dx, DIST_EXTRA[ds])
                dist = DIST_BASE[ds] + dx
                for _ in range(LEN_BASE[li] + lx):
                    out.append(out[-dist])
                continue
        s = rng.choice(lits)
        bw.code(*lc[s])
        out.append(s)
    bw.code(*lc[256])
    return bytes(out), zwrap(bw.data(), bytes(out))


def vectors():
    rng = random.Random(SEED)
    vs = crafted()
    for _ in range(700):
        data = sample_data(rng)
        stream = compress(rng, data)
        desc = "valid"
        if rng.random() < 0.7:
            stream, desc = mutate(rng, stream)
        size = rng.choice([len(data), len(data), max(0, len(data) - 1), len(data) + 1, 0,
                           len(data) // 2, rng.randrange(0, 2 * len(data) + 3)])
        vs.append((f"{desc}:n={len(data)}", size, stream))
    for _ in range(300):
        data, stream = synthetic(rng)
        desc = "synthetic"
        if rng.random() < 0.35:
            stream, d2 = mutate(rng, stream)
            desc += "+" + d2
        k = rng.randrange(0, 320)
        size = max(0, rng.choice([len(data) - k, len(data) - k, len(data) + k // 8, len(data)]))
        vs.append((f"{desc}:n={len(data)}", size, stream))
    return vs


def main():
    vs = vectors()
    blob = b"".join(struct.pack("<II", size, len(s)) + s for _, size, s in vs)
    (HERE / "vectors.bin").write_bytes(blob)
    (HERE / "vectors.txt").write_text("".join(desc + "\n" for desc, _, _ in vs), newline="\n")
    exe = build_oracle()
    target = to_wsl(HERE / "vectors.bin") if on_windows() else str(HERE / "vectors.bin")
    answer = run([exe, target]).stdout.decode()
    (HERE / "expected.txt").write_text(answer, newline="\n")
    print(f"{len(vs)} vectors, {len(blob)} bytes; {answer.splitlines()[0]}")


if __name__ == "__main__":
    main()
