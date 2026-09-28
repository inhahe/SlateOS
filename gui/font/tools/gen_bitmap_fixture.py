r"""Generate `gui/font/src/bitmap_fixture.rs`: faces whose glyphs are colour
bitmaps (`sbix`, `CBLC`/`CBDT`), and the box HarfBuzz reports for each glyph.

    python gui/font/tools/gen_bitmap_fixture.py

Needs `fonttools` and `uharfbuzz` (HarfBuzz 14.3.0).

HarfBuzz asks for a glyph's extents in a fixed order -- `sbix`, then `CBDT`,
then `COLR`, then the outline tables (`hb_ot_get_glyph_extents`) -- and each
bitmap table answers from one strike only: the biggest, the first of those
tied, when the font is not given a size (`choose_strike`). What a strike says
about a glyph is in its own pixels -- a PNG's header size and origin offset
for `sbix`, the glyph metrics for `CBDT` -- scaled to font units by
`roundf (v * upem / ppem)`. Where a table cannot answer, the next is asked.

The tables here are written byte by byte, since fontTools will not build most
of what they hold:

* strikes tied for biggest, strikes of other sizes, and a null one;
* both `CBLC` index formats HarfBuzz reads and the three it does not, and a
  record whose subtable is null;
* image formats 17, 18 and 19;
* `sbix` glyphs that are `dupe`s of others, in chains as long as HarfBuzz
  follows and one longer;
* JPEG and PNG glyphs, PNG data too short to have a header (which HarfBuzz
  reads as a picture of no size), and a picture too big;
* values that scale to exact halves, which HarfBuzz's own `roundf` takes up;
* a box wide enough that the `int16_t` in HarfBuzz's `scale_glyph_extents`
  wraps it.

One face has outlines, a `COLR` table and both bitmap tables, so that every
step of the order shows. The other has `CBLC`/`CBDT` alone, as Noto Color
Emoji's bitmap build does, with a monochrome strike bigger than its colour
one.
"""

import io
import os
import struct

from fontTools.colorLib.builder import buildCOLR, buildCPAL
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib.tables.DefaultTable import DefaultTable

from rustfmt_out import rustfmt

UPEM = 1000


def be(fmt, *values):
    return struct.pack(">" + fmt, *values)


# ---------------------------------------------------------------------------
# sbix
# ---------------------------------------------------------------------------

def png_header(width, height):
    """What HarfBuzz reads of a PNG: the signature (unchecked), then IHDR's
    length, tag, width and height, then five bytes."""
    return (b"\x89PNG\r\n\x1a\n" + be("I", 13) + b"IHDR" + be("II", width, height)
            + bytes([8, 6, 0, 0, 0]) + b"rest of the picture")


def sbix_glyph(x, y, tag, data):
    return be("hh", x, y) + tag + data


def dupe(gid):
    return sbix_glyph(0, 0, b"dupe", be("H", gid))


def sbix_table(num_glyphs, strikes, version=1):
    """`strikes`: (ppem, {gid: glyph bytes}), or None for a null offset;
    every other glyph is empty."""
    count = len(strikes)
    head = be("HHI", version, 1, count)
    offsets_at = len(head)
    body = b""
    strike_offsets = []
    first = offsets_at + 4 * count
    for strike in strikes:
        if strike is None:
            strike_offsets.append(0)
            continue
        ppem, glyphs = strike
        strike_offsets.append(first + len(body))
        offsets = []
        data = b""
        header = 4 + 4 * (num_glyphs + 1)
        for gid in range(num_glyphs):
            offsets.append(header + len(data))
            data += glyphs.get(gid, b"")
        offsets.append(header + len(data))
        body += be("HH", ppem, 72) + b"".join(be("I", o) for o in offsets) + data
    return head + b"".join(be("I", o) for o in strike_offsets) + body


# ---------------------------------------------------------------------------
# CBLC / CBDT
# ---------------------------------------------------------------------------

def small_metrics(height, width, bx, by, advance):
    return be("BBbbB", height, width, bx, by, advance)


def big_metrics(height, width, bx, by, advance):
    return be("BBbbBbbB", height, width, bx, by, advance, 0, 0, 0)


def image(fmt, metrics, payload=b"PNG data"):
    """A CBDT glyph of image format 17 or 18 (metrics, then the data's
    length and the data), or 19 (the length and the data alone)."""
    if fmt == 19:
        return be("I", len(payload)) + payload
    return metrics + be("I", len(payload)) + payload


