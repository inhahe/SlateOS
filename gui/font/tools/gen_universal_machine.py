"""Compile the USE cluster grammar to a DFA and write `universal_machine.rs`.

Same job as `gen_indic_machine.py`, `gen_khmer_machine.py` and
`gen_myanmar_machine.py`, and it imports the Thompson construction, subset
construction and Moore minimisation from the first of those rather than
repeating them. What differs is the grammar — transcribed below from
HarfBuzz's `hb-ot-shaper-use-machine.rl` — and the alphabet.

# The alphabet is USE's own

The other three machines share one category enum: HarfBuzz stores the Indic,
Khmer and Myanmar categories in the same buffer slot and generates all three
from one table, so their scanners all have the same 34 columns. USE does not
take part in that. Its categories come from `gen_universal_table.py`, there are
44 of them, and none of the Indic names mean anything here — `H` is USE's
halant, not Indic's, and the two have different numbers.

The column order is HarfBuzz's numeric order for the USE categories, packed
dense. Upstream's numbers run to 56 with gaps (they are the USE spec's own
category numbering, and the spec has categories HarfBuzz folds together);
keeping the gaps would buy nothing but thirteen dead columns per state, so the
Rust `Category` enum is dense and this table is 44 wide. The upstream number is
recorded per variant in `universal.rs` so the two can still be diffed.

# `broken_cluster` matches the empty string

`tail` bottoms out in a chain of `*`-quantified groups, every one of which may
be absent, so `broken_cluster` accepts ε — as Indic's does. That is harmless
because the Rust driver only records an accept *after* consuming a character
(see `universal::clusters`), which is exactly ragel's scanner semantics: a
zero-length match is not a token.

# What the scanner order means

Ragel breaks ties by the order the rules are listed, and several of these rules
overlap completely. `standard_cluster` and `broken_cluster` differ only in
whether a base was seen, and `virama_terminated_cluster` is a *prefix-closed*
subset of neither — so the listed order below is part of the answer, not
presentation. Note in particular that a bare `FMPst` sits between
`hieroglyph_cluster` and `broken_cluster`: a lone post-base final modifier is
not a broken cluster and gets no dotted circle, even though `broken_cluster`
would match it.
"""

import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from gen_indic_machine import a, compile_rules, minimise, opt, s, star  # noqa: E402
from gen_universal_table import CATEGORIES, RUST_CATEGORY  # noqa: E402

# The column order: HarfBuzz's numeric order, packed dense. `CATEGORIES` is
# written in that order and Python keeps it.
COLUMNS = list(CATEGORIES)


def c(name):
    """One character of USE category `name`."""
    if name not in CATEGORIES:
        raise SystemExit(f"no such USE category {name!r}")
    return ("cat", name)


def plus(p):
    """One or more. Ragel's `+`, which the Indic grammar never needed."""
    return s(p, star(p))


# ---------------------------------------------------------------------------
# The grammar, transcribed from `hb-ot-shaper-use-machine.rl`. One Python
# assignment per ragel line, in the same order and with the same names.
# ---------------------------------------------------------------------------

# h = H | HVM | IS | Sk;
H = a(c("H"), c("HVM"), c("IS"), c("Sk"))
# consonant_modifiers = CMAbv* CMBlw* ((h B | SUB) CMAbv* CMBlw*)*;
CONSONANT_MODIFIERS = s(
    star(c("CMAbv")),
    star(c("CMBlw")),
    star(s(a(s(H, c("B")), c("SUB")), star(c("CMAbv")), star(c("CMBlw")))),
)
# medial_consonants = MPre? MAbv? MBlw? MPst?;
MEDIAL_CONSONANTS = s(
    opt(c("MPre")), opt(c("MAbv")), opt(c("MBlw")), opt(c("MPst"))
)
# dependent_vowels = VPre* VAbv* VBlw* VPst* | H;
DEPENDENT_VOWELS = a(
    s(star(c("VPre")), star(c("VAbv")), star(c("VBlw")), star(c("VPst"))),
    c("H"),
)
# vowel_modifiers = HVM? VMPre* VMAbv* VMBlw* VMPst*;
VOWEL_MODIFIERS = s(
    opt(c("HVM")),
    star(c("VMPre")),
    star(c("VMAbv")),
    star(c("VMBlw")),
    star(c("VMPst")),
)
# final_consonants = FAbv* FBlw* FPst*;
FINAL_CONSONANTS = s(star(c("FAbv")), star(c("FBlw")), star(c("FPst")))
# final_modifiers = FMAbv* FMBlw* | FMPst?;
FINAL_MODIFIERS = a(s(star(c("FMAbv")), star(c("FMBlw"))), opt(c("FMPst")))

