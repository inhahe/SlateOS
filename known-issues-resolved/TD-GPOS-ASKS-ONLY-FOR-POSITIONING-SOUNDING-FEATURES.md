## TD-GPOS-ASKS-ONLY-FOR-POSITIONING-SOUNDING-FEATURES -- FIXED

**Fixed.** `gpos.rs` asked the face for seven feature tags — `abvm`, `blwm`,
`curs`, `dist`, `kern`, `mark`, `mkmk` — on the reasoning that those are the
positioning features. They are the positioning-*sounding* features, which is
not the same thing. HarfBuzz builds **one** `hb_ot_map_t` from its common and
horizontal feature lists and compiles it against both `GSUB` and `GPOS`, so
every tag it turns on may reach a positioning lookup: there is nothing in the
format that stops a font filing a `PairPos` under `calt` or a `MarkBasePos`
under `ccmp`, and fonts do exactly that.

`Monoid-Italic.ttf` is the witness. Its `GPOS` has one feature, `calt`,
reaching one chained-context lookup with 25 subtables that invoke 25 SinglePos
lookups — the whole italic side-bearing correction, the thing that keeps `ic`
from colliding in a monospaced italic. Asking only for the seven tags meant we
read none of it, so `The quick brown fox` put the `c` of `quick` 64 units to
the right of where HarfBuzz puts it.

`FEATURES` is now HarfBuzz's unconditional set minus the vertical tags:
`abvm blwm calt ccmp clig curs dist kern liga locl mark mkmk rclt rlig`.
`KERN_FEATURE` moves from 4 to 7 with the reordering; the existing
`the_kern_bit_names_the_kern_feature` is what keeps that honest. Masks are
otherwise unused in `GPOS` — a positioning feature is gated by its own glyph
coverage, so unlike `GSUB` there is no per-glyph eligibility to enforce.

Covered by `a_substitution_tagged_feature_still_reaches_positioning_lookups`.
The HarfBuzz sweep goes `agree` 11820 -> 11826, `misplaced` 28 -> 22.

**Still open.** ~~The set is still missing HarfBuzz's `rvrn`, which only matters
for variable fonts~~ (in both lists since 2026-09-26, with `FeatureVariations`:
see "[F] Variable fonts' feature variations were ignored"), and the shaper-specific tags (`init`/`medi`/`fina` for
Arabic, the Indic reordering set) — those need the per-glyph masks `GSUB`
already has, and no installed face was observed to file a positioning lookup
under one.

**Mirrored in `GSUB`.** The same argument runs the other way — nothing stops a
face filing a substitution under `mark` or `kern` — so `gsub.rs`'s `FEATURES`
gained the same seven tags and the two lists are now the same set. `ALWAYS`
widens from 7 bits to 14 and the four positional bits move up with it;
`the_masks_match_the_feature_list` is what checks that. No face in the sweep
corpus exercises it (`agree` and `misplaced` are unchanged at 11826/22), so
this half is correctness by symmetry rather than a measured fix.
