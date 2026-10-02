## §418 — Ligature component numbers are carried on the glyph run, not inferred at positioning time

**Date:** 2026-08-14
**Lane:** C
**Decided by:** Claude (autonomous)

**Context.** `GPOS` type 5, mark-to-ligature attachment, is the lookup that
places a mark over one *component* of a ligature. A `LigatureAttach` table
offers one row of anchors per component, and applying it requires knowing which
row — which half of an `ﻻ` the fatha belongs over. Type 4's reader plus a row
index would have been an afternoon's work; the problem is that nothing in our
run answered "which row".

By the time `GPOS` runs, the ligature has already formed. The run holds one
glyph where three stood, and the mark's cluster is the byte offset of the
*first* character behind the ligature — shared with the base and with every
other mark on it, so it cannot tell them apart. The information is destroyed
during substitution and there is no way to recover it afterwards.

**Options.**

1. **Always use the last component.** HarfBuzz's own fallback when it cannot
   identify the component. One line, no bookkeeping, no new state on the glyph.
2. **Re-derive the component from clusters at positioning time.** Count how
   many characters the ligature's cluster spans, count marks, divide.
3. **Record it during substitution**, as HarfBuzz does: a `lig_props` byte per
   glyph written by ligature substitution and read back by `GPOS`.

**Chosen: 3**, ported from HarfBuzz's `ligate_input` / `match_input` /
`Sequence::apply` rather than reinvented.

**Reasoning.**

*For 1.* It is what the format degrades to anyway for uncovered cases, and for
Latin — where a ligature is `ffi` and nobody puts a mark on it — the difference
is unobservable. It would have shipped in twenty minutes.

*Against 1.* It is wrong exactly where the feature exists to be right. Arabic
`لَا` is lam + fatha + alef: the two letters ligate across the fatha, and the
fatha belongs over the *lam* — component 1 of 2. "Last component" puts it over
the alef, every time, on every vowelled Arabic word with a lam-alef in it. That
is not a fractional-pixel error; it is a diacritic on the wrong letter, which
in Arabic changes what the word says. Shipping the fallback as the *only* path
would have meant writing code whose entire purpose is to be correct here and
having it be wrong here.

*Against 2.* It cannot work. A cluster is a byte offset, and a ligature keeps
its first component's — so a two-component and a three-component ligature over
the same text are indistinguishable by cluster. Worse, `ccmp` decompositions
and nested ligatures make the character-to-component mapping non-monotonic, and
a mark can end up *after* the whole match while belonging to a component inside
it. Any inference would be a guess that fails on precisely the fonts that need
type 5.

*For 3, and the real cost.* It is not one field. Getting HarfBuzz's answers
requires its three special cases — a base joining with only marks stays a base
so further marks can still attach to it; a ligature of only marks keeps its old
id so it still belongs to the ligature underneath; the pieces of a
decomposition after the first are worth zero components so a later ligature
counts them as one thing — plus `match_input`'s legality rules that stop a mark
being pulled out of one ligature and joined to a stranger, plus a trailing
re-adjust pass because a component may itself be a ligature whose marks stand
after the match. Each of those exists because a real font broke without it, and
each is cited in the code with the HarfBuzz issue behind it. That is ~120 lines
of transcription where option 1 was one line.

We took it because the alternative was a known-wrong result on a whole script,
and because the standing rule here is that a fix that needs a refactor gets the
refactor. Option 1 survives *as the fallback* — `lig_attachment` uses the last
component when the ids disagree, which is both HarfBuzz's behaviour and the
right answer for the faces that write mark-to-base as a one-component type 5.

**Consequence.** `SubGlyph` is one byte-ish wider (a `Lig` of four small
fields, unpacked from HarfBuzz's bit-packing because keeping the packing would
buy nothing in a struct the caller does not allocate per character, and would
hide the genuinely subtle part — that the component count and the component
index are mutually exclusive). Substitution does bookkeeping work on every
ligature whether or not the face has a type-5 lookup; the cost is a few
arithmetic ops per matched component and no allocation.

**Measured.** Over 556 host faces, 56 of which ship a type-5 lookup, with a
lam-fatha-alef string added to the sweep corpus: `agree` 10055 → 10081,
`misplaced` 125 → 99. Nothing regressed.

**Where.** `gui/font/src/gsub.rs` — `Lig`, `stamp_components`, `renumber`,
`ligation_allowed`, `base_is_hidden`, and the numbering in `apply_multiple`;
`gui/font/src/mark.rs` — `marked`, `lig_attachment`; `gui/font/src/gpos.rs` —
`MARK_LIG_POS` and `attach_to_lig`; `gui/font/tools/harfbuzz_sweep.py` — the
corpus string that can see the difference.
