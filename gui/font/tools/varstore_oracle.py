r"""An independent `ItemVariationStore` reader, used as the oracle for the Rust one.

Prints the expectation tables that `gui/font/tests/host_fonts.rs` compares the
`HVAR`/`MVAR`/`GDEF` readers against, for the variable fonts installed on this
host:

    python varstore_oracle.py                 # every table
    python varstore_oracle.py --hvar          # advance-width deltas only
    python varstore_oracle.py --mvar          # global-metric deltas only
    python varstore_oracle.py --shape         # what the stores are made of
    python varstore_oracle.py --fonts DIR     # somewhere other than C:\Windows\Fonts

Why a second implementation exists at all
-----------------------------------------

Written from the OpenType specification, *not* transcribed from the Rust --
which does not exist yet as this is written, deliberately. A delta the two agree
on has then been derived twice from the format description rather than once,
so a misreading has to be made twice, the same way, to survive. This is the
same arrangement `gvar_oracle.py` documents, and the same reason.

One store, three ways in
------------------------

`HVAR`, `MVAR` and `GDEF` all end in the same structure -- an
`ItemVariationStore`, which is a list of design-space *regions* plus a set of
per-item delta rows over those regions. What differs is only how a caller
arrives at the (outer, inner) index pair that names a row:

* **`HVAR`** maps a **glyph id** through a `DeltaSetIndexMap`. If the map
  offset is zero there is no map, and the convention is `outer = 0,
  inner = glyphID` -- not "no delta". A reader that treats a null map as an
  absent feature reports that no advance varies, on a face where every advance
  does.
* **`MVAR`** looks the pair up by a four-byte **value tag** (`hasc`, `xhgt`,
  ...) in a sorted record array.
* **`GDEF`** is handed the pair *directly*, as the first four bytes of a
  `GPOS` `VariationIndex`. That is the case
  `TD-FONT-DOES-NOT-READ-VARIATION-STORES` is named for.

So the store evaluator below is written once and the three entry points are
thin. The Rust is expected to have the same shape, and this tool would catch it
if it did not, because a bug in the shared evaluator moves all three tables.

The two places the format invites a wrong reader
------------------------------------------------

* **A delta row is not a fixed-width array.** `wordDeltaCount`'s low 15 bits
  say how many of the row's leading deltas are stored *wide*; the rest are
  stored narrow. Bit 15 (`LONG_WORDS`) then doubles both widths, so "wide" is
  int16 or int32 and "narrow" is int8 or int16 depending on a bit ten bytes
  earlier. Four combinations, and three of them look plausible on a face that
  happens to use the fourth.
* **A region with a zero peak is skipped, not scored zero.** An axis the region
  does not mention has `peak == 0`, and its factor is **1** -- the region simply
  does not constrain that axis. Reading it as "distance from zero", which is
  what the interpolation formula would give, multiplies every delta by zero and
  yields a store that never varies anything.
"""

import argparse
import collections
import glob
import os
import struct
import sys

from variable_survey import (
    apply_segment_map,
    f2dot14,
    i16,
    normalize_user,
    read_avar_segments,
    read_fvar,
    read_fvar_instances,
    tables,
    u16,
    u32,
)

# `wordDeltaCount` bit 15: doubles the width of both the wide and narrow halves
# of every delta row.
LONG_WORDS = 0x8000
WORD_DELTA_COUNT_MASK = 0x7FFF

# A sane upper bound on a store's subtable count, so a malformed length cannot
# make this allocate on an attacker's word.
MAX_SUBTABLES = 4096

# The `MVAR` value tags this crate would actually consume, with the metric each
# one corrects. `MVAR` defines several dozen; the rest are typographic detail
# no part of this font stack reads, and printing them would bury the ones that
# matter.
MVAR_TAGS_OF_INTEREST = {
    "hasc": "OS/2 sTypoAscender",
    "hdsc": "OS/2 sTypoDescender",
    "hlgp": "OS/2 sTypoLineGap",
    "hcla": "OS/2 usWinAscent",
    "hcld": "OS/2 usWinDescent",
    "xhgt": "OS/2 sxHeight",
    "cpht": "OS/2 sCapHeight",
    "undo": "post underlineOffset",
    "unds": "post underlineThickness",
}


def u8(b, o):
    return b[o]


def i8(b, o):
    return struct.unpack_from(">b", b, o)[0]


def i32(b, o):
    return struct.unpack_from(">i", b, o)[0]


def f2dot14_at(b, o):
    """A region coordinate, as the region list stores it: F2Dot14."""
    return i16(b, o) / 16384.0


