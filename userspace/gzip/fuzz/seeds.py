"""Seed corpora for gzip's targets: `scripts/fuzz.py` calls `seed(src, corpus)`.

The decoders are seeded with one small file of each format gzip reads --
made here, since none is checked in -- and the round trip with text and
bytes of a few shapes. See README.md."""
import os
import struct
import zlib


def member(data, flags=0, extra=None, name=None, comment=None, hcrc=False):
    """A gzip member, with whichever header fields are asked for."""
    if hcrc:
        flags |= 2
    if extra is not None:
        flags |= 4
    if name is not None:
        flags |= 8
    if comment is not None:
        flags |= 16
    h = bytes([0x1F, 0x8B, 8, flags]) + struct.pack("<I", 981173106) + bytes([0, 3])
    if extra is not None:
        h += struct.pack("<H", len(extra)) + extra
    if name is not None:
        h += name + b"\0"
    if comment is not None:
        h += comment + b"\0"
    if hcrc:
        h += struct.pack("<H", zlib.crc32(h) & 0xFFFF)
    c = zlib.compressobj(9, zlib.DEFLATED, -15)
    return h + c.compress(data) + c.flush() + struct.pack("<II", zlib.crc32(data), len(data))


def seed(src, corpus):
    text = b"hello, world\n" * 40
    samples = {
        "plain.gz": member(text),
        "fields.gz": member(text, extra=b"AB\x02\x00hi", name=b"n.txt", comment=b"c", hcrc=True),
        "two.gz": member(text) + member(b"second\n"),
        "garbage.gz": member(text) + b"trailing",
        "pack.z": bytes.fromhex("1f1e000000060301010061 6e6216c8".replace(" ", "")),
        "lzw.Z": bytes.fromhex("1f9d90") + b"h\x00\x00",
        "lzh": b"\x1f\xa0" + bytes(range(64)),
        "zip": b"PK\x03\x04" + bytes(26) + zlib.compress(text)[2:-4],
        "plain-text": text,
    }
    out = corpus("decompress")
    for name, data in samples.items():
        with open(os.path.join(out, name), "wb") as f:
            f.write(data)
    out = corpus("roundtrip")
    for i, data in enumerate([text, bytes(range(256)) * 300, b"\0" * 70000, os.urandom(5000)]):
        with open(os.path.join(out, "r%d" % i), "wb") as f:
            f.write(bytes([i * 3]) + data)
