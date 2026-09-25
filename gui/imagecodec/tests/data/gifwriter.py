"""A small GIF writer for fixtures: every block written by hand, so a file can
exercise what an ordinary encoder never produces."""

from __future__ import annotations

import struct
from dataclasses import dataclass, field


class BitWriter:
    """LSB-first, as GIF packs its codes."""

    def __init__(self) -> None:
        self.out = bytearray()
        self.acc = 0
        self.nbits = 0

    def write(self, code: int, size: int) -> None:
        self.acc |= code << self.nbits
        self.nbits += size
        while self.nbits >= 8:
            self.out.append(self.acc & 0xFF)
            self.acc >>= 8
            self.nbits -= 8

    def finish(self) -> bytes:
        if self.nbits:
            self.out.append(self.acc & 0xFF)
            self.acc = 0
            self.nbits = 0
        return bytes(self.out)


def lzw_encode(indices: list[int], min_code_size: int, clear_when_full: bool = True,
               clear_every: int | None = None) -> bytes:
    """GIF's variable-length LZW.

    `clear_when_full=False` keeps coding with the full 4096-entry table and
    12-bit codes instead of sending a clear -- the "deferred clear" the
    specification allows and some encoders use. `clear_every` sends a clear
    code after that many codes, to put clears mid-stream on purpose.
    """
    clear = 1 << min_code_size
    eoi = clear + 1
    out = BitWriter()

    def reset():
        return {bytes([i]): i for i in range(clear)}, eoi + 1, min_code_size + 1

    table, next_code, code_size = reset()
    out.write(clear, code_size)
    w = b""
    emitted = 0
    for idx in indices:
        c = bytes([idx])
        wc = w + c
        if wc in table:
            w = wc
            continue
        out.write(table[w], code_size)
        emitted += 1
        if next_code < 4096:
            table[wc] = next_code
            next_code += 1
            # The decoder adds its entry one code later than the encoder, and
            # widens when its next free code no longer fits: widen here when
            # the code just assigned is the last one the width holds.
            if next_code > (1 << code_size) and code_size < 12:
                code_size += 1
        elif clear_when_full:
            out.write(clear, code_size)
            table, next_code, code_size = reset()
        if clear_every is not None and emitted % clear_every == 0 and next_code < 4096:
            out.write(clear, code_size)
            table, next_code, code_size = reset()
        w = c
    if w:
        out.write(table[w], code_size)
        # The decoder adds its lagging entry on reading that last code, and
        # widens if that entry filled the width -- so the end code goes out
        # at the width the decoder will then read (as giflib's does).
        if next_code >= (1 << code_size) and code_size < 12:
            code_size += 1
    out.write(eoi, code_size)
    return out.finish()


def sub_blocks(data: bytes, size: int = 255) -> bytes:
    out = bytearray()
    for i in range(0, len(data), size):
        chunk = data[i : i + size]
        out.append(len(chunk))
        out += chunk
    out.append(0)
    return bytes(out)


def palette_bytes(colours: list[tuple[int, int, int]]) -> tuple[bytes, int]:
    """The table padded to a power of two, and its size field."""
    n = 2
    bits = 0
    while n < len(colours):
        n *= 2
        bits += 1
    padded = list(colours) + [(0, 0, 0)] * (n - len(colours))
    return b"".join(bytes(c) for c in padded), bits


def interlace_order(height: int) -> list[int]:
    rows = []
    for start, step in ((0, 8), (4, 8), (2, 4), (1, 2)):
        rows.extend(range(start, height, step))
    return rows


@dataclass
class Frame:
    width: int
    height: int
    indices: list[int]
    left: int = 0
    top: int = 0
    palette: list[tuple[int, int, int]] | None = None
    interlace: bool = False
    disposal: int | None = None
    transparent: int | None = None
    delay: int = 0
    min_code_size: int | None = None
    clear_when_full: bool = True
    clear_every: int | None = None
    lzw_override: bytes | None = None


@dataclass
class Gif:
    width: int
    height: int
    frames: list[Frame]
    palette: list[tuple[int, int, int]] | None = None
    version: bytes = b"89a"
    loop: int | None = None
    background: int = 0
    extras: list[bytes] = field(default_factory=list)

    def encode(self) -> bytes:
        out = bytearray(b"GIF" + self.version)
        packed = 0
        gct = b""
        if self.palette is not None:
            gct, bits = palette_bytes(self.palette)
            packed = 0x80 | (7 << 4) | bits
        out += struct.pack("<HHBBB", self.width, self.height, packed, self.background, 0)
        out += gct
        if self.loop is not None:
            out += b"\x21\xff\x0bNETSCAPE2.0\x03\x01" + struct.pack("<H", self.loop) + b"\x00"
        for extra in self.extras:
            out += extra
        for f in self.frames:
            if f.disposal is not None or f.transparent is not None or f.delay:
                flags = ((f.disposal or 0) & 7) << 2
                if f.transparent is not None:
                    flags |= 1
                out += b"\x21\xf9\x04" + struct.pack("<BHB", flags, f.delay, f.transparent or 0) + b"\x00"
            packed = 0
            lct = b""
            if f.palette is not None:
                lct, bits = palette_bytes(f.palette)
                packed = 0x80 | bits
            if f.interlace:
                packed |= 0x40
            out += b"\x2c" + struct.pack("<HHHHB", f.left, f.top, f.width, f.height, packed)
            out += lct
            colours = len(f.palette if f.palette is not None else (self.palette or [(0, 0, 0)] * 2))
            mcs = f.min_code_size
            if mcs is None:
                mcs = max(2, (max(colours, 2) - 1).bit_length())
            order = f.indices
            if f.interlace:
                rows = [f.indices[r * f.width : (r + 1) * f.width] for r in range(f.height)]
                order = [i for r in interlace_order(f.height) for i in rows[r]]
            data = f.lzw_override if f.lzw_override is not None else lzw_encode(
                order, mcs, f.clear_when_full, f.clear_every)
            out.append(mcs)
            out += sub_blocks(data)
        out += b"\x3b"
        return bytes(out)