# complex_syllable_start = (R | CS)? (B | GB);
COMPLEX_SYLLABLE_START = s(opt(a(c("R"), c("CS"))), a(c("B"), c("GB")))
# complex_syllable_middle = consonant_modifiers medial_consonants
#                           dependent_vowels vowel_modifiers (Sk B)*;
COMPLEX_SYLLABLE_MIDDLE = s(
    CONSONANT_MODIFIERS,
    MEDIAL_CONSONANTS,
    DEPENDENT_VOWELS,
    VOWEL_MODIFIERS,
    star(s(c("Sk"), c("B"))),
)
# complex_syllable_tail = complex_syllable_middle final_consonants
#                         final_modifiers;
COMPLEX_SYLLABLE_TAIL = s(
    COMPLEX_SYLLABLE_MIDDLE, FINAL_CONSONANTS, FINAL_MODIFIERS
)
# number_joiner_terminated_cluster_tail = (HN N)* HN;
NUMBER_JOINER_TERMINATED_CLUSTER_TAIL = s(star(s(c("HN"), c("N"))), c("HN"))
# numeral_cluster_tail = (HN N)+;
NUMERAL_CLUSTER_TAIL = plus(s(c("HN"), c("N")))
# symbol_cluster_tail = SMAbv+ SMBlw* | SMBlw+;
SYMBOL_CLUSTER_TAIL = a(
    s(plus(c("SMAbv")), star(c("SMBlw"))), plus(c("SMBlw"))
)

# virama_terminated_cluster_tail = consonant_modifiers (IS | RK);
VIRAMA_TERMINATED_CLUSTER_TAIL = s(CONSONANT_MODIFIERS, a(c("IS"), c("RK")))
# virama_terminated_cluster = complex_syllable_start
#                             virama_terminated_cluster_tail;
VIRAMA_TERMINATED_CLUSTER = s(
    COMPLEX_SYLLABLE_START, VIRAMA_TERMINATED_CLUSTER_TAIL
)
# sakot_terminated_cluster_tail = complex_syllable_middle Sk;
SAKOT_TERMINATED_CLUSTER_TAIL = s(COMPLEX_SYLLABLE_MIDDLE, c("Sk"))
# sakot_terminated_cluster = complex_syllable_start
#                            sakot_terminated_cluster_tail;
SAKOT_TERMINATED_CLUSTER = s(
    COMPLEX_SYLLABLE_START, SAKOT_TERMINATED_CLUSTER_TAIL
)
# standard_cluster = complex_syllable_start complex_syllable_tail;
STANDARD_CLUSTER = s(COMPLEX_SYLLABLE_START, COMPLEX_SYLLABLE_TAIL)
# tail = complex_syllable_tail | sakot_terminated_cluster_tail
#      | symbol_cluster_tail | virama_terminated_cluster_tail;
TAIL = a(
    COMPLEX_SYLLABLE_TAIL,
    SAKOT_TERMINATED_CLUSTER_TAIL,
    SYMBOL_CLUSTER_TAIL,
    VIRAMA_TERMINATED_CLUSTER_TAIL,
)
# broken_cluster = R? (tail | number_joiner_terminated_cluster_tail
#                    | numeral_cluster_tail);
BROKEN_CLUSTER = s(
    opt(c("R")),
    a(TAIL, NUMBER_JOINER_TERMINATED_CLUSTER_TAIL, NUMERAL_CLUSTER_TAIL),
)

# number_joiner_terminated_cluster = N number_joiner_terminated_cluster_tail;
NUMBER_JOINER_TERMINATED_CLUSTER = s(
    c("N"), NUMBER_JOINER_TERMINATED_CLUSTER_TAIL
)
# numeral_cluster = N numeral_cluster_tail?;
NUMERAL_CLUSTER = s(c("N"), opt(NUMERAL_CLUSTER_TAIL))
# symbol_cluster = (O | GB | SB) tail?;
SYMBOL_CLUSTER = s(a(c("O"), c("GB"), c("SB")), opt(TAIL))
# hieroglyph_cluster = SB* G HR? HM? SE* (J SB* (G HR? HM? SE*)?)*;
HIEROGLYPH_BODY = s(c("G"), opt(c("HR")), opt(c("HM")), star(c("SE")))
HIEROGLYPH_CLUSTER = s(
    star(c("SB")),
    HIEROGLYPH_BODY,
    star(s(c("J"), star(c("SB")), opt(HIEROGLYPH_BODY))),
)

