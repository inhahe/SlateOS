r"""Generate `gui/font/src/colr_fixture.rs`: a colour font whose colour glyphs
HarfBuzz has to measure by walking their paint, and the box HarfBuzz reports
for each.

    python gui/font/tools/gen_colr_fixture.py

Needs `fonttools` and `uharfbuzz` (HarfBuzz 14.3.0).

A colour glyph that a `COLR` table's `ClipList` covers reports its clip box;
one it does not, HarfBuzz measures by walking its paint -- the union of what
each fill covers, under its transforms, clips and composite modes
(`hb_paint_extents`), or for a version-0 glyph the union of its layers'
outlines. No font on the development host has such a glyph, so this one is
built to have them:

* every paint format that moves or combines what it paints, fixed and
  variable, the variable ones measured at the default instance and at
  weight 550 (normalized 0.3);
* a `PaintColrGlyph` of a glyph with a clip box, fixed and varied, which
  clips to it;
* the walk's limits: a glyph that names itself, two that name each other,
  graphs that reach HarfBuzz's nesting limit of 64 levels through a
  `PaintColrGlyph` -- one level short of it, at it, and past it -- and a
  fan-out that runs out of HarfBuzz's 2,048 edges part-way through a layer,
  where exactly how far it got shows in the box;
* records HarfBuzz reads as the null paint (a base glyph record and a
  `PaintGlyph` with null paint offsets), and a composite mode past the last
  one, which `PaintComposite` takes for `CLEAR`.

HarfBuzz's own answers are recorded beside the font for `colr::extents`'
tests to compare with.
"""

import io
import os

from fontTools.colorLib.builder import buildCOLR, buildCPAL
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.varLib.builder import buildVarData, buildVarRegionList, buildVarStore

from rustfmt_out import rustfmt

UPEM = 1000
# The instance the varied boxes are measured at: normalized 0.3, so that
# every delta is a fraction of a unit.
WGHT = 550.0


def rect(pen, x0, y0, x1, y1):
    pen.moveTo((x0, y0))
    pen.lineTo((x0, y1))
    pen.lineTo((x1, y1))
    pen.lineTo((x1, y0))
    pen.closePath()


def outline_glyphs():
    """The glyphs colour glyphs clip to: each a different box, so that every
    transform shows in the answer. Every glyph's `hmtx` side bearing is 0,
    which HarfBuzz draws it on, whatever its own `xMin`."""
    def square(p):
        rect(p, 100, 100, 500, 500)

    def tri(p):
        p.moveTo((0, 0))
        p.lineTo((300, 700))
        p.lineTo((600, 0))
        p.closePath()

    def bar(p):
        rect(p, -50, 200, 650, 260)

    def ring(p):
        # Quadratic arcs whose control points stand outside the curve: the
        # box HarfBuzz measures holds them.
        p.moveTo((50, 300))
        p.qCurveTo((50, 550), (300, 550))
        p.qCurveTo((550, 550), (550, 300))
        p.qCurveTo((550, 50), (300, 50))
        p.qCurveTo((50, 50), (50, 300))
        p.closePath()

    def odd(p):
        rect(p, 13, -37, 211, 419)

    return {"square": square, "tri": tri, "bar": bar, "ring": ring, "odd": odd}


def solid(i, alpha=1.0):
    return {"Format": 2, "PaletteIndex": i, "Alpha": alpha}


def glyph(name, paint=None):
    return {"Format": 10, "Glyph": name, "Paint": paint or solid(0)}


def translate(dx, dy, paint):
    return {"Format": 14, "dx": dx, "dy": dy, "Paint": paint}


def colr_glyph(name):
    return {"Format": 11, "Glyph": name}


LINEAR = {
    "Format": 4,
    "ColorLine": {
        "Extend": "pad",
        "ColorStop": [
            {"StopOffset": 0.0, "PaletteIndex": 0, "Alpha": 1.0},
            {"StopOffset": 1.0, "PaletteIndex": 1, "Alpha": 1.0},
        ],
    },
    "x0": 0, "y0": 0, "x1": 500, "y1": 500, "x2": 0, "y2": 500,
}


def nested(depth, paint):
    """`paint` under `depth` translations of one unit each."""
    for _ in range(depth):
        paint = translate(1, 0, paint)
    return paint


