"""Which faces on this machine are variable fonts, and which parts of the
variation machinery they actually use.

`TD-FONT-DOES-NOT-READ-VARIATION-STORES` records that this crate declines the
`VariationIndex` arm of a `GPOS` device slot, and that the reason is not a
device-table gap but a missing feature underneath it: variable fonts are not
supported at all. That entry also fixes the order the pieces must arrive in —
`fvar`, then `avar`, then the outlines (`gvar`/`CFF2`), and only then the
`ItemVariationStore` deltas — because a positioning correction for an instance
whose *outlines* are the default instance's moves marks off the shapes they
belong to, which is worse than applying no correction at all.

This answers the question that should come before any of that code: on a real
desktop, how many faces are variable, what axes do they carry, and which of the
optional variation tables do they actually ship? A reader written against the
specification alone will implement five tables of which this host exercises
two.

Three things worth knowing before reading the code:

* **A face can be variable without `gvar`.** `gvar` varies TrueType (`glyf`)
  outlines; a CFF2 face varies through the charstring interpreter instead. A
  face with `fvar` and neither is malformed, and is reported as such rather
  than skipped, since that is the kind of thing a parser meets and must not
  crash on.
* **`avar` is optional and usually absent.** Without it the axis mapping is the
  identity, so a reader that requires `avar` fails on the common case.
* **The `fvar` axis count and the `gvar`/`HVAR` region axis count must agree.**
  They are stored independently, and a mismatch is the first thing a fuzzer
  finds. Counted here so the Rust reader's validation has a real number to be
  measured against rather than an assumed one.

`--normalize` is the part the Rust reader is actually checked against. It
prints, for each named instance of each variable face, the F2Dot14 coordinates
that instance normalizes to *after* `avar` — the numbers pasted into
`installed_variable_fonts_normalize_named_instances` in
`gui/font/tests/host_fonts.rs`.

The endpoints of an axis are the same with and without `avar` (the format
requires the three identity points), so a test that only checks -1/0/+1 passes
on a reader that never found `avar` at all. Named instances are the cheapest
real oracle for the bent interior: they are chosen by the designer, they sit at
the round user-space values where `avar`'s correction is largest, and they are
in the file already. This code is written from the specification rather than
transcribed from `var.rs`, which is the only reason comparing the two means
anything.

Usage:

    python gui/font/tools/variable_survey.py [--fonts DIR] [--verbose]
    python gui/font/tools/variable_survey.py --normalize [--fonts DIR]
"""

import argparse
import collections
import glob
import math
import os
import struct
import sys

# Tables that only exist to serve variation, keyed by what they vary.
VARIATION_TABLES = {
    "fvar": "the axes themselves",
    "avar": "axis value -> normalized coordinate mapping",
    "gvar": "glyf outlines",
    "CFF2": "CFF outlines (varied in the charstring interpreter)",
    "HVAR": "horizontal advances",
    "VVAR": "vertical advances",
    "MVAR": "global metrics (ascender, x-height, ...)",
    "STAT": "style attributes (naming, not geometry)",
}

# A sane upper bound on the axis count. The specification allows 2^16-1; real
# faces carry 1-4, and the largest ever shipped is around 8. A file claiming
# more is malformed, and reading it would allocate on an attacker's word.
MAX_AXES = 64


def u16(b, o):
    return struct.unpack_from(">H", b, o)[0]


def i16(b, o):
    return struct.unpack_from(">h", b, o)[0]


def u32(b, o):
    return struct.unpack_from(">I", b, o)[0]


def fixed(b, o):
    """A 16.16 fixed-point number, as `fvar` stores axis values."""
    return struct.unpack_from(">i", b, o)[0] / 65536.0


def tag(b, o):
    return b[o : o + 4].decode("latin-1")


def tables(data):
    """Map tag -> (offset, length) for one face, or None if not sfnt."""
    if len(data) < 12:
        return None
    sfnt = u32(data, 0)
    if sfnt == 0x74746366:  # 'ttcf' -- a collection; take face 0 only.
        if len(data) < 16:
            return None
        return tables_at(data, u32(data, 12))
    return tables_at(data, 0)


def tables_at(data, base):
    if base + 12 > len(data):
        return None
    sfnt = u32(data, base)
    if sfnt not in (0x00010000, 0x4F54544F):  # 1.0 / 'OTTO'
        return None
    count = u16(data, base + 4)
    out = {}
    for i in range(count):
        rec = base + 12 + i * 16
        if rec + 16 > len(data):
            break
        out[tag(data, rec)] = (u32(data, rec + 8), u32(data, rec + 12))
    return out


