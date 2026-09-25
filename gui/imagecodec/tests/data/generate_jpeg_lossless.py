#!/usr/bin/env python3
"""Regenerate the lossless JPEG fixtures next to this script, and their answers.

Lossless JPEG (`SOF3`) is rare -- medical and scientific imaging, mostly --
and nothing common writes it except libjpeg-turbo's own `cjpeg`, which writes
only a few of its layouts. So the fixtures are written by the small encoder
below, which can produce every layout the decoder has to read: all seven
predictors, point transforms, sample precisions from 2 to 16 bits, one to
four components at any sampling, interleaved or each component in a scan of
its own, restart intervals, and the damaged and odd datastreams whose
treatment is the point of porting libjpeg-turbo rather than the standard.

The encoder mirrors libjpeg-turbo's *decoder*: where a restart marker falls
inside an iMCU row -- a component taller than one sample per MCU, in a scan
of its own -- libjpeg resets its predictor for the iMCU row's first row, not
for the row the marker precedes, and the encoder predicts the same way, so
its files decode to the samples that went in. One fixture is written the way
the standard says instead, to hold the decoder to libjpeg's reading of it.

Every answer is libjpeg-turbo 3.1.1's, built from pinned sources as the TIFF
generator builds it (`generate_tiff.py`, whose build this reuses), and read
through the 8-bit interface with the output colour space the crate asks
for: greyscale for greyscale files, RGB for RGB ones, CMYK for CMYK ones
(converted to RGB by Chrome's formula, `sample * K / 255`). An answer is
width, height, then `AARRGGBB` per pixel, or `REFUSED` and libjpeg's reason.
For each file that is not deliberately damaged the script also checks the
answer is the picture that went in -- which is what lossless means, and so
is a second, independent check on the oracle.

Usage
-----

    python gui/imagecodec/tests/data/generate_jpeg_lossless.py
"""

from __future__ import annotations

import os
import pathlib
import struct
import sys

import generate_tiff as tiff

HERE = pathlib.Path(__file__).parent

# ---------------------------------------------------------------------------
# The oracle: libjpeg-turbo 3.1.1's decompressor through the 8-bit interface
# ---------------------------------------------------------------------------

