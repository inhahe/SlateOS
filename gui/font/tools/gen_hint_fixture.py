r"""Generate `gui/font/src/hint/fixture.rs`: two small fonts and where FreeType's
auto-hinter puts their points.

    python gui/font/tools/gen_hint_fixture.py

Needs `fonttools` and `freetype-py` (whose wheels bundle FreeType 2.13.2 with
HarfBuzz -- the release the hinter in `src/hint/` is a port of).

What the fixture is for
-----------------------

`tools/hint_oracle.py` checks the hinter against FreeType on whole installed
fonts, and is the real verification; but it needs FreeType and the fonts, so
it cannot run as a unit test. This is its checked-in residue: a synthetic face
drawn to exercise each rule the port reproduces, and FreeType's answer for it,
so that `cargo test` catches a regression without either.

The face has every Latin reference letter, each drawn so that its extreme is
flat or round as the rule under test needs:

* capitals: flat tops at 700 and round ones overshooting to 712; flat bottoms
  at 0 and round ones to -12;
* ascenders: flat at 740 (b d h k), round at 752 (f) -- and the dots of i and
  j round at 700, *below* the flat ascenders, which is FreeType's
  "overshoot on the wrong side" case: both lines become their mean;
* x-height: flat at 500 (u v x z n r), round to 512 (o e s c); bottoms flat at
  0 and round to -12; descenders flat at -200 (p q), round to -212 (g j y);
* horizontal stems of several heights (the bars of H, E, e, the hyphen) and
  slab serifs (I), a composite (e acute) and a combining mark, which is never
  snapped to a zone;
* and `w`, drawn in 16.16 fractions of a unit, for what FreeType's CFF loader
  does with them (`CffPoints` in `src/sfnt.rs`): every coordinate floored --
  so -0.387 is -1 -- a line of no length dropped, whether it has none at all
  or none in 1024ths of a unit, and a contour ending a 65536th short of its
  start folded, where one ending a 1024th short is not. As TrueType it is
  rounded, which leaves it with coincident points instead;
* and two of FreeType's *feature styles*: small capitals (`smcp` turns h x z
  o e s into `.sc` glyphs at a 560 cap height, flat and round), so that
  `latn_smcp` claims them and measures its zones from them -- its capitals'
  zones dropped, since `smcp` leaves capitals alone -- and superscripts
  (`sups` turns x and o into `.sups` glyphs; `x.sups` is also raised 300
  units by a `sups` positioning rule). A glyph a feature both substitutes and
  positions is no feature style's, so `x.sups` falls to `latn_dflt`, while
  the raise still counts when `latn_sups` measures its zones -- and decides
  whether its top zone exists at all.

It is written twice: as TrueType (quadratic, clockwise) and as CFF (cubic,
counter-clockwise), since the two reach the hinter through different loaders
and point conventions. For each, at every size in `SIZES`, every glyph's
hinted points are recorded in 1/64 pixel, both coordinates.

And the TrueType face once more as a variable font (`build_var`): Regular and
a Bold master whose every point, accent offset, scaled component and bearing
moves, so that at weight 610 each delta lands on a fraction -- once with
`HVAR` and once without, since only without it does `gvar` move a glyph's
origin. Its answers are taken at that weight, at `VAR_SIZES`.

And the CFF face made variable too (`build_var_cff2`), which makes a `CFF2`
table: its glyphs vary through their charstrings' `blend` operators, which
FreeType weighs in 16.16 and HarfBuzz in floating point. Its Bold master
moves its points as the TrueType one does, fractions kept where the glyph is
drawn without rounding, so that the two libraries' blends can part. Beside
FreeType's hinted points at weight 610, HarfBuzz's drawing of each glyph
there is recorded -- its box and its path -- since that is what the crate
draws and measures from.
"""

import io
import os

import freetype
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.cu2quPen import Cu2QuPen
from fontTools.pens.recordingPen import RecordingPen
from fontTools.pens.reverseContourPen import ReverseContourPen
from fontTools.pens.t2CharStringPen import T2CharStringPen
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.ttGlyphPen import TTGlyphPen

from rustfmt_out import rustfmt

# Whole sizes from 8 to 30 -- chosen so the x-height's fraction of a pixel
# falls in every band the rounding rules separate (26, 28 and 30 are the ones
# where rounding up from 40/64 rather than, say, 44/64 changes the answer) --
# and 8, 11 and 13 points at 96 dpi, which are fractional.
SIZES = [8, 9, 10, 11, 12, 13, 14, 15, 16, 18, 20, 24, 26, 28, 30, 10.666667, 14.666667, 17.333333]
UPEM = 1000


