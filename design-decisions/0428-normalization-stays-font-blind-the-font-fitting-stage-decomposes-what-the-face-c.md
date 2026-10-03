## §428 — Normalization stays font-blind; the font-fitting stage decomposes what the face cannot draw

**Date:** 2026-08-15
**Lane:** C
**Decided by:** Operator (answering `open-questions.md` C-Q1 — "c-q1: c.";
Claude raised the question and recommended C)
**Zone:** gui-core

**In short.** Some accented letters can be written two ways: as one character
(`ḉ`) or as a plain `c` with two accent marks stacked on it. A font may contain
the pieces but not the single combined character. Today, when that happens, we
draw an empty box — the "missing character" rectangle — where other systems draw
the letter correctly by falling back to the pieces. The question was *which part
of our code should notice*. We chose: the part that already knows what the font
contains, rather than the part that converts text into its canonical spelling.
The visible result is that those letters render correctly; the text-conversion
step keeps knowing nothing about fonts, which is what lets it be tested and
cached on its own.

**Context.** `gui/font/src/norm.rs` is layered on a principle written into its
module doc: **`nfc` answers a question about *text*** — NFC is the Unicode rule
that spells `e` + `´` as the single character `é` — **and never looks at a font;
`fit_to_face` answers a question about the *font*** and does not renormalize.
Composition is a property of the string, so it is decided before any face
(a font file as loaded for rendering) is consulted.

HarfBuzz — the reference text shaper (the library that turns characters into
positioned glyphs), which we run a differential sweep against — does the
opposite. It decomposes to NFD (the fully-separated spelling) and then
*recomposes only where the face has a glyph*, so the same string normalizes
differently in two different fonts.

The question surfaced as the entire residue of that sweep. Fixing
`TD-FONT-HAS-A-HANGUL-SHAPER-NOTHING-CALLS` took the disagreement count from 892
to 339, and the remaining 339 are **one question asked 339 times**, not a
scatter: `\u1e09` (ḉ, c with cedilla and acute) 255 cases, `\u212b` (Å, the
angstrom sign) 57, `été` 10, and a short tail. Concretely, for `\u1e09` in a
face holding `c`, the cedilla and the acute but no precomposed `ḉ`: we emit one
missing-glyph box, HarfBuzz emits three glyphs that stack into the right-looking
character.

**Options.**

- **A — keep the current layering unchanged.** *What changes:* nothing; we keep
  drawing a box where HarfBuzz draws correct text. *Pro:* each stage has one job
  and one input. *Con:* the user does not care which stage was principled.
- **B — adopt HarfBuzz's font-aware recomposition wholesale.** *What changes:*
  the sweep residue goes to near zero and partial-coverage faces render
  correctly. *Con:* normalization becomes a function of `(text, face)` — no
  longer hoistable out of a loop, no longer cacheable per string, not reasonable
  about without a font in hand; `norm.rs`'s layering claim becomes false.
- **C — a narrow fallback: `nfc` stays pure, but `fit_to_face` decomposes a
  composed character it cannot draw when the pieces *are* drawable.** *What
  changes:* the same visible outcome as B for exactly the failing case, with A's
  layering intact. *Con:* two mechanisms where HarfBuzz has one — we agree with
  it on output while diverging on structure.

**The decision: option C.** The decomposition happens in the stage that already
owns "what can this face draw", and `split_undrawable` already exists with
exactly that shape — which is why C was the recommendation rather than a
compromise between the other two. Expected result: the 339 disagreements move to
`agree` without `nfc` ever taking a face as input.

**The cost accepted, and what to actually test.** Running two mechanisms where
HarfBuzz runs one means we can match its output while diverging on how we got
there, and divergence in structure eventually shows up as divergence in output.
The concrete risk named in the question is **mark reordering after a late
decomposition** — when several accents attach to one letter, their order matters,
and HarfBuzz gets it right by construction because it decomposes before
reordering, whereas we would decompose after. Treat that as the thing to verify
rather than assume: the sweep is the instrument, and any ordering case it
surfaces is this decision's bill coming due, not a surprise.

**Why B is worth keeping written down.** If a future case cannot be fixed inside
`fit_to_face`, B is the argument that has to be beaten, and it should not be
re-litigated from scratch. It was refused for one reason: it makes normalization
depend on the font, and everything we do with normalized text — caching it,
hoisting it out of a render loop, testing it without a font — depends on it not
doing that.

**Where it lands.** `gui/font/src/norm.rs` (`fit_to_face`, `split_undrawable`,
and the module doc's layering paragraph, which now needs a sentence saying the
fallback exists and why it does not violate the principle),
`gui/font/src/scaled.rs::shape` (call order), and
`gui/font/tools/harfbuzz_sweep.py` (the 339 should move to `agree`). Reference:
HarfBuzz `src/hb-ot-shape-normalize.cc`,
`HB_OT_SHAPE_NORMALIZATION_MODE_COMPOSED_DIACRITICS_NO_SHORT_CIRCUIT`.
