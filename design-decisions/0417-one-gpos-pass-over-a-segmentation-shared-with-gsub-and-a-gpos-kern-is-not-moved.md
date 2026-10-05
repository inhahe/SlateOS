## §417 — One `GPOS` pass over a segmentation shared with `GSUB`; and a `GPOS` kern is not moved when the run reverses

**Date:** 2026-08-14
**Lane:** C
**Decided by:** Claude (autonomous)

**Context.** `GPOS` has eight lookup types. This crate read three of them, and
read them in two places that each went to the table on their own:
`gui/font/src/kern.rs` picked out the type-2 pair lookups and walked them, and
`gui/font/src/mark.rs` picked out the type-4 and type-6 mark lookups and
walked those. Types 1 (single adjustment) and 3 (cursive attachment) had no
walker, so they were skipped entirely, which is what
`TD-FONT-IGNORES-GPOS-SINGLE-AND-CURSIVE-ADJUSTMENTS` was filed on.

The obvious increment — add a third standalone walker for types 1 and 3 —
is wrong for a reason that is worth writing down, because it is the same
reason §408 gave for `GSUB`. **A `GPOS` table is an ordered list of lookups,
and the order is semantic.** A face routinely tunes one letter's fit with a
type-1 adjustment and then kerns the tuned letter with a type-2 pair; run the
pair lookup first and the pair sees a glyph the font never intended it to see.
Three walkers each sweeping the whole run cannot express that ordering no
matter what order the walkers themselves are called in.

**Decision, part one: one pass.** `gui/font/src/gpos.rs` takes a `Run` — the
glyphs, their nominal advances, which of them are marks, the direction, the
script — and returns one `Adjust` per glyph. Inside, the lookups a run's
features reach are collected once and applied in table order, each through the
same `Skipper` that `GSUB` uses, with types 1, 2, 3, 4 and 6 dispatched from
one place. `kern.rs` keeps only the legacy `kern` table, which the pass
genuinely cannot see; `mark.rs` keeps only its anchor and subtable readers,
which the pass calls.

*Against:* it is a bigger change than adding a walker, and it deletes public
API (`Face::mark_on_base` / `mark_on_mark`) that a test was using to sweep
mark placement across every installed face. *For:* the alternative is
knowingly-wrong output that gets harder to correct with every walker added,
and the deleted API was itself the bug in miniature — it let a caller ask for
mark attachment *out of lookup order*, which is not a question with a correct
answer. The test now sweeps through `ScaledFont::shape`, which is what a real
caller does anyway.

**Decision, part two: the two passes share one segmentation.** `shape` cuts
the string into `Segment`s — on tabs and on script changes — once, before
substitution, and hands the same segments to positioning. The alternative,
re-deriving the cut after substitution, is not merely wasteful: it is
impossible. Once a stretch has ligated there is nothing left in the glyph run
that says where it began, because a ligature is one glyph for several
characters. The cost is that `substitute_runs` now runs even for a face with
no `GSUB`, purely to produce segments. That is a walk over the run with no
lookups to apply, which is cheap, and the alternative is a second
segmentation that must be kept in agreement with the first by hand.

**Decision, part three: `recharge_kerns` is for the legacy `kern` table
only.** This is the subtle one, and it was a live bug for the length of one
sweep. When bidi rule L2 reverses a run, `recharge_kerns` strips every kern
and re-charges it to whichever glyph is now on the *left* of each visually
adjacent pair — because an advance pushes the pen rightwards regardless of
which way the text reads, and a legacy `kern` value is a pure gap with no
direction in it.

A `GPOS` pair is not a pure gap. A font expressing a right-to-left adjustment
writes **XPlacement and XAdvance into the same value record**: the placement
moves the ink to open the gap, the advance keeps the rest of the line where it
was. That idiom is self-correcting under reversal — it has to be, since the
font author knows the run will be reversed — which means an engine that also
moves the kern applies the correction twice. Measured on Amiri-Bold shaping
`العربية`: HarfBuzz charges the pair's +83 to gid 1745 (343 → 426) alongside
its x_offset of 83; we were charging it to gid 3113 (247 → 330) *and* keeping
the x_offset, putting glyph 3 at x=960 where HarfBuzz has 877.

So `shape` gates the call on `Face::kerns_outside_gpos()`. A `GPOS` value
belongs exactly where its record put it; a legacy value belongs to whichever
pair ends up visually adjacent. *Against:* two kerning paths that behave
differently under reordering is a thing to remember. *For:* they are two
different things — one is a font author's directional statement and the other
is a direction-blind number from a table that predates `GPOS` — and treating
them alike is what was wrong.

**Measured.** `tools/harfbuzz_sweep.py` over 556 host faces x 19 strings:
agree 9526 → 9539, misplaced 98 → 85, reordered 0 before and after. The
Arabic string the known-issues entry was filed on went from 14 disagreeing
faces to 1.

**Where.** `gui/font/src/gpos.rs` (new — `Run`, `Adjust`, the pass);
`gui/font/src/scaled.rs` — `Segment`, `substitute_runs`, `position_segments`,
the `recharge_kerns` gate, and the deleted `attach_marks`;
`gui/font/src/kern.rs` — `Kerning::is_legacy`; `gui/font/src/sfnt.rs` —
`Face::position`, `has_gpos_lookups`, `kerns_outside_gpos`;
`gui/font/src/mark.rs`; `gui/font/tests/host_fonts.rs`.
