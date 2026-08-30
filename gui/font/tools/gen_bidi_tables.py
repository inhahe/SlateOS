"""Generate `gui/font/src/bidi_tables.rs` from the Unicode bidi data.

Run from anywhere:

    python gui/font/tools/gen_bidi_tables.py

Why this one downloads
----------------------

Python's `unicodedata` carries `Bidi_Class` (as `bidirectional()`) but not the
other two properties the algorithm needs, and its answer for an unassigned
code point is the empty string rather than the *default* that UAX #9 assigns
it. Those defaults are not uniform — most of the plane defaults to
`Left_To_Right`, but the Hebrew, Arabic and other RTL blocks default to
`Right_To_Left` or `Arabic_Letter` so that an unassigned code point in an
Arabic block still behaves like Arabic. Getting that wrong turns a reserved
code point in the middle of a Hebrew word into a left-to-right run.

So all three properties come from unicode.org, for the *same* Unicode version
`unicodedata.unidata_version` reports — the same pinning the joining, script
and normalization tables use, and for the same reason: two tables describing
different Unicode versions can disagree about a character that exists in one
and not the other.

  `extracted/DerivedBidiClass.txt`  `Bidi_Class`, including the `@missing`
                                    lines that state the defaults above.
  `BidiBrackets.txt`                `Bidi_Paired_Bracket` and its type, which
                                    rule N0 needs to make `(` and `)` take the
                                    same direction in `he said "<HE> (foo)"`.
  `BidiMirroring.txt`               `Bidi_Mirroring_Glyph`, which rule L4 uses
                                    to draw `(` as `)` inside an RTL run.

What the tables are for
-----------------------

`BIDI_CLASS_RANGES`
    `Bidi_Class` as sorted, disjoint ranges. `Left_To_Right` is left out: a
    character in no range is `L`, which is both the property's most common
    value and its default over most of the code space. That is what keeps this
    to a couple of thousand rows rather than tens of thousands.

`BRACKETS`
    `(bracket, pair, opening)` sorted by `bracket`, for rule N0 / BD16.

`MIRRORED`
    `(char, mirror)` sorted by `char`, for rule L4.

Canonical equivalence, and why `BRACKETS` carries it already
------------------------------------------------------------

N0 says a bracket matches its pair *under canonical equivalence*: U+2329 LEFT
POINTING ANGLE BRACKET has the singleton decomposition U+3008, so U+2329 must
close against U+232A **and** against U+3009. Rather than make the Rust side
carry a decomposition step it needs nowhere else, this script folds each
bracket to its canonical singleton before writing the pair — so both members
of such a family list the same normalized pair, and a plain equality test in
`bidi.rs` is then correct.
"""

import os

from rustfmt_out import rustfmt
import unicodedata
import urllib.request

# The discriminant order of the generated Rust enum, so it must match the
# `Class` enum in `bidi.rs`. `L` is absent on purpose: it is the default and is
# never written to the table.
CLASSES = (
    "R", "AL", "EN", "ES", "ET", "AN", "CS", "NSM", "BN",
    "B", "S", "WS", "ON",
    "LRE", "LRO", "RLE", "RLO", "PDF", "LRI", "RLI", "FSI", "PDI",
)

# `DerivedBidiClass.txt` writes the long names; `BidiTest.txt` and everyone
# else write the short ones, so normalize to short here.
LONG = {
    "Left_To_Right": "L",
    "Right_To_Left": "R",
    "Arabic_Letter": "AL",
    "European_Number": "EN",
    "European_Separator": "ES",
    "European_Terminator": "ET",
    "Arabic_Number": "AN",
    "Common_Separator": "CS",
    "Nonspacing_Mark": "NSM",
    "Boundary_Neutral": "BN",
    "Paragraph_Separator": "B",
    "Segment_Separator": "S",
    "White_Space": "WS",
    "Other_Neutral": "ON",
    "Left_To_Right_Embedding": "LRE",
    "Left_To_Right_Override": "LRO",
    "Right_To_Left_Embedding": "RLE",
    "Right_To_Left_Override": "RLO",
    "Pop_Directional_Format": "PDF",
    "Left_To_Right_Isolate": "LRI",
    "Right_To_Left_Isolate": "RLI",
    "First_Strong_Isolate": "FSI",
    "Pop_Directional_Isolate": "PDI",
}