# ---------------------------------------------------------------------------
# The store itself.
# ---------------------------------------------------------------------------


class Store:
    """A parsed `ItemVariationStore`: regions, plus subtables of delta rows."""

    def __init__(self, axis_count, regions, subtables):
        self.axis_count = axis_count
        # regions[i] = [(start, peak, end)] * axis_count
        self.regions = regions
        # subtables[i] = (region_indices, [row]) where row is a list of ints
        # parallel to region_indices.
        self.subtables = subtables

    def scalar(self, region, coords):
        """How strongly `region` applies at `coords`, in 0.0 ..= 1.0.

        The product over axes of each axis's factor. Written from the
        specification's own case split rather than condensed, because the
        `peak == 0` case is the one a condensed version loses.
        """
        factor = 1.0
        for axis, (start, peak, end) in enumerate(region):
            coord = coords[axis] if axis < len(coords) else 0.0
            if peak == 0.0:
                # The region does not constrain this axis at all. Factor 1 --
                # NOT a distance from zero.
                continue
            if coord == peak:
                continue
            if coord <= start or coord >= end:
                return 0.0
            if coord < peak:
                # `start < peak` is implied by `start < coord < peak`.
                factor *= (coord - start) / (peak - start)
            else:
                factor *= (end - coord) / (end - peak)
        return factor

    def delta(self, outer, inner, coords):
        """The accumulated delta at row (`outer`, `inner`), or None if unmapped.

        Returned as a float; the caller rounds. Rounding here would round each
        store's contribution separately, which is not what a consumer that sums
        two of them wants.
        """
        if outer >= len(self.subtables):
            return None
        region_indices, rows = self.subtables[outer]
        if inner >= len(rows):
            return None
        row = rows[inner]
        total = 0.0
        for k, region_index in enumerate(region_indices):
            if region_index >= len(self.regions):
                continue
            s = self.scalar(self.regions[region_index], coords)
            if s != 0.0:
                total += s * row[k]
        return total


def read_region_list(data, off, expect_axes):
    """`VariationRegionList` at `off`, or None if malformed."""
    if off + 4 > len(data):
        return None
    axis_count = u16(data, off)
    region_count = u16(data, off + 2)
    if expect_axes is not None and axis_count != expect_axes:
        return None
    size = axis_count * 6
    end = off + 4 + region_count * size
    if size == 0 or end > len(data):
        return None
    regions = []
    for r in range(region_count):
        base = off + 4 + r * size
        regions.append(
            [
                (
                    f2dot14_at(data, base + a * 6),
                    f2dot14_at(data, base + a * 6 + 2),
                    f2dot14_at(data, base + a * 6 + 4),
                )
                for a in range(axis_count)
            ]
        )
    return axis_count, regions


def read_item_variation_data(data, off):
    """One `ItemVariationData` subtable: (region_indices, rows), or None.

    The row layout is the part worth reading twice. `wordDeltaCount`'s low 15
    bits give the number of leading deltas stored in the *wide* form; the
    remaining `regionIndexCount - wordCount` are stored narrow. Bit 15 doubles
    both widths.
    """
    if off + 6 > len(data):
        return None
    item_count = u16(data, off)
    word_delta_count = u16(data, off + 2)
    region_index_count = u16(data, off + 4)
    long_words = bool(word_delta_count & LONG_WORDS)
    word_count = word_delta_count & WORD_DELTA_COUNT_MASK
    if word_count > region_index_count:
        return None

    idx_end = off + 6 + region_index_count * 2
    if idx_end > len(data):
        return None
    region_indices = [u16(data, off + 6 + i * 2) for i in range(region_index_count)]

    wide, narrow = (4, 2) if long_words else (2, 1)
    row_size = word_count * wide + (region_index_count - word_count) * narrow
    if row_size == 0:
        # A subtable over no regions holds no deltas; every row is empty. Legal
        # and useless, but it must not be read as "one byte per row".
        return region_indices, [[] for _ in range(item_count)]
    if idx_end + item_count * row_size > len(data):
        return None

    read_wide = i32 if long_words else i16
    read_narrow = i16 if long_words else i8
    rows = []
    for r in range(item_count):
        base = idx_end + r * row_size
        row = []
        for k in range(region_index_count):
            if k < word_count:
                row.append(read_wide(data, base + k * wide))
            else:
                row.append(
                    read_narrow(data, base + word_count * wide + (k - word_count) * narrow)
                )
        rows.append(row)
    return region_indices, rows