def poly(pen, pts):
    pen.moveTo(pts[0])
    for p in pts[1:]:
        pen.lineTo(p)
    pen.closePath()


def rect(pen, x0, y0, x1, y1):
    # Clockwise with y up: up the left side first.
    poly(pen, [(x0, y0), (x0, y1), (x1, y1), (x1, y0)])


def ellipse(pen, cx, cy, rx, ry, clockwise=True):
    """An ellipse of four quadratic arcs: on-curve at the four extremes, the
    control points at the corners of the bounding box -- so the top and bottom
    are on-curve points with off-curve neighbours, i.e. round."""
    left, right, top, bottom = (cx - rx, cy), (cx + rx, cy), (cx, cy + ry), (cx, cy - ry)
    tl, tr, br, bl = (cx - rx, cy + ry), (cx + rx, cy + ry), (cx + rx, cy - ry), (cx - rx, cy - ry)
    pen.moveTo(left)
    if clockwise:
        pen.qCurveTo(tl, top)
        pen.qCurveTo(tr, right)
        pen.qCurveTo(br, bottom)
        pen.qCurveTo(bl, left)
    else:
        pen.qCurveTo(bl, bottom)
        pen.qCurveTo(br, right)
        pen.qCurveTo(tr, top)
        pen.qCurveTo(tl, left)
    pen.closePath()


def ring(pen, cx, cy, rx, ry, stem_x, stem_y):
    ellipse(pen, cx, cy, rx, ry, clockwise=True)
    ellipse(pen, cx, cy, rx - stem_x, ry - stem_y, clockwise=False)


def dot(pen, cx, top, r):
    ellipse(pen, cx, top - r, r, r)


# A 65536th of a unit, the finest step a charstring's 16.16 operands take.
F = 1 / 65536
# 0.387 to the nearest 65536th: as a height below the baseline, FreeType's CFF
# loader floors it to -1 where rounding would give 0.
A = 25363 * F


def fractional(pen):
    """`w`: drawn for CFF, counter-clockwise, in exact 16.16 fractions."""
    pen.moveTo((-10.25, -A))
    pen.lineTo((140.5, -A))
    pen.lineTo((140.5 + 3 * F, 250))
    # No length in 1024ths of a unit, which is where FreeType measures it.
    pen.lineTo((140.5, 250 + F))
    pen.lineTo((140.5, 499.75))
    # No length at all.
    pen.lineTo((140.5, 499.75))
    pen.lineTo((60.25, 499.75))
    # A 65536th short of the start: folded into it.
    pen.lineTo((-10.25 - F, -A - F))
    pen.closePath()
    pen.moveTo((300.5, 100.125))
    pen.lineTo((420.75, 100.125))
    pen.lineTo((420.75, 380.5))
    pen.curveTo((400.25, 420.375), (320.125, 420.375), (300.5, 380.5))
    # A 1024th short of the start: kept.
    pen.lineTo((300.5 + 1 / 1024, 100.125 + 1 / 1024))
    pen.closePath()


# Glyphs drawn the CFF way round -- counter-clockwise, and in fractions no
# rounding may touch -- rather than the TrueType way.
CFF_NATIVE = {"w"}


