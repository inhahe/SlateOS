## 1359. Marks and the legacy `kern` table are read as HarfBuzz reads them: by `GDEF` class alone, and across marks inside the positioning pass

**Date:** 2026-10-05
**Lane:** F
**Decided by:** Claude (autonomous), revisiting two of lane C's autonomous
calls on `gui/font` (§403's second point and §414's legacy-table alternative),
which lane F has owned since the six-lane split.

**In short:** the text engine is held to HarfBuzz, glyph for glyph, over
every font on the development machine (`gui/font/tools/harfbuzz_sweep.py`).
Strings the sweep had never asked found four places where it disagreed.

- In fonts that kern through the old `kern` table, a pair with an accent
  between it, such as `T́o`, went unkerned.
- An accent after a variation selector the font cannot draw landed a whole
  letter to the left.
- In two fonts whose designers call a combining mark a base glyph, the accent
  lost the width HarfBuzz gives it.
- Syriac could keep its joining forms in a font that asked for none.

All four now match HarfBuzz on every face. Two of the fixes reverse decisions
lane C recorded, which is why this entry exists.

**What was decided.**

1. **A glyph is a mark by its `GDEF` class alone, where the face classifies
   its glyphs** (`mark.rs`, `MarkPositioning::is_mark`). This reverses §403's
   second point, "`GDEF` class 3 or membership of a mark coverage". The union
   was there so that mark attachment, which `mark.rs` then did itself, would
   reach DejaVu Sans Mono's accents. Attachment is now `gpos.rs`'s, by lookup
   coverage, whatever the class. What the class still decides is whether a
   glyph loses its width, and what `IgnoreMarks` steps over, and for both
   HarfBuzz reads the class and nothing else. DejaVu Sans Mono Bold Oblique
   classes `acutecomb` a base, 1233 units wide, and Linux Libertine's Graphite
   build classes `uni0308` a base and anchors it in its `mark` lookup.
   HarfBuzz draws both at their full width. A face with no `GlyphClassDef` is
   unchanged: the shaper asks the character (`Mn`, HarfBuzz's synthesized
   classes).
2. **The legacy `kern` table is read across marks** (`kern.rs`
   `legacy_pair`). This reverses §414's reason for leaving it strictly
   adjacent: "the historically correct reading". HarfBuzz's `hb_kern_machine_t`
   sets `IgnoreMarks` on its matcher, so `A` and `V` kern with an accent
   between them whichever table a face ships. About ninety host faces
   disagreed on such pairs.
3. **…and read inside the positioning pass**, after the `GPOS` lookups and
   before the attachments are resolved (`gpos.rs` `kern_legacy`, through
   `Run::legacy`). That is where HarfBuzz reads it: `hb_ot_layout_kern`
   follows `GPOS` in `hb_ot_shape_plan_t::position`, and
   `propagate_attachment_offsets` runs afterwards. Read after the pass, an
   accent attached across a kerned pair drifted by the half of the kern
   charged to its letter: 92 units in Segoe UI Italic, 24 in Tahoma. Runs
   that no `GPOS` reaches are still kerned after it, where nothing is
   attached and the order cannot matter.
4. **A never-drawn character loses its width before the attachments are
   resolved** (`gpos.rs` `zero_ignorables`, HarfBuzz's
   `hb_ot_zero_width_default_ignorables`). It was taken at the very end, so a
   mark measured back across it still counted it. That cost Cascadia Code's
   1200-unit missing-glyph box under VS16, and 157 faces in all.
5. **Syriac's shaper is called off by `DFLT` alone, and a run whose shaper is
   called off takes no joining forms** (`fallback::shaped_as_default`,
   `scaled.rs`). This transcribes HarfBuzz's Arabic arm, which Syriac goes
   through. It mattered once Syriac had joining forms of its own; it was
   recorded as open in
   `known-issues-resolved/TD-FONT-GATES-THE-MARK-FALLBACK-ON-THE-CHARACTERS-SCRIPT-NOT-THE-FONTS.md`.

**Why autonomous.** The engine's reference is HarfBuzz (§410 lists the
deliberate divergences, and none of these is one of them), and each change was
measured against it. What changes for a reader is HarfBuzz's own rendering,
which is what browsers on the same fonts show.

**Alternatives.**

| | For | Against |
|---|---|---|
| These rules (chosen) | the sweep agrees with HarfBuzz on all 556 host faces, at the em, at 16 ppem and at `wght=700`, as it did before these strings were added | a mark a designer misclassified keeps its width and pushes the next letter on, as it does in a browser |
| Keep §403's union | DejaVu Sans Mono's mis-classed accents take no room | disagrees with HarfBuzz, and so with every browser, on exactly the faces the union exists for; attachment no longer needs it |
| Keep the legacy table strictly adjacent | the reading the table's own era used | about ninety faces kern `T́o` differently from every current engine |
| Kern the legacy table after the pass, as before | one kerning loop instead of two | attached accents drift by half a kern |

**Measured.** The sweep's corpus gained the strings that found these:
`a️́`, `V̈A`, `T́o`, and words of N'Ko, Syriac and
Mongolian. It stands at 556 faces × 118 strings, 0 disagreeing on glyphs
and 1 on positions (Segoe UI Variable's CGJ, §410).

**How to reverse.** Each point is a few lines: the `if let Some(table)` in
`is_mark`; the `Skipper` in `legacy_pair`; `Run::legacy` (pass `None` and the
loop in `ScaledFont::shape` kerns everything); the `zero_ignorables` call; the
Syriac arm of `shaped_as_default` and the `simple` branch beside
`SubGlyph::cursive`. Tests: `kern.rs`
`the_legacy_table_reads_across_a_mark_as_harfbuzz_does`; `mark.rs`
`a_glyph_gdef_classes_as_a_base_is_no_mark_whatever_a_lookup_covers` and
`where_a_face_classifies_its_glyphs_the_class_is_the_whole_answer`; `gpos.rs`
`a_never_drawn_glyph_loses_its_width_before_a_mark_is_measured_across_it` and
`the_legacy_table_kerns_inside_the_pass_so_a_mark_stays_on_its_letter`;
`scaled.rs` `the_legacy_table_kerns_a_pair_across_an_accent` and
`a_face_that_calls_the_joining_shaper_off_keeps_the_letters_unjoined`;
`fallback.rs` `syriac_is_called_off_by_dflt_alone`.