# The variation store's rows, one delta each at the weight axis' maximum, in
# each field's own units -- font units, F2DOT14 or 16.16 -- and the first row
# of each glyph's fields.
ROWS = [
    101, -33,                                   # 0: v_translate
    -3277, 1638,                                # 2: v_scale
    1000, -2000, 57, -23,                       # 4: v_scale_center
    -1500,                                      # 8: v_scale_uniform
    777, -41, 19,                               # 9: v_scale_uniform_center
    1234,                                       # 12: v_rotate
    -999, 45, -67,                              # 13: v_rotate_center
    555, -444,                                  # 16: v_skew
    -333, 222, 17, -29,                         # 18: v_skew_center
    6554, -3277, 9830, -1311, 196608, -98304,   # 22: v_transform
    101, -33, 7, 0,                             # 28: v_clipped's clip box
]


def colour_glyphs():
    """Each colour glyph's paint: a COLRv1 paint (a dict), or a list of
    (layer glyph, palette index) for COLRv0."""
    return {
        "c_fill": glyph("square"),
        "c_gradient": glyph("tri", LINEAR),
        "c_translate": translate(37, -12, glyph("odd")),
        "c_scale": {"Format": 16, "Paint": glyph("tri"), "scaleX": 1.5, "scaleY": 0.75},
        "c_scale_center": {"Format": 18, "Paint": glyph("odd"), "scaleX": 0.5, "scaleY": 1.25,
                           "centerX": 301, "centerY": 199},
        "c_scale_uniform": {"Format": 20, "Paint": glyph("ring"), "scale": 1.3},
        "c_scale_uniform_center": {"Format": 22, "Paint": glyph("odd"), "scale": 0.7,
                                   "centerX": 251, "centerY": 249},
        "c_rotate": {"Format": 24, "Paint": glyph("bar"), "angle": 30},
        "c_rotate_center": {"Format": 26, "Paint": glyph("odd"), "angle": -45,
                            "centerX": 300, "centerY": 211},
        "c_skew": {"Format": 28, "Paint": glyph("square"), "xSkewAngle": 15, "ySkewAngle": -10},
        "c_skew_center": {"Format": 30, "Paint": glyph("odd"), "xSkewAngle": -20, "ySkewAngle": 5,
                          "centerX": 101, "centerY": 333},
        "c_transform": {"Format": 12, "Paint": glyph("tri"),
                        "Transform": {"xx": 0.9, "yx": 0.3, "xy": -0.2, "yy": 1.1,
                                      "dx": 12.5, "dy": -7.25}},
        "c_nested": translate(-40, 25, {
            "Format": 24, "angle": 60, "Paint": {
                "Format": 16, "scaleX": 1.25, "scaleY": 0.5, "Paint": glyph("ring")}}),
        "c_over": {"Format": 32, "CompositeMode": "src_over",
                   "SourcePaint": glyph("bar"), "BackdropPaint": glyph("square")},
        "c_in": {"Format": 32, "CompositeMode": "src_in",
                 "SourcePaint": glyph("bar"), "BackdropPaint": glyph("square")},
        "c_dest_out": {"Format": 32, "CompositeMode": "dest_out",
                       "SourcePaint": glyph("bar"), "BackdropPaint": glyph("tri")},
        "c_src": {"Format": 32, "CompositeMode": "src",
                  "SourcePaint": glyph("odd"), "BackdropPaint": glyph("tri")},
        "c_clear": {"Format": 32, "CompositeMode": "clear",
                    "SourcePaint": glyph("odd"), "BackdropPaint": glyph("tri")},
        "c_multiply": {"Format": 32, "CompositeMode": "multiply",
                       "SourcePaint": glyph("odd"), "BackdropPaint": glyph("bar")},
        "c_layers": {"Format": 1, "Layers": [glyph("odd", solid(1)), glyph("bar", solid(0, 0.5))]},
        "c_colr_glyph": translate(100, 50, colr_glyph("c_rotate")),
        "c_unbounded": solid(1),
        "c_unbounded_composite": {"Format": 32, "CompositeMode": "src_in",
                                  "SourcePaint": solid(0), "BackdropPaint": glyph("odd")},
        "c_empty": glyph("space"),
        "v0_layers": [("square", 0), ("tri", 1), ("odd", 0)],
        # A glyph with a clip box reports it; one that names it is cut to it.
        "c_clipped": glyph("tri"),
        "c_via_clip": translate(-25, 15, colr_glyph("c_clipped")),
        # Cycles, which the glyph decycler cuts.
        "c_self": colr_glyph("c_self"),
        "c_self_layers": {"Format": 1, "Layers": [glyph("square"), colr_glyph("c_self_layers")]},
        "c_ping": translate(7, 0, colr_glyph("c_pong")),
        "c_pong": {"Format": 1, "Layers": [glyph("tri"), colr_glyph("c_ping")]},
        # The nesting limit. A chain of 62 moves, a `PaintGlyph` and its fill
        # is as deep as one glyph's graph can be before HarfBuzz's sanitizer
        # nulls the offset past its own limit of 64 -- which, in a font whose
        # subtables are shared as fontTools shares them, would empty other
        # glyphs too. The walk's limit counts levels through `PaintColrGlyph`,
        # which the sanitizer's does not: 40 moves there and 21, 22 or 23
        # more in the glyph named put the fill one level short of the limit,
        # at it, and past it.
        "c_deep_62": nested(62, glyph("square")),
        "c_hop_21": nested(40, colr_glyph("c_tail_21")),
        "c_hop_22": nested(40, colr_glyph("c_tail_22")),
        "c_hop_23": nested(40, colr_glyph("c_tail_23")),
        "c_tail_21": nested(21, glyph("square")),
        "c_tail_22": nested(22, glyph("square")),
        "c_tail_23": nested(23, glyph("square")),
        # A fan-out: twenty layers of a glyph of 120 layers, three edges each,
        # is far past the edge limit, and each outer layer is far enough
        # right of the last that the box's right edge says how many inner
        # layers of the last one were painted.
        "c_fan_inner": {"Format": 1, "Layers": [
            translate(3 * i, 0, glyph("square")) for i in range(120)]},
        "c_fan": {"Format": 1, "Layers": [
            translate(1000 * j, 0, colr_glyph("c_fan_inner")) for j in range(20)]},
        # Patched below: a null paint, a null PaintGlyph child, and a mode
        # past the last.
        "c_null": solid(0),
        "c_glyph_null": glyph("bar"),
        "c_mode_unknown": {"Format": 32, "CompositeMode": "xor",
                           "SourcePaint": glyph("bar"), "BackdropPaint": glyph("square")},
        # Variable paints.
        "v_translate": {"Format": 15, "Paint": glyph("odd"), "dx": 37, "dy": -12,
                        "VarIndexBase": 0},
        "v_scale": {"Format": 17, "Paint": glyph("tri"), "scaleX": 1.5, "scaleY": 0.75,
                    "VarIndexBase": 2},
        "v_scale_center": {"Format": 19, "Paint": glyph("odd"), "scaleX": 0.5, "scaleY": 1.25,
                           "centerX": 301, "centerY": 199, "VarIndexBase": 4},
        "v_scale_uniform": {"Format": 21, "Paint": glyph("ring"), "scale": 1.3,
                            "VarIndexBase": 8},
        "v_scale_uniform_center": {"Format": 23, "Paint": glyph("odd"), "scale": 0.7,
                                   "centerX": 251, "centerY": 249, "VarIndexBase": 9},
        "v_rotate": {"Format": 25, "Paint": glyph("bar"), "angle": 30, "VarIndexBase": 12},
        "v_rotate_center": {"Format": 27, "Paint": glyph("odd"), "angle": -45,
                            "centerX": 300, "centerY": 211, "VarIndexBase": 13},
        "v_skew": {"Format": 29, "Paint": glyph("square"), "xSkewAngle": 15,
                   "ySkewAngle": -10, "VarIndexBase": 16},
        "v_skew_center": {"Format": 31, "Paint": glyph("odd"), "xSkewAngle": -20,
                          "ySkewAngle": 5, "centerX": 101, "centerY": 333,
                          "VarIndexBase": 18},
        "v_transform": {"Format": 13, "Paint": glyph("tri"),
                        "Transform": {"xx": 0.9, "yx": 0.3, "xy": -0.2, "yy": 1.1,
                                      "dx": 12.5, "dy": -7.25, "VarIndexBase": 22}},
        "v_nested": {"Format": 27, "angle": 20, "centerX": 150, "centerY": -60,
                     "VarIndexBase": 13, "Paint": {
                         "Format": 17, "scaleX": 1.1, "scaleY": 0.9, "VarIndexBase": 2,
                         "Paint": glyph("ring")}},
        "v_clipped": glyph("ring"),
        "v_via_clip": translate(10, 20, colr_glyph("v_clipped")),
    }


