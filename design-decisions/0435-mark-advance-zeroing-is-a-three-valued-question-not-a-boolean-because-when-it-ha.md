## §435 — Mark-advance zeroing is a three-valued question, not a boolean, because *when* it happens changes the width

**Date:** 2026-08-15
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** A combining mark — an accent, a vowel sign, a dot under a
letter — is drawn on top of the letter before it, so it must not push the next
letter along; its "advance" (the distance the pen moves after drawing it) is
set to zero. We used to treat that as a yes/no property of the script. It is
not: for Myanmar the zeroing has to happen *before* the font's positioning
rules run rather than after, and doing it in the wrong order slides every glyph
after a medial-RA hook 440 units to the left. So the answer is now one of
three — never, before, or after — and Myanmar is the one script that says
"before".

**The alternatives.**

| | *What changes* |
|---|---|
| **Boolean, zero after `GPOS`** (what we had) | Myanmar text in `mmrtext.ttf` draws with everything after a medial RA shifted a third of an em to the left. |
| **Boolean, zero before `GPOS`** | Every other script loses the `GPOS` adjustments that were meant to be discarded — the mark keeps whatever a lookup charged it. |
| **Tri-state `Zeroing { Never, BeforeGpos, AfterGpos }`** (chosen) | Each script gets the order HarfBuzz gives it, and the two call sites (`zeroes_marks_first`, `zeroes_marks`) each ask the half of the question they need. |

**Why it is not a refinement.** The distinction is invisible in a face whose
marks are all zero-width in `hmtx`, which is most of them, and decisive in one
whose are not. `mmrtext.ttf` classes U+103C — the hook drawn under and around
its consonant — as a `GDEF` mark *and* gives it a 440-unit advance, then charges
that 440 back on with a `dist` feature. Zero afterwards and the `dist`
adjustment is thrown away with it; zero first and the lookup's own number
survives, which is what HarfBuzz prints. That is the whole of
`HB_OT_SHAPE_ZERO_WIDTH_MARKS_BY_GDEF_EARLY`, which of HarfBuzz's nine shapers
only Myanmar and USE set.

**Cost accepted.** Two predicates on `ScaledFont` where there was one, and a
tri-state whose third arm currently has exactly one member. That is the right
shape anyway: USE is the other `BY_GDEF_EARLY` shaper, so its tags join `mym2`
in the `BeforeGpos` arm the moment it is written, and a boolean would have had
to be widened then regardless.

**If it is never revisited:** nothing degrades. The arms are transcribed from
HarfBuzz's shaper table, not guessed, and the sweep pins them.

**Where:** `gui/font/src/fallback.rs` (`Zeroing`, `zeroes_mark_advances`,
`NO_ZERO_WIDTH_MARKS`), `gui/font/src/scaled.rs` (`zeroes_marks_first`,
`zeroes_marks`), `gui/font/src/gpos.rs` (`Run::zero_marks_first`),
`known-issues.md` (`TD-FONT-HAS-NO-UNIVERSAL-SHAPING-ENGINE`).
