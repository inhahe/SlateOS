## §410 — Normalize to NFC before shaping, as a layer that knows nothing about the font

**Date:** 2026-08-14
**Lane:** C
**Decided by:** Claude (autonomous)

Shaping used to ask `cmap` for exactly the characters the caller typed. So
`"e"` followed by U+0301 COMBINING ACUTE missed the precomposed `é` glyph that
288 of the 556 installed faces carry, and drew a bare `e` with a mark floated
over it by GPOS where every other shaper draws one letter. The fix is a
normalization stage in front of `cmap`. The decisions worth recording are not
*whether* to normalize but how it is split and where it deliberately differs
from HarfBuzz.

### NFC, not NFD, and not NFKC

NFC because fonts are built for it: a face that has `é` at all has it as one
glyph, and its kerning and ligature rules name that glyph. NFD would decompose
into pieces the font's own tables never mention, and would then rely on mark
positioning to reassemble what the font already had. NFKC is simply the wrong
form for rendering — it replaces a superscript with a digit and `ﬁ` with `f`
`i`, which is a change to what the text *says*. The generator skips every
decomposition carrying a `<...>` tag for that reason.

### Two layers: what the text is, then what the face can show

`nfc()` is pure Unicode and never sees a font. `fit_to_face()` then takes a
character back apart when the face cannot draw it. The alternative — one pass
that composes only when the font has the composed glyph — is smaller, faster
and is what HarfBuzz does, and it was rejected.

The reason is that composability depends on context. Whether `a` and `b` join
depends on the marks between them, so a font-dependent compose gives
canonically equivalent spellings different answers. This is observable in
HarfBuzz on an ordinary Windows font: Agency FB has `ç` and `é` but no
combining acute, and HarfBuzz renders `c` + U+0327 + U+0301 as `ç` plus a
missing-glyph box while rendering the identical text spelled U+1E09 as a
single box. Canonical equivalence means those are the same string; a renderer
that draws them differently is wrong, and no amount of care in the caller can
work around it.

The cost is a second pass over the pieces. It is guarded: `fit_to_face`
returns immediately when the face has everything, which is the common case,
and `needs_work` skips `nfc` entirely for text that cannot change — all ASCII,
and nearly all Latin.

**Pro (chosen):** one answer per string, independent of spelling; the Unicode
half is testable against the UCD with no font in the picture; the font half is
testable with a closure and no font file.
**Con:** two passes rather than one, and a rule (below) about partial fits
that a font-dependent compose gets for free.

### Splitting is decided by the base, and the marks ride along

When a face cannot draw a composed character, `fit_to_face` replaces it with
its pieces only if the face can draw what the decomposition chain bottoms out
at. Marks are emitted whether or not the face has them.

Both halves were chosen from measurements against HarfBuzz over all 556
installed faces, and each half fixes a different failure:

* **Requiring the base.** A face with no `가` also has no `ᄀ` and no `ᅡ`.
  Splitting there turns one missing-glyph box into two — one more wrong thing
  on screen, and one more caret stop than the text has characters. 553 of 556
  faces disagreed with us on this before the rule existed.
* **Not requiring the marks.** A face with `ç` but no combining acute should
  still draw the `ç`. The base carries nearly all the meaning; a missing accent
  costs an accent, whereas refusing to split costs the letter too. 263 faces
  disagreed before this half.

### A mark takes its base's cluster

A combining mark is charged to the cluster of the character it attaches to,
not to its own byte offset. This is partly forced — canonical ordering moves
marks around, and clusters must not decrease along a run — and partly a
choice: a caret must not be able to land between a letter and its accent.

**Pro:** the caret moves over `é` as one thing however it is spelled, which is
what a text field wants and what a grapheme-aware editor would have had to
impose anyway.
**Con:** a mark is no longer individually addressable, so an editor that wants
to delete just the accent cannot use cluster boundaries to find it and must
consult the string.

### Deliberate divergences from HarfBuzz, and why they are not bugs

After this change the corpus sweep leaves three classes of disagreement. All
three are cases where HarfBuzz is font-dependent and we are not, and none is
tracked as a defect:

1. **Jamo runs (553 faces).** `ᄀ ᅡ ᆨ` is canonically `각`, and we compose it.
   HarfBuzz's Hangul shaper composes only when the font has the syllable, so on
   a face with no Hangul it emits three boxes where we emit one. One box for
   one character is the honest report of what NFC says the text is.
2. **Singletons (57 faces).** U+212B ANGSTROM SIGN is canonically U+00C5, and
   we fold it. HarfBuzz keeps U+212B when the face has a glyph for it. Ours is
   what the standard says; the two glyphs are the same shape in practice.
3. **Partial fits (27 faces).** The HarfBuzz self-inconsistency described
   above. We give both spellings the same answer, which is the point.

**Code:** `gui/font/src/norm.rs`, `gui/font/tools/gen_norm_tables.py`,
`gui/font/src/norm_tables.rs` (generated); wired in `ScaledFont::shape` in
`gui/font/src/scaled.rs`.
