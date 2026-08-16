"""Generate `gui/font/src/indic_tables.rs` from the Unicode Indic data.

Run from anywhere:

    python gui/font/tools/gen_indic_tables.py

What the table is
-----------------

One `(category, position)` pair per code point, for the scripts the Indic
shaper handles. Both are *derived* properties, not raw UCD ones: Unicode
publishes `Indic_Syllabic_Category` and `Indic_Positional_Category`, and a
shaper needs neither of them unmodified. This script applies the same
derivation HarfBuzz's `gen-indic-table.py` applies, because HarfBuzz is the
oracle this crate is measured against and a table that differs by one code
point is a shaping difference nobody will ever trace back to a table.

The derivation, in the order it runs:

1. Keep only code points in the blocks the shaper covers, plus two singles
   (`U+00A0` NO-BREAK SPACE and `U+25CC` DOTTED CIRCLE) that turn up as
   placeholders in every script.
2. Map each syllabic category to a shaper category (`Consonant` -> `C`,
   `Virama` -> `H`, `Vowel_Dependent` -> `M`, and so on). Several distinct
   Unicode categories collapse: the shaper does not care that a dead
   consonant is dead, only that it is a consonant.
3. A `Bindu`/`Visarga`/`Syllable_Modifier` with no position becomes `SMPst`
   rather than `SM` — it sits after the base by default.
4. Apply the per-code-point category overrides. These are the cases where
   Unicode's answer is right about the character and wrong about what a
   shaper must do with it: `RA` in each script becomes its own category `Ra`
   because reph formation keys on it, Gurmukhi's `U+0A72`/`U+0A73` act like
   consonants, `U+0953`/`U+0954` act like binduses.
5. Blank the position of every category that has no position.
6. Give consonants `BASE_C`, give the mark-like categories `SMVD`, and deal
   with the matras. An Indic matra is *repositioned*: Unicode says it is
   left/top/bottom/right, and the shaper needs to know where in the reordered
   syllable it lands, which is a per-script question — `matra_position` is
   that table. A Khmer matra instead has its position *become* its category,
   via `position_to_category`; see below.
7. Apply the two position overrides.

Why the fetch is pinned
-----------------------

To `unicodedata.unidata_version`, exactly as `gen_joining_tables.py` is: the
Indic table and the normalization table must describe the same Unicode, or a
character can be composed by one and unknown to the other.

Khmer shares this table
-----------------------

HarfBuzz generates one table for three shapers, and the Khmer machine reads it
through the same accessor the Indic one does — the two are one buffer slot
with two enums laid over it. So the Khmer block is here, with the seven
categories only the Khmer machine reads (`VAbv`, `VBlw`, `VPre`, `VPst`,
`Robatic`, `Xgroup`, `Ygroup`) and the branch in step 6 that produces the
first four: for a Khmer matra the *position* becomes the category, because the
Khmer grammar tells vowels apart by where they go and has no position axis of
its own. Whatever position such a character keeps afterwards is inert —
nothing reads it — but it is written the way HarfBuzz writes it, so the two
tables stay diffable.

What is deliberately not here
-----------------------------

**Myanmar.** Its block brings `IV`, `As`, `DB`, `GB`, `MH`, `MW`, `MY`, `PT`,
`VS` and more that only the Myanmar machine reads. This crate has no Myanmar
shaper, so that block and those categories are left out, and with them every
override in HarfBuzz's list whose target category is Myanmar-only; the handful
that name an Indic category but only apply to Myanmar code points are skipped
as unreachable. Adding the shaper means adding its categories here and
re-running.
"""

import os
import unicodedata
import urllib.request

# Blocks whose code points the Indic and Khmer shapers read. HarfBuzz's list
# minus the four Myanmar blocks; Basic Latin and the punctuation blocks are in
# it because a placeholder or a dotted circle can stand in for a base.
#
# "Khmer Symbols" (U+19E0..U+19FF) is deliberately absent, as it is from
# HarfBuzz's list: those are lunar-date signs, not part of a syllable.
ALLOWED_BLOCKS = (
    "Basic Latin",
    "Latin-1 Supplement",
    "Devanagari",
    "Bengali",
    "Gurmukhi",
    "Gujarati",
    "Oriya",
    "Tamil",
    "Telugu",
    "Kannada",
    "Malayalam",
    "Khmer",
    "Vedic Extensions",
    "General Punctuation",
    "Superscripts and Subscripts",
    "Devanagari Extended",
)
ALLOWED_SINGLES = (0x00A0, 0x25CC)