def read_store(data, off, expect_axes=None):
    """An `ItemVariationStore` at `off`, or None if malformed."""
    if off + 8 > len(data):
        return None
    if u16(data, off) != 1:
        return None
    region_list = read_region_list(data, off + u32(data, off + 2), expect_axes)
    if region_list is None:
        return None
    axis_count, regions = region_list
    count = u16(data, off + 6)
    if count > MAX_SUBTABLES or off + 8 + count * 4 > len(data):
        return None
    subtables = []
    for i in range(count):
        rel = u32(data, off + 8 + i * 4)
        sub = read_item_variation_data(data, off + rel) if rel else None
        # A subtable that fails to parse becomes an empty one rather than
        # killing the store: the other subtables are still readable, and a
        # missing delta is a glyph at its default width.
        subtables.append(sub if sub is not None else ([], []))
    return Store(axis_count, regions, subtables)


# ---------------------------------------------------------------------------
# `DeltaSetIndexMap` -- glyph id to (outer, inner).
# ---------------------------------------------------------------------------


class IndexMap:
    def __init__(self, entries, inner_bits):
        self.entries = entries
        self.inner_bits = inner_bits

    def lookup(self, index):
        if not self.entries:
            return None
        # Past the end takes the *last* entry, which is how a face compresses a
        # long tail of glyphs that all share one delta row. Returning None here
        # instead would silently stop varying the back half of the font.
        raw = self.entries[min(index, len(self.entries) - 1)]
        return raw >> self.inner_bits, raw & ((1 << self.inner_bits) - 1)


def read_index_map(data, off):
    """A `DeltaSetIndexMap` at `off`, or None if malformed.

    Format 0 counts entries in a u16, format 1 in a u32; both then pack each
    entry into `entrySize` bytes, big-endian, split into an outer and an inner
    index at a bit position the same byte declares.
    """
    if off + 1 > len(data):
        return None
    fmt = u8(data, off)
    if fmt == 0:
        if off + 4 > len(data):
            return None
        entry_format = u8(data, off + 1)
        map_count = u16(data, off + 2)
        base = off + 4
    elif fmt == 1:
        if off + 6 > len(data):
            return None
        entry_format = u8(data, off + 1)
        map_count = u32(data, off + 2)
        base = off + 6
    else:
        return None

    inner_bits = (entry_format & 0x0F) + 1
    entry_size = ((entry_format & 0x30) >> 4) + 1
    if entry_format & 0xC0:  # reserved bits, must be zero
        return None
    if base + map_count * entry_size > len(data):
        return None
    entries = [
        int.from_bytes(data[base + i * entry_size : base + (i + 1) * entry_size], "big")
        for i in range(map_count)
    ]
    return IndexMap(entries, inner_bits)


# ---------------------------------------------------------------------------
# The three entry points.
# ---------------------------------------------------------------------------


def read_hvar(data, tabs, axis_count):
    """(`Store`, advance `IndexMap` or None) for a face's `HVAR`, or None."""
    if "HVAR" not in tabs:
        return None
    off = tabs["HVAR"][0]
    if off + 20 > len(data):
        return None
    store = read_store(data, off + u32(data, off + 4), axis_count)
    if store is None:
        return None
    rel = u32(data, off + 8)
    # A null mapping offset is not "no variation": it means the implicit
    # identity map, outer 0 / inner glyphID.
    return store, (read_index_map(data, off + rel) if rel else None)


def hvar_advance_delta(store, index_map, gid, coords):
    outer, inner = index_map.lookup(gid) if index_map else (0, gid)
    d = store.delta(outer, inner, coords)
    return 0.0 if d is None else d


def read_mvar(data, tabs, axis_count):
    """(`Store`, {tag: (outer, inner)}) for a face's `MVAR`, or None."""
    if "MVAR" not in tabs:
        return None
    off = tabs["MVAR"][0]
    if off + 12 > len(data):
        return None
    record_size = u16(data, off + 6)
    record_count = u16(data, off + 8)
    rel = u16(data, off + 10)
    if not rel:
        return None
    store = read_store(data, off + rel, axis_count)
    if store is None:
        return None
    if record_size < 8 or off + 12 + record_count * record_size > len(data):
        return None
    records = {}
    for i in range(record_count):
        rec = off + 12 + i * record_size
        records[data[rec : rec + 4].decode("latin-1")] = (
            u16(data, rec + 4),
            u16(data, rec + 6),
        )
    return store, records


