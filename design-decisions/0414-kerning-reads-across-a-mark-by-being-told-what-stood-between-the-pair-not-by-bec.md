## §414 — Kerning reads across a mark by being told what stood between the pair, not by becoming run-aware

**Date:** 2026-08-14
**Lane:** C
**Decided by:** Claude (autonomous)

**Context.** §413 gave GSUB a skipping iterator, but GPOS was left alone.
Kerning was the half that measurably mattered: real faces flag their `kern`
lookups `IgnoreMarks` precisely so that `A` and `V` keep kerning with an
accent between them, and this engine walked the run strictly in order and
could not skip. `scaled.rs` carried a comment apologising for it — pairs
separated by a mark simply went unkerned, so every accented word measured one
kern wider than the face asked for. On this host that is 82 of the 139 faces
that kern `(T,o)`.

The awkwardness is that kerning is not shaped like substitution. `GSUB` walks
a run and can hold an iterator over it; `Face::kern(left, right)` is a
*pair-at-a-time* query with no run behind it, called from the shaping loop
once per adjacent pair. A pair-at-a-time API cannot skip anything, because it
cannot see what it would be skipping.

**The decision.** Give the query a third argument instead of a run:
`kern_across(left, right, between)`, where `between` is the glyphs that stood
between the pair in the caller's run. The caller says what was there; each
lookup decides, from its own `lookupFlag`, whether it may read across it. A
lookup is consulted only if it would have skipped *every* glyph in `between`.

This required `Kerning` to stop flattening its subtables. `otl::feature_subtables`
returns a flat `Vec<usize>` and throws the `Lookup` away with the flag on it;
a new `otl::feature_lookups` returns the lookups whole, and `Kerning` keeps
one `Group { flag, filter, subtables }` per lookup. The flat shape is still
the right one for mark attachment, which asks a coverage-only question, so
both functions stay.

`scaled.rs` tracks the marks between the pair and charges the kern to the
pair's *left* glyph rather than to whatever it pushed last, so the advances
still sum to the run's width when the pair was read across a mark. Mark
attachment already measures its offsets from the accumulated pen, so the
accent lands on the letter regardless of the kern inserted after it.

**Alternatives rejected.**

*Make kerning run-aware, as `GSUB` is.* The honest shape: hand `Kerning` the
whole run and let it hold a `Skipper`. Rejected because `Face::kern` is public
API used by callers who genuinely do have only a pair (the host tests, the
terminal's fixed-cell measurement), and because the shaping loop already
walks the run once — a second walk inside `Kerning` would have to re-derive
the tab and script boundaries the loop already knows about. `between` carries
exactly the information the flag needs and nothing else.

*Skip marks unconditionally, since nearly every `kern` lookup is `IgnoreMarks`
anyway.* This is the tempting shortcut and it is wrong on real faces:
DejaVuSans and Verdana both ship `PairPos` lookups with flag 0, and HarfBuzz
accordingly widens `T`+acute+`o` by their full kern (348 and 220 units). An
engine that always skipped would disagree with both. The flag has to be read.

*Honour the flag in `mark.rs` at the same time.* Deferred, and recorded as
still-open in `known-issues.md`. `scaled.rs` already picks a mark's base by
walking back past marks, which is what `IgnoreMarks` would have said, so the
change would be a no-op today. It becomes real with GPOS 5
(mark-to-ligature), where `IgnoreLigatures` and the mark-attachment class
decide which component a mark lands on.

*Apply the flag to the legacy `kern` table too.* The legacy table predates
lookups and has no flags, so there is nothing to honour; it is parsed as one
group with flag 0, which is also the historically correct reading — engines
that used it kerned strictly adjacent glyphs.

**What it is checked by.** Five unit tests in `kern.rs` (reads across one mark
and across two; declines across a letter, and across a mark-then-letter; an
unflagged lookup declines; a flag with no `GDEF` behind it names an empty set
and so hides nothing; the legacy table kerns only adjacent glyphs), and the
host test `a_mark_between_a_kerning_pair_costs_the_kern_only_if_the_flag_says_so`,
which checks the whole thing at the level a caller sees it — `measure` of
`T`+combining-acute+`o` against HarfBuzz's own answer on five installed faces,
chosen so that three read across and two do not.

**Where.** `feature_lookups` in `gui/font/src/otl.rs`; `Skipper::skips` made
`pub(crate)` in `gui/font/src/skip.rs`; `Kerning` / `Group` / `parse` / `pair`
in `gui/font/src/kern.rs`; `Face::kern_across` in `gui/font/src/sfnt.rs`; the
`kern_left`/`between` shaping loop and `ScaledFont::kern_across` in
`gui/font/src/scaled.rs`.
