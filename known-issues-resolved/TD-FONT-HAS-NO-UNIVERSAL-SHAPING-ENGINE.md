## TD-FONT-HAS-NO-UNIVERSAL-SHAPING-ENGINE — ✅ **RESOLVED 2026-08-16**

**What.** Of HarfBuzz's six complex shapers we have two: Arabic (joining) and
Indic (reordering). The other four — Khmer, Myanmar, Thai/Lao, and the
Universal Shaping Engine that covers roughly ninety further scripts — are not
written, so a run in any of those scripts gets the default shaper: `ccmp`,
`locl`, the ligature features, and nothing positional or reordering.

**Symptom.** Unmeasured. No string in the sweep corpus is Khmer, Myanmar, Thai
(as *shaped* text — the Thai string in the corpus exercises mark fallback on
faces that do not cover it), Tibetan, Javanese or any other USE script, so the
sweep reports zero disagreement for them because it never asks. The expected
failure is the same shape as Devanagari's was before the Indic shaper: pre-base
vowels drawn after their consonant, conjuncts not forming, and in Khmer the
coeng-joined subscripts drawn as full-size letters on the baseline.

**Why it is filed rather than fixed.** It is the largest single piece of
shaping left and it wants its own measurement first. The sweep's corpus has to
gain strings for each family before the work can be checked, and the host's 556
faces have to be surveyed for which of them cover those scripts at all — on a
Windows development host that is likely to be very few, which is itself an
argument for doing it after the things the host *can* measure.

**The survey has now been run** (2026-08-15, `gui/font/tools/script_survey.py`,
579 faces), and it contradicts the guess above. Counting only faces that both
cover the script *and* register its tag in `GSUB` — the sharper test, since a
face that declares nothing gives both implementations nothing to do and agrees
trivially:

| shaper | script | measuring faces |
|---|---|---|
| Thai | Thai | **8** |
| Thai | Lao | **19** |
| Khmer | Khmer | 3 |
| Myanmar | Myanmar | 2 |
| USE | Tifinagh | 6 |
| USE | Buginese | 4 |
| USE | Tibetan / Javanese / Balinese / Sinhala / Cham | 1 each |
| USE | Tai Tham | 0 (covered by one face, declared by none) |
| *(control)* Indic | Devanagari | 5 |
| *(control)* Arabic | Arabic | 43 |

The control row is the point: **Devanagari, already written and measured from
`misplaced 13` to `0`, had only five faces to measure against.** Thai has eight
and Lao nineteen. So the host is not thin for these scripts at all — it is
thin for exactly one of the four families, USE, where most scripts have a
single face and Tai Tham has none.

**Proper fix — order revised by the survey.** ~~USE first, since it subsumes
the most scripts.~~ USE subsumes the most scripts and is the *least*
measurable of the four; writing it first means writing the largest piece of
shaping left with an oracle that can barely disagree with it. Take them in
order of how well the host can check the work:

1. **Thai/Lao first** — best measured (27 faces between them) and smallest.
   It is not a reordering shaper at all: it is the PUA fallback for Thai fonts
   with no `GSUB`, plus a `ccmp`-like normalization of the vowel/tone order.
2. **Khmer, then Myanmar** — 3 and 2 faces, variants of the Indic model that
   can reuse `initial_reordering_syllable`'s base-finding.
3. **USE last** — the cluster grammar from the Unicode Shaping Engine spec,
   the same stage driver `indic_shape.rs` already has, and `Plan`'s probing of
   what the face declares. By then the three smaller shapers will have shaken
   out the stage driver against faces that can actually object.

Each step needs its corpus strings added to `harfbuzz_sweep.py` first, or the
sweep reports agreement it never tested.

**Step 1 of 3 done — Thai/Lao, 2026-08-15** (`gui/font/src/thai.rs`, commits
`48597037a` and `a7130c6b4`). Both halves the survey predicted, and both
measured:

* **SARA AM decomposition.** U+0E33 (Lao U+0EB3) is one character drawn as two
  marks in two places, and Unicode gives it no canonical decomposition, so we
  were asking faces for a glyph almost none have. `thai::preprocess` splits it
  into nikhahit + sara aa and walks the nikhahit back over any above-base
  marks, which is HarfBuzz's `preprocess_text_thai` — Uniscribe's behaviour,
  not the MS OT Thai spec's. It runs from `norm::normalize` *between*
  decomposition and the mark sort, and the ordering is load-bearing rather
  than incidental: sorting first puts the nikhahit on the wrong side of a
  tone mark. Host sweep, 556 faces: **agree 18806 → 21015, reordered 3 → 0,
  differ 3382 → 1176**, every Thai and Lao string now agreeing on every face.
  The residual 1176 is entirely the pre-existing NFC bucket, untouched.