ORACLE_C = r"""
/* Each JPEG named on the command line through libjpeg-turbo's 8-bit
 * interface, as imagecodec drives it: greyscale out for greyscale, RGB for
 * RGB and YCbCr, CMYK for CMYK and YCCK (made RGB as Chrome makes it), 100
 * scans at most, and no jpeg_finish_decompress. Writes <file>.txt beside it:
 * "REFUSED <why>", or width, height and AARRGGBB per pixel. */
#include <setjmp.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "jpeglib.h"

struct err { struct jpeg_error_mgr pub; jmp_buf jb; char msg[JMSG_LENGTH_MAX]; };

static void error_exit(j_common_ptr c) {
  struct err *e = (struct err *)c->err;
  (*c->err->format_message)(c, e->msg);
  longjmp(e->jb, 1);
}

static void output_message(j_common_ptr c) { (void)c; }

static void monitor(j_common_ptr c) {
  j_decompress_ptr d = (j_decompress_ptr)c;
  if (d->input_scan_number >= 100) {
    struct err *e = (struct err *)c->err;
    snprintf(e->msg, sizeof e->msg, "too many scans");
    longjmp(e->jb, 1);
  }
}

static void decode(const char *path, FILE *out) {
  FILE *f = fopen(path, "rb");
  if (!f) { fprintf(out, "REFUSED cannot open\n"); return; }
  fseek(f, 0, SEEK_END);
  long len = ftell(f);
  fseek(f, 0, SEEK_SET);
  unsigned char *buf = malloc(len > 0 ? (size_t)len : 1);
  size_t got = len > 0 ? fread(buf, 1, (size_t)len, f) : 0;
  fclose(f);
  struct jpeg_decompress_struct cinfo;
  struct err jerr;
  struct jpeg_progress_mgr progress;
  unsigned char *row = NULL;
  cinfo.err = jpeg_std_error(&jerr.pub);
  jerr.pub.error_exit = error_exit;
  jerr.pub.output_message = output_message;
  if (setjmp(jerr.jb)) {
    fprintf(out, "REFUSED %s\n", jerr.msg);
    jpeg_destroy_decompress(&cinfo);
    free(row);
    free(buf);
    return;
  }
  jpeg_create_decompress(&cinfo);
  jpeg_mem_src(&cinfo, buf, (unsigned long)got);
  jpeg_read_header(&cinfo, TRUE);
  int kind;
  switch (cinfo.jpeg_color_space) {
  case JCS_GRAYSCALE: cinfo.out_color_space = JCS_GRAYSCALE; kind = 1; break;
  case JCS_RGB: case JCS_YCbCr: cinfo.out_color_space = JCS_RGB; kind = 3; break;
  case JCS_CMYK: case JCS_YCCK: cinfo.out_color_space = JCS_CMYK; kind = 4; break;
  default:
    snprintf(jerr.msg, sizeof jerr.msg, "unknown colour space");
    longjmp(jerr.jb, 1);
  }
  progress.progress_monitor = monitor;
  cinfo.progress = &progress;
  jpeg_start_decompress(&cinfo);
  unsigned w = cinfo.output_width, h = cinfo.output_height;
  row = malloc((size_t)w * cinfo.output_components);
  /* Buffered, so a failure part way leaves no half answer. */
  unsigned *pixels = malloc((size_t)w * h * sizeof(unsigned) + 1);
  for (unsigned y = 0; y < h; y++) {
    JSAMPROW rp = row;
    if (jpeg_read_scanlines(&cinfo, &rp, 1) != 1) {
      free(pixels);
      snprintf(jerr.msg, sizeof jerr.msg, "short read");
      longjmp(jerr.jb, 1);
    }
    for (unsigned x = 0; x < w; x++) {
      unsigned r, g, b;
      if (kind == 1) { r = g = b = row[x]; }
      else if (kind == 3) { r = row[x * 3]; g = row[x * 3 + 1]; b = row[x * 3 + 2]; }
      else {
        unsigned k = row[x * 4 + 3];
        r = row[x * 4] * k / 255; g = row[x * 4 + 1] * k / 255; b = row[x * 4 + 2] * k / 255;
      }
      pixels[(size_t)y * w + x] = 0xFF000000u | (r << 16) | (g << 8) | b;
    }
  }
  fprintf(out, "%u %u", w, h);
  for (size_t p = 0; p < (size_t)w * h; p++) fprintf(out, " %08X", pixels[p]);
  fprintf(out, "\n");
  free(pixels);
  jpeg_destroy_decompress(&cinfo);
  free(row);
  free(buf);
}

int main(int argc, char **argv) {
  for (int i = 1; i < argc; i++) {
    char out_path[4096];
    snprintf(out_path, sizeof out_path, "%s", argv[i]);
    char *dot = strrchr(out_path, '.');
    if (dot) strcpy(dot, ".txt"); else strcat(out_path, ".txt");
    FILE *out = fopen(out_path, "w");
    if (!out) { perror(out_path); return 1; }
    decode(argv[i], out);
    fclose(out);
  }
  return 0;
}
"""

ORACLE_TAG = "jpegll-1"


def build_oracle() -> str:
    """The oracle as a program, built once beside the TIFF generator's
    libjpeg-turbo; its path where it runs (in WSL on Windows)."""
    tiff_exe = tiff.build_oracle()
    root = tiff_exe.rsplit("/", 1)[0]
    exe = f"{root}/jpegdec-{ORACLE_TAG}"
    if tiff.run(["sh", "-c", f"test -x '{exe}' && echo yes || echo no"]).stdout.decode().strip() == "yes":
        return exe
    src = pathlib.Path(os.environ.get("TEMP", "/tmp")) / f"slateos-jpegdec-{ORACLE_TAG}.c"
    src.write_text(ORACLE_C, newline="\n")
    try:
        where = tiff.to_wsl(src) if tiff.on_windows() else str(src)
        tiff.run(["gcc", "-O2", f"-I{root}/prefix/include", "-o", exe, where,
                  f"{root}/prefix/lib/libjpeg.a", "-lm"])
    finally:
        src.unlink()
    return exe


# ---------------------------------------------------------------------------
# Writing lossless JPEGs
# ---------------------------------------------------------------------------