CLIP_BOXES = {
    "c_clipped": (100, 100, 400, 600),
    "v_clipped": (40, 30, 480, 500, 28),
}

# Colour glyphs with an outline of their own, which a measurement that fell
# back to the outline would report instead.
OWN_OUTLINES = {"c_null": "bar"}


def var_store():
    regions = buildVarRegionList([{"wght": (0.0, 1.0, 1.0)}], ["wght"])
    data = buildVarData([0], [[delta] for delta in ROWS], optimize=False)
    return buildVarStore(regions, [data])


def patch(colr):
    """Make the records fontTools will not build: null paint offsets, and a
    composite mode past the last there is."""
    for record in colr.table.BaseGlyphList.BaseGlyphPaintRecord:
        if record.BaseGlyph == "c_null":
            record.Paint = None
        elif record.BaseGlyph == "c_glyph_null":
            record.Paint.Paint = None
        elif record.BaseGlyph == "c_mode_unknown":
            record.Paint.CompositeMode = 40


def build():
    shapes = outline_glyphs()
    colours = colour_glyphs()
    order = [".notdef", "space"] + list(shapes) + list(colours)
    fb = FontBuilder(UPEM, isTTF=True)
    fb.setupGlyphOrder(order)
    fb.setupCharacterMap({0x20: "space", 0x41: "square", 0x42: "tri"})
    glyf = {}
    for name in order:
        pen = TTGlyphPen(None)
        shape = shapes.get(name) or shapes.get(OWN_OUTLINES.get(name))
        if shape:
            shape(pen)
        glyf[name] = pen.glyph()
    fb.setupGlyf(glyf)
    fb.setupHorizontalMetrics({name: (600, 0) for name in order})
    fb.setupHorizontalHeader(ascent=800, descent=-200)
    fb.setupNameTable({"familyName": "ColrFixture", "styleName": "Regular"})
    fb.setupOS2()
    fb.setupPost()
    fb.setupFvar(axes=[("wght", 100, 400, 900, "Weight")], instances=[])
    fb.font["CPAL"] = buildCPAL([[(1.0, 0.0, 0.0, 1.0), (0.0, 0.0, 1.0, 1.0)]])
    # Given a variation store, fontTools writes every colour glyph as
    # version 1; the store is attached afterwards so that `v0_layers` stays
    # a version-0 glyph.
    colr = buildCOLR(colours, glyphMap=fb.font.getReverseGlyphMap(), clipBoxes=CLIP_BOXES)
    colr.table.VarStore = var_store()
    patch(colr)
    fb.font["COLR"] = colr
    buf = io.BytesIO()
    fb.save(buf)
    return buf.getvalue(), order, list(colours)


