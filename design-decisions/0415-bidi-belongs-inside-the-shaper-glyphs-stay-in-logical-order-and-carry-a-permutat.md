## §415 — Bidi belongs inside the shaper: glyphs stay in logical order and carry a permutation beside them

**Date:** 2026-08-14
**Lane:** C
**Decided by:** Claude (autonomous)

**Context.** `gui/font/src/bidi.rs` implements UAX #9 and passes all 91,707
cases of Unicode's `BidiCharacterTest.txt`. Wiring it into `ScaledFont::shape`
raised two questions that the conformance suite says nothing about: *where*
the pass runs, and *what shape* its answer takes in `ShapedRun`.

`known-issues.md` had already proposed an answer to the first —
"the bidi pass belongs above the shaper, next to `script::runs`, and wants to
be written once for the whole toolkit rather than hidden inside the font
crate" — and implementing it showed that answer to be wrong.

**The decision, part one: the levels are resolved inside `shape`.** Five
things depend on the embedding levels, and three of them are shaping
decisions that no layout stage above the shaper could make:

* **Rule L4, mirroring.** `(` in a right-to-left run is drawn with the glyph
  for `)`. That is a *character* substitution, so it has to happen before
  `cmap` is consulted. A caller above the shaper can only mirror by editing
  the string and shaping again.
* **Run splitting.** `script::runs` now splits on level parity as well as on
  script, because a ligature or a kern must not form across a direction
  change. The run list is built inside `shape`.
* **Kern re-charging** (part three below), which needs the glyph vector.
* Reordering itself, and the caret queries — the two a layout stage *could*
  have done.

Three of five is decisive. Resolving levels above the shaper would mean
passing them back down again for the other three, which is the same coupling
with an extra interface.

**The decision, part two: `ShapedRun` stores glyphs in logical order and
carries a `visual: Vec<u32>` permutation.** The obvious alternative is to
store them already reordered. It was rejected because logical order is what
every existing query on the type is built on: `cluster` is documented
non-decreasing along the run, `offset_at` binary-searches on it, `glyphs()` is
zipped against the source text by two callers. Reordering the vector breaks
all of that silently — the code still compiles, the clusters merely stop being
sorted — and the fix would be to re-derive the logical order from the visual
one, which is the same permutation kept in the other direction.

So: `glyphs()` is logical and unchanged, `draw_order()` walks the permutation,
and a renderer uses the latter and needs to know nothing else. The permutation
is left **empty when it would be the identity**, which is the common case, so
`draw_order()` on English text iterates the glyph slice directly with no
indirection and `is_reordered()` is a length check.

**The decision, part three: kerns are re-charged onto the new left glyph.**
The shaping pass charges each pair's kern to the *logically*-first glyph,
which is the left one in left-to-right text and the **right** one after
reversal. Leaving it there puts the gap on the far side of the pair — an
`AV` kern in a Hebrew run would tighten the space to the left of the `A`
rather than between the two. `recharge_kerns` strips every kern, walks the
pairs that are adjacent *in drawing order*, and gives each its kern back on
the left-hand glyph. A pair that became adjacent only because of the reversal
gets nothing, which is correct: those two glyphs were never a kerning pair,
and HarfBuzz does not kern across a direction boundary either.

Mark offsets needed no such treatment, which is worth recording because it
looks like it should. `attach_marks` computes `offset.x = ... + (pen[base] -
pen[mark])`, so the mark lands at `pen[base] + dx` whichever of the two is
drawn first — provided the pens are accumulated in *drawing* order. Making
that loop follow `visual` was the whole change.

**The decision, part four: a fast path that never consults the class table.**
`is_trivially_ltr` rejects a string with one numeric comparison per character:
the lowest right-to-left code point in Unicode is U+0590, so anything below it
cannot be strong-RTL and cannot be an explicit format character. Latin, Greek,
Cyrillic, Han, Kana and Devanagari — that is to say, almost everything — leave
`shape` having paid one scan, with no `resolve` call, no level vector, no
permutation and no re-charging pass.

**How it was checked.** `tools/harfbuzz_sweep.py` compares against HarfBuzz
over every installed face; `examples/shape_dump.rs` prints *both* orders
because HarfBuzz has no bidi in it and answers in whichever one its
`guess_segment_properties` guessed. Across 556 faces × 19 strings the
`reordered` bucket — same glyphs, different order — went from 44 to **0**.

The sweep also learned to compare positions, in font units, which caught
nothing about bidi and three things about everything else (all now filed in
`known-issues.md`). It required one insight to be useful at all: the two
engines charge a kern to different glyphs and *both are right*. HarfBuzz
splits a legacy `kern` value, giving `k >> 1` to the left glyph's advance and
the remainder to the right glyph's advance *and* offset; we put the whole
correction on the left advance, because that is what makes a run's width the
sum of its advances. Arial Rounded Bold shaping `Th` is the worked example:
HarfBuzz says advances 1266, 1224 with the `h` offset -13, we say 1253, 1237
with no offset, and every glyph lands on the same pixel. So the sweep compares
accumulated *ink positions* and the total width, not raw advances.

**Where.** `gui/font/src/scaled.rs` — `shape`, the `byte_levels` and
`recharge_kerns` helpers, and the drawing-order pen loop in `attach_marks`;
`gui/font/src/shape.rs` — `ShapedRun::visual`, `reordered`, `draw_order`,
`is_reordered`; `gui/font/src/script.rs` — `runs`, which now takes levels;
`gui/font/src/bidi.rs` — `visual_order`, `render_levels`, `is_trivially_ltr`;
`gui/font/examples/shape_dump.rs` and `gui/font/tools/harfbuzz_sweep.py`.