def read_fvar(data, off, length):
    """Return (axes, instance_count) where axes is a list of (tag, min, def, max).

    Returns None if the table is too short or claims an implausible axis count;
    the caller reports that as malformed rather than treating it as zero axes,
    because the two are different findings.
    """
    if off + 16 > len(data) or length < 16:
        return None
    axes_off = off + u16(data, off + 4)
    axis_count = u16(data, off + 8)
    axis_size = u16(data, off + 10)
    inst_count = u16(data, off + 12)
    if axis_count == 0 or axis_count > MAX_AXES or axis_size < 20:
        return None
    axes = []
    for i in range(axis_count):
        rec = axes_off + i * axis_size
        if rec + 20 > len(data):
            return None
        axes.append(
            (
                tag(data, rec),
                fixed(data, rec + 4),
                fixed(data, rec + 8),
                fixed(data, rec + 12),
            )
        )
    return axes, inst_count


def read_avar_axis_count(data, off):
    """`avar`'s own axis count, which must equal `fvar`'s."""
    if off + 8 > len(data):
        return None
    return u16(data, off + 6)


def read_fvar_instances(data, off, length, axis_count):
    """Named instances as (subfamily_name_id, [user coordinate per axis])."""
    if off + 16 > len(data) or length < 16:
        return []
    axes_off = off + u16(data, off + 4)
    axis_size = u16(data, off + 10)
    inst_count = u16(data, off + 12)
    inst_size = u16(data, off + 14)
    # A record is nameID + flags + one Fixed per axis, optionally + psNameID.
    if inst_size < 4 + 4 * axis_count:
        return []
    base = axes_off + axis_count * axis_size
    out = []
    for i in range(inst_count):
        rec = base + i * inst_size
        if rec + 4 + 4 * axis_count > len(data):
            break
        out.append(
            (
                u16(data, rec),
                [fixed(data, rec + 4 + 4 * a) for a in range(axis_count)],
            )
        )
    return out


def read_avar_segments(data, off, axis_count):
    """`avar`'s per-axis segment maps as lists of (fromCoord, toCoord) in F2Dot14.

    Returns None when the table disagrees with `fvar` about how many axes there
    are, because pairing curve *k* with axis *k* across a mismatch applies the
    weight correction to the width -- a wrong answer that still looks like a
    font.
    """
    if off + 8 > len(data) or u16(data, off) != 1:
        return None
    if u16(data, off + 6) != axis_count:
        return None
    pos = off + 8
    maps = []
    for _ in range(axis_count):
        if pos + 2 > len(data):
            return None
        n = u16(data, pos)
        pos += 2
        pts = []
        for _ in range(n):
            if pos + 4 > len(data):
                return None
            pts.append((i16(data, pos), i16(data, pos + 2)))
            pos += 4
        # An out-of-order curve cannot be searched; the identity is what the
        # absence of the table would have meant, so use that for this axis.
        if any(pts[j][0] >= pts[j + 1][0] for j in range(len(pts) - 1)):
            pts = []
        maps.append(pts)
    return maps


def normalize_user(axis, value):
    """The specification's normalization: clamp, then scale each side alone."""
    _tag, lo, df, hi = axis
    if value != value:  # NaN
        return 0.0
    value = max(lo, min(hi, value))
    if value < df:
        return -1.0 if df == lo else (value - df) / (df - lo)
    if value > df:
        return 1.0 if hi == df else (value - df) / (hi - df)
    return 0.0


