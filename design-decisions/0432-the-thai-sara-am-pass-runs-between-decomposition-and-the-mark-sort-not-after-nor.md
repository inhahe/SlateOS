## §432 — The Thai SARA AM pass runs between decomposition and the mark sort, not after normalization

**Date:** 2026-08-15
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Thai has one letter, SARA AM, that is drawn as two separate
marks in two different places — a small circle above the consonant and a
stroke after it. Unicode never says so, so we were asking fonts for a single
glyph almost none of them have. Splitting it is straightforward; *when* to
split it is the decision, because the shaper also sorts marks into a standard
order, and doing these two things in the wrong order draws the circle on the
wrong side of a tone mark.

### The two candidate placements

The crate already has a precedent for a script-specific rewrite of the
character sequence: the Korean pass, which runs *after* normalization is
completely finished. Copying that shape would have been the tidy choice.

It is wrong here, and the proof is a trace rather than an argument. Take
`<0E14, 0E4B, 0E38, 0E33>` — a consonant, a tone mark, a below-base vowel, and
SARA AM:

| order | result |
|---|---|
| split first, then sort (HarfBuzz) | `<0E14, 0E38, 0E4B, 0E4D, 0E32>` |
| sort first, then split | `<0E14, 0E38, 0E4D, 0E4B, 0E32>` |

Same five characters, and the circle (`0E4D`) lands on the opposite side of
the tone mark (`0E4B`). Only the first matches HarfBuzz, and the sweep sees
the difference as a different glyph order.

So the pass is a parameter of the normalizer (`SaraAm::Decompose` /
`LeaveAlone`), invoked between decomposition and the sort. It is safe there
because no character decomposes to or from a Thai or Lao one, so the pass
cannot see a half-decomposed sequence and cannot create work for the
decomposer. `norm::nfc` passes `LeaveAlone`, so plain NFC stays exactly NFC —
the pass is shaping, not normalization, and only the shaping entry point asks
for it.

### Why it cannot be a combining-class table instead

The obvious-looking simplification — express the reordering as combining
classes and let the existing sort do it — does not work, and the reason is
worth recording so nobody tries it later. The circle this pass produces has to
move back over above-base marks. A circle the *user typed* (U+0E4D directly)
must not move at all. They are the same character; only their provenance
differs, and a combining class is a property of a character. HarfBuzz has the
same constraint and solves it the same way, by doing the move at the moment
of splitting, while the provenance is still known.

*Cost of the choice:* `norm::normalize` gained a third parameter and a
script-specific call, which is a small dent in its generality. *Measured
benefit:* host sweep agreement 18806 → 21015 and differences 3382 → 1176,
with every Thai and Lao string agreeing on all 556 faces.

**Where:** `gui/font/src/thai.rs` (`preprocess`), `gui/font/src/norm.rs`
(`normalize`, `SaraAm`, `pieces`), `gui/font/src/hangul.rs` (the contrasting
precedent), `known-issues.md` →
`TD-FONT-HAS-NO-UNIVERSAL-SHAPING-ENGINE`.