* **The private-use fallback.** `thai::pua_shape`, two state machines
  transcribed from `hb-ot-shaper-thai.cc`, gated on HarfBuzz's
  `!plan->map.found_script[0]` — the face's `GSUB` did not name `thai`.
  Note that this is *not* `Face::shapes_as_default`, which answers false for a
  face with no `GSUB` at all; a face with no `GSUB` is exactly the font the
  pass exists for, and using the wrong predicate made the pass never fire.

  This one could not be measured against the host at all: **not one of the 556
  installed faces carries a single Thai private-use glyph** (probed directly),
  because every Thai font Windows ships today describes its shaping in `GSUB`,
  which turns the fallback off. A host sweep would have reported agreement it
  never tested. So the oracle was built instead —
  `gui/font/tools/gen_thai_legacy.py` synthesizes three faces with no
  `GSUB`/`GPOS`/`GDEF` (Windows forms, Mac forms, and none), and
  `thai-pua-corpus.txt` names one string per edge of the two machines plus
  controls. **78 of 78 agree**, including the vendor preference and the
  no-private-use face, which the pass must leave untouched.

`harfbuzz_sweep.py` gained a `--corpus FILE` flag along the way, so each of
the remaining steps can bring its own corpus without editing the built-in one.

**Step 2 of 3 done — Khmer, 2026-08-15** (`gui/font/src/khmer.rs`, commits
`1897b9b19` and `8f1247264`). The shaper, the oracle that could disagree with
it, and the bug that oracle found in a shaper that had been shipping for weeks:

