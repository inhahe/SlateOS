### [F] A `COLR` table HarfBuzz's sanitizer would repair is measured as written -- 2026-09-27 -- **OPEN**

**In short:** HarfBuzz checks a colour font's `COLR` table before it uses it,
and quietly repairs what fails the check. It cuts a paint graph off where it
nests more than 64 levels deep. It drops the whole table if the repairs would
take more than 32 edits, or if the check runs out of its work budget. This
crate reads the table as written. So for a font whose paint graph is deeper
than 64 levels, or shared so heavily that checking it is expensive, a colour
glyph's box can differ from HarfBuzz's. Other glyphs' boxes can differ too,
because a repair made in a shared part of the table empties every glyph that
uses that part. No real font is affected; only a hostile or fuzzed one is.

**Where:** `gui/font/src/colr/extents.rs`, which measures boxes for
`Face::glyph_extents_at`. The renderer (`colr.rs`) reads the table the same
way, but it is not a HarfBuzz-parity renderer and has its own limits. The walk
already handles the sanitizer's simplest repairs: a null offset, an offset
past the end of the table and a record cut short all read as the null paint,
as a nulled offset does.

The colour bitmap tables read the same way (`bitmap::glyph_extents`). The
sanitizer's structural checks decide there as they do in HarfBuzz: the
versions, arrays that must fit, a `CBLC` record's first glyph not after its
last, and a strike or subtable HarfBuzz would null. Its 32-edit cap and its
work budget are not emulated there either. A `CBLC` with more than 32 broken
subtables is refused whole by HarfBuzz and read record by record here.

**How to reproduce:** add a glyph `nested(63, glyph("square"))` to
`tools/gen_colr_fixture.py`. That chain is 65 levels deep, and fontTools
shares its `PaintGlyph(square)` record with other glyphs. HarfBuzz nulls that
record's paint offset, so `c_fill` and every other glyph using it report
`[0, 0, 0, 0]`. This crate reports their real boxes. Found while building the
fixture, which now reaches the walk's own nesting limit through
`PaintColrGlyph` hops instead; the sanitizer does not follow those.

**The fix:** once per face, emulate `hb_sanitize_context_t` over `COLR`.
Visit the table in HarfBuzz's order: the base glyph list's records in order,
then the layer list. Count a nesting level for each `Paint::sanitize`, and
check each format's struct size, colour lines and affines as HarfBuzz does.
Record every offset it would null. Run two rounds, as `sanitize_blob` does,
and reject the table past 32 edits or once the operation budget is spent.
The budget is 64 bytes of checking per table byte, and never less than
16,384. Then have `Tables` read the recorded offsets as null. The budget
must be charged byte for byte, as HarfBuzz charges each `check_range`, or the
table is rejected at a different point.
