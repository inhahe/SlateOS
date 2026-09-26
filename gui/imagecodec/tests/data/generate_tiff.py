#!/usr/bin/env python3
"""Regenerate the TIFF fixtures next to this script, and their answers.

Every answer is libtiff's: libtiff 4.7.1, built as distributions build it
(zlib, libdeflate, libjpeg-turbo), asked for each picture through the RGBA
interface image viewers use -- `TIFFReadRGBAImageOriented` with top-left
orientation and stop-on-error set, which is what gdk-pixbuf calls. An answer
is width, height, then `AARRGGBB` per pixel exactly as libtiff returns them
(premultiplied where libtiff premultiplies, flipped as libtiff flips), or
`REFUSED` and why.

The oracle is built once, the first time this runs, from sources downloaded
and checked against the SHA-256 sums below, into a cache directory: under
WSL's home on Windows (it needs a WSL distribution with `cmake` and `gcc`),
or the user's cache directory on Linux.

The fixtures are made two ways:

- by the small TIFF writer below, which can produce every layout libtiff
  reads -- every sample depth and photometric interpretation, strips and
  tiles, planes together and apart, both byte orders, BigTIFF, the
  predictor, `FillOrder`, old-style LZW -- and the damaged and odd files
  whose treatment is the point of porting libtiff rather than the spec;
- by Pillow's writer, which is libtiff's own encoder, so some fixtures are
  exactly what a real program writes.

Usage
-----

    python gui/imagecodec/tests/data/generate_tiff.py
"""

from __future__ import annotations

import hashlib
import io
import os
import pathlib
import random
import struct
import subprocess
import sys
import zlib

from PIL import Image, TiffImagePlugin

HERE = pathlib.Path(__file__).parent

# ---------------------------------------------------------------------------
# The oracle: libtiff 4.7.1's RGBA reader
# ---------------------------------------------------------------------------

SOURCES = {
    "tiff-4.7.1.tar.gz": (
        "https://download.osgeo.org/libtiff/tiff-4.7.1.tar.gz",
        "f698d94f3103da8ca7438d84e0344e453fe0ba3b7486e04c5bf7a9a3fabe9b69",
    ),
    "libdeflate-1.24.tar.gz": (
        "https://github.com/ebiggers/libdeflate/archive/refs/tags/v1.24.tar.gz",
        "ad8d3723d0065c4723ab738be9723f2ff1cb0f1571e8bfcf0301ff9661f475e8",
    ),
    "libjpeg-turbo-3.1.1.tar.gz": (
        "https://github.com/libjpeg-turbo/libjpeg-turbo/releases/download/3.1.1/libjpeg-turbo-3.1.1.tar.gz",
        "aadc97ea91f6ef078b0ae3a62bba69e008d9a7db19b34e4ac973b19b71b4217c",
    ),
}

ORACLE_C = r"""
/* Each TIFF named on the command line through libtiff's RGBA interface
 * (TIFFReadRGBAImageOriented, top-left, stop on error): what gdk-pixbuf
 * shows. Writes <file>.txt beside it: "REFUSED <why>", or width, height and
 * AARRGGBB per pixel -- premultiplied where libtiff premultiplies. */
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "tiffio.h"

static char last_error[512];

static void on_error(const char *module, const char *fmt, va_list ap) {
    (void)module;
    vsnprintf(last_error, sizeof last_error, fmt, ap);
}

static void on_warning(const char *module, const char *fmt, va_list ap) {
    (void)module; (void)fmt; (void)ap;
}

int main(int argc, char **argv) {
    TIFFSetErrorHandler(on_error);
    TIFFSetWarningHandler(on_warning);
    for (int i = 1; i < argc; i++) {
        char out_path[4096];
        snprintf(out_path, sizeof out_path, "%s", argv[i]);
        char *dot = strrchr(out_path, '.');
        if (dot) strcpy(dot, ".txt"); else strcat(out_path, ".txt");
        FILE *out = fopen(out_path, "w");
        if (!out) { perror(out_path); return 1; }
        last_error[0] = 0;
        TIFF *tif = TIFFOpen(argv[i], "r");
        uint32_t w = 0, h = 0;
        if (!tif) {
            fprintf(out, "REFUSED open: %s\n", last_error);
            fclose(out);
            continue;
        }
        TIFFGetField(tif, TIFFTAG_IMAGEWIDTH, &w);
        TIFFGetField(tif, TIFFTAG_IMAGELENGTH, &h);
        if ((uint64_t)w * h > 64u * 1024 * 1024 || w == 0 || h == 0) {
            fprintf(out, "REFUSED size %ux%u\n", w, h);
            TIFFClose(tif);
            fclose(out);
            continue;
        }
        uint32_t *raster = (uint32_t *)_TIFFmalloc((tmsize_t)w * h * sizeof(uint32_t));
        char emsg[1024] = "";
        if (!TIFFRGBAImageOK(tif, emsg)) {
            fprintf(out, "REFUSED not OK: %s\n", emsg);
        } else if (!TIFFReadRGBAImageOriented(tif, w, h, raster, ORIENTATION_TOPLEFT, 1)) {
            fprintf(out, "REFUSED read: %s\n", last_error);
        } else {
            fprintf(out, "%u %u", w, h);
            for (uint64_t p = 0; p < (uint64_t)w * h; p++) {
                uint32_t v = raster[p];
                fprintf(out, " %02X%02X%02X%02X", TIFFGetA(v), TIFFGetR(v), TIFFGetG(v), TIFFGetB(v));
            }
            fprintf(out, "\n");
        }
        _TIFFfree(raster);
        TIFFClose(tif);
        fclose(out);
    }
    return 0;
}
"""

BUILD_SH = r"""
set -euo pipefail
SRC="$1"
ROOT="$2"
P="$ROOT/prefix"
mkdir -p "$ROOT"
cd "$ROOT"
for t in tiff-4.7.1 libdeflate-1.24 libjpeg-turbo-3.1.1; do
  rm -rf "$t"
  tar xzf "$SRC/$t.tar.gz"
done
cmake -S libdeflate-1.24 -B build-deflate -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$P" \
  -DLIBDEFLATE_BUILD_SHARED_LIB=OFF -DLIBDEFLATE_BUILD_GZIP=OFF > /dev/null
cmake --build build-deflate -j 2 > /dev/null
cmake --install build-deflate > /dev/null
cmake -S libjpeg-turbo-3.1.1 -B build-jpeg -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$P" \
  -DENABLE_SHARED=OFF -DWITH_SIMD=OFF -DWITH_TURBOJPEG=OFF > /dev/null
cmake --build build-jpeg -j 2 > /dev/null
cmake --install build-jpeg > /dev/null
cmake -S tiff-4.7.1 -B build-tiff -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF \
  -Dtiff-tools=OFF -Dtiff-tests=OFF -Dtiff-contrib=OFF -Dtiff-docs=OFF -Dcxx=OFF \
  -Dzlib=ON -Dlibdeflate=ON -Djpeg=ON -Djpeg12=OFF -Dold-jpeg=ON \
  -Djbig=OFF -Dlerc=OFF -Dlzma=OFF -Dzstd=OFF -Dwebp=OFF \
  -DDeflate_ROOT="$P" -DJPEG_ROOT="$P" -DCMAKE_PREFIX_PATH="$P" > /dev/null
cmake --build build-tiff -j 2 > /dev/null
cp "$SRC/tiffrgba.c" .
gcc -O2 -Itiff-4.7.1/libtiff -Ibuild-tiff/libtiff -o tiffrgba tiffrgba.c \
  build-tiff/libtiff/libtiff.a "$P/lib/libdeflate.a" "$P/lib/libjpeg.a" -lz -lm
"""

ORACLE_TAG = "tiff471-deflate124-jpeg311-1"


def on_windows() -> bool:
    return sys.platform == "win32"


def to_wsl(path: pathlib.Path) -> str:
    """A Windows path as WSL sees it."""
    p = str(path.resolve()).replace("\\", "/")
    return f"/mnt/{p[0].lower()}{p[2:]}"


def run(args: list[str]) -> subprocess.CompletedProcess:
    if on_windows():
        return subprocess.run(["wsl", "-e"] + args, check=True, capture_output=True)
    return subprocess.run(args, check=True, capture_output=True)


def fetch(url: str, sha256: str) -> bytes:
    import urllib.request

    with urllib.request.urlopen(url) as response:
        data = response.read()
    got = hashlib.sha256(data).hexdigest()
    if got != sha256:
        sys.exit(f"{url}: SHA-256 {got}, expected {sha256}")
    return data


def build_oracle() -> str:
    """libtiff's RGBA reader as a program, built once; its path where it
    runs (in WSL on Windows)."""
    home = run(["sh", "-c", "echo $HOME"]).stdout.decode().strip()
    root = f"{home}/.cache/slateos-tiff-oracle-{ORACLE_TAG}"
    exe = f"{root}/tiffrgba"
    if run(["sh", "-c", f"test -x '{exe}' && echo yes || echo no"]).stdout.decode().strip() == "yes":
        return exe
    staging = pathlib.Path(os.environ.get("TEMP", "/tmp")) / f"slateos-tiff-oracle-src-{ORACLE_TAG}"
    staging.mkdir(parents=True, exist_ok=True)
    for name, (url, sha256) in SOURCES.items():
        target = staging / name
        if not target.exists() or hashlib.sha256(target.read_bytes()).hexdigest() != sha256:
            target.write_bytes(fetch(url, sha256))
    (staging / "tiffrgba.c").write_text(ORACLE_C, newline="\n")
    (staging / "build.sh").write_text(BUILD_SH, newline="\n")
    src = to_wsl(staging) if on_windows() else str(staging)
    run(["bash", f"{src}/build.sh", src, root])
    return exe


def answer(oracle: str, files: list[pathlib.Path]) -> None:
    paths = [to_wsl(f) if on_windows() else str(f) for f in files]
    for i in range(0, len(paths), 200):
        run([oracle] + paths[i : i + 200])


# ---------------------------------------------------------------------------
# Writing TIFFs
# ---------------------------------------------------------------------------

BYTE, ASCII, SHORT, LONG, RATIONAL = 1, 2, 3, 4, 5
SBYTE, UNDEFINED, SSHORT, SLONG, SRATIONAL, FLOAT, DOUBLE = 6, 7, 8, 9, 10, 11, 12
LONG8 = 16
WIDTHS = {BYTE: 1, ASCII: 1, SHORT: 2, LONG: 4, RATIONAL: 8, SBYTE: 1, UNDEFINED: 1,
          SSHORT: 2, SLONG: 4, SRATIONAL: 8, FLOAT: 4, DOUBLE: 8, LONG8: 8}
FORMATS = {BYTE: "B", SHORT: "H", LONG: "I", SBYTE: "b", SSHORT: "h", SLONG: "i",
           FLOAT: "f", DOUBLE: "d", LONG8: "Q"}

(WIDTH, LENGTH, BITS, COMPRESSION, PHOTOMETRIC, FILL_ORDER, STRIP_OFFSETS, ORIENTATION,
 SAMPLES, ROWS_PER_STRIP, STRIP_BYTE_COUNTS, PLANAR, PREDICTOR, COLOR_MAP, TILE_WIDTH,
 TILE_LENGTH, TILE_OFFSETS, TILE_BYTE_COUNTS, INK_SET, EXTRA_SAMPLES, SAMPLE_FORMAT,
 YCBCR_SUBSAMPLING) = (256, 257, 258, 259, 262, 266, 273, 274, 277, 278, 279, 284, 317, 320,
                       322, 323, 324, 325, 332, 338, 339, 530)
WHITE_POINT, YCBCR_COEFFICIENTS, REFERENCE_BLACK_WHITE = 318, 529, 532

NONE, LZW, ADOBE_DEFLATE, PACKBITS, DEFLATE = 1, 5, 8, 32773, 32946


def values_bytes(e: str, kind: int, values) -> bytes:
    if kind in (ASCII, UNDEFINED) and isinstance(values, (bytes, bytearray)):
        return bytes(values)
    if kind in (RATIONAL, SRATIONAL):
        f = "I" if kind == RATIONAL else "i"
        return b"".join(struct.pack(e + f + f, n, d) for n, d in values)
    return b"".join(struct.pack(e + FORMATS[kind], v) for v in values)


def count_of(kind: int, values) -> int:
    if kind in (ASCII, UNDEFINED) and isinstance(values, (bytes, bytearray)):
        return len(values)
    return len(values)