* **The shaper.** Khmer is the Indic model with two moves and one structural
  difference. The moves: a COENG+RO pair jumps to the head of its syllable and
  takes `pref`, and everything it jumped over takes `cfar`; a pre-base vowel
  moves to the head as well, ahead of a fronted pair. The five split vowels
  (U+17BE–U+17C5) get no canonical decomposition from Unicode, so they are
  split by hand into U+17C1 plus a second half before the syllables are cut.
  The structural difference is that the reordering runs *before the first
  lookup* rather than after `locl`/`ccmp` as Indic does. The syllable-serial
  stamping the two shapers share came out into `gui/font/src/syllabic.rs` at
  the same time. `cfar` was appended to `gsub::FEATURES` (and to
  `langsys_survey.py`'s `WANTED`, which `otl.rs` pins equal to it).
* **The host measurement.** 556 faces: **agree 27421 → 27687**, differ back
  down to the same 1176 pre-existing NFC baseline, **reordered 0, misplaced 0**
  — every Khmer string agreeing on every face.
* **The oracle, per §431 of `design-decisions.md`.** That host number is worth
  much less than it looks. Only three installed faces register `khmr` at all
  (the Leelawadee UI family), and **not one face on this machine has a `cfar`
  lookup**, nor `pres`, nor `psts` — probed directly. So the sweep was
  reporting agreement about masks it never applied. `gui/font/tools/
  gen_khmer_probe.py` builds `KhmerProbe.ttf`, where each of thirteen features
  has one lookup rewriting every Khmer glyph as *itself followed by a marker
  glyph unique to that feature*, so a glyph run spells out which features
  reached it and in what order. It is a one-to-two substitution on purpose: the
  base has to survive so the next feature still matches it, and `cfar` — which
  is applied after `blwf`, on the very glyphs `blwf` is set on — is precisely
  the feature a one-to-one substitution would have hidden. Markers carry zero
  advance so a wrong mask can never smear into the positions.
  `khmer-corpus.txt` is one string per edge of the shaper rather than sampled
  text. **43 of 45 agree.**
* **What it found.** `locl`, `ccmp`, `blwf`, `abvf` and `pstf` were each being
  applied **twice** — named in an early stage and then again in the final
  `ALL_FEATURES` stage, which `gsub::apply_stages` does not deduplicate.
  HarfBuzz gives each feature exactly one stage (`hb_ot_map_builder_t::compile`
  merges duplicate tags at the `hb_min` of their stages), so this was a real
  divergence. **The Indic shaper had the same bug**, and 556 installed faces
  had hidden it for as long as that shaper has existed: a duplicate application
  is invisible unless a lookup is not idempotent, and real faces' lookups
  overwhelmingly are. Fixed in both by masking the earlier stages out of the
  last one; `stages()` was factored out of `shape` in each so the "one feature,
  one stage" invariant is four tests rather than a comment.
* **The two that still differ** are the joiner strings, and are not a Khmer
  bug: HarfBuzz emits the face's space glyph for a default-ignorable character
  (ZWJ, ZWNJ, soft hyphen …) once shaping is done, and deletes it outright if
  the face has no space. We emit the character's own glyph. That is crate-wide
  and script-independent — a soft hyphen would draw a visible hyphen mid-word
  on any face that maps it — so it is tracked separately under
  `TD-FONT-DOES-NOT-HIDE-DEFAULT-IGNORABLES` rather than here.

**Step 3 of 3 done — Myanmar, 2026-08-15** (`gui/font/src/myanmar.rs`,
`myanmar_machine.rs`, `tools/gen_myanmar_machine.py`). The shaper is the
smallest of the three, and the pass it forced open — mark positioning — was
the largest thing found in this whole exercise.

* **The shaper.** Myanmar is syllabic like Indic and Khmer and shares their
  category table and machine generator, but it reorders by a different
  mechanism: it assigns a `Position` to *every* glyph of the syllable and then
  **stably sorts** by it, rather than rotating a fixed few. Three things move —
  a kinzi (`Ra + Asat + virama`) sorts to just after the base, a medial RA
  (U+103C) to `PreC`, and a pre-base vowel (U+1031) to `PreM`, with a run of
  several pre-base vowels flipped, the same repair Indic makes. The base search
  is forwards and stops at the first consonant: there is no reph to guess at,
  because a Myanmar kinzi is spelled out and recognised by that spelling.
  `rphf`, `pref`, `blwf` and `pstf` each get a stage of their own with a pause,
  where Khmer runs its basic features in one. The reordering runs *after*
  `locl`/`ccmp` — Khmer's most surprising property inverted.

* **Measured.** Myanmar sweep (`mmrtext.ttf`, `mmrtextb.ttf`): **58 of 58
  agree**, `misplaced 0`. Full host sweep, 556 faces × 89 strings: `agree
  48087`, `reordered 0`, `misplaced 170`, `differ 1178`, `mixed 49` — the 170
  being the deliberate ignorable-caret divergence recorded under
  `TD-FONT-DOES-NOT-HIDE-DEFAULT-IGNORABLES`, unchanged.

* **What it found: our mark fallback had HarfBuzz's *two* zeroing routes fused
  into one.** HarfBuzz zeroes a mark's advance by two independent routes, and
  we had been approximating them with a single union.
  - *Route 1*, `zero_mark_widths_by_gdef`, is gated on the per-script
    `plan->zero_marks` — which eleven scripts turn off — and zeroes every glyph
    whose `GDEF` class is mark, **or**, only when the face has no `GDEF`
    classes at all, every glyph whose general category is `Mn`. Either/or,
    never both: a face that classifies has *stated* which glyphs are marks and
    the character's category must not second-guess it.
  - *Route 2*, `_hb_ot_shape_fallback_mark_position`, zeroes only the marks it
    actually places (combining class ≠ 0), plus — when the base has no
    extents — every `Mn` in the cluster.

  We had the two `||`-ed together and the per-script gate missing entirely.
  `scaled.rs` now encodes them separately: `zeroed_at` carries `zero_marks`
  per segment, `marks` is the either/or, and `synthesize_marks` is a two-phase
  transcription of `position_cluster_impl`/`position_around_base` — walk the
  clusters and apply route-2 zeroing, *then* compute the pens, *then* place.
  See `design-decisions.md` §436.

* **And the bug that made it visible.** A mark whose combining class is zero is
  not placed and not zeroed, which made it look exactly like a base to the old
  cluster splitter, so the measurement **restarted at it** and every mark after
  it was measured against the wrong glyph. In `ကို့` the dot below landed two
  letters right of where HarfBuzz draws it. HarfBuzz cuts clusters on the
  general *category* and takes the base as the first non-mark; the class only
  decides whether a mark is moved once the base is known. Fixed, and pinned by
  `scaled.rs`'s `a_class_zero_mark_does_not_start_a_new_cluster` and
  `only_the_marks_the_fallback_places_lose_their_advance`. It was the whole of
  the remaining `simsun.ttc` divergence (`0;-128;0` → HarfBuzz's `0;-640;0`).

**One known gap, believed unreachable — the cluster splitter reads `Mn` where
HarfBuzz reads `Mn|Mc|Me`.** `norm::is_mark` is `Mn`-only, which is exactly
right for route 1 (it transcribes `hb_synthesize_glyph_classes`, which is also
`Mn`-only) but is *narrower* than the `_hb_glyph_info_is_unicode_mark` that
cuts fallback clusters. A spacing combining mark (`Mc` — an Indic matra) or an
enclosing one (`Me`) would therefore end a cluster here and continue one in
HarfBuzz. It cannot arise in NFC text: reaching it needs an `Mc` followed by a
non-zero-class `Mn` inside one cluster, i.e. a matra with a nukta after it,
which canonical ordering does not produce — and every path into this pass has
already normalised. Filed rather than fixed because the fix is a second
general-category predicate (`is_unicode_mark`) used by the splitter only, and
adding an untestable one costs more than it buys. If a corpus string is ever
found that reaches it, that is the fix.

USE is next, and is now the only shaper left. Note for it: like Myanmar, it
zeroes mark advances *before* `GPOS` rather than after, so its script tags have
to join `mym2` in `fallback::Zeroing::BeforeGpos`.

**Where.** `gui/font/src/sfnt.rs` — `Face::substitute`, which dispatches on
`Script::shaping` and would gain the other families; `gui/font/src/indic.rs`
and `indic_machine.rs` — the models to copy; `gui/font/tools/harfbuzz_sweep.py`
— `CORPUS`, which needs a string per family before any of this is measurable.

**Step 4 of 4 done — the Universal Shaping Engine, 2026-08-16. This entry is
closed: all five complex shapers now exist** (`gui/font/src/universal.rs`,
`universal_machine.rs`, `universal_tables.rs`, `tools/gen_universal_machine.py`,
`tools/gen_universal_table.py`, `tools/use-corpus.txt`, `tools/hb_use_oracle.py`).

* **The shaper.** One engine for eighty-eight scripts, and the only one of the
  five that does not encode a writing system's rules. It encodes the *shape*
  those rules share — a cluster is a base with modifiers hung on it — and leaves
  the particulars to the font. Its own contribution is to say where a cluster
  ends, which glyphs a repha and a pre-base vowel move past, and which of the
  four positional forms a cluster takes. `Category` and its ranges are generated
  from Unicode plus the `ms-use` override files HarfBuzz vendors and checked
  against HarfBuzz's packed table code point by code point; `Cluster` and its
  DFA are generated from the ten regular expressions of the ragel grammar.
  Neither is hand-transcribed.

* **Three things make it unlike the other three syllabic shapers**, and each is
  a class of bug the others cannot have.
  - *The machine reads a **filtered** view of the run.* Indic, Khmer and Myanmar
    hand their machines every glyph; USE first hides the `CGJ` category (the
    combining grapheme joiner, ZWJ and the variation selectors) and a ZWNJ whose
    next visible glyph is a combining mark, so that neither can cut a cluster in
    half. The hidden glyphs are not removed — they stay in the run and belong to
    whichever cluster the machine was in when it stepped over them, and a run
    that *begins* with a hidden glyph leaves it in no cluster at all.
  - *The **font** decides what a repha is.* Indic recognises a reph by spelling
    (`Ra` + virama) and Myanmar a kinzi likewise. USE runs the font's `rphf`
    feature alone, as a stage of its own, and then asks which glyph the font
    rewrote; `pref` is treated the same way and a glyph it rewrote becomes a
    pre-base vowel. Neither question can be answered without having run the
    lookup, which is why `SubGlyph::substituted` exists.
  - *The positional forms are per **cluster**, not per letter.* For most USE
    scripts `isol`/`init`/`medi`/`fina` are not cursive joining at all: they are
    handed out by asking whether the *previous cluster* joined. The eleven USE
    scripts that really do join cursively take the ordinary `joining` path.

* **The corpus, and why it is the host's real fonts this time.** Unlike Khmer
  and Myanmar — where §431 of `design-decisions.md` forced a synthesized probe
  because the host could not falsify the pass — `script_survey.py` reports faces
  declaring `tibt`, `java`, `bali`, `sinh`, `tfng`, `cham` and `bugi`. Seven
  scripts is not eighty-eight, but between them they cover the three things the
  engine actually does: cluster the run, move a pre-base vowel, and give a broken
  cluster a dotted circle. `tools/use-corpus.txt` is 58 strings, one per edge,
  and lines in scripts no installed face declares are kept deliberately — they
  shape to boxes on both sides today and are the regression test for the day a
  face for them is installed.

* **Measured: 556 faces × 58 strings — `agree 32203`, `reordered 0`,
  `misplaced 40`, `differ 0`, `mixed 5`.** No unexplained disagreement remains.
  The 40 `misplaced` are the pre-existing ignorable-caret divergence recorded
  under `TD-FONT-DOES-NOT-HIDE-DEFAULT-IGNORABLES` — they predate USE and are
  not USE's. The 5 `mixed` are the *itemizer*, not the shaper: two corpus lines
  run more than one script, HarfBuzz guesses a single script for the whole
  buffer where we itemize, and the difference that comes back is not a question
  the two halves can be asked the same way. They are counted apart because of it
  (a leading `!` on a corpus line, which `read_corpus` now reads — the marker
  lives next to the string it describes rather than in a second list in another
  file that would go stale the first time a corpus line was edited). The full
  89-string host sweep is byte-identical before and after: `agree 48087`,
  `reordered 0`, `misplaced 170`, `differ 1178`, `mixed 49`. Khmer probe 45/45,
  Myanmar probe 48/48, both unchanged.

* **What it found, and it was not in the shaper: we were recomposing split
  vowels.** A dozen scripts spell one vowel as a single character that is *drawn*
  as two marks on opposite sides of its consonant, and Unicode records the
  canonical decomposition. `norm::pieces` was gluing the two halves back into one
  character before the shaper ever saw them, so the pre-base half could not be
  moved — the Sinhala line `\u0D9A\u0DDC` was wrong on every face that can draw
  it. HarfBuzz covers this by name: `compose_use` and `compose_indic` are both
  the plain Unicode composition guarded by `/* Avoid recomposing split matras. */
  if (HB_UNICODE_GENERAL_CATEGORY_IS_MARK (general_category (a))) return false;`.
  We take it as one switch on the normalizer rather than a hook per shaper,
  having first enumerated the affected set exhaustively: of every canonical
  two-part decomposition in Unicode 16 whose first half is `Mn|Mc|Me` there are
  59, 12 are composition exclusions that never recompose, and the remaining
  **47 all belong to scripts shaped by Indic or USE**. A test walks the table and
  asserts both the predicate and the count, so a regeneration that adds a script
  outside the two engines fails the build.

* **And the correction that cost the most: HarfBuzz's decomposition is
  font-aware, and ours was not.** Splitting unconditionally regressed the sweep
  to `differ 555` — one per face, `ours [0,0,0] vs harfbuzz [0,0]` — on faces
  with no Sinhala coverage whatsoever. Reading `hb-ot-shape-normalize.cc` gives
  the reason: `decompose()` refuses outright when the *second* half has no glyph,
  and falls back to the whole character when the first half has no glyph and does
  not decompose further. So on a face that can draw none of it HarfBuzz emits one
  notdef box and we were emitting two. `norm::rejoin_split_vowels` is the font
  half of the rule — rejoin an adjacent pair when the first is a mark, the two
  share a cluster offset, and the face cannot draw **both** halves. Sharing a
  cluster offset is the provenance test that keeps this from becoming a second
  composition pass: halves at the same offset came from one character this crate
  took apart, halves at different offsets were two characters the author typed,
  and only the former may be put back. It runs *after* `fit_to_face` and not
  before, because the two would otherwise fight. See `design-decisions.md` §439.

* **Zeroing.** As the note left by step 3 predicted, USE zeroes mark advances
  *before* `GPOS` rather than after, so its script tags join `mym2` in
  `fallback::Zeroing::BeforeGpos`.

**What is deliberately not here.** *Vowel constraints* — HarfBuzz's
`preprocess_text_use` inserts a dotted circle between a consonant and an
independent vowel Unicode says may not follow it. That is a table of its own and
a separate pass, and this crate has neither, for USE or for Indic. It is a
missing *diagnostic*, not a missing shaping rule: text that triggers it is
ill-formed either way, and the divergence is that HarfBuzz marks the error
visibly and we render it silently.

**One thing worth doing later, not blocking.** Per §431, the host declares only
seven of the eighty-eight scripts, and none for Tai Tham, Batak, Brahmi or the
Egyptian hieroglyphs the corpus exercises — those lines agree because both sides
draw boxes. A `gen_use_probe.py` on the pattern of `gen_khmer_probe.py` would
turn that agreement into a measurement. Tracked in `todo.txt`.

---

> Moved here from `known-issues.md` on 2026-08-16, at lane A's request
> (`requests/a-c-bench-compositor-entries-are-yours.md`): an entry belongs with
> the code whose behaviour it describes, not with the instrument that measured
> it. The benchmark and `baselines.toml` are lane A's `bench/**`; the thing that
> got faster is `gui/compositor`, which is lane C's. Kept at its original `###`
> level rather than re-levelled, as with the other swept entries.
> `BENCH-COMPOSITOR-SLOW` is lane C's on the same rule but stays in
> `known-issues.md`: it is still open (4.6x improved, still over the 4K frame
> budget, and the remaining work is a bandwidth/parallelism problem).
