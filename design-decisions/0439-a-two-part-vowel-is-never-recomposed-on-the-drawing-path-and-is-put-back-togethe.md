## §439 — A two-part vowel is never recomposed on the drawing path, and is put back together only when the face cannot draw the halves

**Date:** 2026-08-16
**Lane:** C
**Decided by:** Claude (autonomous)
**Zone:** gui-core

**In short.** In a dozen or so Asian scripts — Sinhala, Bengali, Tamil,
Balinese and others — a single vowel character is *drawn as two separate marks*,
one on the left of its consonant and one on the right. Unicode's normal
"tidy up the spelling" rule (NFC) wants to keep such a vowel as one character.
But the shaper — the code that decides where each mark goes — has to move the
left half in front of the consonant on its own, and it cannot move half a
character. So on the drawing path we now leave those vowels split. The catch:
if the font has a glyph for the whole vowel but not for its halves, splitting it
makes the text render as two empty boxes instead of one correct mark. So a
second, font-aware pass puts the halves back together whenever the face cannot
draw both of them. Visible result: pre-base vowels land on the correct side of
their consonant in those scripts, and nothing regresses on the 555 installed
faces that have no Sinhala in them at all.

**Context — what forced it.** The Universal Shaping Engine sweep
(`gui/font/tools/use-corpus.txt`, 58 strings × 556 installed faces) disagreed
with HarfBuzz on the Sinhala line `\u0D9A\u0DDC` (KA + KOMBUVA HAA AELA-PILLA).
That vowel canonically decomposes to `\u0DD9` + `\u0DCF` — a pre-base half and a
post-base half. `norm::pieces` was calling the same composition step `nfc` uses,
so the two halves were glued back into one character before the shaper ever saw
them, and the pre-base half could not be moved. HarfBuzz has exactly this case
covered by name: `compose_use` and `compose_indic` in
`hb-ot-shaper-use.cc` / `hb-ot-shaper-indic.cc` both begin

```c
/* Avoid recomposing split matras. */
if (HB_UNICODE_GENERAL_CATEGORY_IS_MARK (buffer->unicode->general_category (a)))
  return false;
```

— refuse to compose whenever the *first* half is a combining mark.

**Decision 1 — one switch, not a per-shaper hook.** HarfBuzz hangs that rule off
each shaper's vtable, so Indic and USE each carry their own copy of it. We take
it as a single mode on the normalizer (`SplitVowels::{Rejoin, LeaveApart}`) which
`pieces` always sets to `LeaveApart` and `nfc` always sets to `Rejoin`.

*Why that is safe rather than merely convenient:* the set of characters the rule
can possibly affect was enumerated exhaustively from the generated tables rather
than assumed. Of every canonical two-part decomposition in Unicode 16 whose
first half is `Mn|Mc|Me`, there are **59**; **12** are composition exclusions
that never recompose anyway (U+0344 and the Tibetan family); the remaining
**47 all belong to scripts shaped by the Indic or USE engines** — Bengali,
Oriya, Tamil, Telugu, Kannada, Malayalam, Sinhala, Balinese, Chakma, Grantha,
Tulu-Tigalari, Tirhuta, Siddham, Dives Akuru and Gurung Khema. There is no
character for which a per-shaper hook and a global switch would differ. That is
pinned by `norm::only_indic_and_use_compose_from_a_mark`, which walks the table
and asserts both the predicate *and* the count 47, so a table regeneration that
adds a script outside the two engines fails the build rather than changing
behaviour quietly.

*Alternative considered:* mirror HarfBuzz and give each shaper a `compose`
function pointer. Rejected: it is two copies of one rule, it puts a font-blind
decision behind a font-dependent dispatch, and the measurement above says the
generality buys nothing.

**Decision 2 — the halves go back together when the face cannot draw them,
and that pass runs *after* `fit_to_face`, not before.** Splitting unconditionally
regressed the sweep hard: `differ 555` on the very first run, one per face,
`ours [0,0,0] vs harfbuzz [0,0]`. The reason is that **HarfBuzz's decomposition
is font-aware and ours was not**. Reading `hb-ot-shape-normalize.cc`:
`decompose()` refuses outright when the second half has no glyph, and falls back
to emitting the whole character when the first half has no glyph and does not
decompose further. So on a face with no Sinhala coverage at all HarfBuzz emits
*one* notdef box, and we were emitting two.

`rejoin_split_vowels` is the font half of the rule: walk adjacent pieces and
rejoin a pair when the first is a mark, the two share a cluster offset, and the
face cannot draw **both** halves.

- *Sharing a cluster offset is the provenance test.* Two halves that carry the
  same offset came from one character this crate took apart; two that carry
  different offsets were two characters the author typed. Only the former may be
  put back — undoing our own split is not the same operation as composing the
  user's text, and conflating them would silently rewrite `\u0D9A\u0DD9\u0DCF`
  (three characters, deliberately) into two.
- *"Cannot draw both" rather than "cannot draw either"* is what makes the
  bottom-up rejoin exactly equal to HarfBuzz's top-down recursion. HarfBuzz asks
  the question on the way *down* and keeps the composed form when the recursion
  fails; we ask it on the way *up* and restore the composed form when it fails.
  The three-piece case is where the two visibly coincide: U+0DDD decomposes to
  U+0DDC + U+0DCA and U+0DDC to U+0DD9 + U+0DCF, and on a face missing only
  U+0DCF both directions land on the same two-glyph answer `U+0DDC, U+0DCA`.
  That case is a test.
- *Ordering.* The pass runs after `fit_to_face` because the two would otherwise
  fight: this one puts a vowel back together when the face cannot draw the
  halves, and `fit_to_face` — seeing a character with no glyph and a drawable
  base — would immediately take it apart again. Running it second means
  `fit_to_face` never sees the rejoined character and has nothing to say about it.

*Alternative considered:* make the split itself font-aware, i.e. only split when
the face can draw both halves, and drop the second pass. Rejected because the
split has to happen before Khmer matra splitting and `fit_to_face`, at a point
where each character is looked at in isolation; the "both halves drawable" test
is a property of a *pair* that only exists once the pieces are laid out. Doing
it as a second pass is also the version that can be read against
`hb-ot-shape-normalize.cc` line by line.

**Cost.** `rejoin_split_vowels` returns immediately unless the run contains at
least one combining mark, which is nearly every string in every Latin document,
so it is off the hot path by a single scan.

**Measured.** USE corpus, 58 strings × 556 faces: `agree 32203`, `reordered 0`,
`misplaced 40`, `differ 0`, `mixed 5` — the 40 being the pre-existing
ignorable-caret divergence tracked under
`TD-FONT-DOES-NOT-HIDE-DEFAULT-IGNORABLES`, and the 5 being the itemizer rather
than the shaper (two corpus lines are mixed-script and marked `!`). The default
89-string corpus is byte-identical before and after the change
(`agree 48087`, `differ 1178`), as are the Khmer and Myanmar probe sweeps.

**How to reverse.** `SplitVowels::LeaveApart` in `norm::pieces` back to
`Rejoin`, and delete `rejoin_split_vowels`. Nothing else reads either.

**Where it lives.** `gui/font/src/norm.rs` — `SplitVowels`, `compose_pair`,
`normalize`, `rejoin_split_vowels`; `gui/font/tools/use-corpus.txt` — the two
Sinhala lines that measure it.