# Every glyph: name, code point (None for a component only), drawing.
def glyphs():
    g = {}

    # Capitals: flat at 700/0, round at 712/-12.
    g["T"] = (ord("T"), lambda p: poly(p, [(20, 620), (20, 700), (580, 700), (580, 620),
                                             (340, 620), (340, 0), (260, 0), (260, 620)]))
    g["H"] = (ord("H"), lambda p: poly(p, [(60, 0), (60, 700), (140, 700), (140, 390), (460, 390),
                                             (460, 700), (540, 700), (540, 0), (460, 0), (460, 320),
                                             (140, 320), (140, 0)]))
    g["E"] = (ord("E"), lambda p: poly(p, [(60, 0), (60, 700), (520, 700), (520, 630), (140, 630),
                                             (140, 390), (480, 390), (480, 320), (140, 320),
                                             (140, 70), (530, 70), (530, 0)]))
    g["Z"] = (ord("Z"), lambda p: poly(p, [(50, 0), (50, 70), (430, 630), (60, 630), (60, 700),
                                             (540, 700), (540, 630), (160, 70), (550, 70), (550, 0)]))
    g["L"] = (ord("L"), lambda p: poly(p, [(60, 0), (60, 700), (140, 700), (140, 70), (520, 70),
                                             (520, 0)]))
    g["O"] = (ord("O"), lambda p: ring(p, 330, 350, 290, 362, 80, 75))
    g["C"] = (ord("C"), lambda p: ring(p, 320, 350, 270, 362, 70, 70))
    g["Q"] = (ord("Q"), lambda p: (ring(p, 330, 350, 290, 362, 80, 75),
                                   rect(p, 400, -140, 470, 60)))
    g["S"] = (ord("S"), lambda p: (ellipse(p, 290, 530, 220, 182), ellipse(p, 290, 170, 230, 182)))
    g["U"] = (ord("U"), lambda p: (rect(p, 60, 250, 140, 700), rect(p, 460, 250, 540, 700),
                                   ring(p, 300, 250, 240, 262, 80, 70)))

    # Ascenders: flat at 740, f round at 752; the dots of i and j round at
    # 700, on the wrong side of the flat ascenders.
    g["b"] = (ord("b"), lambda p: (rect(p, 60, 0, 140, 740), ring(p, 290, 250, 220, 262, 70, 70)))
    g["d"] = (ord("d"), lambda p: (rect(p, 420, 0, 500, 740), ring(p, 270, 250, 220, 262, 70, 70)))
    g["h"] = (ord("h"), lambda p: (rect(p, 60, 0, 140, 740), rect(p, 400, 0, 480, 380),
                                   rect(p, 140, 380, 480, 450)))
    g["k"] = (ord("k"), lambda p: (rect(p, 60, 0, 140, 740),
                                   poly(p, [(140, 200), (140, 290), (420, 500), (500, 500),
                                            (230, 290), (510, 0), (420, 0), (180, 250)])))
    g["f"] = (ord("f"), lambda p: (rect(p, 100, 0, 180, 620), ellipse(p, 260, 686, 160, 66),
                                   rect(p, 20, 430, 340, 500)))
    g["i"] = (ord("i"), lambda p: (rect(p, 60, 0, 140, 500), dot(p, 100, 700, 55)))
    g["j"] = (ord("j"), lambda p: (rect(p, 60, -140, 140, 500), ellipse(p, 20, -150, 120, 62),
                                   dot(p, 100, 700, 55)))

    # x-height: flat at 500, round to 512; bottoms flat at 0, round to -12.
    g["u"] = (ord("u"), lambda p: (rect(p, 60, 150, 140, 500), rect(p, 400, 0, 480, 500),
                                   ring(p, 250, 150, 190, 162, 70, 60)))
    g["v"] = (ord("v"), lambda p: poly(p, [(20, 500), (110, 500), (260, 90), (410, 500), (500, 500),
                                             (310, 0), (210, 0)]))
    g["x"] = (ord("x"), lambda p: poly(p, [(20, 0), (200, 250), (30, 500), (120, 500), (250, 320),
                                             (380, 500), (470, 500), (300, 250), (480, 0), (390, 0),
                                             (250, 180), (110, 0)]))
    g["z"] = (ord("z"), lambda p: poly(p, [(40, 0), (40, 60), (350, 440), (50, 440), (50, 500),
                                             (450, 500), (450, 440), (150, 60), (460, 60), (460, 0)]))
    g["n"] = (ord("n"), lambda p: (rect(p, 60, 0, 140, 500), rect(p, 400, 0, 480, 380),
                                   rect(p, 140, 380, 480, 500)))
    g["r"] = (ord("r"), lambda p: (rect(p, 60, 0, 140, 500), rect(p, 140, 420, 330, 500)))
    g["o"] = (ord("o"), lambda p: ring(p, 270, 250, 230, 262, 75, 70))
    g["e"] = (ord("e"), lambda p: (ring(p, 270, 250, 230, 262, 75, 70),
                                   rect(p, 80, 230, 460, 290)))
    g["s"] = (ord("s"), lambda p: (ellipse(p, 250, 380, 190, 132), ellipse(p, 250, 120, 200, 132)))
    g["c"] = (ord("c"), lambda p: ring(p, 260, 250, 220, 262, 70, 70))

    # Descenders: flat at -200, round to -212.
    g["p"] = (ord("p"), lambda p: (rect(p, 60, -200, 140, 500), ring(p, 290, 250, 220, 262, 70, 70)))
    g["q"] = (ord("q"), lambda p: (rect(p, 400, -200, 480, 500), ring(p, 250, 250, 220, 262, 70, 70)))
    g["g"] = (ord("g"), lambda p: (ring(p, 250, 250, 220, 262, 70, 70), rect(p, 400, -100, 480, 500),
                                   ellipse(p, 260, -150, 220, 62)))
    g["y"] = (ord("y"), lambda p: (poly(p, [(20, 500), (110, 500), (260, 90), (410, 500), (500, 500),
                                              (240, -120), (160, -120)]),
                                   ellipse(p, 150, -150, 110, 62)))

    # The rest: a bar at no zone's height, slab serifs, a spacing accent and
    # a combining one.
    g["hyphen"] = (ord("-"), lambda p: rect(p, 40, 230, 300, 290))
    g["I"] = (ord("I"), lambda p: poly(p, [(40, 0), (40, 50), (140, 50), (140, 650), (40, 650),
                                             (40, 700), (320, 700), (320, 650), (220, 650), (220, 50),
                                             (320, 50), (320, 0)]))
    g["acute"] = (0xB4, lambda p: poly(p, [(120, 560), (230, 720), (320, 720), (170, 560)]))
    g["acutecomb"] = (0x301, lambda p: poly(p, [(-200, 560), (-90, 720), (0, 720), (-150, 560)]))
    g["w"] = (ord("w"), fractional)

    # Small capitals, reached only through `smcp`: flat at 560 and 0, round
    # to 572 and -12.
    g["h.sc"] = (None, lambda p: (rect(p, 60, 0, 140, 560), rect(p, 400, 0, 480, 560),
                                  rect(p, 140, 250, 400, 320)))
    g["x.sc"] = (None, lambda p: poly(p, [(20, 0), (200, 280), (30, 560), (120, 560), (250, 360),
                                          (380, 560), (470, 560), (300, 280), (480, 0), (390, 0),
                                          (250, 200), (110, 0)]))
    g["z.sc"] = (None, lambda p: poly(p, [(40, 0), (40, 60), (350, 500), (50, 500), (50, 560),
                                          (450, 560), (450, 500), (150, 60), (460, 60), (460, 0)]))
    g["o.sc"] = (None, lambda p: ring(p, 270, 280, 230, 292, 75, 70))
    g["e.sc"] = (None, lambda p: poly(p, [(60, 0), (60, 560), (420, 560), (420, 500), (140, 500),
                                          (140, 310), (380, 310), (380, 250), (140, 250),
                                          (140, 60), (430, 60), (430, 0)]))
    g["s.sc"] = (None, lambda p: (ellipse(p, 240, 420, 180, 152), ellipse(p, 240, 140, 190, 152)))
    # Superscripts, reached only through `sups`: `o.sups` drawn where it
    # stands, round to 712; `x.sups` drawn on the baseline, 400 tall, for
    # `GPOS` to raise 300 -- to a flat top at 700, twelve units under the o's.
    # Measured with the raise, the two make a top zone small enough to be
    # active; measured without it they are 312 apart and make none, so the
    # raise decides where `o.sups`'s top is drawn.
    g["o.sups"] = (None, lambda p: ring(p, 200, 560, 130, 152, 50, 45))
    g["x.sups"] = (None, lambda p: poly(p, [(20, 0), (110, 200), (25, 400), (80, 400), (150, 253),
                                            (220, 400), (275, 400), (190, 200), (280, 0), (225, 0),
                                            (150, 147), (75, 0)]))

    # The CJK writing system (`hani_dflt`), which hints both dimensions.
    # Its standard glyph, 田: stems 60 units thick both ways.
    g["uni7530"] = (0x7530, lambda p: (poly(p, [(100, -60), (100, 840), (900, 840), (900, -60)]),
                                       *(rect_ccw(p, *c) for c in ((160, 0, 470, 360), (530, 0, 840, 360),
                                                                   (160, 420, 470, 780),
                                                                   (530, 420, 840, 780)))))
    # Its zones' letters. Top: the first group (他) reaches 880, the second
    # (军) 860 -- reference and overshoot. Bottom: 个 reaches -80 and 主 -60.
    g["uni4ED6"] = (0x4ED6, lambda p: (rect(p, 120, 0, 880, 880), rect_ccw(p, 180, 60, 820, 820)))
    g["uni519B"] = (0x519B, lambda p: (rect(p, 120, 0, 880, 860), rect_ccw(p, 180, 60, 820, 800)))
    g["uni4E2A"] = (0x4E2A, lambda p: (rect(p, 120, -80, 880, 700), rect_ccw(p, 180, -20, 820, 640)))
    g["uni4E3B"] = (0x4E3B, lambda p: (rect(p, 120, -60, 880, 700), rect_ccw(p, 180, 0, 820, 640)))
    # 一: a stroke whose right end is wider, 380 to 480 against 400 to 460.
    # The end is short, so its sides become serifs of the stroke's.
    g["uni4E00"] = (0x4E00, lambda p: poly(p, [(100, 400), (100, 460), (800, 460), (800, 480),
                                                 (900, 480), (900, 380), (800, 380), (800, 400)]))
    # 亅: a stroke whose upper end is wider and longer than the rest -- the
    # thin part is then no stem at all.
    g["uni4E85"] = (0x4E85, lambda p: poly(p, [(470, 0), (470, 300), (450, 300), (450, 800),
                                                 (550, 800), (550, 300), (530, 300), (530, 0)]))
    # 川: three evenly spaced stems, six edges across -- the rule that keeps
    # a lowercase m's stems even.
    g["uni5DDD"] = (0x5DDD, lambda p: (rect(p, 150, 0, 210, 800), rect(p, 470, 0, 530, 800),
                                       rect(p, 790, 0, 850, 800)))
    # 三: three bars 100 units apart, which at small sizes come within a
    # pixel of each other: a stem that close is interpolated, not placed.
    g["uni4E09"] = (0x4E09, lambda p: (rect(p, 100, 100, 900, 160), rect(p, 150, 260, 850, 320),
                                       rect(p, 100, 420, 900, 480)))
    # 〇: a ring, whose segments are round by this system's rule.
    g["uni3007"] = (0x3007, lambda p: ring(p, 500, 390, 380, 400, 60, 60))
    # →: no script's, so the fallback style's -- CJK hinting too.
    g["arrowright"] = (0x2192, lambda p: poly(p, [(100, 380), (100, 440), (700, 440), (600, 560),
                                                    (680, 560), (820, 410), (680, 260), (600, 260),
                                                    (700, 380)]))
    return g


