"""Build a synthetic Khmer font that reports which features reached which
glyph, so the Khmer shaper's masks and stage order have an oracle.

    python gui/font/tools/gen_khmer_probe.py [--out DIR]
    python gui/font/tools/harfbuzz_sweep.py --fonts DIR --corpus khmer-corpus.txt

Why this exists
---------------

`khmer.rs` decides, per glyph, which of eleven features may touch it: `locl`
and `ccmp` everywhere, `pref` on a fronted COENG+RO pair, `blwf`/`abvf`/`pstf`
on everything after the first glyph of a syllable, `cfar` on everything a
fronted pair jumped over, and `pres`/`abvs`/`blws`/`psts` globally. It also
runs its reordering *before* the first lookup rather than after `locl`/`ccmp`,
which is the one structural way Khmer differs from Indic.

None of that is falsifiable against the host's font collection. Of the 556
faces installed here only three register `khmr` at all — the Leelawadee UI
family — and while they do carry `pref`, `blwf`, `abvf` and `pstf`, **not one
face on this machine has a `cfar` lookup**, nor `pres`, nor `psts`. So a
sweep over the host reports agreement about a mask it never applied, which is
worse than no test: it is a green light wired to nothing. Per
`design-decisions.md` §431, the fix is a font that can disagree.

How it reports
--------------

Every feature gets one lookup that rewrites each Khmer glyph as *itself
followed by a marker glyph unique to that feature*:

    feature cfar { sub uni1780 by uni1780 mk_cfar; ... } cfar;

Three properties fall out of that shape, and all three are why it is a
one-to-two substitution rather than the obvious one-to-one:

* **The base survives**, so the next feature's lookup still matches it and
  appends its own marker. A one-to-one substitution would let the first
  feature to fire hide every later one — and `cfar` is applied *after*
  `blwf`, which `reorder_consonant_syllable` sets on every glyph it sets
  `cfar` on, so `cfar` is exactly the feature that would never be seen.
* **The output spells out the answer.** A glyph run reads as the base plus
  one marker per feature that reached it, in the order they were applied, so
  a disagreement names the feature and the position rather than a glyph id.
* **Markers have zero advance**, so a wrong mask shows up as a difference in
  glyphs and never smears into the positions, which keeps the sweep's
  `reordered`/`misplaced` buckets meaningful.

`liga` and `clig` are in the list for a different reason: Khmer's feature
overrides turn `clig` on and `liga` off, and that pair is only observable on a
face that has both. Here both are single-glyph substitutions — unusual for a
ligature feature, and legal — so `mk_clig` must appear and `mk_liga` must not.

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

# The whole Khmer block plus the Khmer Symbols block, rather than the handful
# of characters the corpus uses. Same reason as `gen_thai_legacy.py`: a corpus
# that grows must not silently start comparing two missing-glyph boxes.
KHMER = list(range(0x1780, 0x1800)) + list(range(0x19E0, 0x1A00))

# Characters that are not Khmer but that a Khmer syllable can contain. The
# dotted circle is the one that matters — a broken cluster is given one, and it
# then sits inside the syllable and takes the syllable's masks like any other
# glyph, so it has to be in the lookups' coverage or half the reordering tests
# would compare a glyph nothing could substitute.
EXTRA = [0x0020, 0x0041, 0x200C, 0x200D, 0x25CC]

# In the order `khmer.rs` applies them, which is also the order the markers
# come out in. `locl` and `ccmp` first, then the five per-syllable features,
# then the four global ones; `liga`/`clig` last because they ride the
# unconditional run.
FEATURES = [
    "locl",
    "ccmp",
    "pref",
    "blwf",
    "abvf",
    "pstf",
    "cfar",
    "pres",
    "abvs",
    "blws",
    "psts",
    "liga",
    "clig",
]

# Khmer marks, by the rule the shapers use: general category `Mn`. Zero advance,
# as a real face gives them, so the comparison stays about substitution.
MARKS = frozenset(
    list(range(0x17B4, 0x17B6))  # the two inherent-vowel signs
    + list(range(0x17B7, 0x17BE))  # the above/below vowel signs
    + list(range(0x17C6, 0x17D4))  # nikahit, the signs, coeng, the shifters
    + [0x17DD, 0x200C, 0x200D]
)


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
    """The feature file: one lookup per tag, each appending its own marker."""
    lines = ["languagesystem DFLT dflt;", "languagesystem khmr dflt;", ""]
    for tag in FEATURES:
        lines.append(f"feature {tag} {{")
        # `sub A by A mk;` is a multiple substitution, which is what keeps the
        # base matchable by the next feature. A class cannot express it — the
        # output of a multiple substitution is a fixed sequence — so the rules
        # are written out one per glyph.
        for name in covered:
            lines.append(f"    sub {name} by {name} {marker_name(tag)};")
        lines.append(f"}} {tag};")
        lines.append("")
    return "\n".join(lines)


def build(path, name):
    codepoints = sorted(set(KHMER + EXTRA))
    # Only the Khmer glyphs and the dotted circle are substituted. Latin and
    # the joiners are deliberately left out of coverage: a marker appearing on
    # them would mean a mask leaked out of the syllable it belongs to, and this
    # is the shape that can show it.
    covered = [glyph_name(cp) for cp in codepoints if cp in set(KHMER) | {0x25CC}]
    markers = [marker_name(tag) for tag in FEATURES]
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
        # the run along would turn every mask difference into a position
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
        default=os.path.join(here, "khmer-probe"),
        help="directory to write the face into",
    )
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)
    path = os.path.join(args.out, "KhmerProbe.ttf")
    count = build(path, "KhmerProbe")
    print(f"{path}  {count} glyphs")


if __name__ == "__main__":
    main()