def apply_segment_map(pts, value):
    """Piecewise-linear `avar` correction, in F2Dot14, rounded to nearest.

    The rounding matters: HarfBuzz's `SegmentMaps::map` interpolates in `float`
    and takes `roundf`, so halves go away from zero and a reader that truncates
    disagrees by one unit on some inputs. Reproduced here so a disagreement
    between this and the Rust means a real bug rather than a rounding choice.
    (This function truncated until 2026-08-17, on the same wrong belief the
    Rust held -- see design-decisions.md 448 and commit 777a040ff. Named
    instances mostly land on segment endpoints, where interpolation is exact,
    which is why the survey agreed anyway.)
    """
    if len(pts) < 2:
        return value
    if value <= pts[0][0]:
        return pts[0][1]
    if value >= pts[-1][0]:
        return pts[-1][1]
    i = 1
    while i < len(pts):
        if value == pts[i][0]:
            return pts[i][1]
        if value < pts[i][0]:
            break
        i += 1
    lo_from, lo_to = pts[i - 1]
    hi_from, hi_to = pts[i]
    span = hi_from - lo_from
    if span == 0:
        return lo_to
    rise = hi_to - lo_to
    run = value - lo_from
    prod = rise * run
    # Nearest, halves away from zero -- what `roundf` gives HarfBuzz. Written
    # in integers so it cannot pick up a float rounding of its own: `span` is
    # positive here, so adding half a span before the floor-divide rounds up
    # from the midpoint, and the negative arm is the mirror of that.
    half = span // 2
    if prod >= 0:
        delta = (prod + half) // span
    else:
        delta = -((-prod + half) // span)
    return max(-16384, min(16384, lo_to + delta))


def f2dot14(x):
    """Round a normalized float to F2Dot14, half away from zero as `roundf`."""
    scaled = x * 16384.0
    rounded = math.floor(scaled + 0.5) if scaled >= 0 else math.ceil(scaled - 0.5)
    return max(-16384, min(16384, int(rounded)))


def read_gvar_axis_count(data, off):
    if off + 20 > len(data):
        return None
    return u16(data, off + 4)


def item_variation_store_axis_count(data, off):
    """Axis count of an `ItemVariationStore`'s VariationRegionList."""
    if off + 8 > len(data):
        return None
    fmt = u16(data, off)
    if fmt != 1:
        return None
    region_off = off + u32(data, off + 2)
    if region_off + 4 > len(data):
        return None
    return u16(data, region_off)


def gdef_store_offset(data, off):
    """Offset of `GDEF`'s ItemVariationStore, or None if the version predates it.

    The field is an Offset32 at +14, which is only present from version 1.3.
    It sits after two Offset16s (`markGlyphSetsDef` at +12, itself only from
    1.2) that a reader counting fields rather than versions will run together
    into a plausible-looking u32 -- which is how this first reported faces with
    fifty thousand axes.
    """
    if off + 4 > len(data):
        return None
    if (u16(data, off), u16(data, off + 2)) < (1, 3):
        return None
    if off + 18 > len(data):
        return None
    rel = u32(data, off + 14)
    return off + rel if rel else None


def sibling_store_offset(data, off, tname):
    """Offset of the `ItemVariationStore` in `HVAR`/`VVAR`/`MVAR`.

    The three do not agree on where it lives: `HVAR` and `VVAR` put an
    Offset32 at +4, directly after the version, while `MVAR` puts an
    **Offset16** at +10, after a reserved word and its value-record shape.
    """
    if tname in ("HVAR", "VVAR"):
        if off + 8 > len(data):
            return None
        rel = u32(data, off + 4)
    else:  # MVAR
        if off + 12 > len(data):
            return None
        rel = u16(data, off + 10)
    return off + rel if rel else None


def survey(paths, verbose):
    total = 0
    variable = []
    malformed = []
    table_counts = collections.Counter()
    axis_counts = collections.Counter()
    axis_tags = collections.Counter()
    outline_kind = collections.Counter()
    mismatches = []

    for path in paths:
        try:
            with open(path, "rb") as fh:
                data = fh.read()
        except OSError as e:
            print(f"  ! cannot read {os.path.basename(path)}: {e}", file=sys.stderr)
            continue
        tabs = tables(data)
        if tabs is None:
            continue
        total += 1
        if "fvar" not in tabs:
            continue

        name = os.path.basename(path)
        parsed = read_fvar(data, *tabs["fvar"])
        if parsed is None:
            malformed.append((name, "fvar unreadable or implausible axis count"))
            continue
        axes, inst_count = parsed

        present = [t for t in VARIATION_TABLES if t in tabs]
        for t in present:
            table_counts[t] += 1
        axis_counts[len(axes)] += 1
        for a in axes:
            axis_tags[a[0]] += 1

        if "gvar" in tabs:
            outline_kind["gvar (TrueType)"] += 1
        elif "CFF2" in tabs:
            outline_kind["CFF2"] += 1
        else:
            outline_kind["NEITHER -- malformed"] += 1
            malformed.append((name, "fvar with no gvar and no CFF2"))

        # Cross-table axis-count agreement.
        for tname, reader in (
            ("avar", read_avar_axis_count),
            ("gvar", read_gvar_axis_count),
        ):
            if tname in tabs:
                got = reader(data, tabs[tname][0])
                if got is not None and got != len(axes):
                    mismatches.append((name, tname, got, len(axes)))
        for tname in ("HVAR", "VVAR", "MVAR"):
            if tname in tabs:
                store = sibling_store_offset(data, tabs[tname][0], tname)
                if store is not None:
                    got = item_variation_store_axis_count(data, store)
                    if got is not None and got != len(axes):
                        mismatches.append((name, tname, got, len(axes)))
        if "GDEF" in tabs:
            store = gdef_store_offset(data, tabs["GDEF"][0])
            if store is not None:
                table_counts["GDEF ItemVariationStore"] += 1
                got = item_variation_store_axis_count(data, store)
                if got is not None and got != len(axes):
                    mismatches.append((name, "GDEF store", got, len(axes)))

        variable.append((name, axes, inst_count, present))

    print(f"faces read: {total}")
    print(f"variable faces (carry `fvar`): {len(variable)}")
    print()

    if not variable:
        print("No variable faces on this host. Any oracle for variable-font work")
        print("must therefore be synthesized (see the Thai private-use precedent,")
        print("design-decisions §432) -- a sweep over these fonts would report")
        print("agreement it never tested.")
        return

    print("axis count distribution:")
    for n, c in sorted(axis_counts.items()):
        print(f"  {n} axis/axes: {c} faces")
    print()
    print("axis tags:")
    for t, c in axis_tags.most_common():
        print(f"  {t}: {c}")
    print()
    print("outlines:")
    for k, c in outline_kind.most_common():
        print(f"  {k}: {c}")
    print()
    print("optional variation tables present:")
    for t, c in table_counts.most_common():
        why = VARIATION_TABLES.get(t, "")
        print(f"  {t}: {c}" + (f"  -- {why}" if why else ""))
    print()
    absent = [t for t in VARIATION_TABLES if table_counts[t] == 0]
    if absent:
        print("NOT PRESENT ON ANY HOST FACE (cannot be measured here):")
        for t in absent:
            print(f"  {t}  -- {VARIATION_TABLES[t]}")
        print()

    if mismatches:
        print("AXIS-COUNT DISAGREEMENTS (fvar vs. another table):")
        for name, tname, got, want in mismatches:
            print(f"  {name}: {tname} says {got}, fvar says {want}")
        print()
    else:
        print("axis counts agree across every table on every face.")
        print()

    if malformed:
        print("MALFORMED:")
        for name, why in malformed:
            print(f"  {name}: {why}")
        print()

    if verbose:
        print("per-face detail:")
        for name, axes, inst_count, present in sorted(variable):
            spec = ", ".join(f"{t}[{lo}..{df}..{hi}]" for t, lo, df, hi in axes)
            print(f"  {name}: {spec}; {inst_count} named instances; {'+'.join(present)}")


def normalize_report(paths):
    """Print each variable face's named instances in normalized F2Dot14.

    Emitted in a shape that can be pasted into the Rust test with only the
    surrounding brackets changed, so transcribing it cannot quietly drop a
    value.
    """
    for path in paths:
        try:
            with open(path, "rb") as fh:
                data = fh.read()
        except OSError:
            continue
        tabs = tables(data)
        if tabs is None or "fvar" not in tabs:
            continue
        parsed = read_fvar(data, *tabs["fvar"])
        if parsed is None:
            continue
        axes, _ = parsed
        segments = None
        if "avar" in tabs:
            segments = read_avar_segments(data, tabs["avar"][0], len(axes))
        instances = read_fvar_instances(data, *tabs["fvar"], len(axes))

        name = os.path.basename(path)
        tags_ = "/".join(a[0] for a in axes)
        print(f'// {name}: {tags_}, avar {"yes" if segments else "no"}')
        print(f'    ("{name}", &[')
        for _name_id, user in instances:
            coords = []
            for i, axis in enumerate(axes):
                c = f2dot14(normalize_user(axis, user[i]))
                if segments is not None:
                    c = apply_segment_map(segments[i], c)
                coords.append(c)
            shown = ", ".join(str(c) for c in coords)
            user_shown = ", ".join(f"{v:g}" for v in user)
            print(f"        &[{shown}],  // {user_shown}")
        print("    ]),")


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--fonts", default=r"C:\Windows\Fonts")
    ap.add_argument("--verbose", action="store_true")
    ap.add_argument(
        "--normalize",
        action="store_true",
        help="print named instances in normalized F2Dot14 instead of surveying",
    )
    args = ap.parse_args()

    paths = []
    for ext in ("ttf", "otf", "ttc", "otc"):
        paths += glob.glob(os.path.join(args.fonts, f"*.{ext}"))
        paths += glob.glob(os.path.join(args.fonts, f"*.{ext.upper()}"))
    paths = sorted(set(paths))
    if not paths:
        print(f"no fonts found under {args.fonts}", file=sys.stderr)
        return 1
    if args.normalize:
        normalize_report(paths)
    else:
        survey(paths, args.verbose)
    return 0


if __name__ == "__main__":
    sys.exit(main())
