"""Build a synthetic Myanmar font that reports which features reached which
glyph, and in what order, so the Myanmar shaper's six stages have an oracle.

    python gui/font/tools/gen_myanmar_probe.py [--out DIR]
    python gui/font/tools/harfbuzz_sweep.py --fonts DIR --corpus myanmar-corpus.txt

Why this exists
---------------

`myanmar.rs` claims three things the host's fonts cannot contradict.

* **Six stages, not one.** `locl` and `ccmp` together, then `rphf`, `pref`,
  `blwf` and `pstf` each *alone with a pause after it*, then everything else.
  The pauses are the whole difference from Khmer, which runs its four basic
  features in a single pass, and they exist so a `pref` rule can be written
  against the glyph `rphf` left behind.
* **Per-syllable confinement, and then not.** The two pre-reordering features
  and the four basic ones are `F_PER_SYLLABLE`: their lookups may not match
  across a syllable boundary. The four global ones may, which is why the
  shaper clears the syllable stamps before them
  (harfbuzz/harfbuzz#3531).
* **`F_MANUAL_ZWJ` without `F_MANUAL_ZWNJ`.** Alone among the shapers, Myanmar
  marks eight of its thirteen features so that a lookup must *name* a ZWJ to
  match across one, while still stepping over a ZWNJ as though it were not
  there. Indic and Khmer take `F_MANUAL_JOINERS`, which is both bits, so a rule
  copied from either is wrong here in a way no table dump shows.

None of the three is falsifiable against the host's font collection. Of the
556 faces installed here only two register `mym2` — `mmrtext.ttf` and
`mmrtextb.ttf` — and a sweep over them reports 58 of 58 agreeing about a
*result*, never about which feature produced it or when. Per
`design-decisions.md` §431, the fix is a font that can disagree.

How it reports
--------------

Every feature gets one lookup that rewrites each Myanmar glyph as *itself
followed by a marker glyph unique to that feature*, exactly as
`gen_khmer_probe.py` does and for the same three reasons: the base survives so
the next feature still matches it, the output spells the answer out as a base
plus one marker per feature that reached it, and markers have zero advance so a
wrong answer never smears into the positions.

Because each new marker is inserted *directly after its base*, the markers come
out in the reverse of the order they were applied. **That sequence is the test
of the stage order**, and it is only a test because of the next paragraph.

Why the feature blocks are written backwards
--------------------------------------------

A shaper has two plausible orders to run lookups in: the order the *features*
are applied, and the order the *lookups* are numbered in the font. HarfBuzz
uses both — lookups are sorted by (stage, lookup index) — and a face whose
lookup indices happen to ascend in application order cannot tell the two apart.
Written in application order, as the Khmer probe is, this file would be exactly
that face: green against an implementation that ignored stages entirely.

So the blocks are emitted in **reverse** application order, which makes lookup
index order the exact opposite of stage order. Now the six stages are load
bearing: get them wrong and every marker sequence in the corpus comes out
backwards. It also pins the *other* half of the rule, since `pres`, `abvs`,
`blws`, `psts`, `calt`, `liga` and `clig` all share the last stage — within
which the answer really is lookup index order, so those seven must come out in
file order while the six before them must not.

`liga` and `clig` are here for a second reason. Khmer's feature overrides turn
`clig` on and `liga` off; Myanmar has no `override_features` at all, so both
must fire, and a Khmer rule copied onto Myanmar shows up as a missing
`mk_liga`.

The five extra lookups
----------------------

Beyond the per-feature markers there are three contextual lookups and two
ligatures. They run on the font's **quiet glyphs** — SPACE and the six Myanmar
signs U+104A..U+104F — which are deliberately left out of the marker rules,
because they are then the only glyphs in this font still adjacent to their
neighbours after thirteen features have each inserted something. Every other
context here is broken up by markers within one stage of the run starting. The
signs are Myanmar rather than Latin so that they cannot be split off into a
script run of their own, where the Myanmar shaper would never see them.

Confinement, from a pair of rules that differ only in which feature holds them:

* `mk_xsyl_blwf` — fires from `blwf`, which is per-syllable, on `SPACE KA'`.
  Space and the consonant after it are different syllables, so this marker must
  **never** appear.
* `mk_xsyl_blws` — the same rule from `blws`, which is global, and which must
  **always** produce it. The pair is what distinguishes confinement from a
  lookup that merely failed to match.

The joiners, which need two shapes because HarfBuzz answers the question
differently on each side of a chain rule. `skippy_iter_t::init` sets
`ignore_zwj = context_match || auto_zwj` and
`ignore_zwnj = is_gpos || (context_match && auto_zwnj)`, so:

* `mk_zwnj` — a chain rule in `psts` on `SPACE AFOREMENTIONED'`. Inside a
  *context* a ZWNJ is stepped over exactly when `auto_zwnj` holds, which for
  Myanmar it does — so with `SPACE ZWNJ AFOREMENTIONED` this must fire. Give
  Myanmar Khmer's `F_MANUAL_JOINERS` by mistake and it stops firing. (A ZWJ in
  the same slot proves nothing: a context ignores ZWJ unconditionally.)
* `mk_lig_psts` / `mk_lig_calt` — the same two-glyph ligature held by `psts`,
  which is `F_MANUAL_ZWJ`, and by `calt`, which is not. A ligature's *input* is
  not a context, so here `ignore_zwj` is `auto_zwj` alone: with a ZWJ between
  the two glyphs the `calt` one must ligate and the `psts` one must not. This
  is the only place in the font where `F_MANUAL_ZWJ` is observable at all.
  They are given different glyph pairs because they share the last stage, where
  `calt` is numbered first and would otherwise eat the input.

What it caught, and what it is known to miss
--------------------------------------------

The shaper passed all 48 corpus strings on the first run, which is the result
`design-decisions.md` §431 says to distrust: a probe that has never been red is
indistinguishable from a probe that cannot go red. So each claim above was
falsified in turn by mutating `myanmar.rs` and re-sweeping. Measured
2026-08-16, agreeing lines out of 48:

| Mutation | Result |
|---|---|
| the four basic features share one stage | 11 — 37 lines reordered |
| `per_syllable: 0` | 46 — the two confinement lines differ |
| `manual_zwnj: all` (Khmer's flags) | 47 — the one ZWNJ-in-context line |
| `manual_zwj: 0` | 47 — the one ZWJ-in-ligature line |
| reorder at the stage-1 pause, not stage-0 | 40 — 4 reordered, 4 differ |
| `syllabic::clear` at the stage-5 pause | **48 — not caught** |

The last is not a hole in the probe: it is a mutation with no effect. Clearing
the syllable stamps only matters to a lookup whose feature is per-syllable, and
by that pause every such feature has already run — so the stamps it leaves
standing are read by nothing. HarfBuzz clears them at the same point for the
same reason, and a face that could tell the two apart does not exist.

The glyphs are boxes. Shaping does not care what a glyph looks like.
"""

