"""Generate `gui/font/src/universal_tables.rs` — the USE shaper's per-character category.

Run from anywhere:

    python gui/font/tools/gen_universal_table.py

What the table is
-----------------

One category per code point, for every script the Universal Shaping Engine
handles — which is "every complex script that does not have a shaper of its
own", so this table spans far more of Unicode than the Indic one: Tibetan,
Sinhala, Javanese, Balinese, Buginese, Cham, Tai Tham, Tifinagh, Kharoshthi,
Chakma, Egyptian Hieroglyphs and around eighty more.

The category is *derived*, not a UCD property. Unicode publishes five inputs
and the USE spec turns them into one answer:

| Input | What it contributes |
|---|---|
| `IndicSyllabicCategory.txt` | the main axis: consonant, vowel, virama, … |
| `IndicPositionalCategory.txt` | above/below/pre/post, which splits several categories in four |
| `ArabicShaping.txt` | joining type, which promotes a joining letter to a base |
| `DerivedCoreProperties.txt` | `Default_Ignorable_Code_Point`, which selects CGJ and word-joiner behaviour |
| `UnicodeData.txt` | General_Category, which is what separates a letter-like mark from a mark |

plus two override files Microsoft maintains and HarfBuzz vendors in
`src/ms-use/`, listing the code points where Unicode's own answer does not
match what the USE spec requires. Those are not derivable from anything and
have to be fetched from HarfBuzz rather than unicode.org.

`Scripts.txt` is read for one purpose: to *drop* Arabic, Lao, Samaritan, Syriac
and Thai, which have their own shapers and must never be handed to USE even
though their characters have Indic syllabic categories.

Why this follows HarfBuzz to the code point
-------------------------------------------

HarfBuzz is the oracle this crate is measured against, so a table that differs
by one code point is a shaping difference nobody will ever trace back to a
table. The derivation below is `gen-use-table.py`'s, predicate for predicate.

It is also *checked* rather than merely intended: if `.hbref/` holds a checkout
of the HarfBuzz sources, this script reads HarfBuzz's own packed table back out
(see `hb_use_oracle.py`) and diffs all 0x110000 code points against the same
derivation. That check covers every one of the thirty-odd predicates at once,
which no amount of reading them side by side does.

The check is run at **HarfBuzz's** Unicode version, not ours, and demands an
exact match. Those differ: the shipped table is pinned to
`unicodedata.unidata_version` (as `gen_indic_tables.py` and
`gen_joining_tables.py` are, so a character cannot be composed by the
normalizer and unknown to the shaper), while a HarfBuzz release carries
whatever Unicode it shipped with. Diffing the two versions directly would
require a tolerance — and a tolerance is exactly where a real bug hides, since
Unicode changes properties of *assigned* characters too, not just adds new
ones. U+1900 LIMBU VOWEL-CARRIER LETTER is one: `Consonant_Placeholder` in
Unicode 16, `Consonant` in 17, so the category legitimately differs at a code
point that has existed since Unicode 4. Re-deriving at HarfBuzz's own version
removes the skew entirely and leaves zero as the only acceptable answer.

Why the default is not in the table
-----------------------------------

`O` (OTHER) is the answer for all but ~15,000 code points, so it is dropped and
the Rust lookup returns it for anything in no range. That is what keeps this to
a few thousand rows instead of a million.
"""

import os
import re
import sys
import unicodedata
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

# The HarfBuzz release the sweep's oracle (uharfbuzz) wraps. Only used to fetch
# the two `ms-use/` override files, which are not published by Unicode.
HARFBUZZ_VERSION = "14.3.0"

# Scripts that reach USE's inputs but have a shaper of their own. Dropping them
# here is what stops a Thai character being categorised as a USE base and
# shaped by the wrong engine.
DISABLED_SCRIPTS = ("Arabic", "Lao", "Samaritan", "Syriac", "Thai")

