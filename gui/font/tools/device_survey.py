"""Which faces on this machine carry `GPOS` device tables, and of what kind.

Device tables are the per-pixel-size corrections a face may hang off any `GPOS`
coordinate: a value record's four fields, and an anchor's x and y. This crate
read past them until 2026-08-16, and the question this answers is the one that
decides how much that mattered — not "does the format exist" but "does anything
on a real desktop use it, and where".

The answer, measured on this machine, is in the module's own output. Two things
in it are worth knowing before reading the code:

* The `deltaFormat` field distinguishes a real hinting device table (`1`, `2`
  or `3`, meaning 2, 4 or 8 bits per delta) from a **`VariationIndex`**
  (`0x8000`), which is the same eight bytes repurposed to index a variable
  font's `ItemVariationStore`. They are counted separately here because they
  are different features that happen to share a slot, and a reader that
  conflates them does not produce a slightly wrong correction — it produces an
  arbitrary one.
* An anchor may be format 3 (that is, may *have* the two offset fields) and
  still write NULL in both. Those are counted as anchors, not as tables.

Usage:

    python gui/font/tools/device_survey.py [--fonts DIR]
"""

import argparse
import collections
import glob
import os
import struct
import sys

# `deltaFormat` values. 1..=3 are bit counts (as a log2); 0x8000 is not a bit
# count at all but a marker that the table is a VariationIndex.
HINTING_FORMATS = (1, 2, 3)
VARIATION_INDEX = 0x8000

# ValueRecord flags whose fields are Device offsets rather than coordinates.
DEVICE_FLAGS = 0x00F0

# GPOS lookup types.
SINGLE_POS, PAIR_POS, CURSIVE_POS = 1, 2, 3
MARK_BASE_POS, MARK_LIG_POS, MARK_MARK_POS = 4, 5, 6
EXTENSION_POS = 9

# A row count past which a face is assumed to be malformed rather than large.
# The largest real BaseArray on this machine is a few thousand rows.
MAX_ROWS = 20000

# Sizes to report the VariationIndex misread hazard at. Three ordinary UI sizes,
# which is where the question matters: nobody ships a hinting correction for a
# 72-pixel heading.
MISREAD_SIZES = (9, 12, 16)


def u16(b, o):
    return struct.unpack_from(">H", b, o)[0]


def u32(b, o):
    return struct.unpack_from(">I", b, o)[0]


def faces(blob):
    """Each face in a file, as a tag -> (offset, length) table directory."""
    offsets = [0]
    if blob[:4] == b"ttcf":
        offsets = [u32(blob, 12 + 4 * i) for i in range(u32(blob, 8))]
    for off in offsets:
        directory = {}
        for i in range(u16(blob, off + 4)):
            record = off + 12 + 16 * i
            directory[bytes(blob[record : record + 4])] = (
                u32(blob, record + 8),
                u32(blob, record + 12),
            )
        yield directory


def value_size(fmt):
    return 2 * bin(fmt & 0xFFFF).count("1")