def write_tiff(entries: list[tuple[int, int, object]], chunks: list[bytes], *, big_endian: bool = False,
               bigtiff: bool = False, offsets_tag: int | None = STRIP_OFFSETS,
               counts_tag: int | None = STRIP_BYTE_COUNTS,
               counts: list[int] | None = None, offsets_kind: int = LONG, counts_kind: int = LONG,
               truncate: int | None = None, magic: bytes | None = None, align: bool = True,
               prefix: bytes = b"") -> bytes:
    """A TIFF of one directory: `entries` as (tag, type, values), and the
    strips or tiles in `chunks`, whose offsets and byte counts are added as
    `offsets_tag` and `counts_tag` (`counts` overriding the true sizes, and
    `counts_tag` None leaving the counts out, and `offsets_tag` None the
    offsets). `prefix` goes straight after the header, before the strips, for
    data the entries point into."""
    e = ">" if big_endian else "<"
    head_size = 16 if bigtiff else 8
    out = bytearray(head_size) + prefix
    offsets = []
    if not align:
        # One byte, so the strips start at an odd offset.
        out += b"\0"
    for chunk in chunks:
        if align and len(out) % 2:
            out += b"\0"
        offsets.append(len(out))
        out += chunk
    if len(out) % 2:
        out += b"\0"
    sizes = counts if counts is not None else [len(c) for c in chunks]
    all_entries = list(entries)
    if offsets_tag is not None:
        all_entries.append((offsets_tag, offsets_kind, offsets))
    if counts_tag is not None:
        all_entries.append((counts_tag, counts_kind, sizes))
    all_entries.sort(key=lambda t: t[0])
    ifd = len(out)
    entry_size, count_fmt, inline = (20, "Q", 8) if bigtiff else (12, "I", 4)
    table_size = (8 if bigtiff else 2) + entry_size * len(all_entries) + (8 if bigtiff else 4)
    extra = bytearray()
    extra_base = ifd + table_size
    table = struct.pack(e + ("Q" if bigtiff else "H"), len(all_entries))
    for tag, kind, values in all_entries:
        data = values_bytes(e, kind, values)
        count = count_of(kind, values) if kind not in (RATIONAL, SRATIONAL) else len(values)
        if len(data) <= inline:
            field = data.ljust(inline, b"\0")
        else:
            if (extra_base + len(extra)) % 2:
                extra += b"\0"
            field = struct.pack(e + ("Q" if bigtiff else "I"), extra_base + len(extra))
            extra += data
        table += struct.pack(e + "HH" + count_fmt, tag, kind, count) + field
    table += struct.pack(e + ("Q" if bigtiff else "I"), 0)
    out += table + extra
    if bigtiff:
        out[0:16] = (b"MM" if big_endian else b"II") + struct.pack(e + "HHHQ", 43, 8, 0, ifd)
    else:
        out[0:8] = (b"MM" if big_endian else b"II") + struct.pack(e + "HI", 42, ifd)
    if magic is not None:
        out[0:2] = magic
    data = bytes(out)
    return data[:truncate] if truncate is not None else data


def picture(w: int, h: int, spp: int, bits: int, seed: int) -> list[list[list[int]]]:
    """Rows of pixels of `spp` samples of `bits` bits: gradients crossed with
    noise, so that a swapped channel, a shifted row or a wrong bit shows."""
    rng = random.Random(seed)
    top = (1 << bits) - 1
    rows = []
    for y in range(h):
        row = []
        for x in range(w):
            px = []
            for s in range(spp):
                gradient = ((x * 37 * (s + 3) + y * 53 * (s + 1)) % 997) / 996
                level = gradient * 0.75 + rng.random() * 0.25
                px.append(min(top, int(level * top + 0.5)))
            row.append(px)
        rows.append(row)
    return rows


def pack_samples(samples: list[int], bits: int, e: str) -> bytes:
    if bits == 8:
        return bytes(samples)
    if bits == 16:
        return b"".join(struct.pack(e + "H", s) for s in samples)
    out = bytearray()
    acc = n = 0
    for s in samples:
        acc = (acc << bits) | s
        n += bits
        while n >= 8:
            n -= 8
            out.append((acc >> n) & 0xFF)
    if n:
        out.append((acc << (8 - n)) & 0xFF)
    return bytes(out)


def difference(samples: list[int], stride: int, bits: int) -> list[int]:
    top = 1 << bits
    return [samples[i] if i < stride else (samples[i] - samples[i - stride]) % top for i in range(len(samples))]


def packbits(data: bytes) -> bytes:
    out = bytearray()
    i = 0
    while i < len(data):
        run = 1
        while i + run < len(data) and run < 128 and data[i + run] == data[i]:
            run += 1
        if run >= 2:
            out += bytes([(257 - run) & 0xFF, data[i]])
            i += run
            continue
        j = i
        while j < len(data) and j - i < 128 and not (j + 1 < len(data) and data[j + 1] == data[j]):
            j += 1
        j = max(j, i + 1)
        out += bytes([j - i - 1]) + data[i:j]
        i = j
    return bytes(out)


def lzw(data: bytes, old_style: bool = False) -> bytes:
    """TIFF LZW as libtiff writes it -- codes most significant bit first,
    widening one code early -- or the pre-6.0 style: least significant bit
    first, widening on time."""
    out = bytearray()
    acc = nacc = 0

    def put(code: int, bits: int) -> None:
        nonlocal acc, nacc
        if old_style:
            acc |= code << nacc
            nacc += bits
            while nacc >= 8:
                out.append(acc & 0xFF)
                acc >>= 8
                nacc -= 8
        else:
            acc = (acc << bits) | code
            nacc += bits
            while nacc >= 8:
                nacc -= 8
                out.append((acc >> nacc) & 0xFF)

    def grow_at(bits: int) -> int:
        return (1 << bits) - 1 + (1 if old_style else 0)

    table = {bytes([i]): i for i in range(256)}
    free, nbits = 258, 9
    put(256, nbits)
    if data:
        w = data[:1]
        for b in data[1:]:
            wb = w + bytes([b])
            if wb in table:
                w = wb
                continue
            put(table[w], nbits)
            table[wb] = free
            free += 1
            w = bytes([b])
            if free == 4094:
                put(256, nbits)
                table = {bytes([i]): i for i in range(256)}
                free, nbits = 258, 9
            elif free > grow_at(nbits):
                nbits += 1
        put(table[w], nbits)
        free += 1
        if free > grow_at(nbits) and nbits < 12:
            nbits += 1
    put(257, nbits)
    if nacc:
        out.append(((acc << (8 - nacc)) if not old_style else acc) & 0xFF)
    return bytes(out)


def compress(data: bytes, scheme: int, old_lzw: bool = False) -> bytes:
    if scheme == NONE:
        return data
    if scheme == PACKBITS:
        return packbits(data)
    if scheme == LZW:
        return lzw(data, old_lzw)
    return zlib.compress(data)


class Layout:
    """How a picture is cut up and stored."""

    def __init__(self, *, rows_per_strip: int = 3, tile: tuple[int, int] | None = None, planar: int = 1,
                 scheme: int = NONE, predictor: int = 1, big_endian: bool = False, bigtiff: bool = False,
                 fill_order: int = 1, old_lzw: bool = False):
        self.rows_per_strip = rows_per_strip
        self.tile = tile
        self.planar = planar
        self.scheme = scheme
        self.predictor = predictor
        self.big_endian = big_endian
        self.bigtiff = bigtiff
        self.fill_order = fill_order
        self.old_lzw = old_lzw


def encode(pixels: list[list[list[int]]], bits: int, layout: Layout) -> list[bytes]:
    """The chunks -- strips or tiles, plane by plane -- of a picture."""
    h, w, spp = len(pixels), len(pixels[0]), len(pixels[0][0])
    e = ">" if layout.big_endian else "<"
    planes = [None] if layout.planar == 1 else list(range(spp))

    def row_samples(y: int, x0: int, x1: int, plane) -> list[int]:
        out = []
        for x in range(x0, x1):
            px = pixels[y][x] if (y < h and x < w) else [0] * spp
            out += px if plane is None else [px[plane]]
        return out

    def finish(raw_rows: list[list[int]]) -> bytes:
        stride = spp if layout.planar == 1 else 1
        body = b""
        for samples in raw_rows:
            if layout.predictor == 2:
                samples = difference(samples, stride, bits)
            body += pack_samples(samples, bits, e)
        # FillOrder 2 reverses the bits of the stored -- compressed -- bytes.
        stored = compress(body, layout.scheme, layout.old_lzw)
        if layout.fill_order == 2:
            stored = bytes(int(f"{b:08b}"[::-1], 2) for b in stored)
        return stored

    chunks = []
    for plane in planes:
        if layout.tile:
            tw, th = layout.tile
            for ty in range(0, h, th):
                for tx in range(0, w, tw):
                    chunks.append(finish([row_samples(y, tx, tx + tw, plane) for y in range(ty, ty + th)]))
        else:
            rps = layout.rows_per_strip
            for y0 in range(0, h, rps):
                chunks.append(finish([row_samples(y, 0, w, plane) for y in range(y0, min(h, y0 + rps))]))
    return chunks


def tiff(pixels: list[list[list[int]]], bits: int, photometric: int | None, layout: Layout = Layout(),
         extra: list[tuple[int, int, object]] | None = None, drop: tuple[int, ...] = (), **kw) -> bytes:
    h, w, spp = len(pixels), len(pixels[0]), len(pixels[0][0])
    entries = [(WIDTH, LONG, [w]), (LENGTH, LONG, [h]), (BITS, SHORT, [bits] * spp),
               (COMPRESSION, SHORT, [layout.scheme]), (SAMPLES, SHORT, [spp])]
    if photometric is not None:
        entries.append((PHOTOMETRIC, SHORT, [photometric]))
    if layout.planar != 1:
        entries.append((PLANAR, SHORT, [layout.planar]))
    if layout.predictor != 1:
        entries.append((PREDICTOR, SHORT, [layout.predictor]))
    if layout.fill_order != 1:
        entries.append((FILL_ORDER, SHORT, [layout.fill_order]))
    if layout.tile:
        entries += [(TILE_WIDTH, LONG, [layout.tile[0]]), (TILE_LENGTH, LONG, [layout.tile[1]])]
        kw.setdefault("offsets_tag", TILE_OFFSETS)
        kw.setdefault("counts_tag", TILE_BYTE_COUNTS)
    else:
        entries.append((ROWS_PER_STRIP, LONG, [layout.rows_per_strip]))
    ours = {tag for tag, _, _ in (extra or [])}
    entries = [x for x in entries if x[0] not in ours and x[0] not in drop] + list(extra or [])
    return write_tiff(entries, encode(pixels, bits, layout), big_endian=layout.big_endian,
                      bigtiff=layout.bigtiff, **kw)


def ycbcr_tiff(w: int, h: int, sub: tuple[int, int], seed: int, *, rows_per_strip: int = 4,
               tile: tuple[int, int] | None = None, planar: int = 1, scheme: int = NONE,
               extra: list[tuple[int, int, object]] | None = None) -> bytes:
    """8-bit YCbCr, packed in blocks of sub[0] x sub[1] luma samples each
    followed by one Cb and one Cr (TIFF 6.0 section 21), or in three planes."""
    hs, vs = sub
    px = picture(w, h, 3, 8, seed)

    def sample(y: int, x: int, k: int) -> int:
        # Past the edge, the edge sample: a writer pads its blocks so.
        return px[min(y, h - 1)][min(x, w - 1)][k]

    def blocks(y0: int, y1: int, x0: int, x1: int) -> bytes:
        out = bytearray()
        for by in range(y0, y1, vs):
            for bx in range(x0, x1, hs):
                out += bytes(sample(by + r, bx + c, 0) for r in range(vs) for c in range(hs))
                out += bytes([sample(by, bx, 1), sample(by, bx, 2)])
        return bytes(out)

    def up(n: int, m: int) -> int:
        return (n + m - 1) // m * m

    chunks = []
    if planar == 2:
        for k in range(3):
            for y0 in range(0, h, rows_per_strip):
                rows = range(y0, min(h, y0 + rows_per_strip))
                chunks.append(compress(bytes(px[y][x][k] for y in rows for x in range(w)), scheme))
    elif tile:
        tw, th = tile
        for ty in range(0, h, th):
            for tx in range(0, w, tw):
                chunks.append(compress(blocks(ty, ty + th, tx, tx + tw), scheme))
    else:
        for y0 in range(0, h, rows_per_strip):
            rows = min(h, y0 + rows_per_strip) - y0
            chunks.append(compress(blocks(y0, y0 + up(rows, vs), 0, up(w, hs)), scheme))
    entries = [(WIDTH, LONG, [w]), (LENGTH, LONG, [h]), (BITS, SHORT, [8, 8, 8]),
               (COMPRESSION, SHORT, [scheme]), (PHOTOMETRIC, SHORT, [6]), (SAMPLES, SHORT, [3]),
               (YCBCR_SUBSAMPLING, SHORT, [hs, vs])]
    if planar == 2:
        entries.append((PLANAR, SHORT, [2]))
    kw = {}
    if tile:
        entries += [(TILE_WIDTH, LONG, [tile[0]]), (TILE_LENGTH, LONG, [tile[1]])]
        kw = {"offsets_tag": TILE_OFFSETS, "counts_tag": TILE_BYTE_COUNTS}
    else:
        entries.append((ROWS_PER_STRIP, LONG, [rows_per_strip]))
    ours = {tag for tag, _, _ in (extra or [])}
    entries = [x for x in entries if x[0] not in ours] + list(extra or [])
    return write_tiff(entries, chunks, **kw)