def rect_ccw(pen, x0, y0, x1, y1):
    """A counter, wound against the outer contour: down the left side."""
    poly(pen, [(x0, y0), (x1, y0), (x1, y1), (x0, y1)])


# The fixture's OpenType features: small capitals and superscripts, as a font
# registers them under Latin and the default script alike.
FEATURES = """
languagesystem DFLT dflt;
languagesystem latn dflt;

feature smcp {
    sub h by h.sc;
    sub x by x.sc;
    sub z by z.sc;
    sub o by o.sc;
    sub e by e.sc;
    sub s by s.sc;
} smcp;

feature sups {
    sub x by x.sups;
    sub o by o.sups;
    pos x.sups <0 300 0 0>;
} sups;
"""


def draw_all(pen_for, g):
    out = {}
    for name, (_, draw) in g.items():
        out[name] = pen_for(name, draw)
    return out


def build_ttf(g, bold=False):
    """The face as TrueType -- or, with `bold`, the Bold master of the
    variable face (see `build_var`): every point moved, the accent's offset
    and the scaled component's too, and each bearing 7 units short of the
    glyph's left edge, so that the left phantom point moves."""
    order = [".notdef"] + list(g) + ["eacute", "o.small"]
    fb = FontBuilder(UPEM, isTTF=True)
    fb.setupGlyphOrder(order)
    cmap = {cp: name for name, (cp, _) in g.items() if cp is not None}
    cmap[0xE9] = "eacute"
    fb.setupCharacterMap(cmap)
    glyf = {}
    empty = TTGlyphPen(None)
    glyf[".notdef"] = empty.glyph()
    for name, (_, draw) in g.items():
        pen = TTGlyphPen(None)
        if name in CFF_NATIVE:
            # Turned round and made quadratic; `glyph()` rounds it.
            draw(Cu2QuPen(pen, 1.0, reverse_direction=True))
        else:
            draw(pen)
        glyph = pen.glyph()
        if bold:
            # Moved point by point after the drawing, so the masters keep one
            # structure whatever quadratic conversion made of a curve.
            glyph.coordinates.transform(((1.06, 0), (0, 1.02)))
            glyph.coordinates.translate((5, -3))
            glyph.coordinates.toInt()
        glyf[name] = glyph
    # A composite: e with the acute lifted over it.
    pen = TTGlyphPen(glyf)
    pen.addComponent("e", (1, 0, 0, 1, 0, 0))
    pen.addComponent("acute", (1, 0, 0, 1, 75, 8) if bold else (1, 0, 0, 1, 60, 0))
    glyf["eacute"] = pen.glyph()
    # And a scaled one: a small o, placed by an offset that also varies.
    pen = TTGlyphPen(glyf)
    pen.addComponent("o", (0.6, 0, 0, 0.6, 126, 305) if bold else (0.6, 0, 0, 0.6, 120, 300))
    glyf["o.small"] = pen.glyph()
    fb.setupGlyf(glyf)
    glyf_table = fb.font["glyf"]

    def left_edge(name):
        glyph = glyf_table[name]
        glyph.recalcBounds(glyf_table)
        return getattr(glyph, "xMin", 0)

    metrics = fb_metrics(g, order)
    metrics["o.small"] = (600, left_edge("o.small"))
    if bold:
        metrics = {name: (600, left_edge(name) - 7) for name in order}
    fb.setupHorizontalMetrics(metrics)
    fb.addOpenTypeFeatures(FEATURES)
    finish(fb, "HintFixture")
    buf = io.BytesIO()
    fb.save(buf)
    return buf.getvalue(), order