import argparse
import os
import sys

try:
    from fontTools.fontBuilder import FontBuilder
    from fontTools.feaLib.builder import addOpenTypeFeaturesFromString
    from fontTools.pens.ttGlyphPen import TTGlyphPen
except ImportError:  # pragma: no cover - a missing builder is a setup error
    sys.exit("fontTools is not installed: pip install fonttools")

UPEM = 1000

# The whole Myanmar block plus both extensions, rather than the characters the
# corpus happens to use. Same reason as `gen_khmer_probe.py`: a corpus that
# grows must not silently start comparing two missing-glyph boxes. Extended-C
# (U+116D0) is left out because it is outside the BMP and the shaper's category
# table does not reach it.
MYANMAR = list(range(0x1000, 0x10A0)) + list(range(0xA9E0, 0xAA00)) + list(range(0xAA60, 0xAA80))

# Not Myanmar, but reachable inside or beside a Myanmar run. The dotted circle
# is given to a broken cluster and then takes the syllable's features like any
# other glyph, so it has to be in coverage. The joiners and the space are in
# the `cmap` but *not* in coverage: a marker on one would mean a feature
# reached a glyph that is not a letter.
EXTRA = [0x0020, 0x0041, 0x200C, 0x200D, 0x25CC]

# The Myanmar signs, kept out of the marker rules so that the contextual and
# ligature lookups have glyphs that stay adjacent to their neighbours. See the
# module docstring. U+104A..U+104F are the section marks, the locative, the
# completed, the aforementioned and the genitive: punctuation, not letters, so
# nothing in the syllable grammar wants them anyway.
QUIET = list(range(0x104A, 0x1050))