def lab_tiff(w: int, h: int, bits: int, seed: int, extra: list[tuple[int, int, object]] | None = None,
             big_endian: bool = False) -> bytes:
    """CIE L*a*b*: L unsigned, a and b signed, 8 or 16 bits."""
    px = picture(w, h, 3, bits, seed)
    half = 1 << (bits - 1)
    # a and b are signed: store the picture's values shifted to straddle 0.
    signed = [[[p[0], (p[1] - half) % (1 << bits), (p[2] - half) % (1 << bits)] for p in row] for row in px]
    return tiff(signed, bits, 8, Layout(big_endian=big_endian), extra=extra)


def colour_map(bits: int, wide: bool = True, seed: int = 5) -> tuple[int, int, object]:
    rng = random.Random(seed)
    n = 1 << bits
    top = 65535 if wide else 255
    values = [rng.randrange(top + 1) for _ in range(3 * n)]
    return (COLOR_MAP, SHORT, values)


# ---------------------------------------------------------------------------
# The fixtures
# ---------------------------------------------------------------------------


def fixtures() -> dict[str, bytes]:
    W, H = 13, 11
    f: dict[str, bytes] = {}
    rgb = picture(W, H, 3, 8, 1)
    rgba = picture(W, H, 4, 8, 2)

    # Grey, every depth, both senses.
    for bits in (1, 2, 4, 8, 16):
        f[f"grey{bits}"] = tiff(picture(W, H, 1, bits, 10 + bits), bits, 1)
    for bits in (1, 4, 8, 16):
        f[f"white{bits}"] = tiff(picture(W, H, 1, bits, 20 + bits), bits, 0)
    f["grey16_big_endian"] = tiff(picture(W, H, 1, 16, 31), 16, 1, Layout(big_endian=True))
    # Palettes, of 16-bit and of old 8-bit colour maps.
    for bits in (1, 2, 4, 8):
        f[f"palette{bits}"] = tiff(picture(W, H, 1, bits, 40 + bits), bits, 3, extra=[colour_map(bits)])
    f["palette8_narrow_map"] = tiff(picture(W, H, 1, 8, 49), 8, 3, extra=[colour_map(8, wide=False)])
    f["palette8_with_extra_sample"] = tiff(picture(W, H, 2, 8, 50), 8, 3,
                                           extra=[colour_map(8), (EXTRA_SAMPLES, SHORT, [2])])
    # RGB, with and without alpha of each kind.
    f["rgb8"] = tiff(rgb, 8, 2)
    f["rgb16"] = tiff(picture(W, H, 3, 16, 3), 16, 2)
    f["rgb16_big_endian"] = tiff(picture(W, H, 3, 16, 4), 16, 2, Layout(big_endian=True))
    f["rgba8_unassociated"] = tiff(rgba, 8, 2, extra=[(EXTRA_SAMPLES, SHORT, [2])])
    f["rgba8_associated"] = tiff(premultiplied(rgba), 8, 2, extra=[(EXTRA_SAMPLES, SHORT, [1])])
    f["rgba8_unspecified"] = tiff(rgba, 8, 2, extra=[(EXTRA_SAMPLES, SHORT, [0])])
    f["rgba8_no_extra_samples_tag"] = tiff(rgba, 8, 2)
    f["rgba16_unassociated"] = tiff(picture(W, H, 4, 16, 5), 16, 2, extra=[(EXTRA_SAMPLES, SHORT, [2])])
    f["rgba16_associated"] = tiff(picture(W, H, 4, 16, 6), 16, 2, extra=[(EXTRA_SAMPLES, SHORT, [1])])
    f["rgb8_two_extra_samples"] = tiff(picture(W, H, 5, 8, 7), 8, 2, extra=[(EXTRA_SAMPLES, SHORT, [0, 2])])
    f["grey_alpha8_unassociated"] = tiff(picture(W, H, 2, 8, 8), 8, 1, extra=[(EXTRA_SAMPLES, SHORT, [2])])
    f["grey_alpha8_associated"] = tiff(picture(W, H, 2, 8, 9), 8, 1, extra=[(EXTRA_SAMPLES, SHORT, [1])])
    f["grey_alpha16"] = tiff(picture(W, H, 2, 16, 11), 16, 1, extra=[(EXTRA_SAMPLES, SHORT, [2])])
    f["corel_alpha_999"] = tiff(rgba, 8, 2, extra=[(EXTRA_SAMPLES, SHORT, [999])])
    # CMYK.
    f["cmyk8"] = tiff(picture(W, H, 4, 8, 12), 8, 5)
    f["cmyk8_five_samples"] = tiff(picture(W, H, 5, 8, 13), 8, 5, extra=[(EXTRA_SAMPLES, SHORT, [2])])
    f["cmyk8_separate"] = tiff(picture(W, H, 4, 8, 14), 8, 5, Layout(planar=2))
    f["cmyk16_refused"] = tiff(picture(W, H, 4, 16, 15), 16, 5)
    f["cmyk_other_inks_refused"] = tiff(picture(W, H, 4, 8, 16), 8, 5, extra=[(INK_SET, SHORT, [2])])
    # Separate planes.
    f["rgb8_separate"] = tiff(rgb, 8, 2, Layout(planar=2))
    f["rgb16_separate_big_endian"] = tiff(picture(W, H, 3, 16, 17), 16, 2, Layout(planar=2, big_endian=True))
    f["rgba8_separate_unassociated"] = tiff(rgba, 8, 2, Layout(planar=2), extra=[(EXTRA_SAMPLES, SHORT, [2])])
    f["rgba8_separate_associated"] = tiff(premultiplied(rgba), 8, 2, Layout(planar=2),
                                          extra=[(EXTRA_SAMPLES, SHORT, [1])])
    f["grey_alpha8_separate"] = tiff(picture(W, H, 2, 8, 18), 8, 1, Layout(planar=2),
                                     extra=[(EXTRA_SAMPLES, SHORT, [2])])
    f["white8_separate"] = tiff(picture(W, H, 2, 8, 19), 8, 0, Layout(planar=2),
                                extra=[(EXTRA_SAMPLES, SHORT, [0])])
    # Tiles, cut at the right and bottom edges.
    big = picture(37, 21, 3, 8, 20)
    f["rgb8_tiled"] = tiff(big, 8, 2, Layout(tile=(16, 16)))
    f["rgb8_tiled_separate"] = tiff(big, 8, 2, Layout(tile=(16, 16), planar=2))
    f["grey8_tiled"] = tiff(picture(37, 21, 1, 8, 21), 8, 1, Layout(tile=(16, 16)))
    f["grey1_tiled"] = tiff(picture(37, 21, 1, 1, 22), 1, 1, Layout(tile=(16, 16)))
    f["rgba16_tiled"] = tiff(picture(37, 21, 4, 16, 23), 16, 2, Layout(tile=(16, 16)),
                             extra=[(EXTRA_SAMPLES, SHORT, [2])])
    f["palette4_tiled_lzw"] = tiff(picture(37, 21, 1, 4, 24), 4, 3, Layout(tile=(16, 16), scheme=LZW),
                                   extra=[colour_map(4)])
    f["rgb8_tile_wider_than_picture"] = tiff(picture(9, 7, 3, 8, 25), 8, 2, Layout(tile=(16, 16)))
    f["rgb8_odd_tile_width"] = tiff(big, 8, 2, Layout(tile=(10, 16)))
    # TileWidth and no TileLength: libtiff takes the tile height from
    # RowsPerStrip, read first, and the file is one row of 16x21 tiles.
    f["rgb8_tile_length_from_rows_per_strip"] = tiff(big, 8, 2, Layout(tile=(16, 21)),
                                                     extra=[(ROWS_PER_STRIP, LONG, [21])], drop=(TILE_LENGTH,))
    f["grey1_odd_tile_width"] = tiff(picture(37, 21, 1, 1, 26), 1, 1, Layout(tile=(10, 16)))
    # Compression.
    for name, scheme in (("packbits", PACKBITS), ("lzw", LZW), ("deflate", ADOBE_DEFLATE), ("zip", DEFLATE)):
        f[f"rgb8_{name}"] = tiff(rgb, 8, 2, Layout(scheme=scheme))
        f[f"grey1_{name}"] = tiff(picture(W, H, 1, 1, 27), 1, 1, Layout(scheme=scheme))
    f["rgb8_lzw_predictor"] = tiff(rgb, 8, 2, Layout(scheme=LZW, predictor=2))
    f["rgba16_lzw_predictor_big_endian"] = tiff(picture(W, H, 4, 16, 28), 16, 2,
                                                Layout(scheme=LZW, predictor=2, big_endian=True),
                                                extra=[(EXTRA_SAMPLES, SHORT, [2])])
    f["grey16_deflate_predictor"] = tiff(picture(W, H, 1, 16, 29), 16, 1, Layout(scheme=DEFLATE, predictor=2))
    f["rgb8_separate_deflate_predictor"] = tiff(rgb, 8, 2, Layout(planar=2, scheme=DEFLATE, predictor=2))
    f["rgb8_tiled_lzw_predictor"] = tiff(big, 8, 2, Layout(tile=(16, 16), scheme=LZW, predictor=2))
    f["rgb8_old_style_lzw"] = tiff(rgb, 8, 2, Layout(scheme=LZW, old_lzw=True))
    f["grey4_predictor_refused"] = tiff(picture(W, H, 1, 4, 30), 4, 1, Layout(scheme=LZW, predictor=2))
    f["rgb8_predictor_3_refused"] = tiff(rgb, 8, 2, Layout(scheme=LZW, predictor=3))
    f["rgb8_uncompressed_predictor_ignored"] = tiff(rgb, 8, 2, extra=[(PREDICTOR, SHORT, [2])])
    # Byte orders and BigTIFF.
    f["rgb8_big_endian"] = tiff(rgb, 8, 2, Layout(big_endian=True))
    f["rgb8_bigtiff"] = tiff(rgb, 8, 2, Layout(bigtiff=True), offsets_kind=LONG8, counts_kind=LONG8)
    f["rgb16_bigtiff_big_endian_lzw"] = tiff(picture(W, H, 3, 16, 32), 16, 2,
                                             Layout(bigtiff=True, big_endian=True, scheme=LZW))
    f["rgb8_mdi_magic"] = tiff(rgb, 8, 2, magic=b"EP")
    # Fill order: bits reversed in every byte, whatever the depth.
    f["grey1_fill_order_2"] = tiff(picture(W, H, 1, 1, 33), 1, 1, Layout(fill_order=2))
    f["rgb8_fill_order_2"] = tiff(rgb, 8, 2, Layout(fill_order=2))
    f["rgb8_fill_order_2_lzw"] = tiff(rgb, 8, 2, Layout(fill_order=2, scheme=LZW))
    # libtiff checks an uncompressed first tile's size against its read
    # buffer, which bit reversal rounds up to a multiple of 1024 bytes: a
    # 16x16 RGB tile (768 bytes) is refused, a 32x32 grey one (1024) is not.
    f["rgb8_tiled_fill_order_2_refused"] = tiff(big, 8, 2, Layout(tile=(16, 16), fill_order=2))
    f["grey8_tiled_fill_order_2"] = tiff(picture(37, 21, 1, 8, 42), 8, 1, Layout(tile=(32, 32), fill_order=2))
    # Orientation, all eight.
    for value in range(1, 9):
        f[f"orientation{value}"] = tiff(rgb, 8, 2, extra=[(ORIENTATION, SHORT, [value])])
    f["orientation9_ignored"] = tiff(rgb, 8, 2, extra=[(ORIENTATION, SHORT, [9])])
    # What libtiff mends.
    f["no_byte_counts"] = tiff(rgb, 8, 2, Layout(rows_per_strip=H), counts_tag=None)
    f["no_byte_counts_many_strips_refused"] = tiff(rgb, 8, 2, counts_tag=None)
    f["no_rows_per_strip"] = tiff(rgb, 8, 2, Layout(rows_per_strip=H), drop=(ROWS_PER_STRIP,))
    f["zero_byte_count"] = tiff(rgb, 8, 2, Layout(rows_per_strip=H), counts=[0])
    f["short_byte_count"] = tiff(rgb, 8, 2, Layout(rows_per_strip=H), counts=[10])
    f["no_photometric_rgb"] = tiff(rgb, 8, None)
    f["no_photometric_grey"] = tiff(picture(W, H, 1, 8, 34), 8, None)
    # No photometric reads as min-is-white's one channel, so the second is
    # an extra sample, and the picture is grey.
    f["no_photometric_two_channels"] = tiff(picture(W, H, 2, 8, 35), 8, None)
    # A palette's one channel makes the other two extra samples before the
    # missing map turns it to RGB -- of one channel, which is refused.
    f["palette_without_map_rgb_refused"] = tiff(rgb, 8, 3)
    f["palette_without_map_grey"] = tiff(picture(W, H, 1, 8, 36), 8, 3)
    f["palette4_without_map_refused"] = tiff(picture(W, H, 1, 4, 37), 4, 3)
    f["bits_per_sample_once"] = tiff(rgb, 8, 2, extra=[(BITS, SHORT, [8])])
    f["short_typed_offsets"] = tiff(rgb, 8, 2, offsets_kind=SHORT, counts_kind=SHORT)
    f["rgb_with_one_colour_refused"] = tiff(picture(W, H, 2, 8, 38), 8, 2)
    f["grey4_two_samples_refused"] = tiff(picture(W, H, 2, 4, 39), 4, 1)
    # Refused outright.
    f["bits3_refused"] = tiff(picture(W, H, 1, 3, 40), 3, 1)
    f["float_refused"] = tiff(picture(W, H, 1, 16, 41), 16, 1, extra=[(SAMPLE_FORMAT, SHORT, [3])])
    f["bits_differ_refused"] = tiff(rgb, 8, 2, extra=[(BITS, SHORT, [8, 8, 16])])
    f["planar_3_refused"] = tiff(rgb, 8, 2, extra=[(PLANAR, SHORT, [3])])
    f["rows_per_strip_0_refused"] = tiff(rgb, 8, 2, extra=[(ROWS_PER_STRIP, LONG, [0])])
    f["extra_samples_5_refused"] = tiff(rgba, 8, 2, extra=[(EXTRA_SAMPLES, SHORT, [5])])
    f["unknown_compression_refused"] = tiff(rgb, 8, 2, extra=[(COMPRESSION, SHORT, [12345])])
    f["lzma_refused"] = tiff(rgb, 8, 2, extra=[(COMPRESSION, SHORT, [34925])])
    f["truncated_refused"] = tiff(rgb, 8, 2, truncate=60)
    whole = tiff(rgb, 8, 2, Layout(scheme=LZW))
    f["lzw_garbage_refused"] = whole[:20] + bytes(b ^ 0x5A for b in whole[20:40]) + whole[40:]
    f["zero_width_refused"] = tiff(rgb, 8, 2, extra=[(WIDTH, LONG, [0])])
    f["ycbcr_subsampling_0_refused"] = tiff(rgb, 8, 2, extra=[(YCBCR_SUBSAMPLING, SHORT, [1, 0])])
    # YCbCr: every subsampling libtiff converts, at sizes that cut blocks.
    for hs, vs in ((1, 1), (2, 1), (2, 2), (4, 1), (4, 2), (4, 4), (1, 2)):
        f[f"ycbcr{hs}{vs}"] = ycbcr_tiff(W, H, (hs, vs), 50 + hs * 5 + vs)
    f["ycbcr22_odd_rows_per_strip"] = ycbcr_tiff(W, H, (2, 2), 60, rows_per_strip=3)
    f["ycbcr42_tiled"] = ycbcr_tiff(37, 21, (4, 2), 61, tile=(16, 16))
    f["ycbcr22_lzw"] = ycbcr_tiff(W, H, (2, 2), 62, scheme=LZW)
    f["ycbcr11_separate"] = ycbcr_tiff(W, H, (1, 1), 63, planar=2)
    f["ycbcr22_separate_refused"] = ycbcr_tiff(W, H, (2, 2), 64, planar=2)
    f["ycbcr24_refused"] = ycbcr_tiff(W, H, (2, 4), 65)
    f["ycbcr22_rec709"] = ycbcr_tiff(W, H, (2, 2), 66, extra=[
        (YCBCR_COEFFICIENTS, RATIONAL, [(2126, 10000), (7152, 10000), (722, 10000)])])
    f["ycbcr22_studio_range"] = ycbcr_tiff(W, H, (2, 2), 67, extra=[
        (REFERENCE_BLACK_WHITE, RATIONAL, [(16, 1), (235, 1), (128, 1), (240, 1), (128, 1), (240, 1)])])
    f["ycbcr22_zero_green_luma_refused"] = ycbcr_tiff(W, H, (2, 2), 68, extra=[
        (YCBCR_COEFFICIENTS, RATIONAL, [(299, 1000), (0, 1), (114, 1000)])])
    f["ycbcr22_float_reference"] = ycbcr_tiff(W, H, (2, 2), 69, extra=[
        (REFERENCE_BLACK_WHITE, FLOAT, [0.5, 254.5, 127.25, 255.0, 128.0, 250.0])])
    # CIE L*a*b*.
    f["lab8"] = lab_tiff(W, H, 8, 70)
    f["lab16"] = lab_tiff(W, H, 16, 71)
    f["lab16_big_endian"] = lab_tiff(W, H, 16, 72, big_endian=True)
    f["lab8_d65"] = lab_tiff(W, H, 8, 73, extra=[(WHITE_POINT, RATIONAL, [(3127, 10000), (3290, 10000)])])
    f["lab8_white_point_zero_refused"] = lab_tiff(W, H, 8, 74, extra=[(WHITE_POINT, RATIONAL, [(3127, 10000), (0, 1)])])
    f["lab8_separate_refused"] = tiff(picture(W, H, 3, 8, 75), 8, 8, Layout(planar=2))
    return f