# The Rust variant names, which spell the classes out rather than abbreviate
# them everywhere except where the abbreviation *is* the name in the spec.
RUST = {
    "R": "R", "AL": "Al", "EN": "En", "ES": "Es", "ET": "Et", "AN": "An",
    "CS": "Cs", "NSM": "Nsm", "BN": "Bn", "B": "B", "S": "S", "WS": "Ws",
    "ON": "On", "LRE": "Lre", "LRO": "Lro", "RLE": "Rle", "RLO": "Rlo",
    "PDF": "Pdf", "LRI": "Lri", "RLI": "Rli", "FSI": "Fsi", "PDI": "Pdi",
}

MAX = 0x110000


def fetch(version, name):
    """One UCD file for exactly `version`, as text."""
    url = f"https://www.unicode.org/Public/{version}/ucd/{name}"
    with urllib.request.urlopen(url, timeout=120) as r:
        # ASCII apart from the copyright line, which is Latin-1.
        return r.read().decode("utf-8", errors="replace")


def field_range(field):
    """`FIRST..LAST` or a bare `CP`, as an inclusive `(lo, hi)`."""
    if ".." in field:
        lo, hi = (int(p, 16) for p in field.split(".."))
        return lo, hi
    cp = int(field, 16)
    return cp, cp


def parse_classes(text):
    """`Bidi_Class` for every code point, defaults included, as a list."""
    # Start everything at the whole-range default, then apply the block
    # defaults, then the explicit assignments — the same layering the file's
    # own comments describe, in the same order, so later lines win.
    out = ["L"] * MAX
    seen_missing = False
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("# @missing:"):
            seen_missing = True
            field, value = (
                f.strip() for f in stripped[len("# @missing:"):].split(";")
            )
            lo, hi = field_range(field)
            short = LONG.get(value, value)
            if short != "L" and short not in CLASSES:
                raise SystemExit(f"unknown Bidi_Class {value!r} in @missing")
            for cp in range(lo, hi + 1):
                out[cp] = short
            continue
        line = stripped.split("#", 1)[0].strip()
        if not line:
            continue
        field, value = (f.strip() for f in line.split(";"))
        short = LONG.get(value, value)
        if short != "L" and short not in CLASSES:
            raise SystemExit(f"unknown Bidi_Class {value!r}")
        lo, hi = field_range(field)
        for cp in range(lo, hi + 1):
            out[cp] = short
    if not seen_missing:
        raise SystemExit(
            "DerivedBidiClass.txt carried no @missing lines — the unassigned "
            "defaults would silently all become L"
        )
    return out


def ranges(classes):
    """The per-code-point list collapsed into `(first, last, class)` runs."""
    runs = []
    for cp, kind in enumerate(classes):
        if kind == "L":
            continue
        if runs and runs[-1][2] == kind and runs[-1][1] + 1 == cp:
            runs[-1] = (runs[-1][0], cp, kind)
        else:
            runs.append((cp, cp, kind))
    return runs


def canonical(cp):
    """`cp` folded through its canonical singleton decomposition, if it has one.

    N0 matches brackets under canonical equivalence, and the only bracket
    equivalences in Unicode are singletons (U+2329 to U+3008, U+232A to
    U+3009). Folding here means `bidi.rs` can compare pairs with `==`.
    """
    d = unicodedata.decomposition(chr(cp))
    if not d or d.startswith("<"):
        return cp
    parts = d.split()
    if len(parts) != 1:
        return cp
    return int(parts[0], 16)