# The USE categories, with the numeric values HarfBuzz's machine uses. The
# numbers themselves are *not* what the generated cluster machine indexes by —
# they run to 56 with thirteen gaps, which would cost thirteen dead columns in
# every state's transition row — so `gen_universal_machine.py` uses the dense
# *order* of this dict instead, and `universal.rs`'s enum is dense to match.
# The numbers are kept because they are what a diff against HarfBuzz is read
# in; each is recorded in the doc comment of its Rust variant.
CATEGORIES = {
    "O": 0,
    "B": 1,
    "N": 4,
    "GB": 5,
    "CGJ": 6,
    "SUB": 11,
    "H": 12,
    "HN": 13,
    "ZWNJ": 14,
    "WJ": 16,
    "R": 18,
    "VPre": 22,
    "VMPre": 23,
    "FAbv": 24,
    "FBlw": 25,
    "FPst": 26,
    "MAbv": 27,
    "MBlw": 28,
    "MPst": 29,
    "MPre": 30,
    "CMAbv": 31,
    "CMBlw": 32,
    "VAbv": 33,
    "VBlw": 34,
    "VPst": 35,
    "VMAbv": 37,
    "VMBlw": 38,
    "VMPst": 39,
    "SMAbv": 41,
    "SMBlw": 42,
    "CS": 43,
    "IS": 44,
    "FMAbv": 45,
    "FMBlw": 46,
    "FMPst": 47,
    "Sk": 48,
    "G": 49,
    "J": 50,
    "SB": 51,
    "SE": 52,
    "HVM": 53,
    "HM": 54,
    "HR": 55,
    "RK": 56,
}

# The Rust variant name for each. Spelled out rather than derived so that
# renaming one in `universal.rs` is a one-line change here and a compile error if it
# is not made. The long names are the USE spec's own; HarfBuzz's short ones are
# unreadable at the call site (`FMBlw` vs `ConsFinalModBelow`).
RUST_CATEGORY = {
    "O": "Other",
    "B": "Base",
    "N": "BaseNum",
    "GB": "BaseOther",
    "CGJ": "Cgj",
    "SUB": "ConsSub",
    "H": "Halant",
    "HN": "HalantNum",
    "ZWNJ": "NonJoiner",
    "WJ": "WordJoiner",
    "R": "Repha",
    "VPre": "VowelPre",
    "VMPre": "VowelModPre",
    "FAbv": "ConsFinalAbove",
    "FBlw": "ConsFinalBelow",
    "FPst": "ConsFinalPost",
    "MAbv": "ConsMedAbove",
    "MBlw": "ConsMedBelow",
    "MPst": "ConsMedPost",
    "MPre": "ConsMedPre",
    "CMAbv": "ConsModAbove",
    "CMBlw": "ConsModBelow",
    "VAbv": "VowelAbove",
    "VBlw": "VowelBelow",
    "VPst": "VowelPost",
    "VMAbv": "VowelModAbove",
    "VMBlw": "VowelModBelow",
    "VMPst": "VowelModPost",
    "SMAbv": "SymModAbove",
    "SMBlw": "SymModBelow",
    "CS": "ConsWithStacker",
    "IS": "InvisibleStacker",
    "FMAbv": "ConsFinalModAbove",
    "FMBlw": "ConsFinalModBelow",
    "FMPst": "ConsFinalModPost",
    "Sk": "Sakot",
    "G": "Hieroglyph",
    "J": "HieroglyphJoiner",
    "SB": "HieroglyphSegBegin",
    "SE": "HieroglyphSegEnd",
    "HVM": "HalantOrVowelModifier",
    "HM": "HieroglyphMod",
    "HR": "HieroglyphMirror",
    "RK": "ReorderingKiller",
}


# --------------------------------------------------------------------------
# Fetching
# --------------------------------------------------------------------------


def _get(url):
    with urllib.request.urlopen(url, timeout=120) as r:
        return r.read().decode("utf-8", errors="replace")


def fetch_ucd(version, name):
    return _get(f"https://www.unicode.org/Public/{version}/ucd/{name}")


def fetch_ms_use(name):
    return _get(
        "https://raw.githubusercontent.com/harfbuzz/harfbuzz/"
        f"{HARFBUZZ_VERSION}/src/ms-use/{name}"
    )


