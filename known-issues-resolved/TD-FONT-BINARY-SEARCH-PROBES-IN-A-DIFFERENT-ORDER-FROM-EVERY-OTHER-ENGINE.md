## TD-FONT-BINARY-SEARCH-PROBES-IN-A-DIFFERENT-ORDER-FROM-EVERY-OTHER-ENGINE -- FIXED

**Fixed.** `gui/font/src/otl.rs`'s shared `binary_search` walked a half-open
interval `[lo, hi)` with `mid = lo + (hi - lo) / 2`. C's `bsearch`, and
HarfBuzz's `hb_bsearch_impl` after it, walk the closed interval `[lo, hi]`
with `mid = (lo + hi) / 2`. On a sorted array the two agree on every query, so
for years the difference was invisible.

Fonts are not always sorted. `ELEPHNT.TTF` ships a legacy `kern` table whose
624 format-0 pairs are ordered by the *second* glyph alone — the spec requires
them ordered by the two glyphs taken together as one 32-bit key, and 290 of the
623 adjacent pairs are inversions under that ordering. A binary search over
such an array is not wrong so much as arbitrary: it finds whatever its probe
sequence happens to land on. Both loops find exactly 13 of the 624 pairs, and
the 13 are disjoint sets, because the first probe is index 311 for the closed
interval and 312 for the half-open one and the descents never meet again.

The witness was `office fluffy waffle` at 512 units/em: HarfBuzz put the `a` of
"waffle" 10 units left of where we did, because `(w, a) = -10` sits at index
301, which the closed descent reaches and the half-open one does not. The
`(f, f) = -23` pair at index 333 was found by *both*, which is what made the
bug look like a missing special case rather than a missing pair — the legacy
kern path was plainly working, just not for that one pair.

`binary_search` now uses the closed interval, and three tests pin it: an
exhaustive correctness sweep over every size and key up to 64, a test that
watches where the first probe lands for counts 1/2/3/624/625, and one that
searches a deliberately half-sorted array and asserts only the single record a
descent from the midpoint can name is found.

Agreeing with every other engine on a malformed font is worth more than the
tidier arithmetic. There is no "correct" answer to a binary search over an
unsorted array, so the only defensible choice is the one that makes our text
measure the same as the same text in a browser. Sweep: `agree` 11828 -> 11829,
`misplaced` 20 -> 19.

**Not fixed: detecting the unsortedness and searching linearly.** That would
apply 611 more of `ELEPHNT`'s pairs than HarfBuzz does and put us alone among
engines. The font was never tested with those pairs applied, so "the
designer's intent" is not a thing the table can be asked about.