def premultiplied(pixels: list[list[list[int]]]) -> list[list[list[int]]]:
    """Colour at most its alpha, as associated alpha must be."""
    return [[[(c * px[-1] + 127) // 255 for c in px[:-1]] + [px[-1]] for px in row] for row in pixels]


def read_tiff(data: bytes) -> tuple[list[tuple[int, int, object]], list[bytes]]:
    """A little-endian classic TIFF's first directory, as (tag, type, values)
    entries without the strip arrays, and its strips' bytes."""
    e = "<"
    ifd = struct.unpack(e + "I", data[4:8])[0]
    n = struct.unpack(e + "H", data[ifd:ifd + 2])[0]
    entries, offsets, counts = [], [], []
    for i in range(n):
        tag, kind, count, raw = struct.unpack(e + "HHI4s", data[ifd + 2 + 12 * i:ifd + 14 + 12 * i])
        width = WIDTHS.get(kind, 1)
        at = struct.unpack(e + "I", raw)[0] if count * width > 4 else None
        blob = data[at:at + count * width] if at is not None else raw[:count * width]
        if kind in (ASCII, UNDEFINED):
            values = blob
        elif kind in (RATIONAL, SRATIONAL):
            values = [struct.unpack(e + "II", blob[k:k + 8]) for k in range(0, len(blob), 8)]
        else:
            values = list(struct.unpack(e + FORMATS[kind] * count, blob))
        if tag == STRIP_OFFSETS:
            offsets = values
        elif tag == STRIP_BYTE_COUNTS:
            counts = values
        else:
            entries.append((tag, kind, values))
    return entries, [data[o:o + c] for o, c in zip(offsets, counts)]


def pillow_fax(img: Image.Image, compression: str, info: dict[int, int] | None = None) -> bytes:
    """A bilevel picture as Pillow's writer -- libtiff's -- encodes it."""
    ti = TiffImagePlugin.ImageFileDirectory_v2()
    for k, v in (info or {}).items():
        ti[k] = v
    buf = io.BytesIO()
    img.save(buf, "TIFF", compression=compression, tiffinfo=ti)
    return buf.getvalue()


def rewrap(data: bytes, *, extra: list[tuple[int, int, object]] | None = None, drop: tuple[int, ...] = (),
           strips=None, **kw) -> bytes:
    """The same strips under different tags: `extra` replacing or adding,
    `drop` removing, `strips` transforming the strips' bytes."""
    entries, chunks = read_tiff(data)
    ours = {tag for tag, _, _ in (extra or [])}
    entries = [x for x in entries if x[0] not in ours and x[0] not in drop] + list(extra or [])
    if strips is not None:
        chunks = strips(chunks)
    return write_tiff(entries, chunks, **kw)


def fax_fixtures() -> dict[str, bytes]:
    """Bilevel pictures in every fax scheme, from libtiff's own encoder."""
    rng = random.Random(77)
    out: dict[str, bytes] = {}
    for (w, h) in ((37, 21), (1, 5), (130, 9)):
        img = Image.new("1", (w, h))
        img.putdata([1 if ((x * 7 + y * 3) % 11 < 4) ^ (rng.random() < 0.15) else 0
                     for y in range(h) for x in range(w)])
        tag = f"{w}x{h}"
        out[f"fax_g3_1d_{tag}"] = pillow_fax(img, "group3")
        out[f"fax_g3_2d_{tag}"] = pillow_fax(img, "group3", {292: 1})
        out[f"fax_g4_{tag}"] = pillow_fax(img, "group4")
        out[f"fax_rle_{tag}"] = pillow_fax(img, "tiff_ccitt")
        # libtiff refuses the 1x5 one, which its own encoder wrote: its
        # word-aligned decoder loses step on rows of one pixel.
        out[f"fax_rlew_{tag}"] = pillow_fax(img, "tiff_raw_16")
    img = Image.new("1", (61, 40))
    img.putdata([1 if (x // 5 + y // 4) % 2 else 0 for y in range(40) for x in range(61)])
    base = pillow_fax(img, "group4")
    # No PhotometricInterpretation: fax is min-is-white by default.
    out["fax_g4_no_photometric"] = rewrap(base, drop=(PHOTOMETRIC,))
    out["fax_g4_min_is_white"] = rewrap(base, extra=[(PHOTOMETRIC, SHORT, [0])])
    reverse = lambda chunks: [bytes(int(f"{b:08b}"[::-1], 2) for b in c) for c in chunks]  # noqa: E731
    out["fax_g4_fill_order_2"] = rewrap(base, extra=[(FILL_ORDER, SHORT, [2])], strips=reverse)
    out["fax_g3_2d_fill_order_2"] = rewrap(pillow_fax(img, "group3", {292: 1}), extra=[(FILL_ORDER, SHORT, [2])],
                                           strips=reverse)
    # A strip cut short. Group 4 keeps the rows that decoded. Group 3 1-D is
    # shown too, and wrongly: at the cut, zero padding reads as an
    # end-of-line code with no end, so libtiff decodes the strip again from
    # its start, without end-of-line codes, into the rows still to fill.
    cut = lambda chunks: [c[: len(c) * 2 // 3] for c in chunks]  # noqa: E731
    out["fax_g3_1d_cut"] = rewrap(pillow_fax(img, "group3"), strips=cut)
    out["fax_g4_cut"] = rewrap(base, strips=cut)
    # A word-aligned stream whose strip starts at an odd offset.
    out["fax_rlew_odd_offset"] = rewrap(pillow_fax(img, "tiff_raw_16"), align=False)
    out["fax_g4_two_bits_refused"] = rewrap(base, extra=[(BITS, SHORT, [2])])
    # Tiles, each encoded by libtiff as a picture of its own. A tile whose
    # data runs out is shown as far as it decoded: libtiff's fax decoders
    # fail with -1, which a tile's read takes for success (a strip's does
    # not).
    tiles = []
    for ty in range(0, 40, 16):
        for tx in range(0, 61, 16):
            tile = Image.new("1", (16, 16), 1)
            tile.paste(img.crop((tx, ty, min(61, tx + 16), min(40, ty + 16))), (0, 0))
            tiles.append(read_tiff(pillow_fax(tile, "group4"))[1][0])
    entries = [(WIDTH, LONG, [61]), (LENGTH, LONG, [40]), (BITS, SHORT, [1]), (COMPRESSION, SHORT, [4]),
               (PHOTOMETRIC, SHORT, [1]), (SAMPLES, SHORT, [1]), (TILE_WIDTH, LONG, [16]),
               (TILE_LENGTH, LONG, [16])]
    tiled = {"offsets_tag": TILE_OFFSETS, "counts_tag": TILE_BYTE_COUNTS}
    out["fax_g4_tiled"] = write_tiff(entries, tiles, **tiled)
    out["fax_g4_tiled_cut"] = write_tiff(entries, [t[: len(t) // 2] for t in tiles], **tiled)
    return out


JPEG_TABLES = 347


def jpeg_of(img: Image.Image, **options) -> bytes:
    """`img` as Pillow's encoder (libjpeg-turbo) writes it."""
    buf = io.BytesIO()
    img.save(buf, "JPEG", **options)
    return buf.getvalue()


def lossless_jpeg_of(img: Image.Image, psv: int = 1) -> bytes:
    """`img` as a lossless JPEG, one interleaved scan, by the encoder in
    `generate_jpeg_lossless.py` (Pillow writes none)."""
    import generate_jpeg_lossless as ll

    comps = [ll.Component(n + 1, 1, 1, [[band.getpixel((x, y)) for x in range(img.width)]
                                        for y in range(img.height)])
             for n, band in enumerate(img.split())]
    pic = ll.Picture(img.width, img.height, 8, comps)
    return ll.lossless_jpeg(pic, [{"comps": list(range(len(comps))), "psv": psv, "pt": 0}])


def lossless_jpeg_strips(img: Image.Image, rows: int, psv: int = 1) -> list[bytes]:
    return [lossless_jpeg_of(img.crop((0, y, img.width, min(img.height, y + rows))), psv)
            for y in range(0, img.height, rows)]


def jpeg_strips(img: Image.Image, rows: int, **options) -> list[bytes]:
    """Each band of `rows` rows of `img` as a JPEG of its own."""
    return [jpeg_of(img.crop((0, y, img.width, min(img.height, y + rows))), **options)
            for y in range(0, img.height, rows)]


def jpeg_tiff(img: Image.Image, photometric: int, strips: list[bytes], *, rows: int,
              extra: list[tuple[int, int, object]] | None = None, **kw) -> bytes:
    """A striped JPEG TIFF of `img` with the given strips' datastreams."""
    spp = len(img.getbands())
    entries = [(WIDTH, LONG, [img.width]), (LENGTH, LONG, [img.height]), (BITS, SHORT, [8] * spp),
               (COMPRESSION, SHORT, [7]), (PHOTOMETRIC, SHORT, [photometric]), (SAMPLES, SHORT, [spp]),
               (ROWS_PER_STRIP, LONG, [rows])]
    ours = {tag for tag, _, _ in (extra or [])}
    entries = [e for e in entries if e[0] not in ours] + list(extra or [])
    return write_tiff(entries, strips, **kw)


def jpeg_fixtures() -> dict[str, bytes]:
    """JPEG-compressed TIFFs: every layout libtiff's JPEG codec takes, and the
    checks it makes of each strip's datastream against the strip. The strips
    are Pillow's JPEGs (libjpeg-turbo's encoder); the files are written here,
    except the ones from Pillow's TIFF writer, which is libtiff's."""
    rng = random.Random(1234)
    base = Image.new("RGB", (29, 19))
    base.putdata([((x * 9 + rng.randrange(40)) % 256, (y * 13) % 256, (x * y + rng.randrange(30)) % 256)
                  for y in range(19) for x in range(29)])
    grey = base.convert("L")
    out: dict[str, bytes] = {}
    sub = [(YCBCR_SUBSAMPLING, SHORT, [2, 2])]
    # YCbCr at every subsampling a TIFF can say and Pillow can write.
    for name, sampling, tag in (("420", 2, [2, 2]), ("422", 1, [2, 1]), ("444", 0, [1, 1])):
        strips = jpeg_strips(base, 8, quality=85, subsampling=sampling)
        out[f"jpeg_ycbcr{name}"] = jpeg_tiff(base, 6, strips, rows=8,
                                             extra=[(YCBCR_SUBSAMPLING, SHORT, tag)])
    # No YCbCrSubsampling: libtiff reads the first strip's frame for it.
    out["jpeg_ycbcr420_no_subsampling_tag"] = jpeg_tiff(base, 6, jpeg_strips(base, 8, subsampling=2), rows=8)
    out["jpeg_ycbcr444_no_subsampling_tag"] = jpeg_tiff(base, 6, jpeg_strips(base, 8, subsampling=0), rows=8)
    out["jpeg_ycbcr422_no_subsampling_tag"] = jpeg_tiff(base, 6, jpeg_strips(base, 8, subsampling=1), rows=8)
    # The tag says 2x2 but the strips are 4:4:4: each strip's sampling is
    # checked against the tag, and refused.
    out["jpeg_ycbcr_sampling_mismatch_refused"] = jpeg_tiff(
        base, 6, jpeg_strips(base, 8, subsampling=0), rows=8, extra=sub)
    # Rows per strip not a multiple of the chroma block.
    out["jpeg_ycbcr420_odd_strips"] = jpeg_tiff(base, 6, jpeg_strips(base, 5, subsampling=2), rows=5, extra=sub)
    # One strip.
    out["jpeg_ycbcr420_one_strip"] = jpeg_tiff(base, 6, [jpeg_of(base, subsampling=2)], rows=19, extra=sub)
    # Tables apart: JPEGTables, and strips of image data alone.
    tables = jpeg_of(base, quality=70, subsampling=2, streamtype=1)
    abbreviated = jpeg_strips(base, 8, quality=70, subsampling=2, streamtype=2)
    out["jpeg_tables_abbreviated"] = jpeg_tiff(
        base, 6, abbreviated, rows=8, extra=sub + [(JPEG_TABLES, UNDEFINED, tables)])
    # Strips that need the tables, and no JPEGTables to give them.
    out["jpeg_abbreviated_without_tables_refused"] = jpeg_tiff(base, 6, abbreviated, rows=8, extra=sub)
    # JPEGTables holding a whole picture rather than tables alone.
    out["jpeg_tables_with_an_image_refused"] = jpeg_tiff(
        base, 6, abbreviated, rows=8, extra=sub + [(JPEG_TABLES, UNDEFINED, jpeg_of(base, subsampling=2))])
    # A strip that redefines the tables leaves them for the strips after it:
    # JPEGTables at quality 30, the first strip whole at quality 90, the rest
    # abbreviated at quality 90 -- right only with the first strip's tables.
    q90 = jpeg_strips(base, 8, quality=90, subsampling=2)
    q90_abbreviated = jpeg_strips(base, 8, quality=90, subsampling=2, streamtype=2)
    out["jpeg_tables_redefined_by_a_strip"] = jpeg_tiff(
        base, 6, [q90[0]] + q90_abbreviated[1:], rows=8,
        extra=sub + [(JPEG_TABLES, UNDEFINED, jpeg_of(base, quality=30, subsampling=2, streamtype=1))])
    # Grey, RGB stored as RGB, RGB stored as YCbCr (shown unconverted, as
    # libtiff shows it), and CMYK.
    out["jpeg_grey"] = jpeg_tiff(grey, 1, jpeg_strips(grey, 8, quality=80), rows=8)
    out["jpeg_grey_min_is_white"] = jpeg_tiff(grey, 0, jpeg_strips(grey, 8, quality=80), rows=8)
    out["jpeg_rgb"] = jpeg_tiff(base, 2, jpeg_strips(base, 8, quality=90, subsampling=0, keep_rgb=True), rows=8)
    out["jpeg_rgb_holding_ycbcr"] = jpeg_tiff(base, 2, jpeg_strips(base, 8, quality=90, subsampling=0), rows=8)
    cmyk = base.convert("CMYK")
    out["jpeg_cmyk"] = jpeg_tiff(cmyk, 5, jpeg_strips(cmyk, 8, quality=85), rows=8)
    # Progressive strips, and restart markers.
    out["jpeg_ycbcr420_progressive"] = jpeg_tiff(
        base, 6, jpeg_strips(base, 8, subsampling=2, progressive=True), rows=8, extra=sub)
    out["jpeg_ycbcr420_restarts"] = jpeg_tiff(
        base, 6, jpeg_strips(base, 8, subsampling=2, restart_marker_blocks=1), rows=8, extra=sub)
    # A last strip whose datastream keeps the full strip height: tolerated.
    tall = jpeg_strips(base, 8, subsampling=2)
    padded = Image.new("RGB", (29, 24))
    padded.paste(base, (0, 0))
    tall[-1] = jpeg_of(padded.crop((0, 16, 29, 24)), subsampling=2)
    out["jpeg_ycbcr420_tall_last_strip"] = jpeg_tiff(base, 6, tall, rows=8, extra=sub)
    # A middle strip that tall is not.
    tall_middle = jpeg_strips(base, 8, subsampling=2)
    tall_middle[0] = jpeg_of(padded.crop((0, 0, 29, 12)), subsampling=2)
    out["jpeg_ycbcr420_tall_first_strip_refused"] = jpeg_tiff(base, 6, tall_middle, rows=8, extra=sub)
    # A strip wider than the image.
    wide = jpeg_strips(base, 8, subsampling=2)
    wide[1] = jpeg_of(Image.new("RGB", (33, 8), (9, 99, 199)), subsampling=2)
    out["jpeg_ycbcr420_wide_strip_refused"] = jpeg_tiff(base, 6, wide, rows=8, extra=sub)
    # A strip shorter and narrower than its segment: what it has, and the
    # rest of the strip buffer as the strip before left it.
    short = jpeg_strips(base, 8, subsampling=2)
    short[1] = jpeg_of(base.crop((0, 8, 21, 13)), subsampling=2)
    out["jpeg_ycbcr420_short_strip"] = jpeg_tiff(base, 6, short, rows=8, extra=sub)
    # The wrong number of components, and BitsPerSample the JPEG does not have.
    out["jpeg_ycbcr_grey_strips_refused"] = jpeg_tiff(
        base, 6, jpeg_strips(grey, 8), rows=8, extra=sub)
    out["jpeg_ycbcr_16_bits_refused"] = jpeg_tiff(
        base, 6, jpeg_strips(base, 8, subsampling=2), rows=8, extra=sub + [(BITS, SHORT, [16, 16, 16])])
    # A datastream with a second scan after its only one: once every row is
    # read jpeg_finish_decompress fails on it -- and libtiff reads the strip
    # all the same, since it returns `rows_left || finish()` and a C `||` is
    # 1 for finish's -1.
    def second_scan(data: bytes) -> bytes:
        at = data.rfind(b"\xff\xd9")
        sos = data.find(b"\xff\xda")
        header_len = int.from_bytes(data[sos + 2:sos + 4], "big")
        return data[:at] + data[sos:sos + 2 + header_len] + b"\x00" * 4 + data[at:]
    out["jpeg_ycbcr420_second_scan"] = jpeg_tiff(
        base, 6, [second_scan(c) for c in jpeg_strips(base, 8, subsampling=2)], rows=8, extra=sub)
    tiles = []
    for ty in range(0, 19, 16):
        for tx in range(0, 29, 16):
            tile = Image.new("RGB", (16, 16), (0, 0, 0))
            tile.paste(base.crop((tx, ty, min(29, tx + 16), min(19, ty + 16))), (0, 0))
            tiles.append(jpeg_of(tile, quality=80, subsampling=2))
    tile_entries = [(WIDTH, LONG, [29]), (LENGTH, LONG, [19]), (BITS, SHORT, [8, 8, 8]),
                    (COMPRESSION, SHORT, [7]), (PHOTOMETRIC, SHORT, [6]), (SAMPLES, SHORT, [3]),
                    (TILE_WIDTH, LONG, [16]), (TILE_LENGTH, LONG, [16])] + sub
    tiled = {"offsets_tag": TILE_OFFSETS, "counts_tag": TILE_BYTE_COUNTS}
    out["jpeg_ycbcr420_tiled"] = write_tiff(tile_entries, tiles, **tiled)
    out["jpeg_ycbcr420_tiled_second_scan"] = write_tiff(tile_entries, [second_scan(t) for t in tiles], **tiled)
    # Planes apart: RGB, and YCbCr at 1x1, each plane a greyscale JPEG.
    planes = []
    for band in base.split():
        planes += jpeg_strips(band, 8, quality=85)
    plane_entries = [(WIDTH, LONG, [29]), (LENGTH, LONG, [19]), (BITS, SHORT, [8, 8, 8]),
                     (COMPRESSION, SHORT, [7]), (SAMPLES, SHORT, [3]), (ROWS_PER_STRIP, LONG, [8]),
                     (PLANAR, SHORT, [2])]
    out["jpeg_rgb_separate"] = write_tiff(plane_entries + [(PHOTOMETRIC, SHORT, [2])], planes)
    ycc_planes = []
    for band in base.convert("YCbCr").split():
        ycc_planes += jpeg_strips(band, 8, quality=85)
    out["jpeg_ycbcr_separate"] = write_tiff(
        plane_entries + [(PHOTOMETRIC, SHORT, [6]), (YCBCR_SUBSAMPLING, SHORT, [1, 1])], ycc_planes)
    # FillOrder 2 does not reverse a JPEG strip's bits.
    out["jpeg_ycbcr420_fill_order_2"] = jpeg_tiff(
        base, 6, jpeg_strips(base, 8, subsampling=2), rows=8, extra=sub + [(FILL_ORDER, SHORT, [2])])
    # A strip cut short in its scan: libjpeg's grey for what did not arrive.
    # Cut in its tables instead, it never reaches a scan and is refused.
    cut = jpeg_strips(base, 8, subsampling=2)
    scan = cut[1].find(b"\xff\xda")
    cut[1] = cut[1][:scan + (len(cut[1]) - scan) // 2]
    out["jpeg_ycbcr420_cut_strip"] = jpeg_tiff(base, 6, cut, rows=8, extra=sub)
    cut_header = jpeg_strips(base, 8, subsampling=2)
    cut_header[1] = cut_header[1][:cut_header[1].find(b"\xff\xda")]
    out["jpeg_ycbcr420_strip_cut_before_its_scan_refused"] = jpeg_tiff(base, 6, cut_header, rows=8, extra=sub)
    # Tables defined after a strip's scan, before its end: jpeg_finish_decompress
    # reads them, and the next strip -- abbreviated -- is decoded with them.
    # JPEGTables at quality 30; the first strip whole at quality 90, with the
    # quality 60 tables after its scan; the rest abbreviated at quality 60.
    q60_tables = jpeg_of(base, quality=60, subsampling=2, streamtype=1)
    first = q90[0]
    end = first.rfind(b"\xff\xd9")
    first = first[:end] + q60_tables[2:-2] + first[end:]
    q60_abbreviated = jpeg_strips(base, 8, quality=60, subsampling=2, streamtype=2)
    out["jpeg_tables_after_a_scan"] = jpeg_tiff(
        base, 6, [first] + q60_abbreviated[1:], rows=8,
        extra=sub + [(JPEG_TABLES, UNDEFINED, jpeg_of(base, quality=30, subsampling=2, streamtype=1))])
    # Lossless JPEG strips, which libtiff reads through libjpeg-turbo 3 as it
    # reads any other: grey, RGB and CMYK kept as they are, planes apart --
    # and YCbCr, which libjpeg will not convert losslessly, refused.
    out["jpeg_lossless_grey"] = jpeg_tiff(grey, 1, lossless_jpeg_strips(grey, 8, psv=4), rows=8)
    out["jpeg_lossless_rgb"] = jpeg_tiff(base, 2, lossless_jpeg_strips(base, 8, psv=7), rows=8)
    out["jpeg_lossless_cmyk"] = jpeg_tiff(cmyk, 5, lossless_jpeg_strips(cmyk, 8, psv=2), rows=8)
    lossless_planes = []
    for band in base.split():
        lossless_planes += lossless_jpeg_strips(band, 8, psv=6)
    out["jpeg_lossless_rgb_separate"] = write_tiff(plane_entries + [(PHOTOMETRIC, SHORT, [2])],
                                                   lossless_planes)
    out["jpeg_lossless_ycbcr_refused"] = jpeg_tiff(
        base, 6, lossless_jpeg_strips(base.convert("YCbCr"), 8, psv=1), rows=8,
        extra=[(YCBCR_SUBSAMPLING, SHORT, [1, 1])])
    # Pillow's writer: libtiff's own encoder, JPEGTables and abbreviated strips.
    for mode in ("RGB", "L", "CMYK"):
        buf = io.BytesIO()
        base.convert(mode).save(buf, "TIFF", compression="jpeg")
        out[f"pillow_{mode.lower()}_jpeg"] = buf.getvalue()
    return out


NEXT, THUNDERSCAN = 32766, 32809


def next_row(row: list[int], width: int, how: str, rng: random.Random) -> bytes:
    """One row of 2-bit pixels as NeXT codes it: `literal` (the packed row),
    `span` (the row's non-white middle, into a white row), or `runs`."""
    packed = pack_samples(row, 2, "<")
    if how == "literal":
        return b"\x00" + packed
    if how == "span":
        first = next((i for i, b in enumerate(packed) if b != 0xFF), len(packed))
        last = max((i for i, b in enumerate(packed) if b != 0xFF), default=first - 1)
        span = packed[first:last + 1]
        return bytes([0x40]) + struct.pack(">HH", first, len(span)) + span
    out = bytearray()
    x = 0
    while x < width:
        grey = row[x]
        n = 1
        while x + n < width and row[x + n] == grey and n < 63:
            n += 1
        out.append((grey << 6) | n)
        x += n
    return bytes(out)


def next_strips(pixels: list[list[int]], rows: int, hows, rng: random.Random) -> list[bytes]:
    width = len(pixels[0])
    strips = []
    for y in range(0, len(pixels), rows):
        strips.append(b"".join(next_row(pixels[r], width, hows(r), rng)
                               for r in range(y, min(len(pixels), y + rows))))
    return strips


def thunder_row(row: list[int], rng: random.Random, style: str = "best") -> bytes:
    """One row of 4-bit pixels as ThunderScan codes it: runs of the last
    pixel, two- and three-bit deltas from it, raw pixels -- chosen greedily,
    or (`raw`) raw only."""
    out = bytearray()
    last, x, width = 0, 0, len(row)
    two = {0: 0, 1: 1, -1: 3}
    three = {0: 0, 1: 1, 2: 2, 3: 3, -3: 5, -2: 6, -1: 7}
    while x < width:
        if style == "best":
            n = 0
            while x + n < width and row[x + n] == last and n < 63:
                n += 1
            if n >= 2:
                out.append(n)
                x += n
                continue
            if x + 3 <= width:
                a, b, c = row[x] - last, row[x + 1] - row[x], row[x + 2] - row[x + 1]
                if a in two and b in two and c in two:
                    out.append(0x40 | (two[a] << 4) | (two[b] << 2) | two[c])
                    last = row[x + 2]
                    x += 3
                    continue
            if x + 2 <= width:
                a, b = row[x] - last, row[x + 1] - row[x]
                if a in three and b in three:
                    out.append(0x80 | (three[a] << 3) | three[b])
                    last = row[x + 1]
                    x += 2
                    continue
        out.append(0xC0 | row[x])
        last = row[x]
        x += 1
    return bytes(out)


def next_thunder_fixtures() -> dict[str, bytes]:
    """NeXT's 2-bit and ThunderScan's 4-bit codecs, which libtiff decodes and
    cannot write: every code each has, and the edges libtiff's decoders have."""
    rng = random.Random(4242)
    out: dict[str, bytes] = {}

    def grey2(w: int, h: int, seed: int) -> list[list[int]]:
        r = random.Random(seed)
        return [[(x // 5 + y // 3 + (r.random() < 0.2)) % 4 for x in range(w)] for y in range(h)]

    def entries(w, h, bits, compression, photometric=1, rows=4, extra=()):
        return [(WIDTH, LONG, [w]), (LENGTH, LONG, [h]), (BITS, SHORT, [bits]),
                (COMPRESSION, SHORT, [compression]), (PHOTOMETRIC, SHORT, [photometric]),
                (SAMPLES, SHORT, [1]), (ROWS_PER_STRIP, LONG, [rows])] + list(extra)

    pic = grey2(21, 9, 1)
    for how in ("literal", "span", "runs"):
        out[f"next_{how}"] = write_tiff(entries(21, 9, 2, NEXT), next_strips(pic, 4, lambda r, h=how: h, rng))
    out["next_mixed"] = write_tiff(entries(21, 9, 2, NEXT),
                                   next_strips(pic, 4, lambda r: ("literal", "span", "runs")[r % 3], rng))
    # Spans into white rows: mostly white, a stretch of grey.
    white = [[3] * 21 for _ in range(6)]
    for y, row in enumerate(white):
        for x in range(3 + y, 12 + y):
            row[x] = (x + y) % 3
    out["next_span_into_white"] = write_tiff(entries(21, 6, 2, NEXT), next_strips(white, 3, lambda r: "span", rng))
    # Data that stops at a row's start: the rest of the strip stays white,
    # and it reads. Stopping inside a row does not.
    strips = next_strips(pic, 9, lambda r: "runs", rng)
    rows = [next_row(pic[r], 21, "runs", rng) for r in range(9)]
    out["next_data_stops_at_a_row"] = write_tiff(entries(21, 9, 2, NEXT, rows=9), [b"".join(rows[:5])])
    out["next_data_stops_in_a_row_refused"] = write_tiff(entries(21, 9, 2, NEXT, rows=9),
                                                        [b"".join(rows[:5]) + rows[5][:1]])
    # A run longer than the row stops at the width.
    over = bytearray()
    for r in range(9):
        over += bytes([(1 << 6) | 63])
    out["next_run_past_the_width"] = write_tiff(entries(21, 9, 2, NEXT, rows=9), [bytes(over)])
    # A literal row cut short, a span past the row.
    out["next_literal_cut_refused"] = write_tiff(entries(21, 9, 2, NEXT, rows=9), [b"\x00" + b"\x55" * 3])
    out["next_span_past_the_row_refused"] = write_tiff(
        entries(21, 9, 2, NEXT, rows=9), [bytes([0x40]) + struct.pack(">HH", 4, 3) + b"\x11\x22\x33"])
    out["next_4_bits_refused"] = write_tiff(entries(21, 9, 4, NEXT), next_strips(pic, 4, lambda r: "literal", rng))
    out["next_min_is_white"] = write_tiff(entries(21, 9, 2, NEXT, photometric=0),
                                          next_strips(pic, 4, lambda r: "runs", rng))
    out["next_palette"] = write_tiff(entries(21, 9, 2, NEXT, photometric=3, extra=[colour_map(2)]),
                                     next_strips(pic, 4, lambda r: ("runs", "literal")[r % 2], rng))
    # Tiles: rows are still measured by the image's scanline.
    tile_pic = grey2(16, 20, 7)
    tiles = []
    for ty in range(0, 32, 16):
        block = [tile_pic[y] if y < 20 else [3] * 16 for y in range(ty, ty + 16)]
        tiles.append(b"".join(next_row(block[r], 16, "runs", rng) for r in range(16)))
    tile_entries = [(WIDTH, LONG, [16]), (LENGTH, LONG, [20]), (BITS, SHORT, [2]),
                    (COMPRESSION, SHORT, [NEXT]), (PHOTOMETRIC, SHORT, [1]), (SAMPLES, SHORT, [1]),
                    (TILE_WIDTH, LONG, [16]), (TILE_LENGTH, LONG, [16])]
    tiled = {"offsets_tag": TILE_OFFSETS, "counts_tag": TILE_BYTE_COUNTS}
    out["next_tiled"] = write_tiff(tile_entries, tiles, **tiled)
    wide = grey2(32, 16, 8)
    wide_tiles = []
    for tx in range(0, 32, 16):
        block = [row[tx:tx + 16] for row in wide]
        wide_tiles.append(b"".join(next_row(block[r], 16, "literal", rng) for r in range(16)))
    wide_entries = [(WIDTH, LONG, [32]), (LENGTH, LONG, [16]), (BITS, SHORT, [2]),
                    (COMPRESSION, SHORT, [NEXT]), (PHOTOMETRIC, SHORT, [1]), (SAMPLES, SHORT, [1]),
                    (TILE_WIDTH, LONG, [16]), (TILE_LENGTH, LONG, [16])]
    out["next_tiled_in_a_wider_image"] = write_tiff(wide_entries, wide_tiles, **tiled)
    odd_entries = [e if e[0] != WIDTH else (WIDTH, LONG, [40]) for e in wide_entries]
    out["next_tiled_fractional_scanlines_refused"] = write_tiff(odd_entries, wide_tiles * 2 + wide_tiles[:1],
                                                                **tiled)

    # ThunderScan.
    def grey4(w: int, h: int, seed: int, flat: float) -> list[list[int]]:
        r = random.Random(seed)
        rows = []
        for y in range(h):
            row, v = [], (y * 3) % 16
            for x in range(w):
                if r.random() > flat:
                    v = max(0, min(15, v + r.choice([-3, -2, -1, 1, 2, 3, 7, -7])))
                row.append(v)
            rows.append(row)
        return rows

    def thunder(pic, rows_per_strip=4, style="best", extra=(), photometric=1, bits=4):
        w, h = len(pic[0]), len(pic)
        strips = [b"".join(thunder_row(pic[r], rng, style) for r in range(y, min(h, y + rows_per_strip)))
                  for y in range(0, h, rows_per_strip)]
        return entries(w, h, bits, THUNDERSCAN, photometric, rows_per_strip, extra), strips

    smooth = grey4(23, 11, 3, 0.1)
    flat = grey4(23, 11, 4, 0.8)
    out["thunder_raw"] = write_tiff(*thunder(smooth, style="raw"))
    out["thunder_deltas"] = write_tiff(*thunder(smooth))
    out["thunder_runs"] = write_tiff(*thunder(flat))
    out["thunder_one_strip"] = write_tiff(*thunder(flat, rows_per_strip=11))
    out["thunder_palette"] = write_tiff(*thunder(flat, photometric=3, extra=[colour_map(4)]))
    # A run that starts on an odd pixel, and runs of odd length.
    odd = [[5, 5, 5, 9, 9, 9, 9, 9, 2, 2, 2, 2, 7] for _ in range(4)]
    out["thunder_odd_runs"] = write_tiff(*thunder(odd))
    # The data cut short: the last row does not finish, and fails.
    e, strips = thunder(flat, rows_per_strip=11)
    out["thunder_cut_refused"] = write_tiff(e, [strips[0][:-2]])
    # A run that overfills its row.
    e, strips = thunder([[4] * 10 for _ in range(4)])
    out["thunder_run_overfills_refused"] = write_tiff(e, [bytes([0xC4, 20]) + strips[0][2:]])
    # Trailing bytes after the last row are ignored.
    e, strips = thunder(flat, rows_per_strip=11)
    out["thunder_trailing_bytes"] = write_tiff(e, [strips[0] + b"\xC1\xC2\xC3"])
    out["thunder_8_bits_refused"] = write_tiff(*thunder(flat, bits=8))
    # No tile decoder.
    tile_pic4 = grey4(16, 16, 5, 0.5)
    tile_strip = b"".join(thunder_row(tile_pic4[r], rng) for r in range(16))
    t_entries = [(WIDTH, LONG, [16]), (LENGTH, LONG, [16]), (BITS, SHORT, [4]),
                 (COMPRESSION, SHORT, [THUNDERSCAN]), (PHOTOMETRIC, SHORT, [1]), (SAMPLES, SHORT, [1]),
                 (TILE_WIDTH, LONG, [16]), (TILE_LENGTH, LONG, [16])]
    out["thunder_tiled_refused"] = write_tiff(t_entries, [tile_strip], **tiled)
    return out


JPEG_PROC, JPEG_IF, JPEG_IF_LENGTH, JPEG_RESTART = 512, 513, 514, 515
JPEG_Q_TABLES, JPEG_DC_TABLES, JPEG_AC_TABLES = 519, 520, 521


def jpeg_parts(data: bytes) -> tuple[list[tuple[int, bytes]], bytes]:
    """A JPEG's marker segments up to its first scan, as (marker, payload),
    and its entropy-coded data -- from the end of the SOS header to its EOI."""
    segments, at = [], 2
    while True:
        marker = data[at + 1]
        length = struct.unpack(">H", data[at + 2:at + 4])[0]
        segments.append((marker, data[at + 4:at + 2 + length]))
        at += 2 + length
        if marker == 0xDA:
            break
    end = data.rfind(b"\xff\xd9")
    return segments, data[at:end]


def split_restarts(scan: bytes) -> list[bytes]:
    """Entropy-coded data cut at its restart markers, which are dropped: old
    JPEG in TIFF keeps one restart interval a strip, and libtiff puts the
    markers back between them."""
    pieces, start, at = [], 0, 0
    while at < len(scan) - 1:
        if scan[at] == 0xFF and 0xD0 <= scan[at + 1] <= 0xD7:
            pieces.append(scan[start:at])
            start = at + 2
            at += 2
        else:
            at += 1
    pieces.append(scan[start:])
    return pieces


def ojpeg_tables(segments: list[tuple[int, bytes]]) -> tuple[list[bytes], list[bytes], list[bytes]]:
    """The quantisation tables (64 bytes each, zig-zag, as JPEGQTables points
    to them) and the DC and AC Huffman tables (16 counts then the values, as
    JPEGDCTables and JPEGACTables point to them), by table number."""
    q, dc, ac = {}, {}, {}
    for marker, payload in segments:
        at = 0
        while marker == 0xDB and at < len(payload):
            q[payload[at] & 15] = payload[at + 1:at + 65]
            at += 65
        while marker == 0xC4 and at < len(payload):
            kind, number = payload[at] >> 4, payload[at] & 15
            count = sum(payload[at + 1:at + 17])
            table = payload[at + 1:at + 17 + count]
            (ac if kind else dc)[number] = table
            at += 17 + count
    return ([q[n] for n in sorted(q)], [dc[n] for n in sorted(dc)], [ac[n] for n in sorted(ac)])


def segment(marker: int, payload: bytes) -> bytes:
    """A JPEG marker segment."""
    return bytes([0xFF, marker]) + struct.pack(">H", len(payload) + 2) + payload


def ojpeg_tiff(img: Image.Image, *, rows: int, subsampling: int, layout: str = "tables",
               photometric: int | None = 6, sampling_tag: list[int] | None = None,
               extra: list[tuple[int, int, object]] | None = None, damage=None,
               big_endian: bool = False, tiles: tuple[int, int] | None = None,
               keep_restarts: bool = False, offsets: bool = True) -> bytes:
    """An old-style JPEG TIFF of `img`, written by Pillow's encoder (libjpeg)
    with a restart interval a strip, laid out one of the ways such files are:

    - `tables`: the tables in JPEGQTables, JPEGDCTables and JPEGACTables, each
      strip one restart interval of bare entropy-coded data;
    - `jif`: the whole JPEG in JPEGInterchangeFormat, the strips pointing
      into its scan;
    - `jif_header`: JPEGInterchangeFormat holding the headers alone, through
      its SOS, the strips the entropy-coded data after it.

    `tiles` (width, length) makes it tiled instead, the tiles in order
    stacked into one JPEG a tile wide, a restart interval a tile.
    `keep_restarts` leaves one strip of the whole scan, restart markers and
    all; `offsets` false leaves StripOffsets and StripByteCounts out.
    """
    grey = img.mode == "L"
    mcu_rows = 8 if grey or subsampling != 2 else 16
    if tiles is not None:
        tw, tl = tiles
        across, down = -(-img.width // tw), -(-img.height // tl)
        tall = Image.new(img.mode, (tw, tl * across * down))
        for r in range(down):
            for c in range(across):
                tall.paste(img.crop((c * tw, r * tl, c * tw + tw, r * tl + tl)), (0, (r * across + c) * tl))
        source, rows = tall, tl
    else:
        source = img
    buf = io.BytesIO()
    options = {"quality": 85}
    if rows < source.height:
        # A restart interval a strip, as the strips of these files are.
        options["restart_marker_rows"] = rows // mcu_rows
    if not grey:
        options["subsampling"] = subsampling
    source.save(buf, "JPEG", **options)
    data = buf.getvalue()
    segments, scan = jpeg_parts(data)
    strips = [scan] if keep_restarts else split_restarts(scan)
    if damage:
        strips = damage(strips)
    spp = 1 if grey else 3
    prefix = bytearray()

    def place(blob: bytes) -> int:
        if len(prefix) % 2:
            prefix.append(0)
        at = 8 + len(prefix)
        prefix.extend(blob)
        return at

    entries = [(WIDTH, LONG, [img.width]), (LENGTH, LONG, [img.height]), (BITS, SHORT, [8] * spp),
               (COMPRESSION, SHORT, [6]), (SAMPLES, SHORT, [spp]), (JPEG_PROC, SHORT, [1])]
    if tiles is not None:
        entries += [(TILE_WIDTH, LONG, [tiles[0]]), (TILE_LENGTH, LONG, [tiles[1]])]
    else:
        entries.append((ROWS_PER_STRIP, LONG, [rows]))
    if photometric is not None:
        entries.append((PHOTOMETRIC, SHORT, [photometric]))
    if sampling_tag is not None:
        entries.append((YCBCR_SUBSAMPLING, SHORT, sampling_tag))
    if layout == "tables":
        q, dc, ac = ojpeg_tables(segments)
        pick = (lambda tables: [tables[0]] * spp if len(tables) == 1 else
                ([tables[0], tables[1], tables[1]] if spp == 3 else [tables[0]]))
        q_at = [place(t) for t in q]
        dc_at = [place(t) for t in dc]
        ac_at = [place(t) for t in ac]
        entries += [(JPEG_Q_TABLES, LONG, pick(q_at)), (JPEG_DC_TABLES, LONG, pick(dc_at)),
                    (JPEG_AC_TABLES, LONG, pick(ac_at))]
    elif layout == "jif":
        at = place(data)
        entries += [(JPEG_IF, LONG, [at]), (JPEG_IF_LENGTH, LONG, [len(data)])]
    elif layout == "jif_header":
        header = data[:len(data) - len(scan) - 2]
        at = place(header)
        entries += [(JPEG_IF, LONG, [at]), (JPEG_IF_LENGTH, LONG, [len(header)])]
    else:
        raise ValueError(layout)
    ours = {tag for tag, _, _ in (extra or [])}
    entries = [e for e in entries if e[0] not in ours] + list(extra or [])
    if not offsets:
        return write_tiff(entries, [], offsets_tag=None, counts_tag=None, prefix=bytes(prefix),
                          big_endian=big_endian)
    if tiles is not None:
        return write_tiff(entries, strips, offsets_tag=TILE_OFFSETS, counts_tag=TILE_BYTE_COUNTS,
                          prefix=bytes(prefix), big_endian=big_endian)
    return write_tiff(entries, strips, prefix=bytes(prefix), big_endian=big_endian)


def ojpeg_planar(img: Image.Image) -> bytes:
    """Planes held apart (PlanarConfiguration 2): one JPEG of three scans, a
    component each, its headers through the first scan in
    JPEGInterchangeFormat and each plane's strip its scan's data -- the second
    and third opening with their own scan header, which libtiff finds by
    reading on from the scan before (`OJPEGReadSecondarySos`)."""
    scans = []
    for plane in img.convert("YCbCr").split():
        buf = io.BytesIO()
        plane.save(buf, "JPEG", quality=85)
        scans.append(jpeg_parts(buf.getvalue()))
    tables = [(m, p) for m, p in scans[0][0] if m in (0xDB, 0xC4)]
    frame = bytes([8]) + struct.pack(">HH", img.height, img.width) + bytes(
        [3, 1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0])

    def sos(component: int) -> bytes:
        return segment(0xDA, bytes([1, component, 0x00, 0, 63, 0]))

    header = b"\xff\xd8" + b"".join(segment(m, p) for m, p in tables) + segment(0xC0, frame) + sos(1)
    strips = [scans[0][1], sos(2) + scans[1][1], sos(3) + scans[2][1]]
    jif = 8
    entries = [(WIDTH, LONG, [img.width]), (LENGTH, LONG, [img.height]), (BITS, SHORT, [8, 8, 8]),
               (COMPRESSION, SHORT, [6]), (PHOTOMETRIC, SHORT, [6]), (SAMPLES, SHORT, [3]),
               (ROWS_PER_STRIP, LONG, [img.height]), (PLANAR, SHORT, [2]),
               (YCBCR_SUBSAMPLING, SHORT, [1, 1]), (JPEG_PROC, SHORT, [1]),
               (JPEG_IF, LONG, [jif]), (JPEG_IF_LENGTH, LONG, [len(header)])]
    return write_tiff(entries, strips, prefix=header)


def ojpeg_fixtures() -> dict[str, bytes]:
    """Old-style JPEG (compression 6), which nothing writes any more and libtiff
    still reads -- scanners and cameras of the 1990s -- in the layouts such
    files come in, and the repairs libtiff makes to them."""
    rng = random.Random(606)
    base = Image.new("RGB", (37, 35))
    base.putdata([((x * 7 + rng.randrange(30)) % 256, (y * 11) % 256, (x * y + rng.randrange(25)) % 256)
                  for y in range(35) for x in range(37)])
    grey = base.convert("L")
    out: dict[str, bytes] = {}
    for name, sub, rows, tag in (("420", 2, 16, [2, 2]), ("422", 1, 8, [2, 1]), ("444", 0, 8, [1, 1])):
        out[f"ojpeg_ycbcr{name}_tables"] = ojpeg_tiff(base, rows=rows, subsampling=sub, sampling_tag=tag)
        out[f"ojpeg_ycbcr{name}_jif"] = ojpeg_tiff(base, rows=rows, subsampling=sub, sampling_tag=tag,
                                                   layout="jif")
        out[f"ojpeg_ycbcr{name}_jif_header"] = ojpeg_tiff(base, rows=rows, subsampling=sub,
                                                          sampling_tag=tag, layout="jif_header")
    # No YCbCrSubsampling: libtiff's 2x2 default, corrected from the JPEG's
    # own frame where there is one.
    out["ojpeg_ycbcr420_no_subsampling_tag"] = ojpeg_tiff(base, rows=16, subsampling=2, layout="jif")
    out["ojpeg_ycbcr422_no_subsampling_tag"] = ojpeg_tiff(base, rows=8, subsampling=1, layout="jif")
    # One strip for the whole picture: no restart interval at all -- or one
    # from the JPEGRestartInterval tag, the strip keeping its markers.
    out["ojpeg_ycbcr420_one_strip"] = ojpeg_tiff(base, rows=35, subsampling=2)
    out["ojpeg_ycbcr420_restart_tag"] = ojpeg_tiff(
        base, rows=16, subsampling=2, keep_restarts=True, extra=[(ROWS_PER_STRIP, LONG, [35]),
                                                                 (JPEG_RESTART, SHORT, [3])])
    # Photometric missing, or RGB: libtiff takes both for YCbCr.
    out["ojpeg_no_photometric"] = ojpeg_tiff(base, rows=16, subsampling=2, photometric=None)
    out["ojpeg_rgb_photometric"] = ojpeg_tiff(base, rows=16, subsampling=2, photometric=2)
    out["ojpeg_grey"] = ojpeg_tiff(grey, rows=8, subsampling=0, photometric=1)
    out["ojpeg_grey_jif"] = ojpeg_tiff(grey, rows=8, subsampling=0, photometric=1, layout="jif")
    # Big-endian: libtiff's byte swap takes the codec's post-decode step's
    # place, so each strip after the first skips a strip's worth of the
    # JPEG, and past its end the buffer's last contents come back.
    out["ojpeg_ycbcr420_big_endian"] = ojpeg_tiff(base, rows=16, subsampling=2, big_endian=True)
    out["ojpeg_grey_big_endian"] = ojpeg_tiff(grey, rows=8, subsampling=0, photometric=1, big_endian=True)
    # Tiles: in one column, a tile wider than the picture; and two across,
    # where libtiff's frame is one column tall and the tiles beyond it keep
    # what the last decoded one left.
    out["ojpeg_ycbcr420_tiled"] = ojpeg_tiff(base, rows=16, subsampling=2, tiles=(48, 16))
    out["ojpeg_ycbcr420_tiles_across"] = ojpeg_tiff(base, rows=16, subsampling=2, tiles=(16, 16))
    # No StripOffsets at all: one strip, its data all in JPEGInterchangeFormat.
    out["ojpeg_no_strip_offsets"] = ojpeg_tiff(base, rows=35, subsampling=2, layout="jif", offsets=False)
    # Planes apart, each a scan of its own; and planes "apart" with one strip,
    # which libtiff reads as planes together.
    out["ojpeg_planar"] = ojpeg_planar(base)
    out["ojpeg_planar_one_strip"] = ojpeg_tiff(base, rows=35, subsampling=2, extra=[(PLANAR, SHORT, [2])])
    # A strip cut short: grey where it ran out. A strip missing its tables.
    out["ojpeg_ycbcr420_cut_strip"] = ojpeg_tiff(
        base, rows=16, subsampling=2, damage=lambda strips: [strips[0][: len(strips[0]) // 2]] + strips[1:])
    out["ojpeg_no_tables_refused"] = ojpeg_tiff(
        base, rows=16, subsampling=2, extra=[(JPEG_Q_TABLES, LONG, [0, 0, 0])])
    return out


SGILOG, SGILOG24 = 34676, 34677
LOGL, LOGLUV = 32844, 32845
# glibc 2.39's `exp` is not correctly rounded for these LogL codes: the
# fixtures carry every one of them.
GLIBC_EXP_EXCEPTIONS = [1446, 1731, 3281, 4127, 7617, 10550, 11288, 14113, 15572, 17888, 18453,
                        19618, 22517, 27488, 29305, 29382, 30029, 30706, 31690, 31814, 32285]


def luv_rle(plane: bytes) -> bytes:
    """One byte plane run-length coded as libtiff's LogLuv encoder codes it:
    runs of 4 or more as `count + 126` and the byte (at most 129), the rest
    as literals of at most 127 bytes after their count."""
    out, i, n = bytearray(), 0, len(plane)
    while i < n:
        j = i
        while j < n and plane[j] == plane[i] and j - i < 129:
            j += 1
        if j - i >= 4:
            out += bytes([j - i + 126, plane[i]])
            i = j
            continue
        k = i
        while k < n and k - i < 127:
            m = k
            while m < n and plane[m] == plane[k] and m - k < 4:
                m += 1
            if m - k >= 4:
                break
            k += 1
        out += bytes([k - i]) + plane[i:k]
        i = k
    return bytes(out)


def luv_rows(values: list[list[int]], kind: str) -> list[bytes]:
    """Each row of stored codes as a LogLuv strip holds it: LogL's two
    planes and LogLuv32's four, each run-length coded, or LogLuv24's three
    bytes a pixel."""
    rows = []
    for row in values:
        if kind == "24":
            rows.append(b"".join(v.to_bytes(3, "big") for v in row))
            continue
        shifts = (8, 0) if kind == "L" else (24, 16, 8, 0)
        rows.append(b"".join(luv_rle(bytes((v >> sh) & 255 for v in row)) for sh in shifts))
    return rows


def luv_codes(w: int, h: int, kind: str, seed: int) -> list[list[int]]:
    """Codes a picture's worth: luminance mostly where the 8-bit reading
    shows it (2^-16 to 2^0), the glibc exceptions, the extremes and signs;
    chroma across the table, and past it for LogLuv24."""
    rng = random.Random(seed)
    specials = GLIBC_EXP_EXCEPTIONS + [0, 1, 0x3dff, 0x3e00, 0x3fff, 0x4000, 0x7fff]
    rows = []
    for y in range(h):
        row = []
        for x in range(w):
            k = y * w + x
            if kind == "24":
                le = specials[k] % 1024 if k < len(specials) else rng.randrange(600, 780)
                ce = rng.randrange(16384) if rng.random() < 0.1 else rng.randrange(16289)
                row.append(le << 14 | ce)
                continue
            if k < len(specials):
                le = specials[k]
            elif rng.random() < 0.05:
                le = rng.randrange(32768)
            else:
                le = rng.randrange(12288, 16500)
            sign = 0x8000 if rng.random() < 0.03 else 0
            if kind == "L":
                row.append(sign | le)
            else:
                ue = rng.randrange(40, 200) if rng.random() < 0.95 else rng.randrange(256)
                ve = rng.randrange(120, 220) if rng.random() < 0.95 else rng.randrange(256)
                row.append((sign | le) << 16 | ue << 8 | ve)
        rows.append(row)
    return rows


def luv_tiff(kind: str, w: int = 29, h: int = 23, *, rows_per_strip: int = 8, seed: int = 1,
             big_endian: bool = False, tiles: tuple[int, int] | None = None,
             extra: list[tuple[int, int, object]] | None = None, damage=None) -> bytes:
    """A LogL (`kind` "L"), LogLuv32 ("32") or LogLuv24 ("24") TIFF of
    synthetic codes, 16-bit signed samples as libtiff writes them."""
    spp = 1 if kind == "L" else 3
    if tiles is None:
        codes = luv_codes(w, h, kind, seed)
        rows = luv_rows(codes, kind)
        chunks = [b"".join(rows[i:i + rows_per_strip]) for i in range(0, h, rows_per_strip)]
    else:
        tw, tl = tiles
        codes = luv_codes(w, h, kind, seed)
        chunks = []
        for ty in range(0, h, tl):
            for tx in range(0, w, tw):
                tile = [[codes[y][x] if y < h and x < w else 0 for x in range(tx, tx + tw)]
                        for y in range(ty, ty + tl)]
                chunks.append(b"".join(luv_rows(tile, kind)))
    if damage:
        chunks = damage(chunks)
    entries = [(WIDTH, LONG, [w]), (LENGTH, LONG, [h]), (BITS, SHORT, [16] * spp),
               (COMPRESSION, SHORT, [SGILOG24 if kind == "24" else SGILOG]),
               (PHOTOMETRIC, SHORT, [LOGL if kind == "L" else LOGLUV]), (SAMPLES, SHORT, [spp]),
               (SAMPLE_FORMAT, SHORT, [2] * spp)]
    if tiles is None:
        entries.append((ROWS_PER_STRIP, LONG, [rows_per_strip]))
    else:
        entries += [(TILE_WIDTH, LONG, [tiles[0]]), (TILE_LENGTH, LONG, [tiles[1]])]
    ours = {tag for tag, _, _ in (extra or [])}
    entries = [e for e in entries if e[0] not in ours] + list(extra or [])
    if tiles is None:
        return write_tiff(entries, chunks, big_endian=big_endian)
    return write_tiff(entries, chunks, big_endian=big_endian, offsets_tag=TILE_OFFSETS,
                      counts_tag=TILE_BYTE_COUNTS)


def luv_overrun() -> bytes:
    """A LogL row whose high plane has a literal three bytes longer than the
    row: libtiff reads what is left over as the low plane's first codes --
    here a run of five, then an empty literal -- before the low plane's own."""
    w, h = 6, 1
    high = bytes([9]) + bytes([0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f]) + bytes([131, 0x40, 0x00])
    low = bytes([2, 0x11, 0x22])
    entries = [(WIDTH, LONG, [w]), (LENGTH, LONG, [h]), (BITS, SHORT, [16]), (COMPRESSION, SHORT, [SGILOG]),
               (PHOTOMETRIC, SHORT, [LOGL]), (SAMPLES, SHORT, [1]), (ROWS_PER_STRIP, LONG, [h])]
    return write_tiff(entries, [high + low])


def luv_fixtures() -> dict[str, bytes]:
    """SGI LogLuv (34676, 34677): Greg Ward Larson's high-dynamic-range
    encodings, which libtiff's RGBA reader turns to 8-bit grey or RGB --
    luminance through glibc's `exp`, colour through CCIR 709 primaries."""
    out: dict[str, bytes] = {}
    out["luv_logl"] = luv_tiff("L")
    out["luv_logl_one_strip"] = luv_tiff("L", rows_per_strip=23, seed=2)
    out["luv_32"] = luv_tiff("32")
    out["luv_32_big_endian"] = luv_tiff("32", seed=3, big_endian=True)
    out["luv_32_tiled"] = luv_tiff("32", 37, 29, tiles=(16, 16), seed=4)
    out["luv_24"] = luv_tiff("24")
    out["luv_24_tiled"] = luv_tiff("24", 37, 29, tiles=(16, 16), seed=5)
    # The RGBA reader asks for 8-bit samples whatever the file says -- but
    # only after checking what it says.
    out["luv_24_bits8"] = luv_tiff("24", seed=6, extra=[(BITS, SHORT, [8, 8, 8])])
    out["luv_32_uint"] = luv_tiff("32", seed=7, extra=[(SAMPLE_FORMAT, SHORT, [1, 1, 1])])
    out["luv_logl_bits32_refused"] = luv_tiff("L", seed=8, extra=[(BITS, SHORT, [32])])
    out["luv_32_float_refused"] = luv_tiff("32", seed=9, extra=[(SAMPLE_FORMAT, SHORT, [3, 3, 3])])
    out["luv_overrun"] = luv_overrun()
    # A strip cut short, LogL of three samples, planes apart, and LogLuv
    # data called RGB: each refused, at a different step.
    out["luv_32_cut_refused"] = luv_tiff("32", seed=10, damage=lambda c: [c[0][: len(c[0]) // 2]] + c[1:])
    out["luv_logl_three_samples_refused"] = luv_tiff(
        "L", seed=11, extra=[(SAMPLES, SHORT, [3]), (BITS, SHORT, [16, 16, 16])])
    out["luv_32_planar_refused"] = luv_tiff("32", seed=12, extra=[(PLANAR, SHORT, [2])])
    out["luv_32_rgb_refused"] = luv_tiff("32", seed=13, extra=[(PHOTOMETRIC, SHORT, [2])])
    return out


def pillow_fixtures() -> dict[str, bytes]:
    """Pictures written by Pillow's writer, which is libtiff's encoder."""
    rng = random.Random(99)
    base = Image.new("RGB", (29, 19))
    base.putdata([(rng.randrange(256), (x * 9) % 256, (y * 13) % 256) for y in range(19) for x in range(29)])
    out: dict[str, bytes] = {}
    modes = {"1": base.convert("1"), "L": base.convert("L"), "P": base.convert("P"), "RGB": base,
             "RGBA": base.convert("RGBA"), "CMYK": base.convert("CMYK"), "LA": base.convert("LA")}
    for mode, img in modes.items():
        for compression in ("raw", "packbits", "tiff_lzw", "tiff_adobe_deflate"):
            if mode == "LA" and compression != "raw":
                continue
            buf = io.BytesIO()
            img.save(buf, "TIFF", compression=compression)
            out[f"pillow_{mode.lower()}_{compression.replace('tiff_', '')}"] = buf.getvalue()
    return out


def main() -> None:
    oracle = build_oracle()
    made = {f"tiff_{name}": data for name, data in
            (fixtures() | pillow_fixtures() | fax_fixtures() | jpeg_fixtures()
             | next_thunder_fixtures() | ojpeg_fixtures() | luv_fixtures()).items()}
    for old in HERE.glob("tiff_*.tif"):
        old.unlink()
    for old in HERE.glob("tiff_*.txt"):
        old.unlink()
    files = []
    for name, data in made.items():
        path = HERE / f"{name}.tif"
        path.write_bytes(data)
        files.append(path)
    answer(oracle, files)
    refused = sum(1 for f in files if f.with_suffix(".txt").read_text().startswith("REFUSED"))
    print(f"{len(files)} fixtures, {refused} refused by libtiff")
    for f in files:
        text = f.with_suffix(".txt").read_text()
        wants_refusal = "refused" in f.stem
        if text.startswith("REFUSED") != wants_refusal:
            print(f"  note: {f.stem}: {text[:160].strip()}")


if __name__ == "__main__":
    main()