def parse_brackets(text):
    """`[(bracket, pair, opening)]` sorted by bracket, canonically folded."""
    out = []
    for line in text.splitlines():
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        cp, pair, kind = (f.strip() for f in line.split(";"))
        if kind not in ("o", "c"):
            raise SystemExit(f"unknown Bidi_Paired_Bracket_Type {kind!r}")
        out.append((int(cp, 16), canonical(int(pair, 16)), kind == "o"))
    out.sort()
    return out


def parse_mirroring(text):
    """`[(char, mirror)]` sorted by char."""
    out = []
    for line in text.splitlines():
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        cp, mirror = (f.strip() for f in line.split(";"))
        out.append((int(cp, 16), int(mirror, 16)))
    out.sort()
    return out


def main():
    version = unicodedata.unidata_version
    classes = parse_classes(fetch(version, "extracted/DerivedBidiClass.txt"))
    runs = ranges(classes)
    brackets = parse_brackets(fetch(version, "BidiBrackets.txt"))
    mirrored = parse_mirroring(fetch(version, "BidiMirroring.txt"))

    here = os.path.dirname(os.path.abspath(__file__))
    out = os.path.join(here, "..", "src", "bidi_tables.rs")
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        w = f.write
        w("//! The Unicode bidi properties, for UAX #9.\n")
        w("//!\n")
        w("//! Generated by `gui/font/tools/gen_bidi_tables.py` from Unicode\n")
        w(f"//! {version}'s `DerivedBidiClass.txt`, `BidiBrackets.txt` and\n")
        w("//! `BidiMirroring.txt`. Do not edit: run the script instead.\n")
        w("//!\n")
        w("//! See that script for why `Left_To_Right` is deliberately absent\n")
        w("//! from the class table, and why the bracket pairs are already\n")
        w("//! folded through canonical equivalence.\n")
        w("\n")
        w("use crate::bidi::Class;\n\n")

        w("/// `Bidi_Class` as sorted, disjoint ranges of code points.\n")
        w("///\n")
        w("/// A character in no range is [`Class::L`], which is the property's\n")
        w("/// default over most of the code space and its most common value —\n")
        w("/// hence its absence here.\n")
        w(f"pub(crate) static BIDI_CLASS_RANGES: [(u32, u32, Class); {len(runs)}] = [\n")
        for lo, hi, kind in runs:
            w(f"    (0x{lo:04X}, 0x{hi:04X}, Class::{RUST[kind]}),\n")
        w("];\n\n")

        w("/// `(bracket, pair, opening)` sorted by bracket, for rule N0.\n")
        w("///\n")
        w("/// `pair` is folded through canonical equivalence, so an opening\n")
        w("/// bracket's `pair` compares equal to the *folded* closing bracket\n")
        w("/// whichever of the canonically-equivalent spellings was typed.\n")
        w(f"pub(crate) static BRACKETS: [(u32, u32, bool); {len(brackets)}] = [\n")
        for cp, pair, opening in brackets:
            w(f"    (0x{cp:04X}, 0x{pair:04X}, {'true' if opening else 'false'}),\n")
        w("];\n\n")

        w("/// `(char, mirror)` sorted by char, for rule L4.\n")
        w(f"pub(crate) static MIRRORED: [(u32, u32); {len(mirrored)}] = [\n")
        for cp, mirror in mirrored:
            w(f"    (0x{cp:04X}, 0x{mirror:04X}),\n")
        w("];\n")
    # Match what `cargo fmt -p osfont` would do to this file, so that
    # regenerating it and formatting it are each a no-op after the other.
    # Without this the two rewrite each other's output forever, and every
    # unrelated diff carries a thousand lines of table churn. See
    # rustfmt_out.py for the whole reason.
    rustfmt(out)

    print(f"wrote {out}")
    print(f"  Unicode {version}")
    print(f"  {len(runs)} class ranges, {len(brackets)} brackets, "
          f"{len(mirrored)} mirrored")
    for kind in CLASSES:
        n = sum(hi - lo + 1 for lo, hi, k in runs if k == kind)
        if n:
            print(f"  {kind:4} {n} code points")


if __name__ == "__main__":
    main()
