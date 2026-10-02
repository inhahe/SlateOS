### [F] Variable fonts were normalized, rounded and measured as HarfBuzz 8 does it, not 14.3.0 -- 2026-09-26 -- **FIXED 2026-09-26**

**Status:** FIXED 2026-09-26 (lane F) — the HarfBuzz-facing arithmetic
follows HarfBuzz 14.3.0 (the version the crate's oracle, uharfbuzz 0.56,
runs) step for step, and HarfBuzz's own `roundf` (`floorf(x + 0.5f)`, a half
up) is `gui/font/src/hbcalc.rs` (design-decisions §1326).

**In short (as found):** a variable font asked for an instance -- Bold, a
narrower width -- could come out one font unit different from what HarfBuzz
gives: one glyph of Bahnschrift one unit narrower at every SemiCondensed
instance (987 in HarfBuzz, 986 here), three of Reem Kufi's kerns one unit
tighter at weight 550, and marks stacked on a missing-glyph box at Sitka's
larger optical sizes two units off. A unit is a fraction of a pixel, so
nothing looked broken; it made the crate disagree with the library it
claims to match.

**Why.** The code followed HarfBuzz 8 and a misreading of it:

* **Normalizing.** HarfBuzz 14.3.0 rounds to 16.16 first, applies `avar` in
  `float`, rounds again, and reaches `F2Dot14` by `(c + 2) >> 2`; this
  rounded once, straight to `F2Dot14`. The two differ on one instance in
  eight (12.5% of a uniform sweep) -- weight 700 of 100..400..900 is 9831 in
  HarfBuzz and was 9830 here.
* **Rounding.** HarfBuzz defines `roundf(x)` as `floorf(x + .5f)`. This took
  it for the C library's, half away from zero, in normalizing and in the
  `HVAR`, `MVAR` and `GDEF` deltas (`round_to_i16`): -4.5 was -5 here and is
  -4 in HarfBuzz.
* **Stores.** Sums were fused (`mul_add`); HarfBuzz's are not. A region at
  the default scored 1 if malformed; HarfBuzz scores it 0. An index map with
  reserved bits was refused and an empty one read as no delta; HarfBuzz reads
  both.
* **Extents.** The mark fallback cut a varied glyph's box to whole units;
  HarfBuzz rounds its edges, then the width and height from them, and calls
  a box of no area empty. A CFF glyph's box is rounded the same way.

**How it hid.** The host tests' tables came from independent Python
transcriptions (`variable_survey.py --normalize`, `varstore_oracle.py`)
written to the same misreading -- the advance oracle even agreed with the
crate on Bahnschrift's 986.5 by accident, rounding it half to even. Those
tools now ask uharfbuzz for their numbers. The HarfBuzz shaping sweep had
been run only at instances whose coordinates happen to round the same way.

**Checked:** `var_fixture.rs` (588 instances of three faces built to reach
each rule, both libraries), `var_oracle.py` (2,598 instances of 13 real
variable faces, both libraries, all agreeing), the host tests with their
tables regenerated from HarfBuzz, and the HarfBuzz shaping sweep -- all 556
host faces at the default instance (61,046 agree; the one misplacement is
the long-standing `a<CGJ>b`) and the 7 variable faces at eleven instances.
