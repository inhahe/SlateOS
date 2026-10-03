## §422 — The face chooses the shaper, not the character

**Date:** 2026-08-14
**Lane:** C
**Decided by:** Claude (autonomous)

Which shaper a run gets looked like a property of the text: Devanagari
characters want the Indic shaper, Arabic characters the joining one. That is
what `fallback::positions_marks` and `Face::substitute` both assumed, and it is
wrong. `hb_ot_shaper_categorize(script, direction, gsub_script)` takes a third
argument, and for most complex scripts the **font** gets a veto: a Devanagari
run in a face that files its `GSUB` features under `DFLT` or `latn` is shaped
by the *default* shaper, which places marks by measurement and zeroes their
advances where the Indic shaper withholds both. `Hack` rendering `हिन्दी` is
that face and that string, and it was the last 13 cases in the sweep's
`misplaced` bucket.

The veto is not uniform, which is the awkward part: Thai, Lao and Khmer reach
their shapers unconditionally, Myanmar treats a third tag (`mymr`, which
predates the Myanmar shaping spec) as a veto alongside `DFLT` and `latn`, and
Arabic inverts the test (`gsub_script != DFLT`). There is no stated principle
behind the asymmetry — the Thai shaper predates the check, Khmer was split out
of Indic after it — so it is transcribed rather than tidied, in
`fallback::ALWAYS_COMPLEX` and the `mymr` arm of `shaped_as_default`.

**Two options for where the answer lives.**

*A `Shaper` enum resolved once per run*, mirroring HarfBuzz's struct: each
variant carrying `fallback_position`, `zero_width_marks` and the substitution
entry point, with every consumer reading fields off it. Structurally the right
shape, and it makes the "three questions, one answer" property unforgeable.
But `COMPLEX_SCRIPTS` and `NO_ZERO_WIDTH_MARKS` are *measured* lists — probed
against real faces, not read off the source — and folding them into an enum
means rederiving both from the shaper table by hand, with 11,834 agreeing runs
riding on getting it right. The rewrite risked more than it bought.

*A predicate beside the lists that already exist.* `shaped_as_default(tags,
gsub)` answers "did the face call this run's shaper off", and the three
consumers take it as a parameter. Less tidy — the shaper is still implicit,
spread across three lists and a predicate — but every existing measurement
survives untouched, and the one new claim is small enough to test directly.

The second was chosen. The discipline that replaces the enum is that
`scaled.rs` resolves the answer **once per run** and feeds all three consumers
from that one binding: whether the Indic shaper runs, whether marks may be
placed by measurement, and whether their advances are zeroed are three fields
of one HarfBuzz struct, and the failure mode of the predicate form is letting
them disagree.

**One correction the implementation forced.** The chosen tag has to be read
from the raw `GSUB` ScriptList, not from `Substitutions`. `Substitutions`
records only the scripts that reached a lookup this crate can apply, and
`ByScript::parse` returns `None` outright when none does — so `Hack`, with 16
`GSUB` lookups and both `DFLT` and `latn` registered, appears to name no script
at all, and routing the question through it left the sweep stuck at
`misplaced 5`. `Face` now holds `gsub_scripts` and `otl::chosen_from` walks
HarfBuzz's `hb_ot_layout_table_select_script` chain over that. `None` is
`HB_TAG_NONE`, which equals neither `DFLT` nor `latn`, so a face with **no**
`GSUB` keeps its complex shaper — which is exactly the kind of face
`NO_ZERO_WIDTH_MARKS` was measured against.

**Known divergence, deliberate.** Arabic's inverted test would demote a Syriac
run in a `latn`-only face to the default shaper and so suppress
`init/medi/fina/isol`. It is not modelled, because both the Arabic and the
default shaper set `fallback_position = true` and `zero_width_marks =
BY_GDEF_LATE` — the divergence is in joining only, and is recorded under
`TD-FONT-GATES-THE-MARK-FALLBACK-ON-THE-CHARACTERS-SCRIPT-NOT-THE-FONTS`.

**Where.** `gui/font/src/fallback.rs` — `shaped_as_default`, `ALWAYS_COMPLEX`,
and the `simple` parameter on `positions_marks`/`zeroes_mark_advances`;
`gui/font/src/sfnt.rs` — `gsub_scripts`, `shapes_as_default`,
`gsub_chosen_script`; `gui/font/src/otl.rs` — `chosen_from`;
`gui/font/src/scaled.rs` — the per-run `simple` binding in `shape`.
