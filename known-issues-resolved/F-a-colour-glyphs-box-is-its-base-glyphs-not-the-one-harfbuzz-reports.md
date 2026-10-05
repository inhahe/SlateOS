### [F] A colour glyph's box is its base glyph's, not the one HarfBuzz reports -- 2026-09-26 -- **FIXED 2026-09-27**

**Status:** FIXED 2026-09-27 (lane F). `Face::glyph_extents_at` asks `COLR`
first, as HarfBuzz does (`colr::extents::glyph_extents`). A glyph the `ClipList` covers
reports its clip box, varied and rounded as `ClipBoxFormat2` does; any other
colour glyph has its paint measured as `hb_paint_extents` measures it
(`gui/font/src/colr/extents.rs`) -- a version-1 graph once HarfBuzz's bounded
pre-pass finds it bounded, a version-0 glyph's layers -- in HarfBuzz's
`float` arithmetic, with its nesting and edge limits and its cycle detectors.

Checked against HarfBuzz 14.3.0 two ways. A fixture font built for it
(`gui/font/tools/gen_colr_fixture.py`) has 56 colour glyphs: every paint that
moves or combines, fixed and variable, at two instances; clip boxes reached
through `PaintColrGlyph`; cycles; the nesting limit on both sides; a fan-out
cut by the edge limit, where one edge more or less moves the box; null
paints; and an unknown composite mode. And `tools/outline_oracle.py` ran over
six colour fonts from Google Fonts and Mozilla. Four are variable COLRv1
fonts with no clip list at all (Nabla, Honk, Foldit, Kalnia Glaze); the
others are Bungee Spice and Twemoji Mozilla's COLRv0. Every one of 115,666
glyph-instances agrees, where 6,658 had disagreed: Honk 2,960, Twemoji
3,689, Nabla 4 and Bungee Spice 5. `seguiemj.ttf` still agrees on all 12,977.
Tables HarfBuzz's sanitizer would edit are the one difference left, and only
hostile fonts have them: see the next entry.

**In short (as found):** asked for the ink box of a colour emoji glyph, this crate
answers with the box of the plain glyph underneath it, while HarfBuzz answers
with the colour glyph's own. Only the fallback placement of a combining mark
on a colour glyph reads that box, so a mark on an emoji may sit a little off
where HarfBuzz would put it.

**Where:** `Face::glyph_extents_at` (`gui/font/src/sfnt.rs`), which knows
`glyf` and CFF boxes only. HarfBuzz's `hb_font_get_glyph_extents` asks `COLR`
first (`OT::COLR::get_extents`): a version-1 colour glyph's `ClipBox`, varied
at the instance, if it has one; otherwise the extents of its paint
(`hb_paint_extents`), the union of its clipped layers under their transforms.

**Found by:** `tools/outline_oracle.py` over `seguiemj.ttf`, whose boxes
disagree for 697 of 12,977 glyphs sampled while every path agrees.

**What was left, and was done:** the paint extents -- a walk of the paint
graph as `hb_paint_extents` walks it (a clip glyph's box is the bounds of its
drawn path, transformed; a paint unions the current clip into the group's
bounds; `to_glyph_extents` rounds the corners) -- checked with a fixture font
whose colour glyphs have no clip boxes, since no host font had one.