def build_var(g, hvar):
    """The TrueType face made variable along `wght`, 400 to 700: the face
    itself the Regular master and `build_ttf(bold=True)` the Bold. At weight
    610 (0.7 of the way, 0.69999695 in FreeType's 16.16) every delta lands on
    a fraction -- the accent's offset moves 10.4999 units, which FreeType
    rounds to 10 where 0.7 in `F2Dot14` would make 11 -- so the points the
    hinter reads are FreeType's only if they are rounded as its loader rounds
    them. Without `HVAR`, FreeType also moves each glyph by its left phantom
    point's delta; with it, not."""
    from fontTools import varLib
    from fontTools.designspaceLib import AxisDescriptor, DesignSpaceDocument, SourceDescriptor
    from fontTools.ttLib import TTFont

    ds = DesignSpaceDocument()
    axis = AxisDescriptor()
    axis.tag, axis.name = "wght", "Weight"
    axis.minimum, axis.default, axis.maximum = 400, 400, 700
    ds.addAxis(axis)
    order = None
    for weight, bold in ((400, False), (700, True)):
        data, order = build_ttf(g, bold=bold)
        src = SourceDescriptor()
        src.font = TTFont(io.BytesIO(data))
        src.location = {"Weight": weight}
        ds.addSource(src)
    vf, _, _ = varLib.build(ds, exclude=[] if hvar else ["HVAR"])
    buf = io.BytesIO()
    vf.save(buf)
    return buf.getvalue(), order


