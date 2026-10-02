## TD-FONT-GATES-THE-MARK-FALLBACK-ON-THE-CHARACTERS-SCRIPT-NOT-THE-FONTS

**What.** `fallback::positions_marks` decides whether a run may have its marks
placed by measurement from the OpenType tag the run's *characters* map to. In
HarfBuzz the same decision is made from the tag the *font* registers its
features under: `hb_ot_shaper_categorize` takes `gsub_script`, and sends
Devanagari to its default shaper — which does run the fallback — when the font
declares only `DFLT` or `latn`, and Myanmar there too when the font declares
`mymr` rather than `mym2`.

**Symptom.** A face that carries Devanagari glyphs, has no `GPOS`, and has no
Devanagari script record in its `GSUB` leaves its Devanagari combining marks
(anusvara, candrabindu, the Vedic accents) unplaced where HarfBuzz would
centre them. The same for a Myanmar face tagged `mymr`. No face on this host
shows it — the sweep's Devanagari divergences are all `GPOS`-path ones on
faces that do have a `GPOS`.

**Why it is filed rather than fixed.** Following HarfBuzz means asking the
font which scripts it declares before deciding how to place a mark, which
couples the fallback to the `GSUB` script list for the sake of a rare
combination. The coupling is not free: `shape` would have to resolve each
run's tag against the face's `ScriptList` *before* building glyphs, which is
work every string would pay for.

**Proper fix.** If it ever matters: `otl.rs` already resolves a script tag
against a table's `ScriptList` for the substitution pass. Hoist that
resolution to run once per run alongside `script::runs`, hand the resolved tag
(or the `DFLT` fallback that was used instead) to `positions_marks`, and let
it apply HarfBuzz's rule.

**Where.** `gui/font/src/fallback.rs` — `positions_marks` and
`COMPLEX_SCRIPTS`; `gui/font/src/scaled.rs` — `shape`, where the runs are
split.

**Fixed** (2026-08-14). It did matter, and on the most ordinary face imaginable:
`Hack` rendering `हिन्दी`. The proper fix above is what was done, with one
correction to its premise.

`fallback::shaped_as_default(tags, gsub)` transcribes the arms of
`hb_ot_shaper_categorize` that a *face* can call off — the Indic nine and
Myanmar's extra `mymr` arm, plus the ~90-script USE list — and
`ALWAYS_COMPLEX` records the three that it cannot (Thai, Lao, Khmer reach
their shapers unconditionally; the asymmetry is history, not design, but it is
observable). `positions_marks` and `zeroes_mark_advances` each gained a
`simple` parameter, and `scaled.rs` computes it once per run and feeds all
three call sites, because whether the Indic shaper runs, whether marks are
placed by measurement, and whether their advances are zeroed are three fields
of *one* HarfBuzz shaper struct and must not be allowed to disagree.

*The resolution has to be asked of the ScriptList, not of the selections.* The
entry above says "`otl.rs` already resolves a script tag against a table's
`ScriptList`", and it does — but `ByScript` only records scripts that reached a
lookup this crate can apply, and `ByScript::parse` returns `None` outright when
no requested feature reaches one. Routing through it left the sweep stuck at
`misplaced 5`: `Hack` has 16 `GSUB` lookups and registers `DFLT` and `latn`,
yet neither default language system selects a feature we ask for, so
`Substitutions` is `None` and the face appeared to name no script at all. So
`Face` now holds `gsub_scripts` — the raw ScriptList tags, sorted — and
`otl::chosen_from` walks HarfBuzz's `hb_ot_layout_table_select_script` chain
over *that*. `None` is `HB_TAG_NONE`, which equals neither `DFLT` nor `latn`,
so a face with no `GSUB` keeps its complex shaper — which is exactly the face
`NO_ZERO_WIDTH_MARKS` was measured against.

The same tag answers a second question the Indic shaper was already asking
badly: `Plan::new`'s `old_spec` (Uniscribe spec vs. the revised one) is
`chosen.is_none_or(|tag| tag.get(3) != Some(&b'2'))`, read from the face's
registered tag rather than from the run's preferred one.

Sweep: `misplaced 13 → 0`, the first zero that bucket has ever shown. Tests:
`fallback::tests::a_face_declaring_no_indic_script_calls_the_indic_shaper_off`,
`only_myanmar_reads_mymr_as_calling_its_shaper_off`,
`three_scripts_keep_their_shaper_whatever_the_face_says`,
`a_face_that_names_no_script_calls_nothing_off`,
`calling_the_shaper_off_restores_both_halves_of_the_mark_handling`;
`sfnt::tests::a_latin_only_face_calls_off_the_indic_shaper_even_with_no_usable_lookups`,
`a_face_naming_an_indic_script_keeps_the_indic_shaper`,
`a_face_with_no_gsub_keeps_the_indic_shaper`.

**Still open, and unflagged elsewhere.** HarfBuzz's Arabic arm demotes on
`gsub_script != DFLT` — so a *Syriac* run in a face that registers `syrc` takes
the Arabic shaper, but one in a face that registers only `latn` takes the
default shaper and loses `init/medi/fina/isol`. We do not model that, because
both shapers set `fallback_position = true` and `zero_width_marks =
BY_GDEF_LATE`, so it changes nothing for either mark question; it is a
divergence in joining only, and joining is not implemented for Syriac yet.
