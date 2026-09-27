r"""Generate `gui/font/src/var_fixture.rs`: variable faces, and where HarfBuzz
and FreeType put their instances.

    python gui/font/tools/gen_var_fixture.py [--out PATH]

Needs `fonttools`, `uharfbuzz` and `freetype-py`.

A variable font's instance -- weight 700, width 87.5 -- reaches its
variation tables as *normalized* coordinates, and HarfBuzz and FreeType
compute them differently: HarfBuzz in `F2Dot14` by way of 16.16 floats,
FreeType in 16.16 integers (see `src/var.rs`). This crate computes both,
and the fixture records what the two libraries answer, so that the test
compares against the libraries rather than against a reading of their
source. Three faces:

* **`AVAR2`**: weight 100..400..900 by width 50..100, with an `avar` table
  of version 2 -- an item variation store that moves one axis's coordinate
  by where the others are. Built from a designspace `<mappings>` element:
  at width 50, weight 700 goes where 550 would; at weight 900 and width 75
  both move. Every instance of a grid across both axes.
* **`CURVES`**: nine axes whose `avar` (version 1) curves each take one of
  the paths HarfBuzz's `SegmentMaps::map_float` and FreeType's
  `ft_var_to_normalized` have for a curve: an ordinary one, repeated
  `from` values in runs of two, three and four, an unsorted curve, a
  one-point and an empty curve, doubled ends, and a curve that stops short
  of the axis ends. The bytes are written by hand, because fontTools will
  not write most of these. Each axis swept end to end with the others at
  their defaults.
* **`LIMITS`**: no `avar`, but `fvar` limits that are awkward -- fractional
  ones no binary fraction reaches, a side of zero width, a minimum above
  the default and a maximum below it, which the libraries repair
  differently. Each axis swept too.

A value of each sweep is an `f32` exactly -- the type this crate is asked
in -- so that both libraries are handed the same number the Rust test is.
"""

import argparse
import io
import os
import struct

import freetype
import uharfbuzz as hb
from fontTools import varLib
from fontTools.designspaceLib import (AxisDescriptor, AxisMappingDescriptor, DesignSpaceDocument,
                                      SourceDescriptor)
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont
from fontTools.ttLib.tables.DefaultTable import DefaultTable

from rustfmt_out import rustfmt


def f32(x):
    """`x` rounded to the nearest `f32`, as a Python float."""
    return struct.unpack("<f", struct.pack("<f", x))[0]


def f2dot14(x):
    return int(round(x * 16384))


def master(width):
    """One master: a single rectangle `width` units wide."""
    fb = FontBuilder(1000, isTTF=True)
    fb.setupGlyphOrder([".notdef", "A"])
    fb.setupCharacterMap({0x41: "A"})
    pen = TTGlyphPen(None)
    pen.moveTo((0, 0))
    pen.lineTo((0, 700))
    pen.lineTo((width, 700))
    pen.lineTo((width, 0))
    pen.closePath()
    fb.setupGlyf({".notdef": TTGlyphPen(None).glyph(), "A": pen.glyph()})
    fb.setupHorizontalMetrics({".notdef": (600, 0), "A": (600, 0)})
    fb.setupHorizontalHeader(ascent=800, descent=-200)
    fb.setupNameTable({"familyName": "VarFixture", "styleName": "Regular"})
    fb.setupOS2()
    fb.setupPost()
    buf = io.BytesIO()
    fb.save(buf)
    return TTFont(io.BytesIO(buf.getvalue()))


def build(axes, mappings=()):
    """A variable face over `axes` -- (tag, min, default, max) -- with one
    master at the default and one at each axis's maximum, plus `mappings`,
    each a pair of {tag: value} locations for `avar` version 2."""
    ds = DesignSpaceDocument()
    for tag, lo, default, hi in axes:
        axis = AxisDescriptor()
        axis.tag, axis.name = tag, tag
        axis.minimum, axis.default, axis.maximum = lo, default, hi
        ds.addAxis(axis)
    for inp, out in mappings:
        m = AxisMappingDescriptor()
        m.inputLocation, m.outputLocation = dict(inp), dict(out)
        ds.axisMappings.append(m)
    defaults = {tag: default for tag, _, default, _ in axes}
    locations = [dict(defaults)]
    for tag, lo, default, hi in axes:
        if hi > default:
            locations.append(dict(defaults, **{tag: hi}))
        if lo < default:
            locations.append(dict(defaults, **{tag: lo}))
    for k, location in enumerate(locations):
        src = SourceDescriptor()
        src.font = master(300 + 40 * k)
        src.location = location
        ds.addSource(src)
    vf, _, _ = varLib.build(ds)
    return vf