def parse(text, field=1, keep=None):
    """`{codepoint: value}` from a semicolon-separated UCD file.

    `field` selects the column, since the property is column 1 in most files
    and column 2 in `ArabicShaping.txt` and `UnicodeData.txt`.

    `keep`, if given, is the one value to retain. That matters for
    `DerivedCoreProperties.txt`, which lists *many* properties in one file and
    names most code points under several of them: without the filter the last
    property block wins and the answer is whatever happens to sort last, not
    the property that was asked for.

    Ranges written `AAAA..BBBB` are expanded. `UnicodeData.txt`'s *other* range
    convention — a `<…, First>` line followed by a `<…, Last>` line — is
    deliberately **not** expanded, matching HarfBuzz. It cannot change the
    result: those ranges are CJK, Hangul, Tangut and the private-use planes,
    and no code point in them appears in any of the four files that decide
    which code points get an entry at all.
    """
    out = {}
    for line in text.splitlines():
        line = line.split("#", 1)[0]
        fields = [f.strip() for f in line.split(";")]
        if len(fields) <= field:
            continue
        value = fields[field]
        if keep is not None and value != keep:
            continue
        span = fields[0].split("..")
        lo = int(span[0], 16)
        hi = int(span[1], 16) if len(span) > 1 else lo
        for cp in range(lo, hi + 1):
            out[cp] = value
    return out


# --------------------------------------------------------------------------
# The derivation
# --------------------------------------------------------------------------
#
# One predicate per USE category, each taking the five resolved properties.
# Exactly one must be true for every code point; `derive` asserts it. They are
# HarfBuzz's `is_*` functions in HarfBuzz's order, and the comments on the
# non-obvious ones are its comments.

JOINING = ("jt_C", "jt_D", "jt_L", "jt_R")


def is_base(u, uisc, udi, ugc, ajt):
    return (
        uisc
        in (
            "Number",
            "Consonant",
            "Consonant_Head_Letter",
            "Tone_Letter",
            "Vowel_Independent",
        )
        # A letter that joins is a base whatever else Unicode calls it. See
        # MicrosoftDocs/typography-issues#484.
        or (ajt in JOINING and uisc != "Joiner")
        # `Lo` is the discriminator between a letter-like mark and a mark: a
        # `Consonant_Final` that is a letter stands on its own as a base.
        or (
            ugc == "Lo"
            and uisc
            in (
                "Avagraha",
                "Bindu",
                "Consonant_Final",
                "Consonant_Medial",
                "Consonant_Subjoined",
                "Vowel",
                "Vowel_Dependent",
            )
        )
    )


def is_base_num(u, uisc, udi, ugc, ajt):
    return uisc == "Brahmi_Joining_Number"


def is_base_other(u, uisc, udi, ugc, ajt):
    if uisc == "Consonant_Placeholder":
        return True
    # Horizontal bar, bullet and the four small squares: conventional
    # stand-ins for a base in isolated-mark display.
    return u in (0x2015, 0x2022, 0x25FB, 0x25FC, 0x25FD, 0x25FE)


def is_cgj(u, uisc, udi, ugc, ajt):
    # Also covers the variation selectors and ZWJ: a default-ignorable mark.
    return uisc == "Joiner" or (udi and ugc in ("Mc", "Me", "Mn"))


def is_cons_final(u, uisc, udi, ugc, ajt):
    return (uisc == "Consonant_Final" and ugc != "Lo") or uisc == "Consonant_Succeeding_Repha"


def is_cons_final_mod(u, uisc, udi, ugc, ajt):
    return uisc == "Syllable_Modifier"


def is_cons_med(u, uisc, udi, ugc, ajt):
    # `Consonant_Initial_Postfixed` is new in Unicode 11 and not in the spec.
    return (uisc == "Consonant_Medial" and ugc != "Lo") or uisc == "Consonant_Initial_Postfixed"


def is_cons_mod(u, uisc, udi, ugc, ajt):
    return uisc in ("Nukta", "Gemination_Mark", "Consonant_Killer")


def is_cons_sub(u, uisc, udi, ugc, ajt):
    return uisc == "Consonant_Subjoined" and ugc != "Lo"