# In the order `myanmar.rs` applies them, which is also the order the markers
# come out in — reversed, since each is inserted directly after its base.
# `locl` and `ccmp` share the first stage; the four basic features get one
# stage each; the last seven share the last one.
FEATURES = [
    "locl",
    "ccmp",
    "rphf",
    "pref",
    "blwf",
    "pstf",
    "pres",
    "abvs",
    "blws",
    "psts",
    "calt",
    "liga",
    "clig",
]

# Where the six stages fall, as the count of features in each. Only used to
# document the file; the shaper is what decides.
STAGES = [2, 1, 1, 1, 1, 7]

# Myanmar's non-spacing marks, general category `Mn`, which get zero advance as
# a real face gives them. The spacing marks (`Mc`: U+102B, U+102C, U+1031,
# U+1038, U+103B, U+103C, U+1056, U+1057, U+1084, U+AA7B, U+AA7D) deliberately
# keep an advance, because they have one — including MEDIAL RA, whose move is
# the most visible thing this script does.
MARKS = frozenset(
    list(range(0x102D, 0x1031))  # vowel signs I, II, U, UU
    + list(range(0x1032, 0x1038))  # AI, MON II, MON O, E ABOVE, anusvara, dot below
    + [0x1039, 0x103A]  # virama and asat
    + [0x103D, 0x103E]  # medial WA and medial HA
    + [0x1058, 0x1059]  # vocalic L and LL
    + list(range(0x105E, 0x1061))  # the three Mon medials
    + list(range(0x1071, 0x1075))  # Geba Karen I and the three Kayah vowels
    + [0x1082, 0x1085, 0x1086, 0x108D, 0x109D]  # Shan and Aiton
    + [0xA9E5, 0xAA7C]  # Tai Laing and Extended-A
    + [0x200C, 0x200D]
)

# The contextual lookups: (name, feature, context glyph, target glyph). All
# three are `SPACE X'`, because a preceding space is the only context in this
# font that survives the marker insertions.
CONTEXTS = [
    ("xsyl_blwf", "blwf", 0x0020, 0x1000),
    ("xsyl_blws", "blws", 0x0020, 0x1000),
    ("zwnj", "psts", 0x0020, 0x104E),
]

# The ligature lookups: (name, feature, first glyph, second glyph). One in a
# feature the shaper marks `F_MANUAL_ZWJ` and one in a feature it does not, on
# disjoint pairs so that the earlier-numbered lookup cannot eat the later one's
# input. A ligature is the only lookup whose *input* matching sees `auto_zwj`.
LIGATURES = [
    ("lig_calt", "calt", 0x104A, 0x104B),
    ("lig_psts", "psts", 0x104C, 0x104D),
]


def glyph_name(cp):
    return f"uni{cp:04X}"


def marker_name(tag):
    return f"mk_{tag}"


def box(pen, x0, y0, x1, y1):
    pen.moveTo((x0, y0))
    pen.lineTo((x1, y0))
    pen.lineTo((x1, y1))
    pen.lineTo((x0, y1))
    pen.closePath()