def read_gdef_store(data, tabs, axis_count):
    """A face's `GDEF` `ItemVariationStore`, or None.

    The offset is an Offset32 at +14, present only from `GDEF` 1.3. It follows
    two Offset16s that a reader counting fields rather than checking the
    version will run together into a plausible-looking u32.
    """
    if "GDEF" not in tabs:
        return None
    off = tabs["GDEF"][0]
    if off + 18 > len(data):
        return None
    if (u16(data, off), u16(data, off + 2)) < (1, 3):
        return None
    rel = u32(data, off + 14)
    return read_store(data, off + rel, axis_count) if rel else None


# ---------------------------------------------------------------------------
# Face plumbing, shared with the Rust and so validated at the default instance.
# ---------------------------------------------------------------------------


def read_num_glyphs(data, tabs):
    if "maxp" not in tabs:
        return None
    off = tabs["maxp"][0]
    return u16(data, off + 4) if off + 6 <= len(data) else None


def read_hmtx_advance(data, tabs, gid, num_glyphs):
    """The default-instance advance width of `gid`, in font units."""
    if "hhea" not in tabs or "hmtx" not in tabs:
        return None
    hhea = tabs["hhea"][0]
    if hhea + 36 > len(data):
        return None
    num_h = u16(data, hhea + 34)
    if num_h == 0:
        return None
    hmtx = tabs["hmtx"][0]
    # Past `numberOfHMetrics` the advance is the last one; only the side
    # bearing continues per glyph.
    at = hmtx + min(gid, num_h - 1) * 4
    return u16(data, at) if at + 2 <= len(data) else None


def instance_coords(data, tabs):
    """[(label, coords)] for the default and each named instance of a face."""
    parsed = read_fvar(data, *tabs["fvar"])
    if parsed is None:
        return None
    axes, _ = parsed
    segments = read_avar_segments(data, tabs["avar"][0], len(axes)) if "avar" in tabs else None
    out = [("default", [0.0] * len(axes))]
    for _name_id, user in read_fvar_instances(data, *tabs["fvar"], len(axes)):
        coords = []
        for i, axis in enumerate(axes):
            c = f2dot14(normalize_user(axis, user[i]))
            if segments is not None:
                c = apply_segment_map(segments[i], c)
            coords.append(c / 16384.0)
        out.append((", ".join(f"{v:g}" for v in user), coords))
    return len(axes), out


def variable_faces(paths):
    """Yield (name, data, tabs, axis_count, instances) for each variable face."""
    for path in paths:
        try:
            with open(path, "rb") as fh:
                data = fh.read()
        except OSError:
            continue
        tabs = tables(data)
        if tabs is None or "fvar" not in tabs:
            continue
        got = instance_coords(data, tabs)
        if got is None:
            continue
        axis_count, instances = got
        yield os.path.basename(path), data, tabs, axis_count, instances


# ---------------------------------------------------------------------------
# Reports.
# ---------------------------------------------------------------------------


