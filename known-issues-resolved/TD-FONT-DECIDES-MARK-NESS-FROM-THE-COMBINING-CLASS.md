## TD-FONT-DECIDES-MARK-NESS-FROM-THE-COMBINING-CLASS

**Status:** FIXED 2026-08-14 (lane C) -- see **Fixed** below. Moved to `known-issues-resolved/` on 2026-10-05 (lane F).

**What.** On a face with no `GPOS`, `scaled.rs` settles whether a glyph is a
combining mark with `synthesize && glyph.klass != 0`, and `klass` is
`fallback::attach_class(ch)` — a *combining class*, and only stored at all
when the run's script passed `fallback::positions_marks`. Two separate
questions are being answered by one field, and both answers are wrong in a
case the sweep now shows.

HarfBuzz keeps them apart. `_hb_glyph_info_is_mark` comes from `GDEF`, or
from the character's **general category** when there is no `GDEF`; it drives
`zero_mark_widths_by_gdef`, which zeroes the advance and shifts the mark back
over its base. `fallback_position` is a *separate* flag that only decides
whether `_hb_ot_shape_fallback_mark_position` gets to compute an offset. A
complex-script shaper turns the second off and leaves the first on.

**Symptom, one.** A complex script on a face that does not cover it keeps a
full advance per mark, so the text measures far too wide. `AGENCYB.TTF` (no
`GPOS`, no Thai) on `U+0E17 U+0E35 U+0E48 U+0E19 U+0E35 U+0E48`:

    ours       adv 1024 x6
    harfbuzz   adv 1024, (x -1024, adv 0), (x -1024, adv 0), adv 1024, ...

Six notdef boxes in a row where HarfBuzz draws three. `positions_marks`
returns false for `thai`, so `klass` is left at `0`, so the marks are not
marks. 215 of the sweep's `misplaced` cases are this one string.

**Symptom, two.** Even inside a script the fallback *does* place, a mark whose
combining class is `0` is not treated as a mark. `U+0E35` THAI SARA II is
general category `Mn` with `ccc = 0`; HarfBuzz zeroes it, we do not. The
Thai/Lao patch at the top of `attach_class` invents a position class for
exactly these characters, which is the same information arriving too late and
by the wrong route.

**Proper fix.** Give `SubGlyph` a `mark: bool` beside `klass`, set from the
character's general category (`Mn | Me | Mc`), carried through substitution
the way `klass` already is — untouched by every lookup, a ligature keeping
its first component's. Then `marks[i]` reads that field and `klass` goes back
to meaning only "where the fallback should put it", zeroed by `placeable` as
now. This needs a general-category table in `norm.rs`; there is a combining
class table there already to model it on, and the M* ranges are the only part
of the category data anything here wants.

**Where.** `gui/font/src/scaled.rs` — the `marks` vector (~line 556) and the
`klass:` initialiser in the piece loop (~line 521); `gui/font/src/gsub.rs` —
`SubGlyph`; `gui/font/src/fallback.rs` — `attach_class`, whose Thai/Lao
special case the general category subsumes; `gui/font/src/norm.rs` — where
the table would go.

**How it was found.** `gui/font/tools/harfbuzz_sweep.py`, after the corpus
gained Thai and pointed Hebrew for the `GPOS` 7/8 work. Run it and look for
the Thai string in the `same glyphs in different places` list.

**Fixed** (2026-08-14). `SubGlyph` gained `mark: bool` beside `klass`, set in
the piece loop from the character's general category and carried through
substitution untouched, and the `marks` vector now reads it. `klass` went back
to meaning only "where the fallback should put it". Sweep: agree 10731 →
10917, misplaced 1027 → 841, the Thai string 215 → 29.

Two things the entry above got wrong, both caught by measuring HarfBuzz rather
than by reading it.

*The category is `Mn`, not `M*`.* Taking all three of `Mn | Mc | Me` made the
sweep **worse** — agree 10731 → 10496, misplaced 1027 → 1262, Devanagari alone
13 → 230 — because `Mc`, the *spacing* combining marks, genuinely occupy width;
zeroing a matra piles the vowel onto its consonant. `hb_synthesize_glyph_classes`
takes `Mn` only. Probed directly against `AGENCYB.TTF` (no `GSUB`, `GPOS` or
`GDEF` at all, so nothing but the synthesized classes can be answering): it
zeroes U+A9B4 JAVANESE VOWEL SIGN TARUNG's `Mn` neighbours and not it.

*Zeroing is gated per script too, by a different list.* Ten scripts — the nine
Indic `*2` tags and `khmr` — do not zero mark advances at all, because their
shapers set `zero_width_marks = NONE`; Thai, Myanmar and USE decline the
*placement* but still zero. So there are three flags, not two, and
`fallback::zeroes_mark_advances` is the new one. Without it Devanagari
regressed on its own.

**And the offset shift.** Zeroing an advance stops the pen but not the drawing:
the mark is still drawn where the pen arrived, which is the far side of the
letter. HarfBuzz's `adjust_mark_offsets` subtracts the advance from the offset,
gated on `!has_gpos_mark && HB_DIRECTION_IS_FORWARD`. `scaled.rs` now does the
same for a zeroed mark the fallback will not place — `klass == 0`, which is the
same claim — which is what took Thai from 233 to 29 after the mark-ness change
alone had left it above its own baseline.

Tests: `norm::tests::a_mark_with_no_combining_class_is_still_a_mark`,
`a_spacing_combining_mark_is_not_a_mark_here`,
`ordinary_letters_and_the_table_edges_are_not_marks`,
`fallback::tests::declining_to_place_a_mark_is_not_declining_to_zero_it`,
`the_scripts_that_keep_their_mark_advances_are_sorted`.

`fallback::attach_class`'s Thai/Lao special case was **kept**, not deleted as
proposed — but only because deleting it is a different entry's job. It turns
out to be unreachable rather than wrong: `attach_class` is called only for a
run whose script passed `positions_marks`, and `thai` and `lao ` are both in
`COMPLEX_SCRIPTS`. See TD-FONT-FALLBACK-CLASSES-SCRIPTS-IT-NEVER-PLACES.
