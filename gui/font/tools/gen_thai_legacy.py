"""Build synthetic pre-OpenType Thai fonts, so the private-use fallback has an
oracle.

    python gui/font/tools/gen_thai_legacy.py [--out DIR]
    python gui/font/tools/harfbuzz_sweep.py --fonts DIR --corpus thai-pua.txt

Why this exists
---------------

`thai::pua_shape` implements the half of Thai shaping that only applies to a
font with no Thai `GSUB`: the shifted tone-mark forms such a font ships in the
private use area, at U+F700 for Windows and U+F880 for the Mac. It is
unfalsifiable against the host's font collection, because **not one** of the
556 faces installed here has a single one of those glyphs — every Thai font
that ships with Windows today describes its own shaping in `GSUB`, which turns
the fallback off. A sweep over them reports agreement it never tested.

So the oracle needs a font that does not exist on this machine, and the honest
way to get one is to build it: a face with the Thai block, the private-use
block, and deliberately no `GSUB` at all. HarfBuzz then takes its fallback
path over the same face this crate does, and the two answers are comparable.

Three faces are generated, and the third is as load-bearing as the first two:

* `ThaiLegacyWin` — the Windows private-use forms only.
* `ThaiLegacyMac` — the Mac forms only, which is how the vendor preference
  gets tested rather than assumed.
* `ThaiNoPua` — Thai glyphs, no `GSUB`, and no private-use forms. The pass
  runs and must find nothing, which is the case every real modern face that
  lacks a Thai `GSUB` is in. A fallback that damaged *this* face would damage
  a lot of real ones.

The glyphs are boxes. Shaping does not care what a glyph looks like, and a box
per character makes each glyph id distinct so a substitution is visible in the
output.
"""

import argparse
import os
import sys

try:
    from fontTools.fontBuilder import FontBuilder
    from fontTools.pens.ttGlyphPen import TTGlyphPen
except ImportError:  # pragma: no cover - a missing builder is a setup error
    sys.exit("fontTools is not installed: pip install fonttools")

UPEM = 1000

# Every Thai character a shaping test can reach: the consonants, the vowels
# above and below, the tone marks, and the pre-base vowels. Deliberately the
# whole block rather than the handful the corpus uses, so that adding a string
# to the corpus does not silently start comparing two missing-glyph boxes.
THAI = (
    list(range(0x0E01, 0x0E3B))  # consonants and the vowels/marks after them
    + [0x0E3F]
    + list(range(0x0E40, 0x0E4F))  # pre-base vowels, tone marks, nikhahit
    + list(range(0x0E50, 0x0E5A))  # digits
)

# The Windows private-use forms, from `hb-ot-shaper-thai.cc`. Kept as a flat
# list because this file only has to *have* the glyphs; which one stands for
# what is `thai.rs`'s business and is what the sweep is checking.
WIN_PUA = [
    0xF700, 0xF701, 0xF702, 0xF703, 0xF704, 0xF705, 0xF706, 0xF707,
    0xF708, 0xF709, 0xF70A, 0xF70B, 0xF70C, 0xF70D, 0xF70E, 0xF70F,
    0xF710, 0xF711, 0xF712, 0xF713, 0xF714, 0xF715, 0xF716, 0xF717,
    0xF718, 0xF719, 0xF71A,
]

MAC_PUA = [
    0xF884, 0xF885, 0xF886, 0xF887, 0xF888, 0xF889, 0xF88A, 0xF88B,
    0xF88C, 0xF88D, 0xF88E, 0xF88F, 0xF890, 0xF891, 0xF892, 0xF893,
    0xF894, 0xF895, 0xF896, 0xF897, 0xF898, 0xF899, 0xF89A, 0xF89B,
    0xF89C, 0xF89D, 0xF89E,
]

# A Thai mark, by the same rule the shapers use: general category `Mn`. These
# get no advance, which is what a real Thai font does and what keeps the
# comparison about substitution rather than about width.
MARKS = frozenset(
    [0x0E31]
    + list(range(0x0E34, 0x0E3B))
    + list(range(0x0E47, 0x0E4F))
    + WIN_PUA
    + MAC_PUA
) - frozenset([0xF700, 0xF70F, 0xF89A, 0xF89E])  # the de-descended consonants


def glyph_name(cp):
    return f"uni{cp:04X}"


def box(pen, x0, y0, x1, y1):
    pen.moveTo((x0, y0))
    pen.lineTo((x1, y0))
    pen.lineTo((x1, y1))
    pen.lineTo((x0, y1))
    pen.closePath()


def build(path, name, pua):
    """Write one face: the Thai block, `pua`, and no layout tables at all."""
    codepoints = [0x0020] + THAI + list(pua)
    order = [".notdef"] + [glyph_name(cp) for cp in codepoints]

    glyphs = {}
    metrics = {}
    pen = TTGlyphPen(None)
    box(pen, 50, 0, 550, 700)
    glyphs[".notdef"] = pen.glyph()
    metrics[".notdef"] = (600, 50)
    for i, cp in enumerate(codepoints):
        pen = TTGlyphPen(None)
        if cp == 0x0020:
            metrics[glyph_name(cp)] = (300, 0)
            glyphs[glyph_name(cp)] = pen.glyph()
            continue
        if cp in MARKS:
            # A mark: drawn above the line, no advance. The width varies with
            # the glyph so that a wrong substitution shows up as a wrong
            # *position* too, not only as a wrong id.
            width = 100 + (i % 7) * 20
            box(pen, -width, 720, 0, 900)
            metrics[glyph_name(cp)] = (0, -width)
        else:
            box(pen, 50, 0, 550, 700)
            metrics[glyph_name(cp)] = (600, 50)
        glyphs[glyph_name(cp)] = pen.glyph()

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
    # No `GSUB`, no `GPOS`, no `GDEF`. That absence is the whole point: it is
    # what makes both engines take their Thai fallback path.
    fb.save(path)
    return len(order)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    here = os.path.dirname(os.path.abspath(__file__))
    ap.add_argument(
        "--out",
        default=os.path.join(here, "thai-legacy"),
        help="directory to write the faces into",
    )
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)
    for name, pua in [
        ("ThaiLegacyWin", WIN_PUA),
        ("ThaiLegacyMac", MAC_PUA),
        ("ThaiNoPua", []),
    ]:
        path = os.path.join(args.out, f"{name}.ttf")
        count = build(path, name, pua)
        print(f"{path}  {count} glyphs")


if __name__ == "__main__":
    main()
