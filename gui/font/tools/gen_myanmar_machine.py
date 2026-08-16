"""Compile the Myanmar syllable grammar to a DFA and write `myanmar_machine.rs`.

Same job as `gen_indic_machine.py` and `gen_khmer_machine.py`, same machinery —
this script imports the Thompson construction, subset construction and Moore
minimisation from the first of those rather than repeating them — over the
grammar transcribed below from HarfBuzz's `hb-ot-shaper-myanmar-machine.rl`.

# The categories are Indic's, under other names

HarfBuzz stores the Indic, Khmer and Myanmar categories in the *same* buffer
slot, one enum laid over another, and generates all three from one table. So
this scanner's columns are `gen_indic_tables.CATEGORIES` in full, and a
category no Myanmar rule names is reachable only through `other = any`.

Three of the names the Myanmar grammar uses are *aliases* rather than
categories of their own, and the transcription below spells them out:

| grammar | table | what it is |
|---|---|---|
| `IV` | `V` | an independent vowel |
| `DB` | `N` | the dot below |
| `GB` | `PLACEHOLDER` | a generic base — a digit, a bullet, NO-BREAK SPACE |

They are `#define`s over the Indic numbers in HarfBuzz, not values its table
can produce, which is why adding Myanmar cost eight new categories and not the
eleven the grammar appears to name.

# What the Myanmar grammar does *not* have

No position axis, exactly as Khmer has none: the four vowel categories
(`VAbv`, `VBlw`, `VPre`, `VPst`) say where a vowel goes, and the grammar never
consults a `Position`. `gen_indic_tables.py` folds a Myanmar matra's position
into its category for that reason — see `position_to_category` there.

A position is still *computed*, by `myanmar::reorder`, and it is what the
syllable is sorted by. That is a different thing from the table's position and
is not derived from it.
"""

import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from gen_indic_machine import a, c, compile_rules, minimise, opt, s, star  # noqa: E402
from gen_indic_tables import CATEGORIES, RUST_CATEGORY  # noqa: E402

# ---------------------------------------------------------------------------
# The grammar, transcribed from `hb-ot-shaper-myanmar-machine.rl`. One Python
# assignment per ragel line, in the same order, with the alias names expanded
# to the table's: `IV` -> `V`, `DB` -> `N`, `GB` -> `PLACEHOLDER`.
# ---------------------------------------------------------------------------

# j = ZWJ|ZWNJ
JOINER = a(c("ZWJ"), c("ZWNJ"))
# k = (Ra As H) -- the kinzi, which is NGA + ASAT + VIRAMA and nothing else.
KINZI = s(c("Ra"), c("As"), c("H"))
# sm = SM | SMPst
SM = a(c("SM"), c("SMPst"))
# c = C|Ra
CONSONANT = a(c("C"), c("Ra"))

# medial_group = MY? As? MR? ((MW MH? ML? | MH ML? | ML) As?)?
#
# The alternation is not `MW? MH? ML?`: MEDIAL WA may be followed by HA and
# then MON LA, HA by MON LA, and MON LA may stand alone — but HA may not
# precede WA, and the empty case is the outer `?`. Writing it as three
# independent options would admit `MH MW`, which the grammar does not.
MEDIAL_GROUP = s(
    opt(c("MY")),
    opt(c("As")),
    opt(c("MR")),
    opt(
        s(
            a(
                s(c("MW"), opt(c("MH")), opt(c("ML"))),
                s(c("MH"), opt(c("ML"))),
                c("ML"),
            ),
            opt(c("As")),
        )
    ),
)
# main_vowel_group = (VPre.VS?)* VAbv* VBlw* A* (DB As?)?
MAIN_VOWEL_GROUP = s(
    star(s(c("VPre"), opt(c("VS")))),
    star(c("VAbv")),
    star(c("VBlw")),
    star(c("A")),
    opt(s(c("N"), opt(c("As")))),
)
# post_vowel_group = VPst MH? ML? As* VAbv* A* (DB As?)?
POST_VOWEL_GROUP = s(
    c("VPst"),
    opt(c("MH")),
    opt(c("ML")),
    star(c("As")),
    star(c("VAbv")),
    star(c("A")),
    opt(s(c("N"), opt(c("As")))),
)
# tone_group = sm | PT A* DB? As?
TONE_GROUP = a(SM, s(c("PT"), star(c("A")), opt(c("N")), opt(c("As"))))

