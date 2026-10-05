## TD-FONT-IGNORES-GPOS-SINGLE-AND-CURSIVE-ADJUSTMENTS

**Status:** FIXED 2026-08-14 -- the resolution note in the entry below gives the change; on `main` since. Stamped 2026-09-28 by lane F, `gui/font`'s owner since the six-lane split, so that the heading no longer reads as unfixed. Moved to `known-issues-resolved/` on 2026-10-05 (lane F).

**What.** Of `GPOS`'s eight lookup types we apply 2 (pair, in `kern.rs`) and
4/6 (mark-to-base and mark-to-mark, in `mark.rs`). Type 1, single adjustment,
and type 3, cursive attachment, are parsed past and ignored, as are 5, 7 and
8.

**Symptom, measured.** Amiri Bold disagrees with HarfBuzz on plain unvowelled
`العربية` on 6 faces' worth of strings: our glyph 3 sits at x=794, HarfBuzz's
at x=877. The face has 30 type-1 lookups and one type-3, and the 83-unit
difference is a single adjustment applying the same value to the glyph's
placement and its advance — the standard way an Arabic face tunes one letter's
fit without touching the pair table. Cursive attachment is what makes a
joining script's letters sit on a common baseline curve rather than on the
straight one; without it a face like Amiri that relies on `curs` draws a
visibly broken join.

**Why it is filed rather than fixed.** Type 1 alone is genuinely small — a
coverage lookup and a `ValueRecord`, both already parsed by `kern.rs` — but
doing it alone would leave the sweep's Arabic differences barely changed,
because the same faces need type 3, and type 3 is a different shape of
problem: it chains, so an attachment moves every glyph after it and the
`RightToLeft` lookup flag changes which end of the chain is anchored.

**Proper fix.** A positioning pass structured like the substitution one:
`feature_lookups` collects the `GPOS` lookups a run's features reach, and each
is applied in order through the same `Skipper` that `GSUB` uses, with types 1,
2, 3, 4 and 6 dispatched from one place. That subsumes `kern.rs`'s standalone
pair walk and `mark.rs`'s standalone mark pass, both of which currently pick
their own lookups out of the table.

**Where.** `gui/font/src/kern.rs` — the pair walk and its `feature_lookups`;
`gui/font/src/mark.rs`; `gui/font/src/scaled.rs` — `shape`, where the passes
are sequenced.

**Fixed**, as the proper fix above describes. `gui/font/src/gpos.rs` is the
unified pass: `Run` in, one `Adjust` per glyph out, lookups walked in table
order through the same `Skipper` as `GSUB`, with types 1, 2, 3, 4 and 6
dispatched from `Adjust::apply`. `kern.rs` keeps only the legacy `kern` table
(the pass cannot see it); `mark.rs` keeps only its anchor/subtable readers,
which `gpos.rs` calls. `scaled.rs::shape` now cuts the string into `Segment`s
once — on tabs and script changes — and feeds the same segments to both
passes, since after ligation nothing left in the glyph run says where a
stretch began.

Two things fell out of doing it. `Face::mark_on_base`/`mark_on_mark` are gone
as public API: mark attachment is not a thing a caller can ask for out of
lookup order any more, so `tests/host_fonts.rs` sweeps it through
`ScaledFont::shape` instead. And `recharge_kerns` had to be gated on
`Face::kerns_outside_gpos()` — see §417 in `design-decisions.md`; charging a
`GPOS` pair's value to the visually-left glyph applies the font author's own
right-to-left correction a second time.

Measured on the HarfBuzz sweep (556 host faces x 19 strings): agree
9526 → 9539, misplaced 98 → 85, reordered 0 throughout. The Arabic
`العربية` disagreement the entry was filed on went from 14 faces to 1
(Scheherazade-Regular, which needs the contextual positioning tracked in
TD-GPOS-HAS-NO-CONTEXTUAL-OR-MARK-TO-LIGATURE-POSITIONING).
