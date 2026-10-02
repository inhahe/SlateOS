## §413 — One skipping iterator for every matcher: skipping is not the same as not matching, and the feature mask gates the input only

**Date:** 2026-08-14
**Lane:** C
**Decided by:** Claude (autonomous)

**Context.** Every GSUB lookup carries a `lookupFlag` naming glyph classes it
is not allowed to see — `IgnoreBaseGlyphs`, `IgnoreLigatures`, `IgnoreMarks`,
a mark-attachment class, or an explicit `MarkGlyphSet`. We parsed the field
and ignored it. Separately, §412 added a per-glyph feature mask but tested it
only at the position a lookup was applied to. Both are the same question —
"may this matcher consider this glyph?" — asked at every position every
matcher steps to, and `gsub.rs` had eight or so places that stepped with
`i + 1`.

**Decision.** A single `Skipper` (`gui/font/src/skip.rs`), built once per
lookup from the flag, the `markFilteringSet` index, the `GDEF` class
definitions and the lookup's feature mask. It exposes `next` / `prev` /
`at_or_after` / `considers` and the two counted walks `walk_forward` /
`walk_backward`. Every matcher in `gsub.rs` — the driver loop, the ligature
component walk, and the backtrack/input/lookahead walks of types 5 and 6 —
goes through it. This is the same shape as HarfBuzz's
`hb_ot_apply_context_t::skipping_iterator_t`, for the same reason: honouring
the flag in one matcher and not the others produces output that is harder to
debug than uniformly ignoring it.

**Three things the design had to get right.**

*Skipping is not the same as not matching.* A glyph the flag excludes is
**stepped over** and the match continues — that is precisely what lets an
`fi` ligature form across a fatha. A glyph the *feature mask* excludes is a
**non-match** and the walk stops. Conflating them in either direction is a
real bug: treat a skip as a stop and `IgnoreMarks` does nothing; treat a
non-match as a skip and a `fina` ligature forms across a letter that is not
in final form. So `skips()` continues the loop and `eligible()` returns
`None`.

*The mask gates the input only.* `Skipper::context()` returns the same
iterator with an all-ones mask, and the backtrack and lookahead walks use it.
A neighbour is a neighbour whatever feature reached the rule; gating context
on the mask made every chaining `fina` rule fail whenever its lookahead was a
medial letter. HarfBuzz draws the same line — `iter_input` carries
`c->lookup_mask`, `iter_context` does not.

*The mask does not change on recursion; the flag does.* A contextual rule
reached by `fina` is still a `fina` rule when it invokes a helper, so `Ctx`
keeps the mask across the nested call — but the nested lookup's own flag is
what the nested `Skipper` is built from.

**Alternatives rejected.**

- *A check inside each matcher.* Eight copies of the same predicate, which is
  how a shaper ends up honouring the flag in three matchers and not the other
  five. Rejected on the same grounds the entry it replaces was filed:
  partial support is worse than none.
- *Repositioning the skipped marks after a ligature, as HarfBuzz does.* When
  `f` mark `i` becomes `fi`, HarfBuzz moves the mark to sit after the whole
  ligature and rewrites its attachment component. We leave the mark where the
  run had it, which for the two-component case is already after the ligature
  glyph and therefore identical. It diverges only for a mark between
  components three and four of a four-component ligature, which no face in
  the 556-face sweep exercises. Filed rather than guessed at, because getting
  it wrong moves diacritics onto the wrong letter — a visible corruption —
  and the correct behaviour needs GPOS 5 (mark-to-ligature) to be meaningful
  anyway.
- *Honouring the flag in GPOS at the same time.* `kern.rs` and `mark.rs`
  reach their subtables through `otl::feature_subtables`, which returns a
  flat list of subtable offsets and has already thrown the `Lookup` away.
  Threading a `Skipper` there means changing that function's return type and
  both callers, which is a separate change with a separate blast radius. Left
  open in `TD-FONT-IGNORES-GSUB-LOOKUP-FLAGS`.

**What it is checked by.** Twelve unit tests in `skip.rs` covering each flag
bit, a `GDEF` too old to have mark glyph sets, an ineligible glyph stopping a
walk, and a walk that runs out of run. Six end-to-end tests in `gsub.rs` —
built on `gsub_flagged` / `gsub_lookups_flagged` / `gdef`, added because
every other GSUB test builder writes a zero flag and so exercised the skipper
only in its do-nothing configuration — proving that `IgnoreMarks` forms a
ligature across a mark and keeps the mark, that it still stops at a non-mark,
that the flag hides nothing when the face ships no `GDEF`, that a mark
filtering set hides every mark it does *not* name, and that a chaining rule
skips marks in its backtrack. Measured end to end by the sweep: the corpus
gained a fully vowelled Arabic string, which 8 of the 556 host faces shaped
wrongly before this change (949 differ / 36 reversed) and none do after
(941 / 44).

**Where.** `gui/font/src/skip.rs` (new), `Lookup::flag` / `Lookup::filter` and
`read_lookup` in `gui/font/src/otl.rs`, `Substitutions::parse` / `Ctx` /
`apply_lookup` / `apply_ligature` / `Matched` / the `forward`/`backward`
walkers in `gui/font/src/gsub.rs`, the `Substitutions::parse` call in
`gui/font/src/sfnt.rs`.