def cbdt_tables(strikes):
    """`strikes`: (ppem_x, ppem_y, bit_depth, subtables), each subtable
    (first, last, index format, image format, {gid: glyph bytes}, extra):
    `extra` is the index format 2/5 metrics and image size."""
    cbdt = bytearray(be("HH", 3, 0))
    size_records = b""
    arrays = b""
    base = 8 + 48 * len(strikes)
    for ppem_x, ppem_y, depth, subtables in strikes:
        array_at = base + len(arrays)
        records = b""
        tables = b""
        records_len = 8 * len(subtables)
        for first, last, index_format, image_format, glyphs, extra in subtables:
            if index_format is None:
                records += be("HHI", first, last, 0)
                continue
            sub_at = records_len + len(tables)
            data_at = len(cbdt)
            ids = list(range(first, last + 1))
            offsets = []
            for gid in ids:
                offsets.append(len(cbdt) - data_at)
                cbdt += glyphs.get(gid, b"")
            offsets.append(len(cbdt) - data_at)
            head = be("HHI", index_format, image_format, data_at)
            if index_format == 1:
                body = b"".join(be("I", o) for o in offsets)
            elif index_format == 3:
                body = b"".join(be("H", o) for o in offsets)
                if len(ids) % 2 == 0:
                    body += be("H", 0)
            elif index_format == 2:
                size, metrics = extra
                body = be("I", size) + metrics
            elif index_format == 4:
                pairs = [(gid, offsets[i]) for i, gid in enumerate(ids)] + [(0, offsets[-1])]
                body = be("I", len(ids)) + b"".join(be("HH", g, o) for g, o in pairs)
            elif index_format == 5:
                size, metrics = extra
                body = be("I", size) + metrics + be("I", len(ids)) + b"".join(be("H", g) for g in ids)
                if len(ids) % 2 == 1:
                    body += be("H", 0)
            else:
                raise ValueError(index_format)
            tables += head + body
            records += be("HHI", first, last, sub_at)
        array = records + tables
        arrays += array
        line = be("bbBbbbbbbbbb", 100, -20, 120, 1, 0, 0, 0, 0, 0, 0, 0, 0)
        first_glyph = min(s[0] for s in subtables)
        last_glyph = max(s[1] for s in subtables)
        size_records += (be("IIII", array_at, len(array), len(subtables), 0) + line + line
                         + be("HHBBBb", first_glyph, last_glyph, ppem_x, ppem_y, depth, 1))
    cblc = be("HHI", 3, 0, len(strikes)) + size_records + arrays
    return cblc, bytes(cbdt)


# ---------------------------------------------------------------------------
# The faces
# ---------------------------------------------------------------------------

def outline(pen, x0, y0, x1, y1):
    pen.moveTo((x0, y0))
    pen.lineTo((x0, y1))
    pen.lineTo((x1, y1))
    pen.lineTo((x1, y0))
    pen.closePath()


# Glyph names, in order: each says which table should answer for it.
NAMES = [
    ".notdef",
    "sbix_png",            # 1
    "sbix_dupe",           # 2: -> sbix_png
    "sbix_dupe_chain",     # 3: eight dupes, then sbix_png
    "sbix_dupe_too_long",  # 4: nine dupes: HarfBuzz gives up
    "sbix_jpg",            # 5: not PNG: on to CBDT
    "sbix_short_png",      # 6: 10 bytes, no header: a picture of no size
    "sbix_one_byte",       # 7: one byte of PNG data
    "sbix_huge_png",       # 8: 70000 wide: refused, on to CBDT
    "sbix_odd",            # 9: negative offsets, sizes that round
    "sbix_dupe_short",     # 10: a dupe of one byte: nothing
    "sbix_dupe_far",       # 11: a dupe of a glyph past the last
    "sbix_other_strike",   # 12: only in a smaller strike: on to CBDT
    "cbdt_17",             # 13
    "cbdt_18",             # 14
    "cbdt_19",             # 15: index format 2, image format 19: not read
    "cbdt_idx3",           # 16: index format 3
    "cbdt_idx4",           # 17: index format 4: not read
    "cbdt_short",          # 18: format 17 with 8 bytes: not read
    "cbdt_empty",          # 19: no data
    "cbdt_negative",       # 20: negative bearings, sizes that round
    "cbdt_other_strike",   # 21: only in a smaller strike: on to COLR
    "colr_glyph",          # 22: COLR answers
    "plain",               # 23: the outline answers
    "sbix_halves",         # 24: values that scale to exact halves
    "cbdt_halves",         # 25: likewise
    "sbix_wide",           # 26: 375,000 units wide, past int16_t
    "cbdt_null_sub",       # 27: first found in a record with a null subtable
    "cbdt_idx5",           # 28: index format 5: not read
]
GID = {name: i for i, name in enumerate(NAMES)}