def is_cons_with_stacker(u, uisc, udi, ugc, ajt):
    return uisc == "Consonant_With_Stacker"


def is_halant(u, uisc, udi, ugc, ajt):
    return uisc == "Virama" and not is_halant_or_vowel_modifier(u, uisc, udi, ugc, ajt)


def is_halant_or_vowel_modifier(u, uisc, udi, ugc, ajt):
    # Split off HALANT. The Sinhala al-lakuna is a virama that also behaves as
    # a vowel modifier, and the grammar needs to admit it in both slots.
    return u == 0x0DCA


def is_halant_num(u, uisc, udi, ugc, ajt):
    return uisc == "Number_Joiner"


def is_hieroglyph(u, uisc, udi, ugc, ajt):
    return uisc == "Hieroglyph"


def is_hieroglyph_joiner(u, uisc, udi, ugc, ajt):
    return uisc == "Hieroglyph_Joiner"


def is_hieroglyph_mirror(u, uisc, udi, ugc, ajt):
    return uisc == "Hieroglyph_Mirror"


def is_hieroglyph_mod(u, uisc, udi, ugc, ajt):
    return uisc == "Hieroglyph_Modifier"


def is_hieroglyph_segment_begin(u, uisc, udi, ugc, ajt):
    return uisc in ("Hieroglyph_Mark_Begin", "Hieroglyph_Segment_Begin")


def is_hieroglyph_segment_end(u, uisc, udi, ugc, ajt):
    return uisc in ("Hieroglyph_Mark_End", "Hieroglyph_Segment_End")


def is_invisible_stacker(u, uisc, udi, ugc, ajt):
    # Split off HALANT.
    return uisc == "Invisible_Stacker" and not is_sakot(u, uisc, udi, ugc, ajt)


def is_zwnj(u, uisc, udi, ugc, ajt):
    return uisc == "Non_Joiner"


def is_other(u, uisc, udi, ugc, ajt):
    # Also covers what the spec calls BASE_IND and SYM.
    return (
        (ugc == "Po" or uisc in ("Consonant_Dead", "Joiner", "Modifying_Letter", "Other"))
        and not is_base(u, uisc, udi, ugc, ajt)
        and not is_base_other(u, uisc, udi, ugc, ajt)
        and not is_cgj(u, uisc, udi, ugc, ajt)
        and not is_sym_mod(u, uisc, udi, ugc, ajt)
        and not is_word_joiner(u, uisc, udi, ugc, ajt)
    )


def is_reordering_killer(u, uisc, udi, ugc, ajt):
    return uisc == "Reordering_Killer"


def is_repha(u, uisc, udi, ugc, ajt):
    return uisc in ("Consonant_Preceding_Repha", "Consonant_Prefixed")


def is_sakot(u, uisc, udi, ugc, ajt):
    # Split off HALANT. TAI THAM SIGN SAKOT stacks like a virama but may also
    # be followed by another base, which no other virama may.
    return u == 0x1A60


def is_sym_mod(u, uisc, udi, ugc, ajt):
    return uisc == "Symbol_Modifier"


def is_vowel(u, uisc, udi, ugc, ajt):
    return uisc == "Pure_Killer" or (ugc != "Lo" and uisc in ("Vowel", "Vowel_Dependent"))


def is_vowel_mod(u, uisc, udi, ugc, ajt):
    return uisc in ("Tone_Mark", "Cantillation_Mark", "Register_Shifter", "Visarga") or (
        ugc != "Lo" and uisc == "Bindu"
    )


def is_word_joiner(u, uisc, udi, ugc, ajt):
    # Also covers the reserved code points. The excluded list is the Hangul and
    # Musical-symbol fillers, which are default-ignorable but act as bases.
    return (
        udi
        and u not in (0x115F, 0x1160, 0x3164, 0xFFA0, 0x1BCA0, 0x1BCA1, 0x1BCA2, 0x1BCA3)
        and uisc == "Other"
        and not is_cgj(u, uisc, udi, ugc, ajt)
    ) or ugc == "Cn"


