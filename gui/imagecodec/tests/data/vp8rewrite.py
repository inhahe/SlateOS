"""Rewrite the header of a VP8 key frame, keeping its macroblocks.

A VP8 frame's first partition holds its header -- segmentation, loop filter,
quantisers, coefficient probabilities -- and every macroblock's modes; the
coefficients live in the partitions after it. This module decodes the first
partition completely, lets a caller change header fields, and encodes it
again, leaving the coefficient partitions byte for byte as they were. The
result is a valid frame -- the same modes and coefficients, dequantised and
filtered differently -- that no encoder would write.

That is the point: `generate_webp.py` uses it to make fixtures for header
settings no encoder produces (segments by delta, segments with no map or no
values, loop-filter deltas under a real filter level, every quantiser delta),
which are exactly where decoders can disagree, and whose right answer is
therefore whatever libwebp -- through Pillow -- makes of them.

The boolean coder is the RFC's own (RFC 6386 §7.3), and the decoding follows
§19's syntax; `read_frame(write_frame(read_frame(f)))` round-trips.
"""

from __future__ import annotations

import pathlib
import re

HERE = pathlib.Path(__file__).parent
TABLES = HERE.parent.parent / "src" / "webp" / "lossy" / "tables.rs"


def _table(name: str) -> list[int]:
    """A table from the decoder's generated `tables.rs`, flattened."""
    text = TABLES.read_text(encoding="utf-8")
    start = text.index(f"const {name}:")
    body = text[text.index("= [", start) : text.index("\n];", start)]
    return [int(n) for n in re.findall(r"\d+", body)]


COEFF_UPDATE_PROBS = _table("COEFF_UPDATE_PROBS")
_BMODE = _table("KF_BMODE_PROBS")
assert len(COEFF_UPDATE_PROBS) == 1056 and len(_BMODE) == 900


def bmode_probs(above: int, left: int) -> list[int]:
    at = (above * 10 + left) * 9
    return _BMODE[at : at + 9]


class Reader:
    """RFC 6386 §7's boolean decoder."""

    def __init__(self, data: bytes) -> None:
        self.data, self.pos = data, 0
        self.value = 0
        self.range = 255
        self.bit_count = 0
        for _ in range(2):
            self.value = (self.value << 8) | self._byte()

    def _byte(self) -> int:
        b = self.data[self.pos] if self.pos < len(self.data) else 0
        self.pos += 1
        return b

    def bit(self, prob: int) -> int:
        split = 1 + (((self.range - 1) * prob) >> 8)
        big = split << 8
        if self.value >= big:
            bit = 1
            self.range -= split
            self.value -= big
        else:
            bit = 0
            self.range = split
        while self.range < 128:
            self.value <<= 1
            self.range <<= 1
            self.bit_count += 1
            if self.bit_count == 8:
                self.bit_count = 0
                self.value |= self._byte()
        return bit

    def literal(self, n: int) -> int:
        v = 0
        for _ in range(n):
            v = (v << 1) | self.bit(128)
        return v

    def signed(self, n: int) -> int:
        v = self.literal(n)
        return -v if self.bit(128) else v

    def optional(self, n: int) -> int | None:
        """A flagged signed field: None if absent."""
        return self.signed(n) if self.bit(128) else None


class Writer:
    """RFC 6386 §7.3's boolean encoder."""

    def __init__(self) -> None:
        self.out = bytearray()
        self.range = 255
        self.bottom = 0
        self.bit_count = 24

    def _carry(self) -> None:
        i = len(self.out) - 1
        while self.out[i] == 255:
            self.out[i] = 0
            i -= 1
        self.out[i] += 1

    def bit(self, prob: int, value: int) -> None:
        split = 1 + (((self.range - 1) * prob) >> 8)
        if value:
            self.bottom += split
            self.range -= split
        else:
            self.range = split
        while self.range < 128:
            self.range <<= 1
            if self.bottom & (1 << 31):
                self._carry()
            self.bottom = (self.bottom << 1) & 0xFFFFFFFF
            self.bit_count -= 1
            if self.bit_count == 0:
                self.out.append((self.bottom >> 24) & 0xFF)
                self.bottom &= (1 << 24) - 1
                self.bit_count = 8

    def literal(self, n: int, v: int) -> None:
        for i in reversed(range(n)):
            self.bit(128, (v >> i) & 1)

    def signed(self, n: int, v: int) -> None:
        self.literal(n, abs(v))
        self.bit(128, 1 if v < 0 else 0)

    def optional(self, n: int, v: int | None) -> None:
        self.bit(128, 0 if v is None else 1)
        if v is not None:
            self.signed(n, v)

    def finish(self) -> bytes:
        # flush_bool_encoder (§7.3).
        c, v = self.bit_count, self.bottom
        if v & (1 << (32 - c)):
            self._carry()
        v = (v << (c & 7)) & 0xFFFFFFFF
        c >>= 3
        while c > 0:
            v = (v << 8) & 0xFFFFFFFF
            c -= 1
        for _ in range(4):
            self.out.append((v >> 24) & 0xFF)
            v = (v << 8) & 0xFFFFFFFF
        return bytes(self.out)