def harfbuzz_extents(data, order, names, wght=None):
    import uharfbuzz as hb

    face = hb.Face(hb.Blob(data))
    font = hb.Font(face)
    font.scale = (face.upem, face.upem)
    if wght is not None:
        font.set_variations({"wght": wght})
    rows = []
    for name in names:
        e = font.get_glyph_extents(order.index(name))
        rows.append([e.x_bearing, e.y_bearing, e.width, e.height])
    return rows


def rust_bytes(data):
    return ", ".join(f"0x{b:02X}" for b in data)


def rust_box(box):
    return "[" + ", ".join(str(v) for v in box) + "]"


def main():
    import uharfbuzz as hb

    # A fixed build time keeps the font's bytes, and so this file, the same
    # from one run to the next.
    os.environ.setdefault("SOURCE_DATE_EPOCH", "1790463450")
    data, order, names = build()
    default = harfbuzz_extents(data, order, names)
    varied = harfbuzz_extents(data, order, names, WGHT)
    here = os.path.dirname(os.path.abspath(__file__))
    out = os.path.join(here, "..", "src", "colr_fixture.rs")
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        w = f.write
        w("//! A colour font whose colour glyphs HarfBuzz measures by walking their\n")
        w("//! paint, and HarfBuzz's box for each.\n")
        w("//!\n")
        w(f"//! Generated by `gui/font/tools/gen_colr_fixture.py` with HarfBuzz {hb.version_string()}.\n")
        w("//! Do not edit: run the script instead.\n\n")
        w("/// The font: outline glyphs, and colour glyphs in every paint format that\n")
        w("/// moves or combines what it paints, fixed and variable, with the walk's\n")
        w("/// limits and null paints; one weight axis, 100 to 900 about 400.\n")
        w(f"pub(crate) static COLR_FACE: [u8; {len(data)}] = [{rust_bytes(data)}];\n\n")
        w("/// The weight the second box of each row is measured at.\n")
        w(f"pub(crate) const COLR_WGHT: f32 = {WGHT};\n\n")
        w("/// HarfBuzz's box for each colour glyph: `(glyph, name, at the default\n")
        w("/// instance, at weight [`COLR_WGHT`])`, each `[x_bearing, y_bearing, width,\n")
        w("/// height]` at one unit per font unit.\n")
        w(f"pub(crate) static COLR_EXTENTS: [(u16, &str, [i32; 4], [i32; 4]); {len(names)}] = [\n")
        for name, at_default, at_wght in zip(names, default, varied):
            w(f"    ({order.index(name)}, \"{name}\", {rust_box(at_default)}, {rust_box(at_wght)}),\n")
        w("];\n")
    rustfmt(out)
    for name, at_default, at_wght in zip(names, default, varied):
        print(f"{order.index(name):3} {name:26} {at_default} {at_wght}")


if __name__ == "__main__":
    main()
