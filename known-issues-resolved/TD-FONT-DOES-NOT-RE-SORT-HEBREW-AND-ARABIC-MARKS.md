## TD-FONT-DOES-NOT-RE-SORT-HEBREW-AND-ARABIC-MARKS

**Status:** FIXED 2026-08-14 -- the resolution note in the entry below gives the change; on `main` since. Stamped 2026-09-28 by lane F, `gui/font`'s owner since the six-lane split, so that the heading no longer reads as unfixed. The positions it left unresolved, at 249, on the pointed-Hebrew and meteg strings agree with HarfBuzz on every host face since `GPOS` types 7 and 8 (`TD-GPOS-HAS-NO-CONTEXTUAL-OR-MARK-TO-LIGATURE-POSITIONING`). Moved to `known-issues-resolved/` on 2026-10-05 (lane F).

**What.** Unicode gives Hebrew points the canonical combining classes 10–26
and Arabic vowel signs 27–36. Those numbers are an *ordering*, not a place on
the glyph, and the order they impose is not the order the marks are drawn in.
HarfBuzz deals with this by permuting them into a private set of "modified
combining classes" (`hb-unicode.hh`) whose numeric order *is* display order,
so that its normalizer's canonical sort leaves the marks stacked bottom-to-top
in the right sequence. `gui/font/src/fallback.rs` implements the
*recategorization* those classes feed — transposed to match on real Unicode
classes, which is sound because the permutation is injective within each
block — but not the re-sorting.

**Symptom.** Two Hebrew points, or two Arabic marks, typed in an order that
canonical ordering does not already fix, stack in the wrong vertical order on
a face with no `GPOS`. Both marks are on the right letter and on the right
side of it; only their order relative to each other is wrong. Not currently
visible in the sweep, whose Hebrew and Arabic strings are all in an order
where the two agree.

**Why it is filed rather than fixed.** It is a change to `norm.rs`'s canonical
ordering pass, not to the fallback: the sort has to run on the modified
classes for the reordering to happen at all, which means `norm::pieces` would
need a second class function used only for sorting, and its output would then
differ from NFC for these scripts. That is a real decision about what
`norm.rs` promises — see `design-decisions.md` §410, which says it normalizes
to NFC as a layer that knows nothing about the font — and it wants making
deliberately rather than as a side-effect of the fallback.

**Proper fix.** Add the modified-class table to `norm_tables.rs`, sort with it
inside the shaper only, and document that `ScaledFont::shape`'s piece order is
display order rather than NFC order for Hebrew and Arabic. Then the fallback's
`attach_class` needs no change at all — it already matches on the real
classes.

**Where.** `gui/font/src/norm.rs` — the canonical ordering pass;
`gui/font/src/fallback.rs` — `attach_class`, whose doc records the
transposition.

**Now visible in the sweep.** The corpus gained pointed Hebrew — `U+05E9
U+05B8 U+05C1 U+05DC U+05D5 U+05B9 U+05DD` — for the `GPOS` 7/8 work, and it
is in exactly the order that shows this. On `AGENCYB.TTF` (no `GPOS`, no
Hebrew, so every glyph is notdef) the qamats and the shin dot come out in
opposite slots from HarfBuzz's, with the right offsets each: ours places the
shin dot at visual 4 with `y +1761` and the qamats at 5 with `y -1761`,
HarfBuzz the reverse, because its sort moved the shin dot ahead of the qamats.
465 of the sweep's `misplaced` cases are this string. The sweep reports it as
`misplaced` rather than `reordered` only because the glyphs are all notdef and
so compare equal — on a face that has the glyphs it is a reordering.

**Fixed** (2026-08-14). `norm::sort_marks` now takes the class function as a
parameter and runs twice: `nfc` sorts with `combining_class`, and
`norm::pieces` sorts again with the new `display_class` — HarfBuzz's
`_hb_modified_combining_class`, a permutation of the fixed-position blocks
whose numeric order is stacking order. Sweep: `agree` 10917 → 11223,
`misplaced` 841 → 625, `reordered` 32 → 0, `differ` 998 → 940, and this string
465 → 249.

The decision the entry was waiting on is recorded as `design-decisions.md`
§419. It went the way that keeps §410 intact: `nfc()` is still exactly NFC, and
the second sort lives in `pieces`, the shaper's entry point, which has been
font-dependent since §410 and promises only "the characters a face should
actually be asked for". Sorting inside `nfc` — which is what HarfBuzz does,
since its normalizer is private to the shaper — would have made the name a lie.

Two things the tests found that reading HarfBuzz would not have:

- Class 26 (Hebrew point varika) and 34–36 (sukun, superscript alef,
  superscript alaph) are **fixed points**, not part of the permutation.
- The Tibetan block is not a bijection onto its own range: 132 maps to 131,
  which is legal only because Unicode assigns no character class 131. Stating
  the claim over the range rather than over the assigned classes fails.

**Still open at 249 for this string, and 249 for the meteg one.** Both are now
a different disagreement — not the order of the marks but where they are put —
and both want their own entry once diagnosed.