def sbix_biggest():
    """The glyphs of the biggest `sbix` strike, but for the chains of dupes,
    which `build_sbix` adds."""
    g = GID
    return {
        g["sbix_png"]: sbix_glyph(3, -7, b"png ", png_header(100, 80)),
        g["sbix_dupe"]: dupe(g["sbix_png"]),
        g["sbix_jpg"]: sbix_glyph(1, 2, b"jpg ", b"\xff\xd8\xff\xe0 some jpeg"),
        g["sbix_short_png"]: sbix_glyph(-4, 9, b"png ", b"0123456789"),
        g["sbix_one_byte"]: sbix_glyph(5, 5, b"png ", b"x"),
        g["sbix_huge_png"]: sbix_glyph(0, 0, b"png ", png_header(70000, 10)),
        g["sbix_odd"]: sbix_glyph(-5, -3, b"png ", png_header(33, 17)),
        g["sbix_dupe_short"]: sbix_glyph(0, 0, b"dupe", b"\x01"),
        g["sbix_dupe_far"]: dupe(9999),
        # At 6.25 units a pixel: -12.5, 0, 12.5 and -12.5, which HarfBuzz's
        # own `roundf` takes up, to -12, 0, 13 and -12 -- the C library's
        # would give -13 for both negative halves.
        g["sbix_halves"]: sbix_glyph(-2, -2, b"png ", png_header(2, 2)),
        # 60000 pixels at 6.25 units: HarfBuzz scales the box's edges
        # through `int16_t` (`scale_glyph_extents`), so the right edge wraps.
        g["sbix_wide"]: sbix_glyph(0, 0, b"png ", png_header(60000, 3)),
    }


def build_sbix(num_glyphs):
    """The `sbix` table, and how many glyphs the face needs for it: the
    chains of dupes pass through glyphs after the named ones, `chain_k`."""
    g = GID
    big = sbix_biggest()
    chain = [num_glyphs + k for k in range(9)]
    total = num_glyphs + len(chain)
    # sbix_dupe_chain: 3 -> chain[1] -> ... -> chain[7] -> sbix_png: eight
    # dupes, as many as HarfBuzz follows.
    links = [g["sbix_dupe_chain"]] + chain[1:8]
    for a, b in zip(links, links[1:]):
        big[a] = dupe(b)
    big[links[-1]] = dupe(g["sbix_png"])
    # sbix_dupe_too_long: 4 -> chain[0] -> chain[1] -> ... -> sbix_png: nine,
    # one more than HarfBuzz follows.
    big[g["sbix_dupe_too_long"]] = dupe(chain[0])
    big[chain[0]] = dupe(chain[1])
    small = {
        g["sbix_png"]: sbix_glyph(0, 0, b"png ", png_header(40, 40)),
        g["sbix_other_strike"]: sbix_glyph(2, 2, b"png ", png_header(20, 20)),
    }
    # Two strikes tied for biggest: the first answers.
    tied = {g["sbix_png"]: sbix_glyph(0, 0, b"png ", png_header(1, 1))}
    # A null strike offset, which is HarfBuzz's null strike, of no size --
    # and in a table of version 500, which HarfBuzz takes, so that reading
    # the offset as the table's own start would find a strike of 500.
    return sbix_table(total, [(64, small), None, (160, big), (160, tied)], version=500), total


