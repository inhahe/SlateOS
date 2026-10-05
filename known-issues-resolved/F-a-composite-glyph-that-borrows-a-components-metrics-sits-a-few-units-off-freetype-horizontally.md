### [F] A composite glyph that borrows a component's metrics sits a few units off FreeType horizontally -- 2026-09-26

**Status: FIXED 2026-09-26** (lane F) — the suspicion below was right.
`read_components` now keeps the `USE_MY_METRICS` flag, and a composite is
drawn where the last component so flagged places it, recursively
(`Face::drawn_shift`), in `outline`, `outline_at` and `tagged_outline_at`
alike -- as FreeType (`load_truetype_glyph`) and HarfBuzz's glyph drawing do.
The ink box (`glyph_bbox`) keeps the composite's own bearing, because
HarfBuzz's *extents* do, and the mark-positioning fallback that reads it has
to agree with HarfBuzz. Arial's and Times' accented capitals now sit where
FreeType puts them (`hint_oracle.py`); regression test
`a_composite_using_its_components_metrics_is_placed_by_them`.

**In short (as filed):** some accented letters built from two glyphs (`î` in Arial, `Ç`,
`Å` in Times) are drawn a fraction of a pixel to one side of where FreeType
draws them -- 10 font units for Arial's `î`, 0.06 px at 13 px. Hinted or not;
this is where the outline is placed, not how it is fitted.

**Why (suspected).** A composite's horizontal placement comes from its
phantom points, and `Face::glyf_shift` takes them from the composite's own
`hmtx` bearing and `xMin`. A component flagged `USE_MY_METRICS` makes the
composite borrow *that component's* metrics instead, which FreeType honours
(`TT_Process_Composite_Glyph`) and this crate does not read at all. The glyphs
that differ are the ones built that way; confirm by dumping their component
flags.

**Where.** `gui/font/src/sfnt.rs`: `read_components` (does not keep the
flag), `glyf_shift`, and the phantom-point handling in `outline_into_at`.

**How to see it.** `python gui/font/tools/hint_oracle.py C:\Windows\Fonts\arial.ttf
--gids 100,118`: every y agrees, every x is off by the same amount.