# Category -> predicate, in HarfBuzz's order. `F`, `FM`, `M`, `CM`, `V`, `VM`
# and `SM` are *stems*: the positional category below turns each into two to
# four real categories, and none of them is ever written bare.
PREDICATES = {
    "B": is_base,
    "N": is_base_num,
    "GB": is_base_other,
    "CGJ": is_cgj,
    "F": is_cons_final,
    "FM": is_cons_final_mod,
    "M": is_cons_med,
    "CM": is_cons_mod,
    "SUB": is_cons_sub,
    "CS": is_cons_with_stacker,
    "H": is_halant,
    "HVM": is_halant_or_vowel_modifier,
    "HN": is_halant_num,
    "IS": is_invisible_stacker,
    "G": is_hieroglyph,
    "HM": is_hieroglyph_mod,
    "HR": is_hieroglyph_mirror,
    "J": is_hieroglyph_joiner,
    "SB": is_hieroglyph_segment_begin,
    "SE": is_hieroglyph_segment_end,
    "ZWNJ": is_zwnj,
    "O": is_other,
    "RK": is_reordering_killer,
    "R": is_repha,
    "Sk": is_sakot,
    "SM": is_sym_mod,
    "V": is_vowel,
    "VM": is_vowel_mod,
    "WJ": is_word_joiner,
}

# How each stem splits, and which `Indic_Positional_Category` values select
# each suffix. A stem absent from this map takes no suffix.
#
# The interesting rows are the ones that collapse a compound position: a vowel
# that Unicode calls `Top_And_Bottom_And_Right` is `VAbv` to USE, because the
# grammar orders vowels by their *primary* attachment and a compound position
# has to pick one.
POSITIONS = {
    "F": {
        "Abv": ("Top",),
        "Blw": ("Bottom",),
        "Pst": ("Right",),
    },
    "M": {
        "Abv": ("Top",),
        "Blw": ("Bottom", "Bottom_And_Left", "Bottom_And_Right"),
        "Pst": ("Right",),
        "Pre": ("Left", "Top_And_Bottom_And_Left"),
    },
    "CM": {
        "Abv": ("Top",),
        "Blw": ("Bottom", "Overstruck"),
    },
    "V": {
        "Abv": ("Top", "Top_And_Bottom", "Top_And_Bottom_And_Right", "Top_And_Right"),
        "Blw": ("Bottom", "Overstruck", "Bottom_And_Right"),
        "Pst": ("Right",),
        "Pre": ("Left", "Top_And_Left", "Top_And_Left_And_Right", "Left_And_Right"),
    },
    "VM": {
        "Abv": ("Top",),
        "Blw": ("Bottom", "Overstruck"),
        "Pst": ("Right",),
        "Pre": ("Left",),
    },
    "SM": {
        "Abv": ("Top",),
        "Blw": ("Bottom",),
    },
    "FM": {
        "Abv": ("Top",),
        "Blw": ("Bottom",),
        "Pst": ("Not_Applicable",),
    },
}

# The `Indic_Syllabic_Category` fixups HarfBuzz applies before the predicates
# run. Each is a code point Unicode gives a positional category and no syllabic
# one, or gives one the USE grammar cannot use.
SYLLABIC_FIXUPS = {
    **{cp: "Cantillation_Mark" for cp in range(0x1CE2, 0x1CE9)},
    # Tibetan: these have a `Indic_Positional_Category` and no syllabic one.
    **{cp: "Vowel_Dependent" for cp in (0x0F18, 0x0F19, 0x0F3E, 0x0F3F)},
    # Should arguably only be allowed after a nasalization mark.
    0x1CED: "Tone_Mark",
}

# Positional fixups, likewise. See harfbuzz/harfbuzz#1631.
POSITIONAL_FIXUPS = {0x11302: "Top", 0x11303: "Top", 0x114C1: "Top"}

DEFAULT_SYLLABIC = "Other"
DEFAULT_POSITIONAL = "Not_Applicable"
DEFAULT_JOINING = "jt_X"
DEFAULT_GENERAL = "Cn"
DEFAULT_SCRIPT = "Unknown"