# The shapers' categories, in the order of the Rust enum: the Indic ones
# first, then the seven only the Khmer machine reads. `X` is the default and is
# never written to the table.
#
# HarfBuzz's list has two more, `VD` and `RS`, which are dropped here because
# nothing can derive to them. No character has `Indic_Syllabic_Category`
# `Vowel_Dependent`: HarfBuzz keeps the slot but maps that property value to
# `M`. And every `Register_Shifter` is Khmer's U+17C9 or U+17CA, both of which
# `CATEGORY_OVERRIDES` turns into `Robatic` before the category is ever
# written — so `RS` is produced by step 2 and destroyed by step 4, and never
# reaches the table. Both are deliberately absent from `RUST_CATEGORY` too, so
# that if a future Unicode ever makes one reachable this script dies with a
# `KeyError` rather than silently writing a variant `indic.rs` does not have.
CATEGORIES = (
    "X",
    "C",
    "V",
    "N",
    "H",
    "ZWNJ",
    "ZWJ",
    "M",
    "SM",
    "A",
    "PLACEHOLDER",
    "DOTTEDCIRCLE",
    "MPst",
    "Repha",
    "Ra",
    "CM",
    "Symbol",
    "CS",
    "SMPst",
    # Khmer only. The first four are produced by step 6 from a matra's
    # position and never by step 2; the last three come only from an override.
    "VAbv",
    "VBlw",
    "VPre",
    "VPst",
    "Robatic",
    "Xgroup",
    "Ygroup",
)

# The Rust variant name for each. Spelled out rather than derived so that
# renaming one in `indic.rs` is a one-line change here and a compile error if
# it is not made.
RUST_CATEGORY = {
    "X": "Other",
    "C": "Consonant",
    "V": "Vowel",
    "N": "Nukta",
    "H": "Halant",
    "ZWNJ": "NonJoiner",
    "ZWJ": "Joiner",
    "M": "Matra",
    "SM": "SyllableModifier",
    "A": "Cantillation",
    "PLACEHOLDER": "Placeholder",
    "DOTTEDCIRCLE": "DottedCircle",
    "MPst": "MatraPost",
    "Repha": "Repha",
    "Ra": "Ra",
    "CM": "ConsonantMedial",
    "Symbol": "Symbol",
    "CS": "ConsonantWithStacker",
    "SMPst": "SyllableModifierPost",
    "VAbv": "VowelAbove",
    "VBlw": "VowelBelow",
    "VPre": "VowelPre",
    "VPst": "VowelPost",
    "Robatic": "Robatic",
    "Xgroup": "XGroup",
    "Ygroup": "YGroup",
}

RUST_POSITION = {
    "START": "Start",
    "RA_TO_BECOME_REPH": "RaToBecomeReph",
    "PRE_M": "PreM",
    "PRE_C": "PreC",
    "BASE_C": "BaseC",
    "AFTER_MAIN": "AfterMain",
    "ABOVE_C": "AboveC",
    "BEFORE_SUB": "BeforeSub",
    "BELOW_C": "BelowC",
    "AFTER_SUB": "AfterSub",
    "BEFORE_POST": "BeforePost",
    "POST_C": "PostC",
    "AFTER_POST": "AfterPost",
    "SMVD": "Smvd",
    "END": "End",
}

CATEGORY_MAP = {
    "Other": "X",
    "Avagraha": "Symbol",
    "Bindu": "SM",
    "Brahmi_Joining_Number": "PLACEHOLDER",
    "Cantillation_Mark": "A",
    "Consonant": "C",
    "Consonant_Dead": "C",
    "Consonant_Final": "CM",
    "Consonant_Head_Letter": "C",
    "Consonant_Initial_Postfixed": "C",
    "Consonant_Killer": "M",
    "Consonant_Medial": "CM",
    "Consonant_Placeholder": "PLACEHOLDER",
    "Consonant_Preceding_Repha": "Repha",
    "Consonant_Prefixed": "X",
    "Consonant_Subjoined": "CM",
    "Consonant_Succeeding_Repha": "CM",
    "Consonant_With_Stacker": "CS",
    "Gemination_Mark": "SM",
    "Invisible_Stacker": "H",
    "Joiner": "ZWJ",
    "Modifying_Letter": "X",
    "Non_Joiner": "ZWNJ",
    "Nukta": "N",
    "Number": "PLACEHOLDER",
    "Number_Joiner": "PLACEHOLDER",
    "Pure_Killer": "M",
    "Register_Shifter": "RS",
    "Syllable_Modifier": "SM",
    "Tone_Letter": "X",
    "Tone_Mark": "N",
    "Virama": "H",
    "Visarga": "SM",
    "Vowel": "V",
    "Vowel_Dependent": "M",
    "Vowel_Independent": "V",
}