# `ZWNJ?` trails seven of the nine cluster rules. It is not part of any of the
# expressions above because the grammar attaches it in the scanner, not in the
# definitions — and the difference is visible: a `broken_cluster` that ends in
# ZWNJ is still broken, but a ZWNJ *alone* is `other`, since `broken_cluster`
# cannot start with one.
ZWNJ = opt(c("ZWNJ"))

# The ten scanner rules, in the order ragel lists them, which is the order ties
# are broken in.
RULES = (
    (s(VIRAMA_TERMINATED_CLUSTER, ZWNJ), "ViramaTerminated"),
    (s(SAKOT_TERMINATED_CLUSTER, ZWNJ), "SakotTerminated"),
    (s(STANDARD_CLUSTER, ZWNJ), "Standard"),
    (s(NUMBER_JOINER_TERMINATED_CLUSTER, ZWNJ), "NumberJoinerTerminated"),
    (s(NUMERAL_CLUSTER, ZWNJ), "Numeral"),
    (s(SYMBOL_CLUSTER, ZWNJ), "Symbol"),
    (s(HIEROGLYPH_CLUSTER, ZWNJ), "Hieroglyph"),
    (c("FMPst"), "NonCluster"),
    (s(BROKEN_CLUSTER, ZWNJ), "Broken"),
    # other = any
    (a(*(c(name) for name in COLUMNS)), "NonCluster"),
)

# The Rust enum the rules name. Order is the order of the variants there,
# which is also HarfBuzz's `use_syllable_type_t` numbering — and that numbering
# is load-bearing: it is stored in the low nibble of the syllable byte and read
# back by `reorder_cluster`.
CLUSTER_TYPES = (
    "ViramaTerminated",
    "SakotTerminated",
    "Standard",
    "NumberJoinerTerminated",
    "Numeral",
    "Symbol",
    "Hieroglyph",
    "Broken",
    "NonCluster",
)


def main():
    transitions, accepts = compile_rules(RULES, COLUMNS)
    before = len(transitions)
    transitions, accepts = minimise(transitions, accepts)
    if len(transitions) > 250:
        raise SystemExit(f"{len(transitions)} states will not fit in a u8")

    out = pathlib.Path(__file__).parent / ".." / "src" / "universal_machine.rs"
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        w = f.write
        w(
            "//! The Universal Shaping Engine's cluster machine, as a DFA.\n"
            "//!\n"
            "//! Generated by `gui/font/tools/gen_universal_machine.py` from\n"
            "//! the same ten regular expressions HarfBuzz's ragel scanner\n"
            "//! uses. Do not edit: run the script instead, where the grammar\n"
            "//! is written out and explained.\n"
            "\n"
            "use crate::universal::{Category, Cluster};\n"
            "\n"
            "/// State 0 is dead — nothing leaves it — and state 1 is the\n"
            "/// start. A row is indexed by\n"
            "/// [`Category`](crate::universal::Category) cast to `usize`, so\n"
            "/// the enum's variant order is part of this table. The width is\n"
            "/// written as `Category::COUNT` rather than as a literal so that\n"
            "/// a category added to the enum without regenerating this file\n"
            "/// fails to compile instead of silently mis-indexing every row.\n"
            f"pub(crate) static TRANSITIONS: [[u8; Category::COUNT]; "
            f"{len(transitions)}] = [\n"
        )
        for row in transitions:
            w("    [" + ", ".join(str(x) for x in row) + "],\n")
        w("];\n\n")
        w(
            "/// Which rule a state accepts, if any. Where more than one\n"
            "/// rule could accept, this is the first one the grammar lists,\n"
            "/// which is how the scanner breaks a tie.\n"
            f"pub(crate) static ACCEPTS: [Option<Cluster>; {len(accepts)}] = [\n"
        )
        for rule in accepts:
            if rule is None:
                w("    None,\n")
            else:
                w(f"    Some(Cluster::{RULES[rule][1]}),\n")
        w("];\n")

    print(f"wrote {out}")
    print(
        f"  {len(transitions)} states over {len(COLUMNS)} categories"
        f" ({before} before minimisation)"
    )
    named = sorted({RULES[r][1] for r in accepts if r is not None})
    print(f"  accepting: {', '.join(named)}")
    unused = [t for t in CLUSTER_TYPES if t not in named]
    if unused:
        raise SystemExit(f"no state accepts {unused} — the grammar lost a rule")
    if set(RUST_CATEGORY) != set(CATEGORIES):
        raise SystemExit("the category table and the machine disagree")


if __name__ == "__main__":
    main()
