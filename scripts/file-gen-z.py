#!/usr/bin/env python3
"""Generate compressed files for testing `file -z` (libmagic's compress.c).

    python3 scripts/file-gen-z.py OUTDIR SEED

Writes gzip (three levels, every header flag), bzip2, xz, LZMA, zlib (with and
without a preset dictionary), zip and compress(1) files of a dozen payloads,
then truncations, bit flips and random garbage behind valid headers -- which
reach every message zlib's inflate gives -- and files for the decompressors
that are not installed. Used by scripts/file-diff.sh.
"""
import gzip
import bz2
import lzma
import io
import os
import random
import struct
import subprocess
import sys
import zipfile
import zlib

OUT, SEED = sys.argv[1], int(sys.argv[2])
os.makedirs(OUT, exist_ok=True)
R = random.Random(SEED)

payloads = {
    "text": b"hello world\n" * 50,
    "short": b"hi\n",
    "empty": b"",
    "elf": open("/usr/bin/true", "rb").read(),
    "tar": None,
    "rand": bytes(R.randrange(256) for _ in range(3000)),
    "longline": b"x" * 5000,
    "utf8": "héllo wörld ünïcödé\n".encode() * 20,
    "png": open("/usr/share/pixmaps/debian-logo.png", "rb").read() if os.path.exists("/usr/share/pixmaps/debian-logo.png") else b"\x89PNG\r\n\x1a\n" + b"\0" * 40,
    "script": b"#!/bin/sh\necho hi\n",
    "gz_inner": gzip.compress(b"nested text\n"),
    "zeros": b"\0" * 70000,
}
tbuf = io.BytesIO()
import tarfile
with tarfile.open(fileobj=tbuf, mode="w") as t:
    ti = tarfile.TarInfo("a.txt")
    data = b"tar member\n"
    ti.size = len(data)
    t.addfile(ti, io.BytesIO(data))
payloads["tar"] = tbuf.getvalue()

files = {}
for name, p in payloads.items():
    for lvl in (1, 6, 9):
        files["%s.l%d.gz" % (name, lvl)] = gzip.compress(p, compresslevel=lvl, mtime=0)
    files[name + ".bz2"] = bz2.compress(p)
    files[name + ".xz"] = lzma.compress(p)
    files[name + ".lzma"] = lzma.compress(p, format=lzma.FORMAT_ALONE)
    files[name + ".zlib"] = zlib.compress(p)
    files[name + ".raw0.zlib"] = zlib.compress(p, 0)
    zb = io.BytesIO()
    with zipfile.ZipFile(zb, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("member", p)
    files[name + ".zip"] = zb.getvalue()
    zb = io.BytesIO()
    with zipfile.ZipFile(zb, "w", zipfile.ZIP_STORED) as z:
        z.writestr("member", p)
    files[name + ".stored.zip"] = zb.getvalue()

# A zlib stream with a preset dictionary.
c = zlib.compressobj(zdict=b"hello world")
files["dict.zlib"] = c.compress(b"hello world hello\n") + c.flush()
# gzip with every header flag.
body = zlib.compressobj(9, zlib.DEFLATED, -15)
raw = body.compress(b"flagged text\n") + body.flush()
hdr = b"\x1f\x8b\x08" + bytes([0x1e]) + b"\0\0\0\0\0\x03"
hdr += struct.pack("<H", 4) + b"XTRA" + b"name.txt\0" + b"a comment\0" + b"\xab\xcd"
files["allflags.gz"] = hdr + raw + struct.pack("<II", zlib.crc32(b"flagged text\n"), 13)
files["fextra_short.gz"] = b"\x1f\x8b\x08\x04\0\0\0\0\0\x03\x05"
files["tiny.gz"] = b"\x1f\x8b\x08\x00"
files["hdronly.gz"] = b"\x1f\x8b\x08\x00\0\0\0\0\0\x03"
# compress(1) data, hand-made LZW: 'a', '\n' as 9-bit codes.
files["a.Z"] = b"\x1f\x9d\x90\x61\x14\x00"
files["bad.Z"] = b"\x1f\x9d\x90\xff\xff\xff\xff"
# Programs that are not installed.
files["x.lrz"] = b"LRZI" + b"\0" * 20
files["x.zst"] = b"\x28\xb5\x2f\xfd" + b"\0" * 10
files["x.lz4"] = b"\x04\x22\x4d\x18" + b"\0" * 10
files["x.lz"] = b"LZIP\x01\x0c" + b"\0" * 10
files["frozen"] = b"\x1f\x9e" + b"\0" * 10
files["sco"] = b"\x1f\xa0" + b"\0" * 10
files["packed"] = b"\x1f\x1e" + b"\0" * 10

# Truncations and corruptions of the deflate-based ones.
base = [k for k in files if k.endswith((".gz", ".zlib")) and len(files[k]) > 12]
for k in base:
    d = files[k]
    for n in sorted({len(d) - 1, len(d) - 4, len(d) - 8, len(d) // 2, 12, 11}):
        if 2 <= n < len(d):
            files["%s.cut%d" % (k, n)] = d[:n]
    for j in range(8):
        dd = bytearray(d)
        p = R.randrange(10 if k.endswith(".gz") else 2, len(dd))
        dd[p] ^= 1 << R.randrange(8)
        files["%s.flip%d" % (k, j)] = bytes(dd)
        dd = bytearray(d)
        p = R.randrange(10 if k.endswith(".gz") else 2, len(dd))
        dd[p] = R.randrange(256)
        files["%s.byte%d" % (k, j)] = bytes(dd)
# Random deflate garbage after valid headers.
for j in range(300):
    g = bytes(R.randrange(256) for _ in range(R.randrange(1, 60)))
    files["garbage%d.gz" % j] = b"\x1f\x8b\x08\x00\0\0\0\0\0\x03" + g
    files["garbage%d.zlib" % j] = b"\x78\x9c" + g
for k in [k for k in files if k.endswith((".bz2", ".xz", ".lzma"))][:40]:
    d = files[k]
    files[k + ".cut"] = d[: max(len(d) // 2, 14)]
    dd = bytearray(d)
    p = R.randrange(4, len(dd))
    dd[p] ^= 0x10
    files[k + ".flip"] = bytes(dd)

for k, v in files.items():
    with open(os.path.join(OUT, k), "wb") as f:
        f.write(v)
print(len(files), "files")
