## §416 — Place a mark by measurement when the face has no `GPOS` at all, reimplementing HarfBuzz bug-for-bug, and refuse the scripts whose clusters need a shaper we do not have

**Date:** 2026-08-14
**Lane:** C
**Decided by:** Claude (autonomous)

**Context.** `tools/harfbuzz_sweep.py` compares this crate against HarfBuzz
over every installed face. Its `misplaced` bucket — same glyphs, different
positions — stood at **559** of 10,564 runs, and roughly **489** of those were
one failure: a combining mark drawn at the pen. `c` + U+0327 + U+0301 on a
face with no `GPOS` put both accents in the gap *after* the letter, where they
overprinted each other and the next glyph. 222 of the host's 556 installed
faces have no `GPOS` table whatsoever; they were built when `é` was one
character with one glyph and a bare U+0301 was somebody else's problem.

Three decisions came out of fixing it.

**Part one: the trigger is "no `GPOS` table", not "no `mark` feature".**
The tempting rule is "if the font cannot tell me where this mark goes,
measure". That is wrong, and a face on this machine proves it: Candara has a
`GPOS` with kerning and no `mark` feature at all. HarfBuzz leaves its marks
exactly where they fall, and so must we. The distinction is that a `GPOS`
table is a *statement* — the designer opened the file, wrote down positioning
rules, and did not write one for this mark. Overruling that is second-guessing
a decision that was made. A missing `GPOS` is not a statement; it is the
absence of one, and there is nothing to overrule. `Face::has_positioning`
exists to carry exactly this distinction, separately from the three things
this crate actually reads out of `GPOS` (`kern`, `mark`, `mkmk`).

**Part two: reimplement HarfBuzz's arithmetic exactly, including its
rounding.** `gui/font/src/fallback.rs` is a transcription of
`hb-ot-shape-fallback.cc`: the `upem/16` gap, the truncating integer division
in the centring, the two `if` branches that pull a mark back when the gap and
the computed offset have the same sign, the growth of the base's box after
each mark so a second accent clears the first. It would have been easy to
"improve" any of these. Two reasons not to. The output is *checked* against
HarfBuzz, so an improvement reads as a regression in the sweep and hides real
divergence in the noise. And the numbers are not arbitrary taste — they are
what a decade of complaints about specific fonts settled on, and this crate
has no evidence to overrule them with. The arithmetic is done in font units
as `i32` for the same reason: doing it in floats would round differently in
the last unit and make an exact comparison impossible.

The one deliberate divergence is that we do not implement HarfBuzz's
*modified* combining classes, which permute Hebrew ccc 10–26 and Arabic 27–36
so that marks sort into display order. We do implement the recategorization
those classes feed — transposed to match on real Unicode classes, which is
sound because the permutation is injective within each block — but not the
re-sorting. Two Hebrew points in the wrong canonical order stack in the wrong
order. Filed in `known-issues.md`.

**Part three: the script gate, which is the part that was nearly got wrong.**
The first working version ran the fallback on every script, and the sweep
immediately caught a *new* failure: 33 Devanagari runs, where the virama's
advance was zeroed and the glyph centred on the consonant before it.

The available quick fix was to skip combining class 9 (virama). It would have
made the sweep green. It would also have been a coincidence: HarfBuzz's actual
rule is per-script, `hb_ot_shaper_t::fallback_position`, and it is `false` for
the Indic, Khmer, Myanmar and USE shapers and for Thai/Lao — the whole
Brahmic-and-relatives family, viramas and matras and nuktas alike, not one
combining class out of it. Matching the symptom rather than the rule would
have left the other classes wrong and put a band-aid where the reason belongs.

So `fallback::positions_marks` gates on the run's OpenType script tag, against
the script set of `hb_ot_shaper_categorize` in `hb-ot-shaper.hh` mapped
through this crate's own tag table — 101 tags, binary-searched. The reason it
is the right rule and not merely HarfBuzz's rule: in a Brahmic cluster the
marks are not a stack of accents on one base. A virama is a *spacing* glyph
that suppresses a vowel; a matra may be reordered to before the consonant it
logically follows; which glyph a mark belongs to is the output of a reordering
pass, not "the last non-mark to my left". Measuring a box and centring on it
produces a confident wrong answer. Leaving the mark at its natural advance
produces an obviously unshaped one, which is both more honest and, in
practice, closer to legible.

Getting the gate required hoisting `script::runs` out of the
`if has_substitutions()` branch of `shape`, so it is now computed once and
used by both the substitution pass and the placement pass. That is strictly
better regardless: the two passes were always answering the same question.

**One thing not copied.** HarfBuzz decides the shaper from the tag the *font*
registers its features under, so a Devanagari-glyph face whose `GSUB` says
only `DFLT` gets the default shaper and does get the fallback. We match on
the tag the *character's script* maps to, unconditionally. Following
HarfBuzz here would mean asking the font which scripts it declares before
deciding how to place a mark — a coupling not worth having for the sake of
faces that carry Devanagari glyphs, no `GPOS`, and no Devanagari `GSUB`
script record. Filed in `known-issues.md`.

**What it measured.** Over 556 faces × 19 strings the `misplaced` bucket fell
from **559 to 98**, and the two largest contributors — `c` + cedilla + acute,
and vowelled Arabic — vanished entirely. The 98 that remain are kerning-charge
and `GPOS`-path divergences that predate this work. Unit tests went 320 → 336;
`installed_fonts_without_gpos_still_place_combining_marks` checks the 22 host
faces that have no `GPOS` and do draw a combining acute.

**Where.** `gui/font/src/fallback.rs` (new); `gui/font/src/sfnt.rs` —
`Face::has_positioning`, `Face::glyph_bbox`; `gui/font/src/gsub.rs` —
`SubGlyph::klass`, which rides the attachment class through substitution
because a `cluster` is shared by a base and its marks and a glyph id is
changed by substitution; `gui/font/src/scaled.rs` — `synthesize_marks`, the
`pens` helper factored out of `attach_marks`, and the hoisted `script::runs`;
`gui/font/tests/host_fonts.rs`.