def build_cbdt(num_glyphs, alone=False):
    g = GID
    first, last = g["cbdt_17"], g["cbdt_negative"]
    main = {
        g["cbdt_17"]: image(17, small_metrics(90, 80, 5, 70, 100)),
        g["cbdt_18"]: image(18, big_metrics(60, 50, -3, 44, 64)),
        g["cbdt_short"]: small_metrics(10, 10, 0, 0, 10) + b"xyz",
        g["cbdt_negative"]: image(17, small_metrics(37, 29, -11, -7, 30)),
    }
    idx3 = {g["cbdt_idx3"]: image(17, small_metrics(20, 30, 4, 25, 33))}
    idx4 = {g["cbdt_idx4"]: image(17, small_metrics(20, 30, 4, 25, 33))}
    fmt19 = {g["cbdt_19"]: image(19, None)}
    idx5 = {g["cbdt_idx5"]: image(17, small_metrics(20, 30, 4, 25, 33))}
    # At 12.5 units a pixel across and 6.25 up: -12.5, -12.5, 12.5 and
    # -12.5, which HarfBuzz's `roundf` takes to -12, -12, 13 and -12.
    halves = {g["cbdt_halves"]: image(17, small_metrics(2, 1, -1, -2, 2))}
    null_first = {g["cbdt_null_sub"]: image(17, small_metrics(5, 5, 1, 4, 6))}
    subtables = [
        # HarfBuzz stops at the first record covering a glyph, null or not.
        (g["cbdt_null_sub"], g["cbdt_null_sub"], None, None, None, None),
        (g["cbdt_null_sub"], g["cbdt_null_sub"], 1, 17, null_first, None),
        (first, g["cbdt_18"], 1, 17, main, None),
        (g["cbdt_19"], g["cbdt_19"], 2, 19, fmt19, (12, big_metrics(10, 10, 1, 9, 12))),
        (g["cbdt_idx3"], g["cbdt_idx3"], 3, 17, idx3, None),
        (g["cbdt_idx4"], g["cbdt_idx4"], 4, 17, idx4, None),
        (g["cbdt_short"], last, 1, 17, main, None),
        (g["cbdt_halves"], g["cbdt_halves"], 1, 17, halves, None),
        (g["cbdt_idx5"], g["cbdt_idx5"], 5, 17, idx5, (43, big_metrics(20, 30, 4, 25, 33))),
    ]
    if alone:
        # A colour strike with everything, and a bigger monochrome one with
        # one glyph: HarfBuzz asks the biggest whatever its depth, so every
        # other glyph has no box at all -- in a face with no outlines to
        # fall back on.
        mono = [(g["cbdt_18"], g["cbdt_18"], 1, 17, {
            g["cbdt_18"]: image(17, small_metrics(70, 64, 2, 61, 70))}, None)]
        return cbdt_tables([(109, 109, 32, subtables), (150, 150, 1, mono)])
    small = [(g["cbdt_17"], g["cbdt_other_strike"], 1, 17, {
        g["cbdt_17"]: image(17, small_metrics(9, 8, 0, 7, 10)),
        g["cbdt_other_strike"]: image(17, small_metrics(12, 12, 1, 10, 13)),
    }, None)]
    tied = [(g["cbdt_17"], g["cbdt_17"], 1, 17, {
        g["cbdt_17"]: image(17, small_metrics(1, 1, 0, 1, 1))}, None)]
    # 80 by 160 is the biggest (by its larger side); 160 by 80 ties it and
    # comes after, so the first answers; a monochrome strike smaller still.
    return cbdt_tables([(40, 40, 32, small), (80, 160, 32, subtables), (160, 80, 32, tied),
                        (20, 20, 1, small)])


def build_face(with_outlines=True):
    names = list(NAMES)
    fb = FontBuilder(UPEM, isTTF=True)
    sbix, total = build_sbix(len(names))
    names += [f"chain_{k}" for k in range(total - len(names))]
    fb.setupGlyphOrder(names)
    fb.setupCharacterMap({0x41 + i: n for i, n in enumerate(names[1:len(NAMES)])})
    glyf = {}
    for i, name in enumerate(names):
        pen = TTGlyphPen(None)
        if with_outlines and name != ".notdef":
            # Every glyph a different box, so the outline's answer shows.
            outline(pen, 10 + i, -20 - i, 300 + 7 * i, 500 + 3 * i)
        glyf[name] = pen.glyph()
    fb.setupGlyf(glyf)
    fb.setupHorizontalMetrics({name: (600, 10 + i if with_outlines else 0)
                               for i, name in enumerate(names)})
    fb.setupHorizontalHeader(ascent=800, descent=-200)
    fb.setupNameTable({"familyName": "BitmapFixture", "styleName": "Regular"})
    fb.setupOS2()
    fb.setupPost()
    font = fb.font
    if with_outlines:
        font["CPAL"] = buildCPAL([[(1.0, 0.0, 0.0, 1.0)]])
        font["COLR"] = buildCOLR({
            "colr_glyph": {"Format": 10, "Glyph": "plain",
                           "Paint": {"Format": 2, "PaletteIndex": 0, "Alpha": 1.0}},
            "cbdt_other_strike": {"Format": 10, "Glyph": "plain",
                                  "Paint": {"Format": 2, "PaletteIndex": 0, "Alpha": 1.0}},
        }, glyphMap=font.getReverseGlyphMap())
        table = DefaultTable("sbix")
        table.data = sbix
        font["sbix"] = table
        cblc, cbdt = build_cbdt(len(names))
    else:
        cblc, cbdt = build_cbdt(len(names), alone=True)
        del font["glyf"]
        del font["loca"]
    for tag, data in (("CBLC", cblc), ("CBDT", cbdt)):
        table = DefaultTable(tag)
        table.data = data
        font[tag] = table
    buf = io.BytesIO()
    font.save(buf)
    return buf.getvalue(), names