def save(vf):
    buf = io.BytesIO()
    vf.save(buf)
    return buf.getvalue()


def raw_avar(curves):
    """`avar` version 1 with these curves, written byte by byte, in order."""
    out = struct.pack(">HHHH", 1, 0, 0, len(curves))
    for curve in curves:
        out += struct.pack(">H", len(curve))
        for frm, to in curve:
            out += struct.pack(">hh", f2dot14(frm), f2dot14(to))
    return out


# --- the faces -------------------------------------------------------------

AVAR2_AXES = [("wght", 100, 400, 900), ("wdth", 50, 100, 100)]
AVAR2_MAPPINGS = [({"wght": 700, "wdth": 50}, {"wght": 550, "wdth": 50}),
                  ({"wght": 900, "wdth": 75}, {"wght": 800, "wdth": 80})]
AVAR2_WEIGHTS = [100, 175, 250, 325.5, 400, 475, 550, 625, 700, 775, 850, 900]
AVAR2_WIDTHS = [50, 55.5, 62.5, 70, 75, 80, 87.5, 99, 100]

# (tag, curve): every curve on an axis of 0..100..200, so that 1.0 is 200.
CURVES = [
    ("lerp", [(-1, -1), (-0.5, -0.75), (0, 0), (0.25, 0.1), (0.6, 0.7), (1, 1)]),
    ("dup2", [(-1, -1), (-0.5, -0.3), (-0.5, -0.7), (0, 0), (0.5, 0.3), (0.5, 0.7), (1, 1)]),
    ("dup3", [(-1, -1), (0, 0), (0.5, 0.2), (0.5, 0.5), (0.5, 0.9), (1, 1)]),
    ("dup4", [(-1, -1), (0, -0.1), (0, 0.05), (0, 0.2), (0, 0.3), (1, 1)]),
    ("unst", [(-1, -1), (0.5, 0.6), (0, 0), (1, 1)]),
    ("one_", [(0.25, 0.5)]),
    ("none", []),
    ("ends", [(-1, -1), (-1, -0.5), (0, 0), (1, 0.5), (1, 1)]),
    ("part", [(-0.5, -0.25), (0, 0), (0.5, 0.75)]),
]

# (tag, (min, default, max) as fvar will say, (min, default, max) to build
# with, since varLib will not build the malformed ones).
LIMITS = [
    ("frac", (0.3, 7.7, 1000.123), (0.3, 7.7, 1000.123)),
    ("degn", (400, 400, 700), (400, 400, 700)),
    ("mnhi", (500, 400, 900), (100, 400, 900)),
    ("mxlo", (100, 800, 700), (100, 800, 900)),
]


def sweep(lo, default, hi, n):
    """`n` points end to end, both sides of the default, plus a few that
    land between round numbers, as `f32`s."""
    vs = {lo, default, hi}
    for k in range(1, n):
        vs.add(lo + (default - lo) * k / n)
        vs.add(default + (hi - default) * k / n)
    for frac in (0.1234567, 0.3141593, 0.4999, 0.5001, 0.6180339, 0.7071068, 0.9999):
        vs.add(lo + (hi - lo) * frac)
    return sorted({f32(v) for v in vs})


def answers(data, instances):
    """HarfBuzz's F2Dot14 and FreeType's 16.16 at each instance, a list of
    user values in fvar order; FreeType's is None where it refuses."""
    path = os.path.join(os.environ.get("TEMP", "/tmp"), "slateos_var_fixture.ttf")
    with open(path, "wb") as f:
        f.write(data)
    font = hb.Font(hb.Face(hb.Blob(data)))
    face = freetype.Face(path)
    tags = [a.tag for a in face.get_variation_info().axes]
    rows = []
    for values in instances:
        font.set_variations(dict(zip(tags, values)))
        harfbuzz = [round(c * 16384) for c in font.get_var_coords_normalized()]
        try:
            face.set_var_design_coords(list(values))
            free = [round(c * 65536) for c in face.get_var_blend_coords()]
        except freetype.FT_Exception:
            free = None
        rows.append((values, harfbuzz, free))
    return rows


def avar2_face():
    vf = build(AVAR2_AXES, AVAR2_MAPPINGS)
    assert vf["avar"].majorVersion == 2
    data = save(vf)
    instances = [(f32(w), f32(d)) for w in AVAR2_WEIGHTS for d in AVAR2_WIDTHS]
    return data, answers(data, instances)