class Survey:
    """Counts, keyed by where the table was found and what kind it is."""

    def __init__(self, blob):
        self.b = blob
        self.count = collections.Counter()
        # `startSize..endSize` of every real device table, which is the number
        # that decides what size a cross-check has to be run at. It is not a
        # detail: `micross.ttf`'s 130 tables apply at 11 ppem and at no other
        # size at all, so a sweep at 12 reaches none of them and reports
        # agreement that both halves obtained by doing nothing.
        self.sizes = collections.Counter()

    def classify(self, base, offset, where):
        """Record the device table at `base + offset`, if there is one."""
        if not offset:
            return
        try:
            fmt = u16(self.b, base + offset + 4)
        except Exception:  # noqa: BLE001 - a truncated table is not a result
            self.count[f"{where}/unreadable"] += 1
            return
        if fmt in HINTING_FORMATS:
            self.count[f"{where}/device"] += 1
            try:
                lo = u16(self.b, base + offset)
                hi = u16(self.b, base + offset + 2)
            except Exception:  # noqa: BLE001
                return
            self.sizes[(lo, hi)] += 1
            return
        if fmt != VARIATION_INDEX:
            self.count[f"{where}/undefined"] += 1
            return
        self.count[f"{where}/variation"] += 1
        # How dangerous getting the check order wrong would have been: a reader
        # that range-checked before it read the format would treat this table's
        # two variation-store indices as a start and end ppem, and hand back a
        # delta out of the wrong array whenever they happened to bracket the
        # size being drawn at. Count the ones that *would* have bracketed.
        try:
            lo, hi = u16(self.b, base + offset), u16(self.b, base + offset + 2)
        except Exception:  # noqa: BLE001
            return
        for ppem in MISREAD_SIZES:
            if lo <= ppem <= hi:
                self.count[f"misread@{ppem}"] += 1

    def anchor(self, base, offset, where):
        """Record an anchor's two device offsets, if it is format 3."""
        if not offset:
            return
        at = base + offset
        try:
            if u16(self.b, at) != 3:
                return
            x, y = u16(self.b, at + 6), u16(self.b, at + 8)
        except Exception:  # noqa: BLE001
            return
        if not x and not y:
            self.count[f"{where}/null"] += 1
            return
        self.classify(at, x, where)
        self.classify(at, y, where)

    def value_record(self, sub, at, fmt, where):
        """Record a value record's device offsets."""
        if not fmt & DEVICE_FLAGS:
            return
        for bit in (0x0010, 0x0020, 0x0040, 0x0080):
            if not fmt & bit:
                continue
            try:
                offset = u16(self.b, at + value_size(fmt & (bit - 1)))
            except Exception:  # noqa: BLE001
                continue
            self.classify(sub, offset, where)

    def subtable(self, kind, sub):
        b = self.b
        if kind == SINGLE_POS:
            fmt = u16(b, sub + 4)
            if u16(b, sub) == 1:
                self.value_record(sub, sub + 6, fmt, "value:single")
            else:
                n = u16(b, sub + 6)
                for i in range(n):
                    self.value_record(
                        sub, sub + 8 + i * value_size(fmt), fmt, "value:single"
                    )
        elif kind == PAIR_POS:
            # Only whether the *format* names a device field: walking every
            # pair of a class grid to find which cells use it would take far
            # longer than the question is worth, and the format is the thing
            # that decides whether the reader has to handle it at all.
            v1, v2 = u16(b, sub + 4), u16(b, sub + 6)
            if (v1 | v2) & DEVICE_FLAGS:
                self.count["value:pair/format"] += 1
        elif kind == CURSIVE_POS:
            for i in range(u16(b, sub + 4)):
                for k in (0, 2):
                    self.anchor(sub, u16(b, sub + 6 + 4 * i + k), "anchor:cursive")
        elif kind in (MARK_BASE_POS, MARK_LIG_POS, MARK_MARK_POS):
            self.mark_subtable(kind, sub)

    def mark_subtable(self, kind, sub):
        b = self.b
        classes = u16(b, sub + 6)
        mark_array = sub + u16(b, sub + 8)
        to_array = sub + u16(b, sub + 10)
        where = f"anchor:mark{kind}"
        for i in range(u16(b, mark_array)):
            self.anchor(mark_array, u16(b, mark_array + 2 + 4 * i + 2), where)
        if not classes:
            return
        where = f"anchor:base{kind}"
        if kind == MARK_LIG_POS:
            for i in range(min(u16(b, to_array), MAX_ROWS)):
                offset = u16(b, to_array + 2 + 2 * i)
                if not offset:
                    continue
                attach = to_array + offset
                for c in range(min(u16(b, attach), MAX_ROWS)):
                    for k in range(classes):
                        self.anchor(
                            attach, u16(b, attach + 2 + (c * classes + k) * 2), where
                        )
            return
        for i in range(min(u16(b, to_array), MAX_ROWS)):
            for k in range(classes):
                self.anchor(
                    to_array, u16(b, to_array + 2 + (i * classes + k) * 2), where
                )

    def gpos(self, off):
        b = self.b
        lookup_list = off + u16(b, off + 8)
        for i in range(u16(b, lookup_list)):
            try:
                lk = lookup_list + u16(b, lookup_list + 2 + 2 * i)
                kind = u16(b, lk)
                subs = [lk + u16(b, lk + 6 + 2 * j) for j in range(u16(b, lk + 4))]
                if kind == EXTENSION_POS:
                    resolved = []
                    for s in subs:
                        kind = u16(b, s + 2)
                        resolved.append(s + u32(b, s + 4))
                    subs = resolved
                for s in subs:
                    try:
                        self.subtable(kind, s)
                    except Exception:  # noqa: BLE001
                        self.count["subtable/unreadable"] += 1
            except Exception:  # noqa: BLE001
                self.count["lookup/unreadable"] += 1


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--fonts", default=r"C:\Windows\Fonts")
    args = ap.parse_args()

    paths = []
    for ext in ("ttf", "otf", "ttc", "otc"):
        paths += sorted(glob.glob(os.path.join(args.fonts, "*." + ext)))
    if not paths:
        print(f"no fonts under {args.fonts}", file=sys.stderr)
        return 1

    total = collections.Counter()
    sizes = collections.Counter()
    hits = []
    scanned = 0
    for path in paths:
        try:
            blob = open(path, "rb").read()
        except OSError:
            continue
        for directory in faces(blob):
            if b"GPOS" not in directory:
                continue
            scanned += 1
            s = Survey(blob)
            try:
                s.gpos(directory[b"GPOS"][0])
            except Exception:  # noqa: BLE001
                continue
            if s.count:
                hits.append((os.path.basename(path), s.count, s.sizes))
                total.update(s.count)
                sizes.update(s.sizes)

    print(f"{len(paths)} files, {scanned} faces with GPOS")
    real = [(n, c, z) for n, c, z in hits if any("/device" in k for k in c)]
    print(f"{len(real)} faces carry at least one real (hinting) device table")
    for name, c, z in sorted(real):
        shown = {k: v for k, v in sorted(c.items()) if "/device" in k}
        print(f"  {name:24} {shown}")
        print(f"  {'':24} sizes {span(z)}")
    print()
    print("totals across every face:")
    for k, v in sorted(total.items()):
        print(f"  {k:28} {v}")
    print()
    print(f"real device tables apply at: {span(sizes)}")
    return 0


def span(sizes):
    """`startSize..endSize` counts, smallest range first.

    Printed because it is the one number a cross-check needs: a device table
    corrects nothing outside its own range, so a sweep run at a size no face
    names measures the reader by never calling it.
    """
    return ", ".join(f"{lo}-{hi}:{n}" for (lo, hi), n in sorted(sizes.items()))


if __name__ == "__main__":
    raise SystemExit(main())