def build_otf(g, bold=False):
    """The face as CFF -- or, with `bold`, the Bold master of the `CFF2` face
    (see `build_var_cff2`): every point moved as the TrueType Bold's are, the
    glyphs drawn without rounding keeping the fractions that makes."""
    order = [".notdef"] + list(g)
    fb = FontBuilder(UPEM, isTTF=False)
    fb.setupGlyphOrder(order)
    cmap = {cp: name for name, (cp, _) in g.items() if cp is not None}
    fb.setupCharacterMap(cmap)
    charstrings = {}
    pen = T2CharStringPen(600, None)
    charstrings[".notdef"] = pen.getCharString()

    def moved(pen):
        return TransformPen(pen, (1.06, 0, 0, 1.02, 5, -3)) if bold else pen

    for name, (_, draw) in g.items():
        if name in CFF_NATIVE:
            pen = T2CharStringPen(600, None, roundTolerance=0)
            draw(moved(pen))
            # Unoptimized: fontTools' specializer drops a line of no length,
            # and that line is what the glyph is drawn to test.
            charstrings[name] = pen.getCharString(optimize=False)
        else:
            rec = RecordingPen()
            draw(rec)
            pen = T2CharStringPen(600, None)
            # PostScript winds its outer contours the other way.
            rec.replay(ReverseContourPen(moved(pen)))
            charstrings[name] = pen.getCharString()
    fb.setupCFF("HintFixtureCFF", {"FullName": "HintFixtureCFF"}, charstrings, {})
    fb.setupHorizontalMetrics(fb_metrics(g, order))
    fb.addOpenTypeFeatures(FEATURES)
    finish(fb, "HintFixtureCFF")
    buf = io.BytesIO()
    fb.save(buf)
    return buf.getvalue(), order


def build_var_cff2(g):
    """The CFF face made variable along `wght`, 400 to 700, as `build_var`
    makes the TrueType one: `build_otf` the Regular master and
    `build_otf(bold=True)` the Bold. fontTools merges CFF masters into a
    `CFF2` table, each point that moves a `blend`."""
    from fontTools import varLib
    from fontTools.designspaceLib import AxisDescriptor, DesignSpaceDocument, SourceDescriptor
    from fontTools.ttLib import TTFont

    ds = DesignSpaceDocument()
    axis = AxisDescriptor()
    axis.tag, axis.name = "wght", "Weight"
    axis.minimum, axis.default, axis.maximum = 400, 400, 700
    ds.addAxis(axis)
    order = None
    for weight, bold in ((400, False), (700, True)):
        data, order = build_otf(g, bold=bold)
        src = SourceDescriptor()
        src.font = TTFont(io.BytesIO(data))
        src.location = {"Weight": weight}
        ds.addSource(src)
    vf, _, _ = varLib.build(ds)
    assert "CFF2" in vf
    buf = io.BytesIO()
    vf.save(buf)
    return buf.getvalue(), order


