### [F] A colour bitmap glyph's box was empty, or its outline's, not its picture's -- 2026-09-27 -- **FIXED 2026-09-27**

**Status:** FIXED 2026-09-27 (lane F). `Face::glyph_extents_at` asks `sbix`,
then `CBDT`, before `COLR` and the outlines, as HarfBuzz does
(`bitmap::glyph_extents`, `gui/font/src/bitmap.rs`). It reads the tables
HarfBuzz's way:

- one strike per table, the biggest, the first of those tied, whatever its
  bit depth;
- up to eight `sbix` dupes, and a PNG header read unchecked (0 by 0 when the
  data is too short for one);
- `CBDT` image formats 17 and 18 through index formats 1 and 3 only, from the
  first record that covers the glyph;
- HarfBuzz's own `roundf`;
- HarfBuzz's `upem`;
- the `int16_t` in `scale_glyph_extents`
  (`hbcalc::scale_glyph_extents_at_upem`), which the `COLR` clip-box path now
  goes through too.

A face of pictures alone now has no box for a glyph neither table answers
for, as HarfBuzz has none.

Checked against HarfBuzz 14.3.0 in two ways. `tools/gen_bitmap_fixture.py`
builds 29 glyphs in two byte-built faces, one per case of that reading; five
mutations of the rules are each caught. `tools/outline_oracle.py` ran over
Noto Color Emoji's `CBDT` build and HarfBuzz's own `sbix` and `CBDT` test
fonts (`test/api/fonts`, `test/fuzzing/fonts`): all 4,046 of Noto's glyphs
and every glyph of the others agree.

**In short (as found):** a font whose emoji are pictures rather than
outlines is Noto Color Emoji's bitmap build, the usual emoji font on Linux,
or Apple's `sbix` fonts. It gave every glyph an empty box, or the box of the
plain glyph underneath, where HarfBuzz gives the box of the picture. The
fallback placement of a combining mark reads that box, so a mark on such an
emoji sat where HarfBuzz would not put it. In a face of pictures alone, a
glyph with no picture got an empty box where HarfBuzz has none. That placed
marks HarfBuzz leaves alone, or zeroes.

**Where:** `Face::glyph_extents_at` (`gui/font/src/sfnt.rs`) asked `COLR`
and then the outlines; `hb_ot_get_glyph_extents` asks `sbix` and `CBDT`
first.