def curves_face():
    vf = build([(tag, 0, 100, 200) for tag, _ in CURVES])
    table = DefaultTable("avar")
    table.data = raw_avar([curve for _, curve in CURVES])
    vf["avar"] = table
    data = save(vf)
    defaults = [100.0] * len(CURVES)
    instances = []
    for i in range(len(CURVES)):
        for v in sweep(0, 100, 200, 16):
            values = list(defaults)
            values[i] = v
            instances.append(tuple(values))
    return data, answers(data, instances)


def limits_face():
    vf = build([(tag, *built) for tag, _, built in LIMITS])
    for axis, (tag, said, _) in zip(vf["fvar"].axes, LIMITS):
        assert axis.axisTag == tag
        axis.minValue, axis.defaultValue, axis.maxValue = said
    data = save(vf)
    defaults = [f32(said[1]) for _, said, _ in LIMITS]
    instances = []
    for i, (_, said, _) in enumerate(LIMITS):
        lo, default, hi = min(said), said[1], max(said)
        for v in sweep(lo, default, hi, 16) + [f32(lo - 50), f32(hi + 50)]:
            values = list(defaults)
            values[i] = v
            instances.append(tuple(values))
    return data, answers(data, instances)


# --- the Rust ----------------------------------------------------------------

def rust_f32(v):
    """The shortest decimal that reads back as the `f32` `v` is -- what Rust
    prints for one, and all the precision a `f32` literal may carry."""
    for digits in range(1, 10):
        s = f"{v:.{digits}g}"
        if f32(float(s)) == v:
            break
    if "e" in s:
        mantissa, exponent = s.split("e")
        s = f"{mantissa if '.' in mantissa else mantissa + '.0'}e{int(exponent)}"
    return s if ("." in s or "e" in s) else s + ".0"


def emit(w, name, doc, data, rows):
    n = len(rows[0][0])
    w(f"/// {doc}\n")
    w(f"pub(crate) static {name}: [u8; {len(data)}] = [{', '.join(f'0x{b:02X}' for b in data)}];\n\n")
    w(f"/// Each instance of [`{name}`]: the user values asked for, in `fvar` order;\n")
    w("/// HarfBuzz's `F2Dot14` coordinates; FreeType's 16.16 ones, `None` where it\n")
    w("/// refuses the instance.\n")
    w(f"pub(crate) static {name}_EXPECTED: [Expected<{n}>; {len(rows)}] = [\n")
    for values, harfbuzz, free in rows:
        ft = "None" if free is None else f"Some([{', '.join(str(c) for c in free)}])"
        w(f"    ([{', '.join(rust_f32(v) for v in values)}], "
          f"[{', '.join(str(c) for c in harfbuzz)}], {ft}),\n")
    w("];\n\n")


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=os.path.join(here, "..", "src", "var_fixture.rs"))
    args = ap.parse_args()
    faces = [
        ("AVAR2", "Weight 100..400..900 by width 50..100, with an `avar` table of version 2.",
         *avar2_face()),
        ("CURVES", "Nine axes of 0..100..200, each with an `avar` curve taking another path.",
         *curves_face()),
        ("LIMITS", "Four axes whose `fvar` limits are awkward or malformed; no `avar`.", *limits_face()),
    ]
    ft_version = ".".join(str(v) for v in freetype.version())
    with open(args.out, "w", encoding="utf-8", newline="\n") as f:
        w = f.write
        w("//! Variable faces, and where HarfBuzz and FreeType put their instances.\n")
        w("//!\n")
        w(f"//! Generated by `gui/font/tools/gen_var_fixture.py` with HarfBuzz {hb.version_string()}\n")
        w(f"//! and FreeType {ft_version}. Do not edit: run the script instead.\n\n")
        w("/// An instance: user values, HarfBuzz's `F2Dot14`, FreeType's 16.16.\n")
        w("pub(crate) type Expected<const N: usize> = ([f32; N], [i16; N], Option<[i32; N]>);\n\n")
        for name, doc, data, rows in faces:
            emit(w, name, doc, data, rows)
    rustfmt(args.out)
    for name, _, data, rows in faces:
        refused = sum(1 for r in rows if r[2] is None)
        print(f"{name}: {len(data)} bytes, {len(rows)} instances ({refused} refused by FreeType)")
    print(f"-> {args.out}")


if __name__ == "__main__":
    main()