def feature_code(covered):
    """The feature file: one lookup per tag, each appending its own marker.

    Emitted in reverse application order on purpose — see the module docstring.
    The contextual lookups' replacements are declared standalone and referenced
    from inside their feature, because feaLib only accepts a *single*
    substitution inline in a chain rule and these are one-to-two like every
    other rule here.
    """
    lines = ["languagesystem DFLT dflt;", "languagesystem mym2 dflt;", ""]
    for name, _feature, _before, target in CONTEXTS:
        lines.append(f"lookup {name}_inner {{")
        lines.append(f"    sub {glyph_name(target)} by {glyph_name(target)} {marker_name(name)};")
        lines.append(f"}} {name}_inner;")
        lines.append("")
    for tag in reversed(FEATURES):
        lines.append(f"feature {tag} {{")
        # `sub A by A mk;` is a multiple substitution, which is what keeps the
        # base matchable by the next feature. A class cannot express it — the
        # output of a multiple substitution is a fixed sequence — so the rules
        # are written out one per glyph.
        for name in covered:
            lines.append(f"    sub {name} by {name} {marker_name(tag)};")
        for name, feature, before, target in CONTEXTS:
            if feature != tag:
                continue
            lines.append(
                f"    sub {glyph_name(before)} {glyph_name(target)}' lookup {name}_inner;"
            )
        for name, feature, first, second in LIGATURES:
            if feature != tag:
                continue
            lines.append(
                f"    sub {glyph_name(first)} {glyph_name(second)} by {marker_name(name)};"
            )
        lines.append(f"}} {tag};")
        lines.append("")
    return "\n".join(lines)


def build(path, name):
    codepoints = sorted(set(MYANMAR + EXTRA))
    # Only the Myanmar letters and the dotted circle are substituted. Latin,
    # the space, the joiners and the six Myanmar signs are deliberately left
    # out: a marker on one of the first three would mean a feature reached a
    # glyph that is not a letter, and the signs are the extra lookups' quiet
    # glyphs.
    quiet = set(QUIET)
    lettered = set(MYANMAR) | {0x25CC}
    covered = [glyph_name(cp) for cp in codepoints if cp in lettered and cp not in quiet]
    markers = [marker_name(tag) for tag in FEATURES]
    markers += [marker_name(name) for name, _f, _b, _t in CONTEXTS]
    markers += [marker_name(name) for name, _f, _a, _b in LIGATURES]
    order = [".notdef"] + [glyph_name(cp) for cp in codepoints] + markers

    glyphs = {}
    metrics = {}

    pen = TTGlyphPen(None)
    box(pen, 50, 0, 550, 700)
    glyphs[".notdef"] = pen.glyph()
    metrics[".notdef"] = (600, 50)

    for cp in codepoints:
        pen = TTGlyphPen(None)
        gname = glyph_name(cp)
        if cp == 0x0020:
            metrics[gname] = (300, 0)
        elif cp in MARKS:
            box(pen, -200, 720, 0, 900)
            metrics[gname] = (0, -200)
        else:
            box(pen, 50, 0, 550, 700)
            metrics[gname] = (600, 50)
        glyphs[gname] = pen.glyph()

    for i, mk in enumerate(markers):
        # Zero advance, so which features fired can never move anything: the
        # sweep compares positions as well as glyphs, and a marker that pushed
        # the run along would turn every stage-order difference into a position
        # difference too.
        pen = TTGlyphPen(None)
        box(pen, 0, 900 + i * 10, 100, 910 + i * 10)
        glyphs[mk] = pen.glyph()
        metrics[mk] = (0, 0)

    fb = FontBuilder(UPEM, isTTF=True)
    fb.setupGlyphOrder(order)
    fb.setupCharacterMap({cp: glyph_name(cp) for cp in codepoints})
    fb.setupGlyf(glyphs)
    fb.setupHorizontalMetrics(metrics)
    fb.setupHorizontalHeader(ascent=800, descent=-200)
    fb.setupNameTable(
        {
            "familyName": name,
            "styleName": "Regular",
            "psName": name.replace(" ", ""),
            "fullName": name,
            "version": "1.0",
            "uniqueFontIdentifier": f"{name};1.0",
        }
    )
    fb.setupOS2(sTypoAscender=800, sTypoDescender=-200, usWinAscent=800, usWinDescent=200)
    fb.setupPost()
    addOpenTypeFeaturesFromString(fb.font, feature_code(covered))
    fb.save(path)
    return len(order)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    here = os.path.dirname(os.path.abspath(__file__))
    ap.add_argument(
        "--out",
        default=os.path.join(here, "myanmar-probe"),
        help="directory to write the face into",
    )
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)
    path = os.path.join(args.out, "MyanmarProbe.ttf")
    count = build(path, "MyanmarProbe")
    assert sum(STAGES) == len(FEATURES), "STAGES does not account for every feature"
    print(f"{path}  {count} glyphs")


if __name__ == "__main__":
    main()
