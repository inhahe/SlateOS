## TD-FONT-DRAWS-A-GLYPH-AT-ITS-STORED-XMIN-NOT-ITS-STATED-BEARING

**Status:** FIXED 2026-08-14 (lane C) -- see **Fixed** below. Moved to `known-issues-resolved/` on 2026-10-05 (lane F).

*Filed and fixed 2026-08-14 (lane C).*

`Face::outline` returned a `glyf` glyph's points exactly as stored, and
`Face::glyph_bbox` returned the glyph header's `xMin`/`xMax` exactly as
stored. Both are wrong for a glyph whose header disagrees with `hmtx`.

The spec says a glyph's `xMin` and its `hmtx` left side bearing are the same
number, and in most fonts, for most glyphs, they are — which is why this went
unnoticed. Where they disagree, every real rasterizer believes `hmtx` and
moves the outline. FreeType computes `pp1.x = bbox.xMin - left_bearing` and
then translates the loaded points by `-pp1.x` (`TT_Process_Simple_Glyph`);
HarfBuzz reports `x_bearing = lsb` with the header's width, under a comment in
`hb-ot-glyf-table.hh` calling it "undocumented rasterizer behavior". We
believed the header and so drew such a glyph at the wrong place on the line.

Windows ships one: `LATINWD.TTF` stores `.notdef` at `xMin = 0, xMax = 546`
with an advance of 682 and a left side bearing of **68** — the box belongs
centred in its cell, 68 units of air on each side, and we drew it flush
against the pen.

Found through the mark fallback rather than through rendering, because that is
what the HarfBuzz sweep exercises: `fallback::place` measures a mark's ink box
against its base's, so a base and a mark both mis-boxed by 68 units put every
combining mark on a `.notdef` base 68 units too far right. On
`שָׁלוֹם` in `LATINWD.TTF` the three
marks came out at `+136 / +68 / 0` where HarfBuzz has `+68 / 0 / -68` — a
uniform `+68`, which is what identified the bearing as the culprit rather than
the centring arithmetic. Substituting `mark.x_bearing = 68` into `place`'s
three arms reproduces HarfBuzz's three numbers exactly.

**Fixed.** `Face::glyf_shift(gid)` returns `lsb - xMin` in font units — zero
when the two agree, when the glyph has no outline, and when `hmtx` cannot
answer, since a face with a damaged `hmtx` should still draw. `Face::outline`
applies it through the new `Outline::translate_x` after parsing (once, to the
finished top-level glyph, composites included — the same place FreeType
applies it), and `Face::glyph_bbox` adds it to `x_min` and `x_max`, which
leaves the width alone. CFF is untouched: a charstring's points are already
positioned at the origin and its `hmtx` bearing is derived from them, so
`glyph_bbox` keeps measuring the path.

Sweep: `agree` 11709 -> 11724, `misplaced` 139 -> 124, `differ` unchanged at
940. The two Hebrew corpus strings now match HarfBuzz on every face that
shapes them. Test
`sfnt::an_outline_lands_on_the_bearing_hmtx_states_not_its_stored_x_min`,
which needed the fixture to become honest first: glyph 3's trailing `hmtx`
bearing had been an arbitrary 25 against a stored `xMin` of 600 (it existed
only to prove the bare-bearing array was read), so it is now `TRUE_LSB_3` =
600 and `build_test_font_with_trailing_lsb` is how a test disagrees with it
deliberately.