def harfbuzz_extents(data, count):
    import uharfbuzz as hb

    face = hb.Face(hb.Blob(data))
    font = hb.Font(face)
    font.scale = (face.upem, face.upem)
    rows = []
    for gid in range(count):
        e = font.get_glyph_extents(gid)
        rows.append(None if e is None else [e.x_bearing, e.y_bearing, e.width, e.height])
    return rows


def rust_bytes(data):
    return ", ".join(f"0x{b:02X}" for b in data)


def rust_box(box):
    return "None" if box is None else "Some([" + ", ".join(str(v) for v in box) + "])"


def rust_row(gid, name, in_full, in_alone):
    return f"    ({gid}, \"{name}\", {rust_box(in_full)}, {rust_box(in_alone)}),\n"


def main():
    import uharfbuzz as hb

    # A fixed build time keeps the fonts' bytes, and so this file, the same
    # from one run to the next.
    os.environ.setdefault("SOURCE_DATE_EPOCH", "1790463450")
    full, _ = build_face(True)
    alone, _ = build_face(False)
    full_rows = harfbuzz_extents(full, len(NAMES))
    alone_rows = harfbuzz_extents(alone, len(NAMES))
    here = os.path.dirname(os.path.abspath(__file__))
    out = os.path.join(here, "..", "src", "bitmap_fixture.rs")
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        w = f.write
        w("//! Faces whose glyphs are colour bitmaps, and HarfBuzz's box for each glyph.\n")
        w("//!\n")
        w(f"//! Generated by `gui/font/tools/gen_bitmap_fixture.py` with HarfBuzz {hb.version_string()}.\n")
        w("//! Do not edit: run the script instead.\n\n")
        w("/// Outlines, `COLR`, `sbix` and `CBLC`/`CBDT`: strikes of several sizes in\n")
        w("/// each bitmap table, the biggest tied, a null one, and a glyph for each case\n")
        w("/// of HarfBuzz's reading.\n")
        w(f"pub(crate) static BITMAP_FACE: [u8; {len(full)}] = [{rust_bytes(full)}];\n\n")
        w("/// `CBLC`/`CBDT` and nothing to draw with besides, as Noto Color Emoji's\n")
        w("/// bitmap build is -- with a monochrome strike bigger than its colour one.\n")
        w(f"pub(crate) static BITMAP_ALONE_FACE: [u8; {len(alone)}] = [{rust_bytes(alone)}];\n\n")
        w("/// A glyph's box as HarfBuzz reports it -- `[x_bearing, y_bearing, width,\n")
        w("/// height]` at one unit per font unit -- or `None` where it has none.\n")
        w("pub(crate) type HbBox = Option<[i32; 4]>;\n\n")
        w("/// HarfBuzz's box for each named glyph: `(glyph, name, in `BITMAP_FACE`, in\n")
        w("/// `BITMAP_ALONE_FACE`)`.\n")
        w(f"pub(crate) static BITMAP_EXTENTS: [(u16, &str, HbBox, HbBox); {len(NAMES)}] = [\n")
        for gid, name in enumerate(NAMES):
            w(rust_row(gid, name, full_rows[gid], alone_rows[gid]))
        w("];\n")
    rustfmt(out)
    for gid, name in enumerate(NAMES):
        print(f"{gid:3} {name:22} {full_rows[gid]} {alone_rows[gid]}")


if __name__ == "__main__":
    main()
