## TD-FONT-DOES-NOT-REORDER-RIGHT-TO-LEFT-TEXT

**Status:** FIXED 2026-08-14 -- the resolution note in the entry below gives the change; on `main` since. Stamped 2026-09-28 by lane F, `gui/font`'s owner since the six-lane split, so that the heading no longer reads as unfixed. Moved to `known-issues-resolved/` on 2026-10-05 (lane F).

**What.** `ScaledFont::shape` returns glyphs in logical order for every
script. For Arabic and Hebrew the caller therefore gets the glyphs in the
order the characters were typed, and drawing them left to right puts the
first letter of the word on the left — the word runs backwards. HarfBuzz
reverses an RTL buffer before returning it, precisely so that a caller who
advances the pen left-to-right draws the word correctly.

**Symptom, measured.** In the HarfBuzz sweep the 44 Arabic-capable faces are
counted under `reversed` rather than `agree`: same glyphs, opposite order.
The sweep classifies that case separately (`got == expected[::-1]`) so the
distinction between "we shaped it wrong" and "we did not reorder it" stays
visible; after the joining work the `differ` count for `العربية` is zero.

**Why it is filed rather than fixed.** Reversing the run is a two-line change
and would be the wrong fix on its own. The real requirement is UAX #9 bidi:
a paragraph of mixed Arabic and Latin has to be resolved into directional
runs by the algorithm — embedding levels, neutral resolution, the whole
thing — before anyone knows which spans to reverse. Reversing every run whose
script is RTL gets simple cases right and mixed text subtly wrong, which is
worse than the current state because the failure stops being obvious. The
bidi pass belongs above the shaper, next to `script::runs`, and wants to be
written once for the whole toolkit rather than hidden inside the font crate.

**Proper fix.** A UAX #9 implementation producing embedding levels per
character; `script::runs` splits on level as well as script; the shaper keeps
returning logical order and the layout stage reverses the runs whose level is
odd. Mirroring (`U+0028` ↔ `U+0029` and friends) belongs in the same pass.

**Where.** `gui/font/src/scaled.rs` — `shape`, which builds the glyph vector
in logical order; `gui/font/src/script.rs` — `runs`, where levels would join
scripts as a run boundary; `gui/font/tools/harfbuzz_sweep.py` — the
`reversed_only` counter, which will drop to zero when this is done.

**Resolved (2026-08-14).** `gui/font/src/bidi.rs` is the UAX #9 pass this
asked for — all 91,707 cases of Unicode's `BidiCharacterTest.txt` pass — and
`shape` now runs it. The shape of the fix is the one proposed above with one
correction: the levels are resolved *inside* `shape` rather than above it,
because three of the five things that depend on them are shaping decisions
that a layout stage above the shaper cannot make. Rule L4 mirroring has to
happen before `cmap`, or the wrong glyph is looked up; `script::runs` has to
split on level parity, or a ligature forms across a direction change; and
kerning has to be re-charged after the reversal, because a kern is charged to
the pair's *logically*-first glyph and reversal makes that the right-hand one.
`ShapedRun` keeps its glyphs in logical order — every existing query, every
cluster invariant, unchanged — and carries a `visual: Vec<u32>` permutation
beside them, which `draw_order()` walks and which is left empty when it would
be the identity, so left-to-right text pays one `is_trivially_ltr` scan and
nothing else. See `design-decisions.md` §415.

The sweep's `reordered` count is **0** across all 556 host faces × 19 strings:
every right-to-left string now matches HarfBuzz's glyph order exactly. Two
things this entry mentioned were *not* resolved with it and were filed
separately, and both are now closed: `shape` could not be told a base direction
other than `Base::Auto` (`TD-FONT-CANNOT-BE-TOLD-A-PARAGRAPH-DIRECTION`, fixed
2026-08-16), and the caret queries measured into the text rather than across
the line (`TD-FONT-CARETS-ARE-NOT-BIDIRECTIONAL`, fixed 2026-08-16).
