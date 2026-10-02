## TD-FONT-ZEROES-A-MARKS-ADVANCE-ONLY-WHEN-THE-FALLBACK-RUNS

Filed 2026-08-14, lane C. Fixed the same day.

A combining mark takes no room on the line. We zeroed its advance from
exactly one place -- the measuring fallback in `scaled.rs`, whose gate was
`synth && zeroed && !tab && norm::is_mark(ch)`, i.e. "we are synthesising
mark positions ourselves, and this run's script is not one of the ten that
keep mark advances". A face that has a `GPOS` therefore never reached the
zeroing at all, and any mark that `GPOS` did not itself move was charged a
full advance.

HarfBuzz zeroes a mark's advance in **two independent passes**, on two
different grounds, and a mark only needs to be caught by one of them:

1. `zero_mark_widths_by_gdef` (`hb-ot-shape.cc`), gated on the shaper's
   `zero_width_marks` enum (`NONE` / `BY_GDEF_EARLY` / `BY_GDEF_LATE`; the
   default shaper is `BY_GDEF_LATE`). It reads `_hb_glyph_info_is_mark`,
   i.e. the *glyph props*, which come from `GDEF`'s `GlyphClassDef` -- or,
   when the face has no glyph classes, from `hb_synthesize_glyph_classes`,
   which calls a general-category `Mn` that is not default-ignorable a
   MARK. **This pass runs whether or not `GPOS` applies.**
2. `_hb_ot_shape_fallback_mark_position` -> `position_cluster` ->
   `zero_mark_advances`, which reads the *character's* general category
   (`== HB_UNICODE_GENERAL_CATEGORY_NON_SPACING_MARK`) and runs only when
   the fallback positioner does, i.e. `!apply_gpos && shaper->fallback_position`.

Ours modelled only the second. The witness is `DejaVuMathTeXGyre.ttf`,
which has `GSUB` and `GPOS` but no `GDEF` whatsoever: on
`ที่นี่` HarfBuzz returns advances
`[364, 0, 0, 364, 0, 0]` -- it synthesises classes, finds the four Thai
`Mn` characters, and zeroes them even though they all draw as `.notdef` --
while we returned six full 364-unit boxes. 29 faces on that string.

**Fixed.** `MarkPositioning::classifies()` reports whether the face's
`GDEF` classifies glyphs at all (`self.class_def.is_some()`), surfaced as
`Face::classifies_glyphs()`; `scaled.rs` hoists
`let by_category = !self.face.classifies_glyphs();` out of the piece loop,
which is HarfBuzz's `fallback_glyph_classes`, exactly
`!hb_ot_layout_has_glyph_classes(face)`. The `SubGlyph.mark` gate then
takes the **union** of the two grounds:

```rust
mark: !tab
    && norm::is_mark(ch)
    && ((synth && placeable) || (by_category && zeroed)),
```

`synth && placeable` is pass 2 -- it runs exactly when the fallback runs.
`by_category && zeroed` is pass 1 restricted to the faces where we can
stand in for `GDEF`, and switched off for the ten scripts of
`fallback::zeroes_mark_advances`. Neither is gated on the combining class,
which is an ordering, not a width, and leaves plenty of marks at 0.

Taking the union matters: replacing pass 2 with pass 1 rather than unioning
them passed all 383 unit tests and clippy while regressing the sweep from
`agree` 11724 to 11240 and `misplaced` 124 to 608 -- 253 faces broke on
each Hebrew string, because a face *with* a `GDEF` that declines to
position its marks still needs the fallback's zeroing.

Sweep: `agree` 11724 -> 11777, `misplaced` 124 -> 71, `differ` unchanged at
940. Test
`scaled::a_face_that_classifies_its_glyphs_decides_which_of_them_take_room`,
which needed a fixture that can carry a `GDEF` and a `GPOS` at once:
`build_test_font_with_layout(scripts, classes)` is now the general form and
`build_test_font_with_gpos_scripts` / `build_test_font_with_gdef_classes`
are its two one-sided callers.

**Still open.** `hb_synthesize_glyph_classes` excludes default-ignorables
from MARK (U+034F CGJ, U+180B-180D, U+FE00-FE0F, U+E0100+); our
`by_category` arm does not, so a variation selector on a `GDEF`-less face
is zeroed where HarfBuzz would leave it. It draws nothing either way, so
the only observable difference is the advance, and no corpus string
exercises it -- deferred rather than guessed at.