def load_inputs(version):
    """Every property the derivation reads, for one Unicode version.

    The `ms-use` overrides are applied on top of the UCD — they are corrections
    to it, not a separate axis — and are *not* versioned with it: HarfBuzz
    vendors one copy and uses it against whatever Unicode it ships with, so the
    same copy is correct for both versions this script derives at.
    """
    print(f"  fetching Unicode {version} …")
    syllabic = parse(fetch_ucd(version, "IndicSyllabicCategory.txt"))
    positional = parse(fetch_ucd(version, "IndicPositionalCategory.txt"))
    joining = {u: "jt_" + v for u, v in parse(fetch_ucd(version, "ArabicShaping.txt"), 2).items()}
    ignorable = set(
        parse(fetch_ucd(version, "DerivedCoreProperties.txt"), keep="Default_Ignorable_Code_Point")
    )
    general = parse(fetch_ucd(version, "UnicodeData.txt"), 2)
    scripts = parse(fetch_ucd(version, "Scripts.txt"))

    for u, v in parse(fetch_ms_use("IndicSyllabicCategory-Additional.txt")).items():
        # See MicrosoftDocs/typography-issues#336: the override file uses a
        # property value that Unicode itself does not have.
        syllabic[u] = "Syllable_Modifier" if v == "Consonant_Final_Modifier" else v
    for u, v in parse(fetch_ms_use("IndicPositionalCategory-Additional.txt")).items():
        positional[u] = "Not_Applicable" if v == "NA" else v

    return syllabic, positional, joining, ignorable, general, scripts


def derive(syllabic, positional, joining, ignorable, general, scripts):
    """`{codepoint: category}` for every code point USE gives a non-`O` answer.

    Only code points named by one of the four *property* files get an entry at
    all: `UnicodeData.txt` and `Scripts.txt` annotate what is already there and
    never introduce a code point. That is what bounds the table — otherwise
    every unassigned code point would be a `WJ` row.
    """
    universe = set(syllabic) | set(positional) | set(joining) | set(ignorable)

    out = {}
    for u in sorted(universe):
        if scripts.get(u, DEFAULT_SCRIPT) in DISABLED_SCRIPTS:
            continue

        uisc = SYLLABIC_FIXUPS.get(u, syllabic.get(u, DEFAULT_SYLLABIC))
        uipc = POSITIONAL_FIXUPS.get(u, positional.get(u, DEFAULT_POSITIONAL))
        ajt = joining.get(u, DEFAULT_JOINING)
        udi = u in ignorable
        ugc = general.get(u, DEFAULT_GENERAL)

        matched = [k for k, p in PREDICATES.items() if p(u, uisc, udi, ugc, ajt)]
        if len(matched) != 1:
            raise SystemExit(
                f"U+{u:04X}: {len(matched)} categories match ({', '.join(matched) or 'none'}); "
                f"uisc={uisc} uipc={uipc} ajt={ajt} udi={udi} ugc={ugc}"
            )
        cat = matched[0]

        split = POSITIONS.get(cat)
        if split:
            suffixes = [s for s, values in split.items() if uipc in values]
            if len(suffixes) != 1:
                raise SystemExit(
                    f"U+{u:04X}: category {cat} has no unique position for "
                    f"uipc={uipc} (matched {suffixes or 'nothing'})"
                )
            cat += suffixes[0]

        if cat != "O":
            out[u] = cat
    return out


def ranges(data):
    """The map collapsed into sorted, disjoint `(first, last, category)` runs."""
    runs = []
    for cp in sorted(data):
        cat = data[cp]
        if runs and runs[-1][2] == cat and runs[-1][1] + 1 == cp:
            runs[-1] = (runs[-1][0], cp, cat)
        else:
            runs.append((cp, cp, cat))
    return runs


# --------------------------------------------------------------------------
# Verification against HarfBuzz's own packed table
# --------------------------------------------------------------------------


ORACLE = os.path.join(
    os.path.dirname(os.path.abspath(__file__)), "..", "..", "..", ".hbref",
    "hb-ot-shaper-use-table.hh",
)