class Recorder:
    """A fontTools-style pen keeping HarfBuzz's drawing as ops."""

    def __init__(self):
        self.ops = []

    def moveTo(self, p):
        self.ops.append(("M", [p]))

    def lineTo(self, p):
        self.ops.append(("L", [p]))

    def curveTo(self, *points):
        self.ops.append(("C", list(points)))

    def qCurveTo(self, *points):
        self.ops.append(("Q", list(points)))

    def closePath(self):
        self.ops.append(("Z", []))

    def endPath(self):
        self.ops.append(("Z", []))


def drawn(data, order, weight):
    """HarfBuzz's box and path for each glyph at `weight`, one unit per font
    unit: `(gid, name, [x_bearing, y_bearing, width, height], ops, coords)`,
    the path's contours with nothing drawn left out and a closing line back
    to a contour's start dropped -- the two ways HarfBuzz spells a contour
    that this crate's outline does not."""
    import uharfbuzz as hb

    face = hb.Face(hb.Blob(data))
    font = hb.Font(face)
    font.scale = (face.upem, face.upem)
    font.set_variations({"wght": weight})
    rows = []
    for gid, name in enumerate(order):
        e = font.get_glyph_extents(gid)
        pen = Recorder()
        font.draw_glyph_with_pen(gid, pen)
        contours, cur = [], None
        for kind, pts in pen.ops:
            if kind == "M":
                cur = (pts[0], [])
            elif kind == "Z":
                if cur is not None:
                    contours.append(cur)
                cur = None
            elif cur is not None:
                cur[1].append((kind, pts))
        ops, coords = "", []
        for start, segs in contours:
            if segs and segs[-1][0] == "L" and segs[-1][1][0] == start:
                segs = segs[:-1]
            if not segs:
                continue
            ops += "M"
            coords += list(start)
            for kind, pts in segs:
                ops += kind
                for p in pts:
                    coords += list(p)
            ops += "Z"
        rows.append((gid, name, [e.x_bearing, e.y_bearing, e.width, e.height], ops, coords))
    return rows


def x_min(draw):
    """The left edge of what `draw` draws."""
    from fontTools.pens.boundsPen import BoundsPen

    bp = BoundsPen(None)
    draw(bp)
    return int(round(bp.bounds[0])) if bp.bounds else 0


def fb_metrics(g, order):
    """Every glyph 600 wide, its left side bearing its left edge, as a real
    font's is."""
    out = {}
    for name in order:
        if name in g:
            out[name] = (600, x_min(g[name][1]))
        elif name == "eacute":
            out[name] = (600, min(x_min(g["e"][1]), x_min(g["acute"][1]) + 60))
        else:
            out[name] = (600, 0)
    return out


def finish(fb, family):
    fb.setupHorizontalHeader(ascent=800, descent=-250)
    fb.setupNameTable({"familyName": family, "styleName": "Regular"})
    fb.setupOS2(sTypoAscender=800, sTypoDescender=-250, usWinAscent=800, usWinDescent=250)
    fb.setupPost()


# The variable face's instance, and the sizes its answers are kept at.
VAR_WEIGHT = 610
VAR_SIZES = [10, 13, 16, 20]


def expectations(data, order, weight=None, sizes=None):
    path = os.path.join(os.environ.get("TEMP", "/tmp"), "hint_fixture.bin")
    with open(path, "wb") as f:
        f.write(data)
    face = freetype.Face(path)
    if weight is not None:
        face.set_var_design_coords([weight])
    rows = []
    flags = freetype.FT_LOAD_NO_BITMAP | freetype.FT_LOAD_FORCE_AUTOHINT | freetype.FT_LOAD_TARGET_LIGHT
    for px in sizes or SIZES:
        face.set_char_size(0, int(round(px * 64)), 72, 72)
        for gid, name in enumerate(order):
            face.load_glyph(gid, flags)
            points = list(face.glyph.outline.points)
            if points:
                rows.append((px, gid, name, points))
    return rows


