## TD-FONT-IGNORES-GSUB-LOOKUP-FLAGS

**Status:** FIXED 2026-08-14 -- the resolution note in the entry below gives the change; on `main` since. Stamped 2026-09-28 by lane F, `gui/font`'s owner since the six-lane split, so that the heading no longer reads as open.

**What.** Every GSUB lookup carries a `lookupFlag`: `RightToLeft`,
`IgnoreBaseGlyphs`, `IgnoreLigatures`, `IgnoreMarks`, `UseMarkFilteringSet`
and a mark-attachment class in the high byte. We parse the field and then
apply every lookup to every glyph regardless of it.

**Symptom.** The flag that matters in practice is `IgnoreMarks`. A face that
writes its Arabic ligatures as "these two letters, skipping any marks between
them" expects the shaper to step over a fatha when matching; we do not, so
the ligature silently fails to form on vowelled text. The sweep does not
currently catch this because the Arabic corpus entry has one mark
(`\u0629` is a letter, `\u064a` a letter) and the faces that would show it
are the fully-vowelled Quranic ones.

**Why it is filed rather than fixed.** It is not a local change. "Skip this
glyph" has to be honoured by *every* matcher — single, ligature component
walk, the backtrack/input/lookahead walks of types 5 and 6 — and each of
those currently indexes the glyph vector directly. Doing it properly means a
skipping iterator that all of them go through, which is the same refactor
HarfBuzz did with `hb_ot_apply_context_t::skipping_iterator_t`. Doing it
partially — honouring the flag in one matcher and not the others — produces
inconsistent output that is harder to debug than uniformly ignoring it.

**Proper fix.** A `Skipper` built once per lookup from its flag plus the
`GDEF` glyph class definitions (which we already parse for mark placement),
exposing `next(from)` / `prev(from)`; every matcher in `gsub.rs` and `gpos.rs`
moves through it instead of `i + 1`. `UseMarkFilteringSet` additionally reads
`GDEF`'s `MarkGlyphSetsDef`.

**Where.** `gui/font/src/otl.rs` — `Lookup`, which holds the flag; every
`apply_*` in `gui/font/src/gsub.rs`. (An earlier version of this entry said
"the same in `gui/font/src/gpos.rs`" — there is no such file: positioning
lives in `kern.rs` and `mark.rs`.) The module doc of `gsub.rs` names this ID
under "not implemented".

**Resolved for GSUB (2026-08-14).** `gui/font/src/skip.rs` is the skipping
iterator this asked for. `Skipper::new(data, defs, flag, filter, mask)` is
built once per lookup from the flag, the `markFilteringSet` index and the
`GDEF` class definitions; `next` / `prev` / `at_or_after` / `walk_forward` /
`walk_backward` replace every `i + 1` in `gsub.rs`, so the single, ligature,
context and chaining matchers all skip the same glyphs. `IgnoreBaseGlyphs`,
`IgnoreLigatures`, `IgnoreMarks`, `UseMarkFilteringSet` and the
mark-attachment class are all honoured; `otl.rs` now also reads
`markFilteringSet`, which sits after the subtable-offset array rather than in
the Lookup header. Twelve unit tests in `skip.rs` and six end-to-end GSUB
tests (`ignore_marks_forms_the_ligature_across_the_mark` and its neighbours)
cover it. The sweep corpus gained the vowelled Arabic string this entry said
it was missing (`\u0628\u0650\u0633\u0652\u0645\u0650`), and it
discriminates: against the pre-`Skipper` source 8 faces shape it wrongly
(949 differ / 36 reversed), against the current one none do (941 / 44).

**Resolved for kerning (2026-08-14).** `otl::feature_lookups` now hands back
the `Lookup` whole rather than a flat list of subtables, so `kern.rs` keeps
its lookups grouped and each group carries its own `lookupFlag`. `Face::kern`
gained a sibling, `kern_across(left, right, between)`, and the shaping loop in
`scaled.rs` tracks the marks standing between a pair instead of declining to
kern across them; a group is consulted only if its flag would have skipped
every glyph in `between`. Checked by five unit tests in `kern.rs` and one host
test, `a_mark_between_a_kerning_pair_costs_the_kern_only_if_the_flag_says_so`,
which measures `T` + combining acute + `o` against HarfBuzz's own answer on
five faces in *both* directions — arial/times/segoeui read across the mark
(+0 units), DejaVuSans (+348) and verdana (+220) do not, because their
`PairPos` lookups carry flag 0. All five agree with HarfBuzz to the unit. Of
the 139 host faces that kern `(T,o)`, 82 now read the pair across a mark.

**Still open — mark attachment and `RightToLeft`.** `mark.rs` still does not
consult the flag, and `RightToLeft` is still not honoured (it governs cursive
attachment, which we do not implement). The practical cost of the first is
nil today — `scaled.rs` picks a mark's base by walking back past marks, which
is what `IgnoreMarks` would have said anyway — but it will matter for GPOS 5
(mark-to-ligature), where `IgnoreLigatures` and the mark-attachment class
change which component a mark lands on.
