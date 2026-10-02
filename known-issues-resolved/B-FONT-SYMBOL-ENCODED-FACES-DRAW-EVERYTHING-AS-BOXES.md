### B-FONT-SYMBOL-ENCODED-FACES-DRAW-EVERYTHING-AS-BOXES. Wingdings and friends mapped no ASCII at all — 2026-08-14 — FIXED 2026-08-14

**Where:** `Face::glyph_index` in `gui/font/src/sfnt.rs`.

**Symptom.** Every string drawn in Wingdings 2, Wingdings 3, MT Extra,
Bookshelf Symbol 7 or Reference Specialty came out as a row of empty boxes.
Nine of the 556 installed faces, and every one of them 100% wrong for every
string in the corpus — not a subtle disagreement but a total failure to
render.

**Cause.** A platform-3 encoding-0 `cmap` subtable does not key on Unicode. It
keys on the byte the character had in the font's own 8-bit encoding, lifted
into the private-use area at U+F000, so Wingdings stores its `A` at U+F041.
Looking up U+0041 found nothing. `select_cmap` already recognised these tables
and ranked them below a real Unicode table — it just discarded the fact after
choosing, so the lookup had no way to know it needed the other spelling.

**Why nothing caught it.** A face is *allowed* to have no `A`. "No glyph" is a
legal answer, so no self-consistency check and no round-trip property can tell
a symbol-encoded face from a genuinely sparse one. 247 unit tests and 13 host
tests were green throughout. It was found by shaping every installed face
against HarfBuzz and diffing — the same method that found the `MAX_SUBTABLES`
truncation, and the second bug in a row that only an independent oracle could
see. See design-decisions.md §409.

**Fixed** in `d7fe50bb5`: `CmapSub` now carries the symbol flag, and
`glyph_index` retries at `0xF000 + cp` for `cp <= 0xFF` when the chosen
subtable is a symbol one. Regression test
`symbol_encoded_fonts_still_map_ascii` in `gui/font/tests/host_fonts.rs`
asserts the retry is a re-spelling rather than a guess: for every printable
ASCII code point, the two spellings must give the same answer, including
where neither is present. That last clause matters — MT Extra is a maths font
and genuinely lacks most letters, so the test cannot simply demand that `A`
exists.
