### [F] A variable font at another weight was hinted from points a unit off FreeType's -- 2026-09-26 -- **FIXED 2026-09-26**

**Status:** FIXED 2026-09-26 (lane F) — the hinter now reads a variable
glyph as FreeType's loader gives it to FreeType's hinter: the instance in
FreeType's 16.16 normalization (`Coords::fixed`), `gvar` in FreeType's
fixed point, each point's delta and each component offset rounded to a
unit, scaled components by `FT_MulFix`, and the origin moved by the left
phantom point only without `HVAR` (`Face::load_unscaled`,
`Gvar::deltas_fixed`). Every glyph agrees with FreeType at fifteen
instances of eight variable fonts; the hint fixture holds a variable face
with and without `HVAR` (design-decisions §1325).

**In short (as found):** bold or condensed text in a variable font -- Segoe UI
Variable, Bahnschrift, Noto Sans -- was hinted from glyph points a fraction of
a font unit off the ones FreeType's hinter sees. Mostly that moved a point a
64th of a pixel; on a few accented letters it moved a stroke a whole pixel.
The default weight was never affected.

**Why.** The glyph's points came from the drawing path: `gvar` deltas summed
exactly and HarfBuzz's `F2Dot14` coordinates, with the final sum rounded
once. FreeType's loader normalizes in 16.16, sums in its fixed point, rounds
each point's delta and each component offset separately, and with `HVAR`
does not move the glyph by its phantom point's delta at all -- where this
subtracted it. Found by `hint_oracle.py --var` (new): 69% agreement for Noto
Sans at weight 700, width 87.5.
