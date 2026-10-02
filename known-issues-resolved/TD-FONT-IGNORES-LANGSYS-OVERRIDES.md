## TD-FONT-IGNORES-LANGSYS-OVERRIDES — a font's per-language rules were unreachable — ✅ **RESOLVED 2026-08-15**

**Resolution.** Exactly the fix sketched below, and the required-feature gap
beside it. `ScaledFont::shape_lang(text, Option<Lang>)` and
`SystemFont::shape_lang` take a language; `shape(text)` is `shape_lang(text,
None)`, so the change is purely additive and no caller that names no language
can shape differently than it did. `otl::ByScript::parse` now precomputes a
lookup selection per **(script, language)** rather than per script, preferring
the named LangSysRecord over the DefaultLangSys, and `feature_indices` finally
reads `requiredFeatureIndex` — the one feature a language system states outside
its index list, which the walk had been dropping for the default language too.

`lang.rs` does the BCP 47 → OpenType mapping, following HarfBuzz's
`hb_ot_tags_from_language` rule for rule: complex rules first (`ro-MD` →
`MOL `, `zh-Hant` → `ZHT `), then extended-language-subtag substitution, then
the 2- and 3-letter registries, then the blocked list, else uppercase. It is
allocation-free and puts no bound on the tag's length.
`tools/gen_lang_tables.py` generates its four tables from HarfBuzz's source, so
a registry update is a regeneration rather than an edit.

Four things worth keeping in mind about the shape of the fix:

- **A LangSysRecord replaces the default's feature list; it does not add to
  it.** So naming a language can take a feature *away* — which is exactly what
  `TRK ` does to `liga`. Callers should pass `None` rather than a guess: a
  wrong language is worse than no language.
- **One BCP 47 tag resolves to a *list* of up to three OpenType tags, not to
  one.** `ro-MD` is `MOL ` and then `ROM `; `ml` is Malayalam Traditional and
  then Reformed; `ga` is `IRI ` and then `IRT `. They are candidates and not
  synonyms: a face is asked for each in turn and the first it **registers**
  wins. The cap of three is HarfBuzz's `HB_OT_MAX_TAGS_PER_LANGUAGE`, and
  truncating where HarfBuzz truncates is what keeps the two engines answering
  alike. See "What the oracle caught" below — the first version of this fix
  kept only the head of each list and was wrong on 66 of the host's 556 faces.
- **Language selection deliberately does not fall back the way script
  selection does.** A script that does not register the language takes its own
  default, never another script's — HarfBuzz's split between
  `hb_ot_layout_table_select_script` and
  `hb_ot_layout_script_select_language`. `gsub::tests::language_selection_does_not_fall_back_to_another_script`
  pins it.
- **A script's default entry is stored even when it selects nothing**, because
  that entry is what says the script exists and stops the fallback chain.
  Language entries identical to their script's default are *not* stored; two
  thirds of the host's are. This is why `ByScript` keeps a second list of every
  (script, language) the face *registers*: "which candidate wins" is decided by
  what the font registered, never by what happened to be worth storing, or a
  `MOL ` that says nothing would hand Moldavian to `ROM `'s overrides on the
  strength of an optimisation.

**Scale.** `tools/langsys_survey.py` measured the host before the fix: of 581
installed faces, 290 register at least one LangSysRecord, 3031 (script,
language) records in all, 1203 of which differ from their default and **996 of
which differ in a feature tag this crate asks for, across 230 faces**. Moved
tags: `locl` 856, `ccmp` 90, `liga` 67, `calt` 28, `mark` 25. The survey's
feature list is pinned equal to the shaper's by
`otl::tests::the_survey_matches_the_shapers_feature_list`, so a number it
reports cannot quietly come to mean something else.

**Tested** by seven new unit tests over hand-built ScriptLists that
`fixture::script_list` cannot express (a script with no DefaultLangSys, a
script with named languages, a `requiredFeatureIndex`, a face registering only
a language's second candidate, and both orders of a face registering two of
them), by 20 tests over the BCP 47 mapping, and by `tools/harfbuzz_sweep.py`,
which grew a language field: each new corpus entry is a string already in the
corpus plus a tag, so a difference between the two halves is the language and
nothing else, and both halves map the tag with the same rules. The sweep's
buffer language is set *after* `guess_segment_properties`, and explicitly to
`""` for the language-less entries, because the guess otherwise fills it in
from the machine's locale and the run would pass or fail by where it was made.

**What the oracle caught.** The first version of this fix passed 521 unit
tests, was clippy-clean, and was wrong. The sweep found it in one run: `ro-MD`
disagreed with HarfBuzz on **345** faces where plain `ro` disagreed on 279, and
the 66-face gap was the bug. HarfBuzz's `hb_ot_tags_from_language` returns an
ordered list of up to `HB_OT_MAX_TAGS_PER_LANGUAGE = 3` candidate tags and asks
the face for each in turn; `gen_lang_tables.py` had deliberately kept only the
first of each list, on the reasoning that a language has one tag. Those 66
faces — `Candara.ttf` among them — register `('latn', 'ROM ')` and no `MOL `,
so HarfBuzz reached Romanian's comma-below `locl` for Moldavian through the
second candidate and we did not. After the generator was reworked to keep all
of them, the `ro-MD` bucket is 279: exactly `ro`'s, exactly the language-less
twin's, and entirely the pre-existing NFC divergence recorded in
`design-decisions.md` §410. Final sweep: 556 faces × 35 strings, 18235 agree,
reordered 0, misplaced 0.

This is the third bug the HarfBuzz oracle has found that a green unit-test
suite could not, and for the same reason every time: "this face has no glyph
/ no language system for that" is a *legal* answer, so no self-consistency
check can tell it apart from the truth. Only a second implementation can.

The original entry follows.

---

**What.** `otl::select` reads each ScriptRecord's DefaultLangSys and ignores
its LangSysRecords entirely. The per-language overrides — Turkish dotless `i`
under `TRK `, Serbian Cyrillic italic letterforms under `SRB `, Moldovan
comma-below under `MOL ` — are never reached, and a face whose *only* route to
a feature is a language system contributes nothing at all.

**Why it bites.** It is invisible until it is not. A Turkish reader gets the
wrong dot on `i`/`ı`; a Serbian reader gets Russian italics for бгпт. Both are
the kind of wrongness a native reader notices immediately and nobody else ever
does.

**Why it is filed rather than fixed.** There is nothing to select *with*.
Language is a property of the text's provenance, not of its characters — the
same Cyrillic codepoints are Serbian or Russian depending on who typed them —
so it cannot be derived the way script is. It needs a language carried on the
text down to `ScaledFont::shape`, which means an API change reaching the
toolkit and the locale system, neither of which has a language to hand yet.

**Proper fix.** Add an optional BCP 47 language to the shaping call, map it to
an OpenType language system tag (the registry is a fixed table, `tr` → `TRK `,
`sr` → `SRB `), and have `select` prefer that LangSysRecord over the
DefaultLangSys. Default stays "no language", which is what every shaper does
when not told and what this crate does now — so the change is additive and
cannot regress text that names no language.

**Reproduce.** `gsub::tests::a_feature_only_a_language_system_reaches_is_not_applied`
pins the current behaviour: a `locl` reachable only through `TRK ` yields no
`Substitutions` at all.

**Where.** `gui/font/src/otl.rs` — `select`, `LangSys`, and the module doc's
"What is not here".