# The generated table names its inputs in a header comment, e.g.
# `# IndicSyllabicCategory-17.0.0.txt`. That is how the Unicode version to
# re-derive at is discovered, rather than being hard-coded next to
# `HARFBUZZ_VERSION` where the two could drift apart silently.
_ORACLE_VERSION = re.compile(r"IndicSyllabicCategory-(\d+\.\d+\.\d+)\.txt")


def verify():
    """Re-derive at HarfBuzz's Unicode version and diff against its own table.

    Returns `True` if the check ran, `False` if `.hbref/` is absent. Any
    difference at all aborts the run: at a matched Unicode version there is no
    such thing as an acceptable one.
    """
    if not os.path.exists(ORACLE):
        print("  (no .hbref/hb-ot-shaper-use-table.hh; skipping the oracle diff)")
        return False

    from hb_use_oracle import load_oracle

    header = open(ORACLE, encoding="utf-8").read(4096)
    m = _ORACLE_VERSION.search(header)
    if not m:
        raise SystemExit(f"{ORACLE}: cannot tell which Unicode it was generated from")
    version = m.group(1)

    print(f"verifying against HarfBuzz {HARFBUZZ_VERSION}'s own table (Unicode {version}) …")
    theirs, limit = load_oracle(ORACLE, CATEGORIES)
    ours = derive(*load_inputs(version))

    # Above HarfBuzz's packed range the accessor returns `O` unconditionally,
    # so anything we derive up there is a difference too.
    bad = [
        (u, ours.get(u, "O"), theirs.get(u, "O") if u < limit else "O")
        for u in sorted(set(ours) | set(theirs))
        if ours.get(u, "O") != (theirs.get(u, "O") if u < limit else "O")
    ]
    if bad:
        for u, a, b in bad[:40]:
            print(f"  MISMATCH U+{u:04X}  ours={a:<6} harfbuzz={b:<6} {unicodedata.name(chr(u), '?')}")
        if len(bad) > 40:
            print(f"  ... and {len(bad) - 40} more")
        raise SystemExit(
            f"{len(bad)} code point(s) disagree with HarfBuzz at its own Unicode "
            "version. The derivation is wrong; the table was not written."
        )

    print(f"  clean: {len(ours)} categorised code points, identical to HarfBuzz")
    return True


def main():
    checked = verify()

    version = unicodedata.unidata_version
    print(f"deriving the shipped table at Unicode {version} …")
    data = derive(*load_inputs(version))
    runs = ranges(data)

    here = os.path.dirname(os.path.abspath(__file__))
    out = os.path.join(here, "..", "src", "universal_tables.rs")
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        w = f.write
        w("//! The Universal Shaping Engine's per-character category.\n")
        w("//!\n")
        w("//! Generated by `gui/font/tools/gen_universal_table.py` from Unicode\n")
        w(f"//! {version} and the `ms-use` override files HarfBuzz vendors. Do not\n")
        w("//! edit: run the script instead.\n")
        w("//!\n")
        w("//! The category is derived from five Unicode properties rather than\n")
        w("//! read from one. See that script for the derivation, for why it\n")
        w("//! follows HarfBuzz to the code point, and for the check that proves\n")
        w("//! it does.\n")
        w("\n")
        w("use crate::universal::Category;\n\n")
        w("/// Category as sorted, disjoint ranges of code points.\n")
        w("///\n")
        w("/// A character in no range is [`Category::Other`], which is the answer\n")
        w("/// for all but a few thousand of them.\n")
        w(f"pub(crate) static USE_RANGES: [(u32, u32, Category); {len(runs)}] = [\n")
        for lo, hi, cat in runs:
            w(f"    (0x{lo:04X}, 0x{hi:04X}, Category::{RUST_CATEGORY[cat]}),\n")
        w("];\n")

    print(f"wrote {out}")
    print(f"  Unicode {version}{'' if checked else '  (UNVERIFIED: no .hbref/)'}")
    print(f"  {len(runs)} ranges, {len(data)} code points")
    for cat in CATEGORIES:
        n = sum(1 for c in data.values() if c == cat)
        if n:
            print(f"  {RUST_CATEGORY[cat]:24} {n}")


if __name__ == "__main__":
    main()