def _read_subblock_mode(r: Reader, p: list[int]) -> int:
    if not r.bit(p[0]):
        return 0  # DC
    if not r.bit(p[1]):
        return 1  # TM
    if not r.bit(p[2]):
        return 2  # VE
    if not r.bit(p[3]):
        if not r.bit(p[4]):
            return 3  # HE
        return 6 if r.bit(p[5]) else 5  # VR : RD
    if not r.bit(p[6]):
        return 4  # LD
    if not r.bit(p[7]):
        return 7  # VL
    return 9 if r.bit(p[8]) else 8  # HU : HD


# The path through the subblock-mode tree to each mode: (probability index,
# bit) pairs.
_BMODE_PATHS = {
    0: [(0, 0)],
    1: [(0, 1), (1, 0)],
    2: [(0, 1), (1, 1), (2, 0)],
    3: [(0, 1), (1, 1), (2, 1), (3, 0), (4, 0)],
    5: [(0, 1), (1, 1), (2, 1), (3, 0), (4, 1), (5, 0)],
    6: [(0, 1), (1, 1), (2, 1), (3, 0), (4, 1), (5, 1)],
    4: [(0, 1), (1, 1), (2, 1), (3, 1), (6, 0)],
    7: [(0, 1), (1, 1), (2, 1), (3, 1), (6, 1), (7, 0)],
    8: [(0, 1), (1, 1), (2, 1), (3, 1), (6, 1), (7, 1), (8, 0)],
    9: [(0, 1), (1, 1), (2, 1), (3, 1), (6, 1), (7, 1), (8, 1)],
}

# Whole-block luma modes (DC, V, H, TM) and the subblock mode each implies
# for its neighbours' contexts.
_IMPLIED = {0: 0, 1: 2, 2: 3, 3: 1}


def read_frame(frame: bytes) -> dict:
    """Decode a key frame's first partition into a dict of its header fields
    and a list of its macroblocks' modes; keep the rest of the frame whole."""
    tag = frame[0] | frame[1] << 8 | frame[2] << 16
    assert tag & 1 == 0, "not a key frame"
    first = tag >> 5
    width = (frame[6] | frame[7] << 8) & 0x3FFF
    height = (frame[8] | frame[9] << 8) & 0x3FFF
    r = Reader(frame[10 : 10 + first])
    f: dict = {
        "tag_low": tag & 0x1F,
        "size_bytes": frame[6:10],
        "rest": frame[10 + first :],
        "width": width,
        "height": height,
    }
    f["colour_space"] = r.bit(128)
    f["clamping"] = r.bit(128)
    f["segmentation"] = r.bit(128)
    f["update_map"] = f["update_data"] = 0
    f["absolute"] = 1
    f["segment_quant"] = [None] * 4
    f["segment_filter"] = [None] * 4
    f["segment_probs"] = [None] * 3
    if f["segmentation"]:
        f["update_map"] = r.bit(128)
        f["update_data"] = r.bit(128)
        if f["update_data"]:
            f["absolute"] = r.bit(128)
            f["segment_quant"] = [r.optional(7) for _ in range(4)]
            f["segment_filter"] = [r.optional(6) for _ in range(4)]
        if f["update_map"]:
            f["segment_probs"] = [r.literal(8) if r.bit(128) else None for _ in range(3)]
    f["simple"] = r.bit(128)
    f["level"] = r.literal(6)
    f["sharpness"] = r.literal(3)
    f["lf_deltas"] = r.bit(128)
    f["lf_update"] = 0
    f["ref_deltas"] = [None] * 4
    f["mode_deltas"] = [None] * 4
    if f["lf_deltas"]:
        f["lf_update"] = r.bit(128)
        if f["lf_update"]:
            f["ref_deltas"] = [r.optional(6) for _ in range(4)]
            f["mode_deltas"] = [r.optional(6) for _ in range(4)]
    f["log2_parts"] = r.literal(2)
    f["base_q"] = r.literal(7)
    f["quant_deltas"] = [r.optional(4) for _ in range(5)]
    f["refresh_entropy"] = r.bit(128)
    f["prob_updates"] = [r.literal(8) if r.bit(p) else None for p in COEFF_UPDATE_PROBS]
    f["skip_prob"] = r.literal(8) if r.bit(128) else None

    probs = [255 if p is None else p for p in f["segment_probs"]]
    mb_w, mb_h = (width + 15) // 16, (height + 15) // 16
    above = [[0] * 4 for _ in range(mb_w)]
    mbs = []
    for _ in range(mb_h):
        left = [0] * 4
        for mx in range(mb_w):
            mb: dict = {"segment": None, "skip": None}
            if f["segmentation"] and f["update_map"]:
                mb["segment"] = 2 + r.bit(probs[2]) if r.bit(probs[0]) else r.bit(probs[1])
            if f["skip_prob"] is not None:
                mb["skip"] = r.bit(f["skip_prob"])
            if r.bit(145):
                y = (3 if r.bit(128) else 2) if r.bit(156) else (1 if r.bit(163) else 0)
                mb["y"] = y
                above[mx] = [_IMPLIED[y]] * 4
                left = [_IMPLIED[y]] * 4
            else:
                modes = []
                for by in range(4):
                    for bx in range(4):
                        m = _read_subblock_mode(r, bmode_probs(above[mx][bx], left[by]))
                        above[mx][bx] = m
                        left[by] = m
                        modes.append(m)
                mb["y"] = None
                mb["b"] = modes
            mb["uv"] = 0 if not r.bit(142) else 1 if not r.bit(114) else (3 if r.bit(183) else 2)
            mbs.append(mb)
    f["mbs"] = mbs
    return f


