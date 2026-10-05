## §430 — A language is a *list* of OpenType tags generated from HarfBuzz, and the first one the font registers wins

**Date:** 2026-08-15
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** A font can hold rules that apply to one language and not another
written in the same alphabet — Turkish spells the lowercase of `I` as a dotless
`ı`, Romanian wants a comma under `ș` rather than a cedilla. To reach those
rules the shaper has to turn what the caller knows ("this text is Turkish",
written `tr`) into the four-letter code the font filed them under (`TRK `).
There is no rule that derives one from the other: it is a lookup table of about
eleven hundred entries that Microsoft maintains. Three things were decided
here — where that table comes from, what to do when one language maps to
several codes, and what to store per font.

### The decisions

**1. The table is generated from HarfBuzz's source, not written by hand.**
`gui/font/tools/gen_lang_tables.py` parses HarfBuzz's `hb-ot-tag-table.hh` and
`hb-ot-tag.cc` and emits `gui/font/src/lang_tables.rs` (148 complex rules, 188
two-letter keys, 916 three-letter keys, 162 blocked codes). A registry update
is a regeneration, not an edit.

*Alternative:* transcribe the Microsoft registry by hand, or write the mapping
as code. Both were rejected for the same reason: the crate measures itself
against HarfBuzz with `tools/harfbuzz_sweep.py`, and a table that disagrees
with HarfBuzz's turns every sweep difference into an argument about whose
registry is right instead of a bug report. Taking the data from the shaper we
compare against makes any remaining difference *ours*.

*Cost:* the generator is coupled to the layout of two HarfBuzz source files and
will break when they are restructured. It is written to fail loudly rather than
silently emit a short table — it checks HarfBuzz's own stated run lengths, and
refuses on a key collision between two of the tables — so a break is a stopped
run and not a wrong answer.

**2. One BCP 47 tag maps to up to three OpenType tags, tried in order, and the
first the *font registers* wins.** `ro-MD` is `MOL ` then `ROM `; `ml` is
Malayalam Traditional then Reformed; `ga` is `IRI ` then `IRT `. The cap of
three is HarfBuzz's `HB_OT_MAX_TAGS_PER_LANGUAGE`.

*Alternative — and this is what the first version of the fix did:* keep only
the first tag of each list, on the reasoning that a language has one code and
the rest are historical spellings. That is wrong, and the HarfBuzz sweep proved
it within one run: 66 of this host's 556 faces (`Candara.ttf` among them)
register `('latn', 'ROM ')` and no `MOL ` at all, so Romanian's comma-below
reached Moldavian in HarfBuzz and not in us. The `ro-MD` disagreement bucket
was 345 against plain `ro`'s 279; after the rework it is 279, exactly. The
candidates are not synonyms — they are an ordered search, and a font gets to
answer at whichever spelling it chose.

*Why cap at three rather than keep every candidate:* HarfBuzz truncates there,
and a fourth candidate we honoured and HarfBuzz did not would be a divergence
in the one place the two engines are meant to agree exactly. The cap is also
the widest run the registry actually contains, so today it truncates nothing.

**3. `ByScript` stores a language's lookup selection only when it differs from
its script's default — but stores the language's *tag* either way.** Two thirds
of the 3031 LangSysRecords on this host select exactly what their script's
default does; storing those would be storing a second copy of an answer already
present.

The second half of that sentence is the subtle part, and it is forced by
decision 2. "Which candidate wins" must be decided by what the font
**registers**, never by what happened to be worth storing — otherwise a face
that registers `MOL ` and gives it no rules of its own would fall through to
`ROM ` and apply Romanian's overrides to Moldavian, on the strength of an
optimisation. So `ByScript` carries a second sorted list of every (script,
language) pair the face names, at 8 bytes each (~5 per face here), and consults
that to choose the candidate before looking up what it selects.
`gsub::tests::the_first_candidate_a_face_registers_wins_even_when_it_selects_nothing`
is the regression guard; mutating `selection` to search the stored selections
directly fails exactly that test and nothing else.

### If this is ever revisited

The thing to preserve is the invariant that the mapping is *HarfBuzz's*, not a
reasonable approximation of it. Every one of the three decisions above bends
toward that, and the one time it was bent away from — keeping the head of each
candidate list — cost a wrong answer on 12% of the host's fonts that 521 green
unit tests could not see.

**Where:** `gui/font/src/lang.rs`, `gui/font/src/lang_tables.rs` (generated),
`gui/font/tools/gen_lang_tables.py`, `gui/font/src/otl.rs`
(`select`, `ByScript::parse`, `ByScript::selection`),
`known-issues.md` → `TD-FONT-IGNORES-LANGSYS-OVERRIDES`.