POSITION_MAP = {
    "Not_Applicable": "END",
    "Left": "PRE_C",
    "Top": "ABOVE_C",
    "Bottom": "BELOW_C",
    "Right": "POST_C",
    # A split matra resolves to the position of its *last* part.
    "Bottom_And_Right": "POST_C",
    "Left_And_Right": "POST_C",
    "Top_And_Bottom": "BELOW_C",
    "Top_And_Bottom_And_Left": "BELOW_C",
    "Top_And_Bottom_And_Right": "POST_C",
    "Top_And_Left": "ABOVE_C",
    "Top_And_Left_And_Right": "POST_C",
    "Top_And_Right": "POST_C",
    "Overstruck": "AFTER_MAIN",
    "Visual_order_left": "PRE_M",
}

# Where Unicode's answer is right about the character and wrong about what a
# shaper must do with it. HarfBuzz's list, minus every entry whose target is a
# Myanmar-only category and every entry that only names a Myanmar code point.
CATEGORY_OVERRIDES = {
    # RA is its own category in each script that forms a reph from it. Which
    # of them actually do is the shaper's business, not the table's.
    0x0930: "Ra",  # Devanagari
    0x09B0: "Ra",  # Bengali
    0x09F0: "Ra",  # Bengali
    0x0A30: "Ra",  # Gurmukhi, which forms no reph
    0x0AB0: "Ra",  # Gujarati
    0x0B30: "Ra",  # Oriya
    0x0BB0: "Ra",  # Tamil, which forms no reph
    0x0C30: "Ra",  # Telugu, reph only with ZWJ
    0x0CB0: "Ra",  # Kannada
    0x0D30: "Ra",  # Malayalam, logical repha
    # These act like binduses.
    0x0953: "SM",
    0x0954: "SM",
    # GURMUKHI VOWEL SIGN II may be preceded by GURMUKHI SIGN BINDI.
    0x0A40: "MPst",
    # These act like consonants.
    0x0A72: "C",
    0x0A73: "C",
    # Vedic tone marks, treated as ordinary cantillation marks.
    0x1CE2: "A",
    0x1CE3: "A",
    0x1CE4: "A",
    0x1CE5: "A",
    0x1CE6: "A",
    0x1CE7: "A",
    0x1CE8: "A",
    0x1CED: "A",
    # These take marks in a standalone cluster, like an avagraha.
    0xA8F2: "Symbol",
    0xA8F3: "Symbol",
    0xA8F4: "Symbol",
    0xA8F5: "Symbol",
    0xA8F6: "Symbol",
    0xA8F7: "Symbol",
    0x1CE9: "Symbol",
    0x1CEA: "Symbol",
    0x1CEB: "Symbol",
    0x1CEC: "Symbol",
    0x1CEE: "Symbol",
    0x1CEF: "Symbol",
    0x1CF0: "Symbol",
    0x1CF1: "Symbol",
    0x0A51: "M",
    # Grantha marks, which ScriptExtensions says may also be used in Tamil.
    0x11301: "SM",
    0x11302: "SM",
    0x11303: "SM",
    0x1133B: "N",
    0x1133C: "N",
    0x0AFB: "N",
    0x0B55: "N",
    0x09FC: "PLACEHOLDER",
    0x0C80: "PLACEHOLDER",
    0x0D04: "PLACEHOLDER",
    0x25CC: "DOTTEDCIRCLE",
    # In the OpenType Myanmar spec but not Myanmar-specific, so the Indic
    # machine sees them too and must agree about what they are.
    0x2015: "PLACEHOLDER",
    0x2022: "PLACEHOLDER",
    0x25FB: "PLACEHOLDER",
    0x25FC: "PLACEHOLDER",
    0x25FD: "PLACEHOLDER",
    0x25FE: "PLACEHOLDER",
    # Khmer. RO, because a COENG+RO pair moves to the front of the syllable
    # and no other subjoined consonant does.
    0x179A: "Ra",
    # The robat and the two register shifters, which the grammar lets stand in
    # one distinguished slot right after the consonant.
    0x17CC: "Robatic",
    0x17C9: "Robatic",  # Unicode calls this a Register_Shifter.
    0x17CA: "Robatic",  # Likewise.
    # Signs the grammar admits between the matras. `Xgroup` may repeat and may
    # be preceded by a joiner; `Ygroup` may only trail.
    0x17C6: "Xgroup",
    0x17CB: "Xgroup",
    0x17CD: "Xgroup",  # Unicode calls this a Consonant_Killer.
    0x17CE: "Xgroup",
    0x17CF: "Xgroup",
    0x17D0: "Xgroup",
    0x17D1: "Xgroup",
    0x17C7: "Ygroup",
    0x17C8: "Ygroup",
    0x17DD: "Ygroup",
    # HarfBuzz's comment: "Just guessing. Uniscribe doesn't categorize it."
    0x17D3: "Ygroup",
    # KHMER SIGN PHNAEK MUAN, which takes marks like a base.
    0x17D9: "PLACEHOLDER",
}

