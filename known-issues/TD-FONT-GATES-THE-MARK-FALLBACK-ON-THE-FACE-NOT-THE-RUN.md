## TD-FONT-GATES-THE-MARK-FALLBACK-ON-THE-FACE-NOT-THE-RUN

**What.** `ScaledFont::shape` decides whether marks have to be placed by
measurement with one boolean for the whole call:

```rust
let synthesize = !self.face.has_positioning();
```

That asks a question about the *file* — is there a `GPOS` table — when the
question that decides the answer is about the *run*: does this run's script
reach any of that `GPOS`. A face can carry a full `GPOS` for Arabic and Latin
and nothing at all for Hebrew, and today every Hebrew mark in it is left
exactly where `hmtx` put it: full width, no displacement, stacked nowhere.

**The evidence.** Amiri-Bold shaping the corpus's pointed-Hebrew string
`U+05E9 U+05B8 U+05C1 U+05DC U+05D5 U+05B9 U+05DD` (`fontTools` says its `GPOS`
and `GSUB` ScriptLists are `DFLT`, `arab`, `latn` — no `hebr`; every one of the
seven characters maps to `.notdef`):

```
ours      [(364,0,0), (364,0,0), (364,0,0), (364,0,0), (364,0,0), (364,0,0), (364,0,0)]
harfbuzz  [(364,0,0), (0,-33,728), (364,0,0), (364,0,0), (0,16,-728), (0,66,728), (364,0,0)]
```

as `advance;dx;dy`. Our glyph order already matches HarfBuzz's exactly — that
was TD-FONT-DOES-NOT-RE-SORT-HEBREW-AND-ARABIC-MARKS, fixed. What is left is
that HarfBuzz zeroes and places the three points and we do neither, on a face
whose `GPOS` cannot possibly have told it where they go.

**Why HarfBuzz does that, exactly.** `hb_ot_shape_plan_t::init0` computes

```c
bool disable_gpos = plan.shaper->gpos_tag &&
                    plan.shaper->gpos_tag != plan.map.chosen_script[1];
```

and only the **Hebrew** shaper sets a `gpos_tag` (`HB_TAG('h','e','b','r')`,
added for harfbuzz#347). So a Hebrew run in a face whose `GPOS` ScriptList does
not name `hebr` gets `apply_gpos = false`, hence
`fallback_mark_positioning = true`, hence `_hb_ot_shape_fallback_mark_position`
— which classifies marks by Unicode general category and so works even on
`.notdef`, with no `GDEF` involved.

Measured on this host rather than taken from the source. Shaping one string per
script through `uharfbuzz` in Consolas — `GPOS` ScriptList `cyrl`, `grek`,
`latn`, with no `DFLT` — HarfBuzz applies the fallback to **Hebrew alone**:

| script | consola.ttf output | fallback ran |
|---|---|---|
| `hebr` | `(0,0,-1435)`, `(0,88,1435)`, `(0,1126,0,0)` | yes |
| `arab`, `deva`, `mymr`, `khmr`, `mong`, `syrc`, `thaa`, `ethi`, `thai` | every glyph `(1126,0,0)` | no |

The negative half is the load-bearing half: it says the rule is a property of
the Hebrew *shaper*, not a general "no lookups for this script" rule. (Arabic
in Consolas reaches `latn` through HarfBuzz's last-ditch fallback and so keeps
`apply_gpos`.)

**Proper fix.** Make `synthesize` a per-run question and thread it through the
three places that consume it:

1. a predicate — the face has a `GPOS` *and*, if the run's script demands its
   own script table, the `GPOS` ScriptList names it;
2. `position_segments` skips a segment the predicate refuses, so a Hebrew
   segment in Amiri is not positioned by Arabic's lookups;
3. `SubGlyph::klass`/`::mark`, the `x_offset -= x_advance` shift, and the
   `synthesize_marks` call all read the per-run answer instead of the per-face
   one.

The per-glyph answer after substitution comes from the `Segment`s, not from the
pieces: a run that ligates is shorter than the pieces it came from.

**Size.** ~498 of the 625 `misplaced` cases the sweep still reports — the two
Hebrew corpus strings at 249 each.

**Where.** `gui/font/src/scaled.rs:492` (the gate), `:530` and `:541` (the two
`SubGlyph` fields), `:644` (the shift), `:723` (the `synthesize_marks` call),
`:867` (the `GPOS` early-out).

**Related, not fixed here.** HarfBuzz also falls back to the legacy `kern`
table when it disables `GPOS` this way (`plan.apply_kern` when
`!has_gpos_kern || !plan.apply_gpos`). Our `Face::kerns_outside_gpos` is a
per-face answer too, so a Hebrew run in a `GPOS`-kerned face that also ships a
`kern` table stays unkerned where HarfBuzz kerns it. Narrow — it needs a face
with both tables, `hebr` in neither's ScriptList, and Hebrew kern pairs in the
legacy one — and left open deliberately rather than overlooked.

**Fixed** (2026-08-14). `ScaledFont::applies_gpos` is the new gate and it takes
a script, not a face: the face must carry a `GPOS` *and* the run's script must
accept it, where `fallback::demands_own_gpos_script` is the whole of the second
condition and Hebrew is the whole of that. `position_segments` skips a segment
it refuses, so a Hebrew segment in Amiri is no longer offered Arabic's lookups;
the piece loop gates `SubGlyph::klass` and `::mark` on the per-run answer; and
the per-glyph answer the offset shift and `synthesize_marks` need is rebuilt
from the `Segment`s after substitution, since a stretch that ligated is shorter
than the pieces it came from.

Measured over the host's 556 faces x 23 strings, before and after:

| | agree | reordered | misplaced | differ |
|---|---|---|---|---|
| before | 11223 | 0 | 625 | 940 |
| after | **11709** | 0 | **139** | 940 |

The two pointed-Hebrew strings went 249 -> 6 each; **every other string in the
report is byte-for-byte identical**, which is the claim that matters — the gate
was widened for Hebrew and for nothing else, and the baseline was re-measured
from a stash of this very change rather than quoted from memory.

Three tests, in the three places the claim lives:
`fallback::only_hebrew_demands_a_gpos_registered_under_its_own_name` (the
negative half is the load-bearing half: another script here would refuse the
`DFLT` `GPOS` of most faces on the host),
`sfnt::a_face_names_the_gpos_scripts_its_script_list_registers` (with the tags
deliberately unsorted in the file, since the lookup binary-searches), and
`scaled::a_hebrew_run_refuses_a_gpos_written_for_another_script` together with
`a_latin_run_takes_whatever_gpos_the_face_offers` — the second exists so the
first cannot be passed by a gate that simply always falls back. The fixture
grew `build_test_font_with_gpos_scripts`, a `GPOS` with a real ScriptList and
an empty FeatureList and LookupList, so a test cannot pass by positioning
something.

**Residual, not this entry.** Six faces per Hebrew string still disagree, and
it is a different disagreement: LATINWD.TTF puts our point 68 units to the
right of HarfBuzz's at the same height (`(682, 1493)` vs `(614, 1493)`), which
is the fallback's *horizontal* centring rather than whether it ran. Filed
separately once diagnosed — it was not the centring at all, see
`TD-FONT-DRAWS-A-GLYPH-AT-ITS-STORED-XMIN-NOT-ITS-STATED-BEARING` below.