def optimal_table(frequencies: dict[int, int]) -> tuple[list[int], list[int]]:
    """`jpeg_gen_optimal_table`: code lengths for the symbols' frequencies, at
    most 16 bits, one code point kept back so no code is all ones. Returns
    `BITS` (index 1 to 16) and `HUFFVAL`."""
    freq = [0] * 257
    for symbol, count in frequencies.items():
        freq[symbol] = count
    freq[256] = 1
    codesize = [0] * 257
    others = [-1] * 257
    while True:
        c1, v = -1, None
        for i in range(257):
            if freq[i] and (v is None or freq[i] <= v):
                v, c1 = freq[i], i
        c2, v = -1, None
        for i in range(257):
            if freq[i] and i != c1 and (v is None or freq[i] <= v):
                v, c2 = freq[i], i
        if c2 < 0:
            break
        freq[c1] += freq[c2]
        freq[c2] = 0
        codesize[c1] += 1
        while others[c1] >= 0:
            c1 = others[c1]
            codesize[c1] += 1
        others[c1] = c2
        codesize[c2] += 1
        while others[c2] >= 0:
            c2 = others[c2]
            codesize[c2] += 1
    bits = [0] * 33
    for size in codesize:
        if size:
            bits[size] += 1
    for i in range(32, 16, -1):
        while bits[i] > 0:
            j = i - 2
            while bits[j] == 0:
                j -= 1
            bits[i] -= 2
            bits[i - 1] += 1
            bits[j + 1] += 2
            bits[j] -= 1
    i = 16
    while bits[i] == 0:
        i -= 1
    bits[i] -= 1
    values = [s for length in range(1, 33) for s in range(256) if codesize[s] == length]
    return bits[:17], values


def codes_of(bits: list[int], values: list[int]) -> dict[int, tuple[int, int]]:
    """Each symbol's code and length, assigned in order (figure C.2)."""
    codes = {}
    code, p = 0, 0
    for length in range(1, 17):
        for _ in range(bits[length]):
            codes[values[p]] = (code, length)
            code += 1
            p += 1
        code <<= 1
    return codes


class BitWriter:
    """Entropy-coded data: bits most significant first, `FF` stuffed with
    `00`, padded with ones at a restart or the end."""

    def __init__(self) -> None:
        self.out = bytearray()
        self.acc = 0
        self.n = 0

    def put(self, value: int, length: int) -> None:
        for i in range(length - 1, -1, -1):
            self.acc = (self.acc << 1) | ((value >> i) & 1)
            self.n += 1
            if self.n == 8:
                self.out.append(self.acc)
                if self.acc == 0xFF:
                    self.out.append(0)
                self.acc, self.n = 0, 0

    def flush(self) -> None:
        if self.n:
            self.put((1 << (8 - self.n)) - 1, 8 - self.n)


def category(d: int) -> int:
    """SSSS: how many bits the difference takes; 16 is 32768 and no bits."""
    if d == 32768:
        return 16
    return abs(d).bit_length()


def predict(psv: int, ra: int, rb: int, rc: int) -> int:
    """Table H.1, as libjpeg computes it (Python's `>>` floors, as the C's
    arithmetic shift does)."""
    return {
        1: ra,
        2: rb,
        3: rc,
        4: ra + rb - rc,
        5: ra + ((rb - rc) >> 1),
        6: rb + ((ra - rc) >> 1),
        7: (ra + rb) >> 1,
    }[psv]


def segment(marker: int, payload: bytes) -> bytes:
    return struct.pack(">BBH", 0xFF, marker, len(payload) + 2) + payload


class Component:
    def __init__(self, cid: int, h: int, v: int, plane: list[list[int]]):
        self.id, self.h, self.v, self.plane = cid, h, v, plane