def write_frame(f: dict) -> bytes:
    """Encode a frame from `read_frame`'s dict, whatever has been changed in it."""
    w = Writer()
    w.bit(128, f["colour_space"])
    w.bit(128, f["clamping"])
    w.bit(128, f["segmentation"])
    if f["segmentation"]:
        w.bit(128, f["update_map"])
        w.bit(128, f["update_data"])
        if f["update_data"]:
            w.bit(128, f["absolute"])
            for q in f["segment_quant"]:
                w.optional(7, q)
            for level in f["segment_filter"]:
                w.optional(6, level)
        if f["update_map"]:
            for p in f["segment_probs"]:
                w.bit(128, 0 if p is None else 1)
                if p is not None:
                    w.literal(8, p)
    w.bit(128, f["simple"])
    w.literal(6, f["level"])
    w.literal(3, f["sharpness"])
    w.bit(128, f["lf_deltas"])
    if f["lf_deltas"]:
        w.bit(128, f["lf_update"])
        if f["lf_update"]:
            for d in f["ref_deltas"] + f["mode_deltas"]:
                w.optional(6, d)
    w.literal(2, f["log2_parts"])
    w.literal(7, f["base_q"])
    for d in f["quant_deltas"]:
        w.optional(4, d)
    w.bit(128, f["refresh_entropy"])
    for p, update in zip(COEFF_UPDATE_PROBS, f["prob_updates"]):
        w.bit(p, 0 if update is None else 1)
        if update is not None:
            w.literal(8, update)
    w.bit(128, 0 if f["skip_prob"] is None else 1)
    if f["skip_prob"] is not None:
        w.literal(8, f["skip_prob"])

    probs = [255 if p is None else p for p in f["segment_probs"]]
    mb_w = (f["width"] + 15) // 16
    above = [[0] * 4 for _ in range(mb_w)]
    left = [0] * 4
    for i, mb in enumerate(f["mbs"]):
        mx = i % mb_w
        if mx == 0:
            left = [0] * 4
        if f["segmentation"] and f["update_map"]:
            s = mb["segment"] or 0
            w.bit(probs[0], s >> 1)
            w.bit(probs[2] if s >> 1 else probs[1], s & 1)
        if f["skip_prob"] is not None:
            w.bit(f["skip_prob"], mb["skip"] or 0)
        if mb["y"] is not None:
            y = mb["y"]
            w.bit(145, 1)
            w.bit(156, y >> 1)
            if y >> 1:
                w.bit(128, 1 if y == 3 else 0)
            else:
                w.bit(163, y)
            above[mx] = [_IMPLIED[y]] * 4
            left = [_IMPLIED[y]] * 4
        else:
            w.bit(145, 0)
            for k, m in enumerate(mb["b"]):
                bx, by = k % 4, k // 4
                p = bmode_probs(above[mx][bx], left[by])
                for index, bit in _BMODE_PATHS[m]:
                    w.bit(p[index], bit)
                above[mx][bx] = m
                left[by] = m
        uv = mb["uv"]
        w.bit(142, 0 if uv == 0 else 1)
        if uv:
            w.bit(114, 0 if uv == 1 else 1)
            if uv != 1:
                w.bit(183, 1 if uv == 3 else 0)
    first = w.finish()
    assert len(first) < 1 << 19
    tag = f["tag_low"] | len(first) << 5
    return bytes([tag & 0xFF, (tag >> 8) & 0xFF, tag >> 16, 0x9D, 0x01, 0x2A]) + f["size_bytes"] + first + f["rest"]