def rust_f32(v):
    """The shortest decimal that reads back as the `f32` `v` is: what Rust
    prints for one, and all the precision an `f32` literal may carry."""
    import struct

    def f32(x):
        return struct.unpack("<f", struct.pack("<f", x))[0]

    v = f32(v)
    for digits in range(1, 10):
        s = f"{v:.{digits}g}"
        if f32(float(s)) == v:
            break
    if "e" in s:
        mantissa, exponent = s.split("e")
        s = f"{mantissa if '.' in mantissa else mantissa + '.0'}e{int(exponent)}"
    return s if ("." in s or "e" in s) else s + ".0"


def rust_bytes(data):
    return ", ".join(f"0x{b:02X}" for b in data)


def main():
    g = glyphs()
    ttf, ttf_order = build_ttf(g)
    otf, otf_order = build_otf(g)
    here = os.path.dirname(os.path.abspath(__file__))
    out = os.path.join(here, "..", "src", "hint", "fixture.rs")
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        w = f.write
        w("//! A synthetic face drawn to exercise the hinter, and where FreeType's\n")
        w("//! auto-hinter puts its points.\n")
        w("//!\n")
        w("//! Generated by `gui/font/tools/gen_hint_fixture.py` with FreeType\n")
        w(f"//! {'.'.join(str(v) for v in freetype.version())} (freetype-py). Do not edit: run the script\n")
        w("//! instead. See that script for what each glyph is drawn to test.\n\n")
        var, var_order = build_var(g, hvar=True)
        var_nohvar, _ = build_var(g, hvar=False)
        var_cff2, cff2_order = build_var_cff2(g)
        what = {
            "TTF": "The face as TrueType.",
            "OTF": "The face as CFF.",
            "VAR": f"The TrueType face made variable, with `HVAR`; answers at weight {VAR_WEIGHT}.",
            "VAR_NOHVAR": f"The same without `HVAR`, so `gvar` moves each glyph's origin; at weight {VAR_WEIGHT}.",
            "VAR_CFF2": f"The CFF face made variable, a `CFF2` table; answers at weight {VAR_WEIGHT}.",
        }
        for label, data, order, weight in (
            ("TTF", ttf, ttf_order, None),
            ("OTF", otf, otf_order, None),
            ("VAR", var, var_order, VAR_WEIGHT),
            ("VAR_NOHVAR", var_nohvar, var_order, VAR_WEIGHT),
            ("VAR_CFF2", var_cff2, cff2_order, VAR_WEIGHT),
        ):
            w(f"/// {what[label]}\n")
            w(f"pub(crate) static {label}: [u8; {len(data)}] = [{rust_bytes(data)}];\n\n")
            rows = expectations(data, order, weight, VAR_SIZES if weight else None)
            # x and y in turn in one flat array, not as pairs: rustfmt packs a
            # list of numbers into lines but gives every tuple a line of its
            # own, which would triple the file.
            w(f"/// FreeType's hinted points for the {label} face: `(px, glyph, name, x and y\n")
            w("/// in turn, in 1/64 pixel, of each stored point)`.\n")
            w(f"pub(crate) static {label}_EXPECTED: [(f32, u16, &str, &[i32]); {len(rows)}] = [\n")
            for px, gid, name, points in rows:
                flat = ", ".join(f"{x}, {y}" for (x, y) in points)
                w(f"    ({float(px)}, {gid}, \"{name}\", &[{flat}]),\n")
            w("];\n\n")
        rows = drawn(var_cff2, cff2_order, VAR_WEIGHT)
        w(f"/// HarfBuzz's drawing of each glyph of the VAR_CFF2 face at weight {VAR_WEIGHT}: `(glyph,\n")
        w("/// name, [x_bearing, y_bearing, width, height], path ops (`M`, `L`, `C`, `Q`, `Z`),\n")
        w("/// their points' x and y in turn)`, at one unit per font unit; a contour with\n")
        w("/// nothing drawn is left out, and a closing line back to its start dropped.\n")
        w("pub(crate) type Drawn = (u16, &'static str, [i32; 4], &'static str, &'static [f32]);\n\n")
        w("/// Each glyph's row: see [`Drawn`].\n")
        w(f"pub(crate) static VAR_CFF2_DRAWN: [Drawn; {len(rows)}] = [\n")
        for gid, name, ext, ops, coords in rows:
            flat = ", ".join(rust_f32(v) for v in coords)
            w(f"    ({gid}, \"{name}\", [{', '.join(str(v) for v in ext)}], \"{ops}\", &[{flat}]),\n")
        w("];\n\n")
    rustfmt(out)
    print(f"{len(ttf)} + {len(otf)} + {len(var)} + {len(var_nohvar)} + {len(var_cff2)} bytes of font -> {out}")


if __name__ == "__main__":
    main()
