## TD-FONT-LEGACY-KERNING-DISAGREES-ACROSS-AN-INVISIBLE-CHARACTER -- 2026-09-17

**In short:** when an invisible character sits between two letters that would
otherwise kern — a soft hyphen, a zero-width space, a joiner — we put the
following letter in a slightly different place than HarfBuzz does. Nobody can
see 13 font units, but it is the whole of the sweep's remaining disagreement,
and three plausible explanations were measured and all three made it worse.

**Date:** 2026-09-17. **Lane:** C.

**The bucket.** With `differ` at 0, `misplaced` is 168 and every entry is the
same shape:

| String | Faces | Between the pair |
|---|---|---|
| `super<SHY>cali<SHY>fragi<SHY>listic` | 57 | U+00AD soft hyphen |
| `f<ZWJ>i` / `f<ZWNJ>i` | 25 each | U+200D / U+200C |
| `a<CGJ>b` | 16 | U+034F |
| `a<VS16>b`, `a<WJ>b`, `a<ZWSP>b` | 15 each | U+FE0F, U+2060, U+200B |

**The numbers, on ARLRDBD.TTF with `a<CGJ>b`.** Ours places the second glyph
at 1190, HarfBuzz at 1203. Measured with `shape_dump`: our `a` advances 1190
both with and without the joiner, so we kern *across* it. Removing the kern
entirely gives 1217, so 1217 is the unkerned advance and our pair kern is -27.
HarfBuzz's 1203 is neither — it is 1217 - 14. **HarfBuzz is applying some
-14 adjustment that is not the `a`/`b` pair kern**, and finding where it comes
from is the open part.

**Three mechanisms measured, all worse than doing nothing:**

| Change | agree | misplaced |
|---|---|---|
| *baseline* | 51427 | 168 |
| record the ignorable in `between`, suppressing the pair | 51421 | 174 |
| treat it as an ordinary glyph (new left half, clears `between`) | 51421 | 174 |
| the above, and let it be a pair's right half (`!erased` dropped) | 51416 | 179 |

The second and third give identical numbers because the `!erased` term in the
kern gate stops the pair being computed at the ignorable either way, so both
land on the unkerned 1217.

**What is probably going on.** HarfBuzz replaces default ignorables with the
*space* glyph before positioning (`hb_ot_shape_hide_default_ignorables`), and
both shapers emit gid 3 here, so the glyph sequences agree. The remaining -14
looks like a kern HarfBuzz reads for a pair involving that space which we read
as 0. That is a guess; it was not confirmed, and the three experiments above
are what ruled out the simpler readings.

**Why it is not urgent.** 13 font units at a typical size is a fraction of a
pixel, and it only arises with an invisible character between two kerning
letters. The reason to fix it is that it is the last thing between this crate
and exact agreement with HarfBuzz on every one of 556 host faces, and the
sweep is worth more when its remaining output is zero.

**Where.** `gui/font/src/scaled.rs`, the `erased` branch of the kerning loop
(and its comment, which argues the current behaviour from GPOS matcher rules
— true of GPOS lookups, but this path is the legacy `kern` table, which has
no flags). `gui/font/src/kern.rs` `legacy_pair` correctly answers 0 when
anything stands between.

---

**RESOLVED 2026-09-17, and the diagnosis above was wrong in an instructive
way.** Moves to `known-issues-resolved.md` once the fix reaches `main`.

**The invisible characters were a red herring.** Asked directly through
`uharfbuzz`, HarfBuzz reports the same advance for `ab` and for `a<CGJ>b` on
Arial Rounded Bold DASH 1203 either way DASH so it kerns straight across the
joiner exactly as we do. The bucket only *looked* like an ignorable problem
because the corpus contains no bare kerning pair, so a plain `a`/`b`
disagreement could surface only on strings that happened to carry one.

**The real cause is how a legacy kern is charged.** Natural advances agree
(`a` 1217, `b` 1280). For `ab` HarfBuzz reports 1203 and 1267 DASH it reduces
*both* glyphs DASH where we reported 1190 and 1280, the whole -27 on the left.
The totals match, which is why nothing looked wrong: the ink lands in the same
place. What differs is every question asked *between* the two glyphs, which is
a caret position, a hit test, and where a run may be cut for wrapping.

HarfBuzz's `hb_kern_machine_t` splits the pair kern: `kern >> 1` onto the left
glyph's advance, the remainder onto the right glyph's advance *and* its
offset. `-27 >> 1` is `-14` because the shift floors toward negative infinity,
leaving `-13` DASH which is precisely the 13 and 14 unit gaps this entry
recorded without recognising them. The split is done in font units; rounding
twice at a scaled size does not reproduce it.

**misplaced 168 -> 1**, agree 51427 -> 51594, with `differ` still 0 and osfont's
885 tests unchanged. The survivor is `SegUIVar.ttf`, a variable font, where
HarfBuzz gives the joiner `x_offset = -1042`, cancelling the preceding advance
so the glyph sits on its base: it treats U+034F as the mark its general
category says it is. Invisible either way, and not chased.

**What the three failed experiments were worth.** They ruled out the reading
the code's own comment invited, which is what forced the question to be asked
of HarfBuzz directly rather than of our source. The lesson is narrower than
"measure": *when a disagreement is about positions and the totals match, the
disagreement is about apportionment, not about what was applied.*