def shape_report(paths):
    """What the stores on this host are actually made of.

    The point is to find out which of the format's four delta-row encodings and
    which index-map entry sizes a real desktop uses, so the Rust reader's tests
    can cover the ones that exist here *and* the ones that do not are known to
    need a synthetic fixture rather than assumed to be covered.
    """
    rows = collections.Counter()
    entry_sizes = collections.Counter()
    inner_bits = collections.Counter()
    null_maps = 0
    faces = 0
    for name, data, tabs, axis_count, _instances in variable_faces(paths):
        faces += 1
        hvar = read_hvar(data, tabs, axis_count)
        if hvar is None:
            print(f"{name}: no readable HVAR")
            continue
        store, index_map = hvar
        if index_map is None:
            null_maps += 1
        else:
            entry_sizes[max(1, (max(index_map.entries, default=0).bit_length() + 7) // 8)] += 1
            inner_bits[index_map.inner_bits] += 1
        for region_indices, subtable_rows in store.subtables:
            if not subtable_rows:
                continue
            rows[(len(region_indices), len(subtable_rows))] += 1
        print(
            f"{name}: {len(store.regions)} regions, "
            f"{len(store.subtables)} subtables, "
            f"map {'implicit' if index_map is None else len(index_map.entries)}"
        )
    print(f"\n{faces} variable faces; {null_maps} with an implicit HVAR index map")
    print("(regionIndexCount, itemCount) seen:")
    for key, n in sorted(rows.items()):
        print(f"  {key}: {n}")
    print(f"index-map inner bit counts: {dict(sorted(inner_bits.items()))}")
    print(f"index-map entry byte widths: {dict(sorted(entry_sizes.items()))}")


def hvar_report(paths, sample):
    """Advance-width deltas for a sample of glyphs, per named instance."""
    for name, data, tabs, axis_count, instances in variable_faces(paths):
        hvar = read_hvar(data, tabs, axis_count)
        if hvar is None:
            continue
        store, index_map = hvar
        num_glyphs = read_num_glyphs(data, tabs) or 0
        gids = [g for g in range(0, num_glyphs, max(1, num_glyphs // sample))][:sample]
        print(f'    ("{name}", &[')
        for label, coords in instances:
            deltas = []
            for gid in gids:
                base = read_hmtx_advance(data, tabs, gid, num_glyphs)
                d = hvar_advance_delta(store, index_map, gid, coords)
                deltas.append("null" if base is None else str(round(base + d)))
            print(f'        &[{", ".join(deltas)}],  // {label}')
        print(f'    ]),  // gids {gids}')


def mvar_report(paths):
    """The `MVAR` corrections this stack would consume, per named instance."""
    for name, data, tabs, axis_count, instances in variable_faces(paths):
        mvar = read_mvar(data, tabs, axis_count)
        if mvar is None:
            continue
        store, records = mvar
        present = [t for t in MVAR_TAGS_OF_INTEREST if t in records]
        if not present:
            print(f"{name}: MVAR carries none of the metrics this stack reads")
            continue
        print(f'    ("{name}", &[  // {", ".join(present)}')
        for label, coords in instances:
            vals = []
            for t in present:
                outer, inner = records[t]
                d = store.delta(outer, inner, coords)
                vals.append("0" if d is None else str(round(d)))
            print(f'        &[{", ".join(vals)}],  // {label}')
        print("    ]),")


def gdef_report(paths, sample):
    """The `GDEF` store's rows -- the ones a `GPOS` `VariationIndex` names.

    A `VariationIndex` carries its (outer, inner) pair directly, so unlike
    `HVAR` there is no mapping step to get wrong; what this checks is the
    shared evaluator against rows that are reached the third way. Printed for a
    sample of rows rather than for the indices real `GPOS` tables use, because
    those are spread across thousands of anchors and any row exercises the same
    arithmetic.
    """
    for name, data, tabs, axis_count, instances in variable_faces(paths):
        store = read_gdef_store(data, tabs, axis_count)
        if store is None:
            print(f"{name}: no GDEF ItemVariationStore")
            continue
        # Rows with a non-empty delta list, so the sample is not all zeroes.
        pairs = [
            (outer, inner)
            for outer, (region_indices, rows) in enumerate(store.subtables)
            if region_indices
            for inner in range(len(rows))
        ]
        if not pairs:
            print(f"{name}: GDEF store varies nothing")
            continue
        step = max(1, len(pairs) // sample)
        chosen = pairs[::step][:sample]
        print(f'    ("{name}", &[  // rows {chosen}')
        for label, coords in instances:
            vals = [str(round(store.delta(o, i, coords) or 0.0)) for o, i in chosen]
            print(f'        &[{", ".join(vals)}],  // {label}')
        print("    ]),")


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--fonts", default=r"C:\Windows\Fonts")
    ap.add_argument("--hvar", action="store_true", help="advance-width deltas only")
    ap.add_argument("--mvar", action="store_true", help="global-metric deltas only")
    ap.add_argument("--gdef", action="store_true", help="GDEF store rows only")
    ap.add_argument("--shape", action="store_true", help="what the stores are made of")
    ap.add_argument("--sample", type=int, default=8, help="glyphs per face in --hvar")
    args = ap.parse_args()

    paths = []
    for ext in ("ttf", "otf", "ttc", "otc"):
        paths += glob.glob(os.path.join(args.fonts, f"*.{ext}"))
        paths += glob.glob(os.path.join(args.fonts, f"*.{ext.upper()}"))
    paths = sorted(set(paths))
    if not paths:
        print(f"no fonts found under {args.fonts}", file=sys.stderr)
        return 1

    everything = not (args.hvar or args.mvar or args.gdef or args.shape)
    if args.shape or everything:
        shape_report(paths)
    if args.hvar or everything:
        print("\n// HVAR: advance widths per named instance")
        hvar_report(paths, args.sample)
    if args.mvar or everything:
        print("\n// MVAR: global metric corrections per named instance")
        mvar_report(paths)
    if args.gdef or everything:
        print("\n// GDEF: store rows a GPOS VariationIndex names")
        gdef_report(paths, args.sample)
    return 0


if __name__ == "__main__":
    sys.exit(main())
