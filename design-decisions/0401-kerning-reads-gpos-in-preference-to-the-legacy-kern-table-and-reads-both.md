## §401 — Kerning reads GPOS in preference to the legacy `kern` table, and reads both

**Date:** 2026-08-14
**Lane:** C
**Decided by:** Claude (autonomous)

### The problem

The layout path advanced the pen by one glyph's own advance width and nothing
else. That is the width the designer drew around the letter *in isolation*;
for a pair like `AV`, `To`, `Yo`, `P.` or `r.` they also specify a correction,
and without it the diagonal of the `V` sits a visible gap away from the `A`.
Text set this way does not look broken so much as amateur, and the error is
systematic — it accumulates along a run, so a long right-aligned string drifts.

The corrections live in one of two places, and which one is not a matter of the
font's age: `GPOS`'s `kern` feature (OpenType, class-based, the table a modern
designer maintains) or the legacy `kern` table (a flat pair list). Supporting
one is much less work than supporting both.

### The options

**(a) Legacy `kern` only.** A few hundred lines: a flat, sorted pair array with
a binary search over it.

**(b) `GPOS` only.** The table modern tooling emits, and the one that is
authoritative where a face has both.

**(c) Both, preferring `GPOS`.**

### The decision: (c), and the reason is measured, not assumed

A sweep of the 556 faces installed on this machine, reading only each file's
table directory, gives:

| carries | faces |
|---|---|
| neither | 126 |
| `GPOS` only | 188 |
| `kern` only | 94 |
| both | 133 |

So (a) leaves 188 faces unkerned and (b) leaves 94 — in both cases a large,
arbitrary slice of the installed fonts silently renders worse than the rest,
with no way for a user to tell why one font looks right and another does not.
Neither table is a legacy concern that can be skipped. `GPOS` wins when a face
has both, because it is the table the designer's tooling generates and the
legacy copy in such a face is a compatibility shim that may be a lossy
flattening of it.

The host sweep is kept as an `#[ignore]`d integration test
(`installed_fonts_kern_the_pairs_that_need_it`), not because the counts must
hold on another machine but because kerning is the one part of the stack whose
correctness cannot be seen in a glyph: a wrong pair value still parses, still
rasterizes, and still has ink. The test therefore also pins five known faces
(Arial, Times, Segoe UI, DejaVu Sans, Verdana) to the assertion that `AV` is
*narrower* than `A` plus `V`, which is an oracle independent of our own parser.

### What is deliberately left out

- **Script and language selection.** Every lookup reachable from any feature
  tagged `kern` is used, whatever script system it hangs under. Correct
  selection needs the itemised script of the run, which the layout path does
  not yet compute; using all of them is wrong only for a face that kerns a pair
  differently per script, which is rare, and the failure is a slightly wrong
  gap rather than a wrong glyph.
- **Contextual and cross-stream kerning**, and the legacy table's format 2.
  Vanishingly rare; the subtables are skipped rather than misread.
- **Device tables** on value records. They tune a value for one specific ppem
  and are a sub-pixel concern.

Each of these is noted in `gui/font/src/kern.rs`'s module documentation so the
next reader does not have to rediscover that the omission was a decision.

### Consequences

- `SystemFont::kern` and `ScaledFont::kern` are *public*, and kerning is
  applied inside `measure`/`draw_text` as well. The public form exists because
  the compositor draws one glyph at a time through its own clip stack and
  cannot call `draw_text`; if it could not ask about a pair it would space runs
  differently from the way the toolkit measured them, which is exactly the
  cross-process divergence §400 exists to prevent.
- `Face::kern` is infallible and returns 0 for a malformed table. A bad kerning
  table means text spaced slightly wrong, which is not worth failing a draw
  over when the alternative is a blank window.
- Parsing is eager at face load (it resolves to a short list of subtable
  offsets) so that "GPOS or the legacy table?" is not re-decided on every pair
  of glyphs drawn.

**Where it lives.** `gui/font/src/kern.rs` (all table parsing),
`gui/font/src/sfnt.rs` (`Face::kern`, `Face::has_kerning`),
`gui/font/src/scaled.rs` and `gui/font/src/system.rs` (application during
measurement and drawing), `gui/compositor/src/main.rs`
(`RenderEngine::draw_text`).
