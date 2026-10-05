## 1329. A `glyf` glyph is drawn by HarfBuzz's own steps, its quirks included

**Date:** 2026-09-26
**Lane:** F
**Decided by:** Claude (autonomous) -- within §448, which settled that
drawing and measuring follow HarfBuzz.

**In short:** to draw a TrueType letter, the program collects the letter's
points, moves them for the weight asked for, and joins them into curves.
HarfBuzz, the library this crate matches, does those steps in a particular
order with particular rounding, and has a few oddities of its own. The crate
now does the same steps in the same order with the same rounding -- oddities
too, where the oddity depends only on the font -- so that its drawing can be
checked against HarfBuzz's to the last bit.

**Decision.**

* **Points first** (`crate::glyf`): HarfBuzz's `Glyph::get_points`,
  component placement, cycle detector and limits, then its `path_builder_t`
  and draw session, pen call for pen call. One path for the default instance
  and the varied ones.
* **`gvar` as 14.3.0 applies it** (`gvar::Gvar::apply`): scalars in `f64`;
  deltas scaled before interpolation; added straight in or in batches, as
  HarfBuzz decides; its packed-data readers. FreeType's reading, for the
  hinter, is untouched (§1325).
* **The scalar cache in its steady state.** HarfBuzz draws through a cache
  that rounds a shared tuple's scalar to 2^-30ths; the first glyph to reach a
  tuple sees the exact value, every later one the rounded. Drawing reproduces
  the later; measuring, which HarfBuzz does without the cache, the exact.
* **Quirks reproduced when they depend only on the font:** the cubic
  contour's control point leaking into the next contour, the parent point of
  a point-matched component counted over the whole glyph, the cycle detector
  that notices late. **Not reproduced when they depend on history:** the cache
  answering an intermediate-region tuple with a plain tuple's 0 or 1.

**Alternatives.**

| | For | Against |
|---|---|---|
| HarfBuzz's steps and quirks (chosen) | the oracle compares with `==`; any difference is a bug | reproduces what look like HarfBuzz bugs |
| HarfBuzz's steps, quirks fixed | draws what the font means | the oracle then needs exceptions, which is where real bugs hide |
| The old path, oracle with a tolerance | no rewrite | a tolerance hides exactly the last-bit bugs this found |

**How to reverse.** A quirk is one branch in `glyf.rs` (`contour_end`'s
reset, `Builder::place`'s anchor, `Decycler`); dropping it changes only the
glyphs that meet it. The cache reading is `gvar::Scalars`.
