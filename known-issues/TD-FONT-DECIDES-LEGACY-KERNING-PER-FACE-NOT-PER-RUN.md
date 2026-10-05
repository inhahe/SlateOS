## TD-FONT-DECIDES-LEGACY-KERNING-PER-FACE-NOT-PER-RUN

Filed 2026-08-14, lane C. Fixed the same day. Closes the residual left open
by `TD-FONT-GATES-THE-MARK-FALLBACK-ON-THE-FACE-NOT-THE-RUN`
("legacy-`kern` fallback when `GPOS` is disabled per run").

A face may carry kerning in `GPOS`'s `kern` feature, in the legacy `kern`
table, or in both. When it carries both, `GPOS` wins -- the positioning
pass has already applied those lookups in order and in the company of every
other lookup, and charging them a second time would double every kern. We
made that call **once per face**: `Kerning::parse` kept whichever source won
and discarded the other, and the shaper asked `Face::kerns_outside_gpos()`,
which is `true` only when `GPOS` carries no `kern` feature *anywhere in the
table*.

That is the wrong granularity. `GPOS` files its features under particular
scripts, so "does `GPOS` kern this text" is a question about the **run**.
`LEELAWAD.TTF` (Leelawadee) is the witness: its `GPOS` ScriptList is
`['thai']` and nothing else, its FeatureList is `kern`/`mark`/`mkmk`, and it
also ships a legacy `kern` table with 1422 pairs. A Latin run in that face
therefore reaches no `GPOS` feature at all -- the fallback chain tries
`latn`, `DFLT`, `dflt` and finds none of them -- while our face-wide test
saw the `thai` `kern` feature, concluded `GPOS` kerns this face, and left
the legacy table unread. Every Latin pair in Leelawadee went unkerned.

HarfBuzz asks per plan, and the plan has a script (`hb-ot-shape.cc`):

```c
  if (!apply_kerx && (!has_gpos_kern || !apply_gpos))
  {
    if (hb_aat_layout_has_positioning (face)) apply_kerx = true;
    else if (hb_ot_layout_has_kerning (face)) apply_kern = true;
  }
```

`has_gpos_kern` is `map.get_feature_index (1, HB_TAG('k','e','r','n'))`,
looked up in the **selected** script. On `ete` in Leelawadee it comes out
false, so `hb_kern_machine_t` runs the legacy table: pair `(t, eacute)` is
-16, split `kern1 = kern >> 1 = -8` onto the left glyph's advance and
`kern2 = kern - kern1 = -8` onto both the right glyph's advance and its
x-offset. Our whole-kern-on-the-left distribution lands every glyph's ink in
the same place and totals the same width, so the sweep's `places` agree; it
was the missing -16 that showed.

**Fixed.**

* `Kerning` keeps both sources side by side (`gpos: Vec<Group>`,
  `legacy: Vec<usize>`) instead of one and a `Kind` discriminant. `pair` --
  the pair-at-a-time public API, which has no run behind it -- still prefers
  `GPOS` face-wide, so `Face::kern`/`kern_across` are unchanged.
  `legacy_pair` is the new run-aware reader.
* `Positioning::kerns(script)` answers HarfBuzz's `has_gpos_kern`, through
  the same `ByScript::for_script` fallback chain the lookups themselves use,
  by testing the `kern` bit of the mask `for_script` already returns.
* `Face::has_legacy_kern()` and `Face::gpos_kerns(script)` are the two halves
  the shaper composes; `Face::legacy_kern_across` reads the legacy table.
* `scaled.rs` fills a per-glyph `legacy_at` from the segments, exactly as it
  already fills `synth_at`: a segment kerns from the legacy table when the
  face has one and either the positioning pass skipped the segment outright
  or the segment's script reaches no `GPOS` `kern`. So the Latin and Thai
  halves of one mixed line can differ, which is the point.

Sweep: `agree` 11777 -> 11806, `misplaced` 71 -> 42. `probe.py` on
`LEELAWAD.TTF` with `ete` now reports "no difference". Tests
`gpos::a_kern_feature_belongs_only_to_the_scripts_that_name_it` (which
needed `gpos_table_for`, the script/feature-parameterised form of the test
table builder) and `kern::a_face_with_both_keeps_both`.

**Still open.** HarfBuzz's `hb_kern_machine_t` sets
`OT::LookupFlag::IgnoreMarks` on its context, so the legacy table kerns `A`
and `V` across an intervening accent. Ours treats the legacy table as
flagless and so refuses any pair with anything between it -- which is the
historically correct reading (the engines that used the table kerned
strictly adjacent glyphs) but is not what HarfBuzz does. No corpus string
puts a mark inside a legacy-kerned pair, so it is not currently visible in
the sweep. Changing it is a separate, separately-measurable step.