# complex_syllable_tail = As* medial_group main_vowel_group post_vowel_group* tone_group* j?
COMPLEX_SYLLABLE_TAIL = s(
    star(c("As")),
    MEDIAL_GROUP,
    MAIN_VOWEL_GROUP,
    star(POST_VOWEL_GROUP),
    star(TONE_GROUP),
    opt(JOINER),
)
# syllable_tail = (H (c|IV).VS?)* (H | complex_syllable_tail)
SYLLABLE_TAIL = s(
    star(s(c("H"), a(CONSONANT, c("V")), opt(c("VS")))),
    a(c("H"), COMPLEX_SYLLABLE_TAIL),
)

# consonant_syllable = (k|CS)? (c|IV|GB|DOTTEDCIRCLE).VS? syllable_tail
CONSONANT_SYLLABLE = s(
    opt(a(KINZI, c("CS"))),
    a(CONSONANT, c("V"), c("PLACEHOLDER"), c("DOTTEDCIRCLE")),
    opt(c("VS")),
    SYLLABLE_TAIL,
)
# broken_cluster = k? VS? syllable_tail
BROKEN_CLUSTER = s(opt(KINZI), opt(c("VS")), SYLLABLE_TAIL)

# The four scanner rules, in the order ragel lists them -- which is the order
# ties are broken in, so it is part of the answer, not presentation. Note that
# `j | SMPst` sits *between* the two syllable rules: a lone joiner or a lone
# post-base syllable modifier is not a broken cluster and gets no dotted
# circle, even though `broken_cluster` would match the second of them.
RULES = (
    (CONSONANT_SYLLABLE, "Consonant"),
    (a(JOINER, c("SMPst")), "NonMyanmar"),
    (BROKEN_CLUSTER, "Broken"),
    # other = any
    (a(*(c(name) for name in CATEGORIES)), "NonMyanmar"),
)

# The Rust enum the rules name. Order is the order of the variants there,
# which is also HarfBuzz's `myanmar_syllable_type_t` numbering.
SYLLABLE_TYPES = ("Consonant", "Broken", "NonMyanmar")


def main():
    transitions, accepts = compile_rules(RULES)
    before = len(transitions)
    transitions, accepts = minimise(transitions, accepts)
    if len(transitions) > 250:
        raise SystemExit(f"{len(transitions)} states will not fit in a u8")

    out = pathlib.Path(__file__).parent / ".." / "src" / "myanmar_machine.rs"
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        w = f.write
        w(
            "//! The Myanmar syllable machine, as a DFA.\n"
            "//!\n"
            "//! Generated by `gui/font/tools/gen_myanmar_machine.py` from the\n"
            "//! same four regular expressions HarfBuzz's ragel scanner uses.\n"
            "//! Do not edit: run the script instead, where the grammar is\n"
            "//! written out and explained.\n"
            "\n"
            "use crate::myanmar::Syllable;\n"
            "\n"
            "/// State 0 is dead — nothing leaves it — and state 1 is the\n"
            "/// start. A row is indexed by [`Category`](crate::indic::Category)\n"
            "/// cast to `usize`: Myanmar, Khmer and Indic share one category\n"
            "/// enum, so this table has a column for every category of the\n"
            "/// other two, reachable only through the grammar's `other = any`.\n"
            f"pub(crate) static TRANSITIONS: [[u8; {len(CATEGORIES)}]; "
            f"{len(transitions)}] = [\n"
        )
        for row in transitions:
            w("    [" + ", ".join(str(x) for x in row) + "],\n")
        w("];\n\n")
        w(
            "/// Which rule a state accepts, if any. Where more than one\n"
            "/// rule could accept, this is the first one the grammar lists,\n"
            "/// which is how the scanner breaks a tie.\n"
            f"pub(crate) static ACCEPTS: [Option<Syllable>; {len(accepts)}] = [\n"
        )
        for rule in accepts:
            if rule is None:
                w("    None,\n")
            else:
                w(f"    Some(Syllable::{RULES[rule][1]}),\n")
        w("];\n")

    print(f"wrote {out}")
    print(
        f"  {len(transitions)} states over {len(CATEGORIES)} categories"
        f" ({before} before minimisation)"
    )
    named = sorted({RULES[r][1] for r in accepts if r is not None})
    print(f"  accepting: {', '.join(named)}")
    unused = [t for t in SYLLABLE_TYPES if t not in named]
    if unused:
        raise SystemExit(f"no state accepts {unused} — the grammar lost a rule")
    if set(RUST_CATEGORY) != set(CATEGORIES):
        raise SystemExit("the category table and the machine disagree")


if __name__ == "__main__":
    main()