POSITION_OVERRIDES = {
    0x0A51: "BELOW_C",
    0x0B01: "BEFORE_SUB",  # The Oriya bindu is BeforeSub in the spec.
}

# Where a matra lands in the reordered syllable, by its Unicode position and
# the script it belongs to. This is the part of the derivation that is purely
# a shaper's, and the reason the raw UCD property is not enough: Unicode says
# *where the mark is drawn*, and the shaper needs *where in the sequence it
# goes*, which differs per script for the same visual position.
def matra_position(u, pos, block):
    if pos == "PRE_C":
        return "PRE_M"
    if pos == "POST_C":
        return {
            "Devanagari": "AFTER_SUB",
            "Bengali": "AFTER_POST",
            "Gurmukhi": "AFTER_POST",
            "Gujarati": "AFTER_POST",
            "Oriya": "AFTER_POST",
            "Tamil": "AFTER_POST",
            "Telugu": "BEFORE_SUB" if u <= 0x0C42 else "AFTER_SUB",
            "Kannada": "BEFORE_SUB" if u < 0x0CC3 or u > 0x0CD6 else "AFTER_SUB",
            "Malayalam": "AFTER_POST",
        }.get(block, "AFTER_SUB")
    if pos == "ABOVE_C":
        # Bengali and Malayalam have no top matras.
        return {
            "Devanagari": "AFTER_SUB",
            "Gurmukhi": "AFTER_POST",  # Deviates from the spec, as HarfBuzz does.
            "Gujarati": "AFTER_SUB",
            "Oriya": "AFTER_MAIN",
            "Tamil": "AFTER_SUB",
            "Telugu": "BEFORE_SUB",
            "Kannada": "BEFORE_SUB",
        }.get(block, "AFTER_SUB")
    if pos == "BELOW_C":
        return {
            "Devanagari": "AFTER_SUB",
            "Bengali": "AFTER_SUB",
            "Gurmukhi": "AFTER_POST",
            "Gujarati": "AFTER_POST",
            "Oriya": "AFTER_SUB",
            "Tamil": "AFTER_POST",
            "Telugu": "BEFORE_SUB",
            "Kannada": "BEFORE_SUB",
            "Malayalam": "AFTER_POST",
        }.get(block, "AFTER_SUB")
    raise SystemExit(f"matra at an impossible position {pos!r}")


# The Khmer half of step 6. The Khmer grammar has no position axis: it tells
# vowels apart by name, so a matra's position has to be folded into its
# category before the machine ever sees it.
def position_to_category(pos):
    try:
        return {
            "PRE_C": "VPre",
            "ABOVE_C": "VAbv",
            "BELOW_C": "VBlw",
            "POST_C": "VPst",
        }[pos]
    except KeyError:
        raise SystemExit(f"Khmer matra at an impossible position {pos!r}") from None


# Keep in sync with `indic.rs`: the categories the shaper treats as a base.
CONSONANT_CATEGORIES = ("C", "CS", "Ra", "CM", "V", "PLACEHOLDER", "DOTTEDCIRCLE")
MATRA_CATEGORIES = ("M", "MPst")
SMVD_CATEGORIES = ("SM", "SMPst", "A", "Symbol")
# The only categories a position means anything for.
POSITIONED_CATEGORIES = ("CM", "SM", "H", "M", "MPst")


def fetch(version, name):
    url = f"https://www.unicode.org/Public/{version}/ucd/{name}"
    with urllib.request.urlopen(url, timeout=60) as r:
        return r.read().decode("utf-8", errors="replace")


def parse(text):
    """`{codepoint: value}` from a two-field UCD file."""
    out = {}
    for line in text.splitlines():
        line = line.split("#", 1)[0]
        fields = [f.strip() for f in line.split(";")]
        if len(fields) < 2:
            continue
        span = fields[0].split("..")
        lo = int(span[0], 16)
        hi = int(span[1], 16) if len(span) > 1 else lo
        for cp in range(lo, hi + 1):
            out[cp] = fields[1]
    return out