class Picture:
    """A frame's components, each a plane already at its own sampling."""

    def __init__(self, width: int, height: int, precision: int, components: list[Component]):
        self.width, self.height, self.precision = width, height, precision
        self.components = components
        self.max_h = max(c.h for c in components)
        self.max_v = max(c.v for c in components)
        self.imcu_rows = -(-height // self.max_v)
        for c in components:
            c.width = -(-width * c.h // self.max_h)
            c.height = -(-height * c.v // self.max_v)
            assert len(c.plane) == c.height and all(len(r) == c.width for r in c.plane), (
                f"component {c.id}: plane is not {c.width}x{c.height}"
            )


def picture(width: int, height: int, precision: int, sampling: list[tuple[int, int]],
            ids: list[int] | None = None, seed: int = 1) -> Picture:
    """A picture with structure in every component: gradients, a diagonal
    texture, an edge, and noise, so every predictor has something to do."""
    max_h = max(h for h, _ in sampling)
    max_v = max(v for _, v in sampling)
    top = (1 << precision) - 1
    components = []
    state = seed * 2654435761 & 0xFFFFFFFF
    for n, (h, v) in enumerate(sampling):
        w = -(-width * h // max_h)
        ht = -(-height * v // max_v)
        plane = []
        for y in range(ht):
            row = []
            for x in range(w):
                state = (state * 1103515245 + 12345) & 0x7FFFFFFF
                value = (x * (37 + 11 * n) + y * (23 + 7 * n)) % (top + 1)
                if (x + 2 * y + n) % 7 == 0:
                    value = top - value
                if x > w // 2:
                    value = (value + top // 3) % (top + 1)
                value = (value + (state >> 16) % 5 - 2) % (top + 1)
                row.append(value)
            plane.append(row)
        cid = ids[n] if ids else n + 1
        components.append(Component(cid, h, v, plane))
    return Picture(width, height, precision, components)


def scan_differences(pic: Picture, comps: list[int], psv: int, pt: int, restart: int,
                     spec_restarts: bool) -> tuple[list[list[int]], list[int]]:
    """Every MCU row of a scan as its differences, MCU by MCU, sample by
    sample; and which MCU rows a restart marker precedes."""
    interleaved = len(comps) > 1
    if interleaved:
        per_row = -(-pic.width // pic.max_h)
    else:
        per_row = pic.components[comps[0]].width
    assert restart % per_row == 0, "the restart interval must be whole MCU rows"
    rows_per_restart = restart // per_row if restart else 0
    initial = 1 << (pic.precision - pt - 1)
    # The samples the decoder reconstructs: the picture after the point
    # transform, which is what the predictors see.
    shifted = {ci: [[s >> pt for s in row] for row in pic.components[ci].plane] for ci in comps}
    mcu_rows_out: list[list[int]] = []
    restarts: list[int] = []
    rows_to_go = rows_per_restart
    for k in range(pic.imcu_rows):
        last = k == pic.imcu_rows - 1
        if interleaved:
            mcu_rows = 1
        else:
            c = pic.components[comps[0]]
            rem = c.height % c.v
            mcu_rows = (rem or c.v) if last else c.v
        # Which of this iMCU row's MCU rows follow a restart, as the decoder
        # counts them.
        after_restart = []
        for y in range(mcu_rows):
            if restart and rows_to_go == 0:
                after_restart.append(y)
                rows_to_go = rows_per_restart
            if restart:
                rows_to_go -= 1
        # The difference of each real sample, by the decoder's rule for
        # which rows are first rows.
        diffs = {}
        for ci in comps:
            c = pic.components[ci]
            rows = ((c.height % c.v) or c.v) if last else c.v
            reset_first = k == 0 or bool(after_restart)
            for r in range(rows):
                y = k * c.v + r
                if spec_restarts and not interleaved:
                    first = (k == 0 and r == 0) or r in after_restart
                else:
                    first = r == 0 and reset_first
                line = shifted[ci][y]
                above = shifted[ci][y - 1] if y > 0 else None
                for x in range(c.width):
                    if first:
                        pred = initial if x == 0 else line[x - 1]
                    elif psv == 1 or x == 0:
                        pred = above[x] if x == 0 else line[x - 1]
                    else:
                        pred = predict(psv, line[x - 1], above[x], above[x - 1])
                    d = (line[x] - pred) & 0xFFFF
                    diffs[(ci, y, x)] = d - 65536 if d > 32768 else d
        # The MCU rows, in decoding order; dummy samples outside a plane are
        # sent as zero, and ignored.
        for y in range(mcu_rows):
            if y in after_restart:
                restarts.append(len(mcu_rows_out))
            out = []
            for m in range(per_row):
                if interleaved:
                    for ci in comps:
                        c = pic.components[ci]
                        for yy in range(c.v):
                            for xx in range(c.h):
                                out.append(diffs.get((ci, k * c.v + yy, m * c.h + xx), 0))
                else:
                    c = pic.components[comps[0]]
                    out.append(diffs.get((comps[0], k * c.v + y, m), 0))
            mcu_rows_out.append(out)
    return mcu_rows_out, restarts


def lossless_jpeg(pic: Picture, scans: list[dict], restart: int = 0, markers: bytes = b"",
                  spec_restarts: bool = False, sof: int = 0xC3, extra_symbols: tuple = (),
                  corrupt: dict | None = None) -> bytes:
    """A lossless JPEG of `pic`. Each scan is `{"comps": [...], "psv": n,
    "pt": n}`, optionally `"table": n`. `corrupt` replaces the difference at
    (scan, MCU row, position) with another value."""
    frame = struct.pack(">BHHB", pic.precision, pic.height, pic.width, len(pic.components))
    for c in pic.components:
        frame += struct.pack(">BBB", c.id, (c.h << 4) | c.v, 0)
    coded_scans = []
    for number, scan in enumerate(scans):
        rows, restarts = scan_differences(pic, scan["comps"], scan["psv"], scan["pt"], restart,
                                          spec_restarts)
        if corrupt and corrupt.get("scan") == number:
            rows[corrupt["row"]][corrupt["at"]] = corrupt["value"]
        freq: dict[int, int] = {}
        for row in rows:
            for d in row:
                freq[category(d)] = freq.get(category(d), 0) + 1
        for symbol in extra_symbols:
            freq.setdefault(symbol, 1)
        bits, values = optimal_table(freq)
        table = scan.get("table", 0)
        dht = segment(0xC4, bytes([table]) + bytes(bits[1:17]) + bytes(values))
        coded_scans.append((scan, rows, restarts, codes_of(bits, values), table, dht))
    out = b"\xff\xd8" + markers
    if restart:
        out += segment(0xDD, struct.pack(">H", restart))
    out += segment(sof, frame)
    # Each scan's table just before it: a later scan's would replace an
    # earlier one's if they were all sent first.
    for scan, rows, restarts, codes, table, dht in coded_scans:
        out += dht
        header = bytes([len(scan["comps"])])
        for ci in scan["comps"]:
            header += bytes([pic.components[ci].id, table << 4])
        header += bytes([scan["psv"], 0, scan["pt"]])
        out += segment(0xDA, header)
        writer = BitWriter()
        rst = 0
        for n, row in enumerate(rows):
            if n in restarts:
                writer.flush()
                writer.out += bytes([0xFF, 0xD0 + rst])
                rst = (rst + 1) & 7
            for d in row:
                s = category(d)
                code, length = codes[s]
                writer.put(code, length)
                if 0 < s < 16:
                    writer.put(d if d > 0 else d + (1 << s) - 1, s)
        writer.flush()
        out += bytes(writer.out)
    return out + b"\xff\xd9"


def expected(pic: Picture, pt) -> tuple[int, int, list[int]]:
    """What a lossless decode of `pic` is: its samples (after the point
    transform -- one for all components, or one each -- and back), each
    component brought up to full size by repetition, as the pixels the
    oracle writes."""
    w, h = pic.width, pic.height
    comps = pic.components
    pts = pt if isinstance(pt, list) else [pt] * len(comps)

    def sample(n: int, x: int, y: int) -> int:
        c = comps[n]
        v = (c.plane[y * c.v // pic.max_v][x * c.h // pic.max_h] >> pts[n]) << pts[n]
        return v & 0xFF
    pixels = []
    for y in range(h):
        for x in range(w):
            s = [sample(n, x, y) for n in range(len(comps))]
            if len(s) == 1:
                r = g = b = s[0]
            elif len(s) == 3:
                r, g, b = s
            else:
                c, m, yy, k = s
                r, g, b = c * k // 255, m * k // 255, yy * k // 255
            pixels.append(0xFF000000 | (r << 16) | (g << 8) | b)
    return w, h, pixels


# ---------------------------------------------------------------------------
# The fixtures
# ---------------------------------------------------------------------------

ADOBE_RGB = segment(0xEE, b"Adobe" + struct.pack(">HHHB", 100, 0, 0, 0))
ADOBE_YCCK = segment(0xEE, b"Adobe" + struct.pack(">HHHB", 100, 0, 0, 2))
JFIF = segment(0xE0, b"JFIF\x00\x01\x01\x00\x00\x01\x00\x01\x00\x00")


def fixtures() -> dict[str, tuple[bytes, tuple | None]]:
    """Each fixture's bytes, and the decode it must have if it is not a
    damaged one (else None: libjpeg alone says what it is)."""
    out: dict[str, tuple[bytes, tuple | None]] = {}
    grey = picture(13, 11, 8, [(1, 1)])
    rgb = picture(13, 11, 8, [(1, 1)] * 3, seed=2)
    one = lambda psv, pt=0: [{"comps": [0], "psv": psv, "pt": pt}]
    three = lambda psv, pt=0: [{"comps": [0, 1, 2], "psv": psv, "pt": pt}]

    for psv in range(1, 8):
        out[f"jpegll_grey_psv{psv}"] = (lossless_jpeg(grey, one(psv)), expected(grey, 0))
        out[f"jpegll_rgb_psv{psv}"] = (lossless_jpeg(rgb, three(psv), markers=ADOBE_RGB),
                                       expected(rgb, 0))
    for pt in (1, 3, 7):
        out[f"jpegll_grey_pt{pt}"] = (lossless_jpeg(grey, one(6, pt)), expected(grey, pt))
    for precision in (2, 4, 6, 7):
        pic = picture(11, 9, precision, [(1, 1)], seed=precision)
        out[f"jpegll_grey_{precision}bit"] = (lossless_jpeg(pic, one(4)), expected(pic, 0))
    pic = picture(11, 9, 5, [(1, 1)] * 3, seed=5)
    out["jpegll_rgb_5bit_pt2"] = (lossless_jpeg(pic, three(7, 2)), expected(pic, 2))
    for w, h in ((1, 1), (1, 7), (9, 1)):
        pic = picture(w, h, 8, [(1, 1)], seed=w + h)
        out[f"jpegll_grey_{w}x{h}"] = (lossless_jpeg(pic, one(5)), expected(pic, 0))
    # Colour spaces: ids 1, 2, 3 and no marker, which lossless reads as RGB;
    # CMYK; and three that libjpeg will not convert.
    out["jpegll_rgb_no_marker"] = (lossless_jpeg(rgb, three(1)), expected(rgb, 0))
    cmyk = picture(9, 7, 8, [(1, 1)] * 4, seed=4)
    out["jpegll_cmyk"] = (lossless_jpeg(cmyk, [{"comps": [0, 1, 2, 3], "psv": 3, "pt": 0}]),
                          expected(cmyk, 0))
    out["jpegll_rgb_jfif_refused"] = (lossless_jpeg(rgb, three(1), markers=JFIF), None)
    out["jpegll_ycck_refused"] = (
        lossless_jpeg(cmyk, [{"comps": [0, 1, 2, 3], "psv": 3, "pt": 0}], markers=ADOBE_YCCK), None)
    two = picture(9, 7, 8, [(1, 1)] * 2, seed=6)
    out["jpegll_two_components_refused"] = (
        lossless_jpeg(two, [{"comps": [0, 1], "psv": 1, "pt": 0}]), None)
    # Sampling: every component is brought up by repetition.
    for name, sampling in (("h2v2", [(2, 2), (1, 1), (1, 1)]), ("h2v1", [(2, 1), (1, 1), (1, 1)]),
                           ("h1v2", [(1, 2), (1, 1), (1, 1)]), ("h4v1", [(4, 1), (1, 1), (1, 1)]),
                           ("h3v1", [(3, 1), (1, 1), (1, 1)])):
        pic = picture(15, 9, 8, sampling, seed=len(name))
        out[f"jpegll_rgb_{name}"] = (lossless_jpeg(pic, three(4), markers=ADOBE_RGB),
                                     expected(pic, 0))
    frac = picture(15, 9, 8, [(3, 1), (2, 1), (1, 1)], seed=9)
    out["jpegll_fractional_sampling_refused"] = (lossless_jpeg(frac, three(4)), None)
    # A greyscale frame taller than one row per MCU.
    tall = picture(10, 9, 8, [(2, 2)], seed=11)
    out["jpegll_grey_h2v2"] = (lossless_jpeg(tall, one(4)), expected(tall, 0))
    # Components in scans of their own.
    sub = picture(15, 9, 8, [(2, 2), (1, 1), (1, 1)], seed=12)
    separate = [{"comps": [0], "psv": 4, "pt": 0}, {"comps": [1], "psv": 7, "pt": 1},
                {"comps": [2], "psv": 2, "pt": 0}]
    out["jpegll_rgb_separate_scans"] = (lossless_jpeg(sub, separate, markers=ADOBE_RGB),
                                        expected(sub, [0, 1, 0]))
    mixed = [{"comps": [0, 1], "psv": 5, "pt": 0}, {"comps": [2], "psv": 6, "pt": 0}]
    out["jpegll_rgb_two_scans"] = (lossless_jpeg(rgb, mixed, markers=ADOBE_RGB),
                                   expected(rgb, 0))
    out["jpegll_rgb_scan_missing_refused"] = (
        lossless_jpeg(rgb, [{"comps": [0, 1], "psv": 5, "pt": 0}], markers=ADOBE_RGB), None)
    # Restart intervals: every row, every other row; interleaved and not.
    out["jpegll_grey_restarts"] = (lossless_jpeg(grey, one(4), restart=13), expected(grey, 0))
    out["jpegll_rgb_restarts"] = (lossless_jpeg(rgb, three(7), restart=26, markers=ADOBE_RGB),
                                  expected(rgb, 0))
    out["jpegll_grey_h2v2_restarts"] = (lossless_jpeg(tall, one(4), restart=10),
                                        expected(tall, 0))
    # ...and the same, predicted as the standard says: libjpeg resets its
    # predictor for the iMCU row's first row, not the restart's.
    out["jpegll_grey_h2v2_restarts_as_the_standard_says"] = (
        lossless_jpeg(tall, one(4), restart=10, spec_restarts=True), None)
    out["jpegll_bad_restart_interval_refused"] = (lossless_jpeg(grey, one(1), restart=13)
                                                  .replace(b"\xff\xdd\x00\x04\x00\x0d",
                                                           b"\xff\xdd\x00\x04\x00\x07"), None)
    # Damage: cut short, a restart marker missing, a difference of 32768.
    whole = lossless_jpeg(rgb, three(4), markers=ADOBE_RGB)
    out["jpegll_rgb_cut"] = (whole[: len(whole) * 2 // 3], None)
    tall_cut = lossless_jpeg(tall, one(4))
    out["jpegll_grey_h2v2_cut"] = (tall_cut[: len(tall_cut) // 2], None)
    marked = lossless_jpeg(grey, one(4), restart=13)
    at = marked.find(b"\xff\xd2")
    out["jpegll_grey_restart_missing"] = (marked[:at] + marked[at + 2:], None)
    # 32768 leaves a sample's low byte alone; the averaging predictor halves
    # it along the row and down the picture until it shows.
    wide = picture(29, 13, 8, [(1, 1)], seed=13)
    out["jpegll_grey_difference_32768"] = (
        lossless_jpeg(wide, one(7), extra_symbols=(16,),
                      corrupt={"scan": 0, "row": 3, "at": 2, "value": 32768}), None)
    # Refusals.
    out["jpegll_arithmetic_refused"] = (lossless_jpeg(grey, one(1), sof=0xCB), None)
    for precision in (12, 16):
        pic = picture(7, 5, precision, [(1, 1)], seed=precision)
        out[f"jpegll_grey_{precision}bit_refused"] = (lossless_jpeg(pic, one(1)), None)
    for name, (psv, pt) in (("psv0", (0, 0)), ("psv8", (8, 0)), ("pt8", (1, 8))):
        good = lossless_jpeg(grey, one(1))
        sos = good.find(b"\xff\xda")
        params = sos + 2 + 2 + 1 + 2
        bad = good[:params] + bytes([psv, 0, pt]) + good[params + 3:]
        out[f"jpegll_{name}_refused"] = (bad, None)
    no_table = lossless_jpeg(grey, one(1))
    dht = no_table.find(b"\xff\xc4")
    length = struct.unpack(">H", no_table[dht + 2:dht + 4])[0]
    out["jpegll_no_table_refused"] = (no_table[:dht] + no_table[dht + 2 + length:], None)
    return out


def main() -> None:
    oracle = build_oracle()
    made = fixtures()
    paths = []
    for name, (data, _) in made.items():
        path = HERE / f"{name}.jpg"
        path.write_bytes(data)
        paths.append(path)
    tiff.answer(oracle, paths)
    failures = 0
    for name, (_, want) in made.items():
        text = (HERE / f"{name}.txt").read_text()
        refused = text.startswith("REFUSED")
        if name.endswith("_refused") != refused:
            print(f"{name}: {'refused' if refused else 'decoded'} ({text[:60].strip()})")
            failures += 1
            continue
        if want is None or refused:
            continue
        w, h, pixels = want
        words = text.split()
        got = (int(words[0]), int(words[1]), [int(p, 16) for p in words[2:]])
        if got != (w, h, pixels):
            print(f"{name}: libjpeg-turbo does not decode it to the picture that went in")
            failures += 1
    print(f"{len(made)} fixtures, {failures} surprises")
    if failures:
        sys.exit(1)


if __name__ == "__main__":
    main()