def derive(syllabic, positional, blocks):
    """`{codepoint: (category, position)}` for every code point that has one."""
    data = {}
    for cp in set(syllabic) | set(positional):
        block = blocks.get(cp, "No_Block")
        if cp not in ALLOWED_SINGLES and block not in ALLOWED_BLOCKS:
            continue
        cat = CATEGORY_MAP[syllabic.get(cp, "Other")]
        pos = positional.get(cp, "Not_Applicable")
        if cat == "SM" and pos == "Not_Applicable":
            cat = "SMPst"
        data[cp] = (cat, POSITION_MAP[pos], block)

    for cp, cat in CATEGORY_OVERRIDES.items():
        _, pos, _ = data.get(cp, ("X", "END", "No_Block"))
        data[cp] = (cat, pos, blocks.get(cp, "No_Block"))

    for cp, (cat, pos, block) in list(data.items()):
        if cat not in POSITIONED_CATEGORIES:
            pos = "END"
        if cat in CONSONANT_CATEGORIES:
            pos = "BASE_C"
        elif cat in MATRA_CATEGORIES:
            if block.startswith("Khmer"):
                # The position is left as Unicode gave it. Nothing reads it —
                # it is now redundant with the category — but HarfBuzz writes
                # it, so the two tables stay diffable.
                cat = position_to_category(pos)
            else:
                pos = matra_position(cp, pos, block)
        elif cat in SMVD_CATEGORIES:
            pos = "SMVD"
        data[cp] = (cat, pos, block)

    for cp, pos in POSITION_OVERRIDES.items():
        cat, _, block = data.get(cp, ("X", "END", "No_Block"))
        data[cp] = (cat, pos, block)

    return {cp: (cat, pos) for cp, (cat, pos, _) in data.items()}


def ranges(data):
    """The map collapsed into sorted, disjoint `(first, last, cat, pos)` runs.

    Code points whose answer is the default — `X` with position `END` — are
    dropped, which is what keeps this to a few hundred rows: every Latin
    letter in the allowed blocks is one.
    """
    runs = []
    for cp in sorted(data):
        value = data[cp]
        if value == ("X", "END"):
            continue
        if runs and runs[-1][2:] == value and runs[-1][1] + 1 == cp:
            runs[-1] = (runs[-1][0], cp) + value
        else:
            runs.append((cp, cp) + value)
    return runs


def main():
    version = unicodedata.unidata_version
    syllabic = parse(fetch(version, "IndicSyllabicCategory.txt"))
    positional = parse(fetch(version, "IndicPositionalCategory.txt"))
    blocks = parse(fetch(version, "Blocks.txt"))
    runs = ranges(derive(syllabic, positional, blocks))

    here = os.path.dirname(os.path.abspath(__file__))
    out = os.path.join(here, "..", "src", "indic_tables.rs")
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        w = f.write
        w("//! The Indic shaper's per-character category and position.\n")
        w("//!\n")
        w("//! Generated by `gui/font/tools/gen_indic_tables.py` from Unicode\n")
        w(f"//! {version}'s `IndicSyllabicCategory.txt`,\n")
        w("//! `IndicPositionalCategory.txt` and `Blocks.txt`. Do not edit: run\n")
        w("//! the script instead.\n")
        w("//!\n")
        w("//! Neither property is used raw. See that script for the derivation\n")
        w("//! and for why it follows HarfBuzz's to the code point.\n")
        w("\n")
        w("use crate::indic::{Category, Position};\n\n")
        w("/// Category and position as sorted, disjoint ranges of code points.\n")
        w("///\n")
        w("/// A character in no range is [`Category::Other`] at\n")
        w("/// [`Position::End`], which is the answer for everything outside the\n")
        w("/// Indic blocks and for most characters inside them.\n")
        w(
            "pub(crate) static INDIC_RANGES: [(u32, u32, Category, Position); "
            f"{len(runs)}] = [\n"
        )
        for lo, hi, cat, pos in runs:
            w(
                f"    (0x{lo:04X}, 0x{hi:04X}, Category::{RUST_CATEGORY[cat]}, "
                f"Position::{RUST_POSITION[pos]}),\n"
            )
        w("];\n")

    print(f"wrote {out}")
    print(f"  Unicode {version}")
    print(f"  {len(runs)} ranges")
    for cat in CATEGORIES:
        n = sum(hi - lo + 1 for lo, hi, c, _ in runs if c == cat)
        if n:
            print(f"  {RUST_CATEGORY[cat]:22} {n} code points")


if __name__ == "__main__":
    main()
