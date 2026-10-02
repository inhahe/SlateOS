## TD-FONT-DOES-NOT-READ-VARIATION-STORES

**Status: FIXED 2026-08-17 — all four steps landed.** `fvar`, `avar`, `gvar`,
the instance on `ScaledFont`, `HVAR` + `MVAR` (commit `94748f2c0`), and now the
`GDEF` store reached from a `GPOS` `VariationIndex` (commit `2ee86901d`). A
variable face draws at the weight you ask for, is spaced at that weight, *and
kerns and stacks its marks at that weight*. Only `CFF2` remains out of scope,
deliberately and for want of a face to check against — see below.

**What landed for the last third.** `device.rs` gained `Corrections<'a>`
alongside `Ppem`: `Ppem` is the pixel size alone and answers device tables,
`Corrections` adds a borrow of `GDEF`'s store plus the normalized coordinates
and answers both kinds. `Face::ppem` became `Face::corrections`; `gpos`, `kern`
and `mark` thread the new type through. `Face` parses `GDEF`'s Offset32 store.
See design-decisions §451 for why it is two types rather than one widened one.

**Three real bugs came out of proving it, and two were not about variations at
all.** None was reachable before the oracle could be asked at a non-default
instance, which is why they had survived a 556-face sweep:

* **`PairPos` format 1 measured its device offsets from the subtable instead of
  from the `PairSet`** (commit `8a70341eb`). The spec measures a value record's
  device offsets from its *immediate parent*, and format 1's parent is the
  `PairSet` where format 2's is the subtable. Pre-existing, and it mis-read
  plain device tables just as happily — it stayed invisible only because no host
  face had an in-range device table on a format 1 record, so the wrong base
  always landed somewhere that decoded to nothing.
* **`avar` interpolated with truncating integer division** where HarfBuzz rounds
  half away from zero (commit `777a040ff`) — and the code comment asserting
  HarfBuzz truncates was simply wrong. One F2Dot14 unit, but the normalized
  coordinate is the input to *every* delta the face computes, so at Segoe UI
  Variable's wght=550 every advance in the face came out a unit short. Reaches
  `gvar` outlines and `HVAR`/`MVAR` metrics too, not just this entry's arm.
* **Fallback mark positioning measured the default instance's ink box** (commit
  `1f542b478`). `gvar` moves a glyph's points without rewriting the bbox in its
  header, and the format has no per-instance box, so the stored one describes a
  shape the face no longer draws. `Face::glyph_bbox_at` measures the varied
  outline instead — which is *looser* off the default (the control-point hull),
  which is also what HarfBuzz produces there.

**Measured.** HarfBuzz over the host's 7 variable faces x 95 strings, at the
default instance and at wght=700, wght=300, wght=550,wdth=75,
opsz=27.5,wght=600 and opsz=5 — identical every time: agree 663, reordered 0,
misplaced 1, differ 0, mixed 1. The one misplaced is the pre-existing
`a\u034fb` CGJ divergence, present at the default instance too; the mixed is the
documented itemizer difference. Full-host static sweep unchanged at its recorded
baseline (556 faces x 95 strings, agree 51423, misplaced 170, reordered 0,
differ 1178). Pinned by `a_variation_index_kern_follows_the_instance` in
`gui/font/tests/host_fonts.rs`, whose expectations were computed independently
from fontTools' region scalars plus a raw byte parse rather than snapshotted.

**Still out of scope:**

* **`CFF2`.** A `CFF2` face varies through the charstring interpreter, not
  through `gvar`. No face on this host has one, so it is neither implemented
  nor checkable against a real file. `Face::outline_at` falls back to the
  default outline for a non-`glyf` face rather than pretending. *(Read since
  2026-09-26, checked against real `CFF2` fonts downloaded for the purpose:
  see "[F] A variable font whose outlines are CFF (`CFF2`) would not open".)*

Six things learned doing the `HVAR`/`MVAR` two thirds of step 4, recorded
because every one of them bears on the `GDEF` third:

* **What the two new host tests actually measured.**
  `installed_variable_fonts_vary_their_advances` reads 8 sampled glyphs on each
  of the 7 variable faces at the default instance and at every named instance,
  against a table pinned from `gui/font/tools/varstore_oracle.py`: **7 faces
  read through `HVAR`, 5 of them varying.**
  `installed_variable_fonts_vary_their_global_metrics` does the same for
  `hasc`/`hdsc`/`hlgp`: **4 faces read through `MVAR`, all 4 varying.** The two
  faces that read but do not move are Cascadia Code and Cascadia Mono, whose
  advances are constant by construction — a monospace face has one width. That
  is why both tests carry a floor (`>= 5`, `>= 4`) rather than "some face
  moved": the null implementation passes the equality half.
* **A null `HVAR` advance-mapping offset means the *implicit* map, not
  "no variation".** Outer index 0, inner index = the glyph id. Reading it as
  absent silently spaces the whole face at its default width — and on this host
  the only two faces that take that path are exactly the two whose advances do
  not vary, so **no host font can catch the mistake**. It is caught by
  `a_null_advance_map_means_outer_zero_and_inner_equals_the_glyph_id` in
  `varstore.rs`'s test module, on a synthetic store.
* **A delta row is not fixed-width, and getting that wrong misreads every
  row after the first column.** `wordDeltaCount`'s low 15 bits give the number
  of leading *wide* columns; bit 15 (`LONG_WORDS`, 0x8000) doubles both widths,
  so "wide" is `i16` or `i32` and "narrow" is `i8` or `i16` depending on one
  flag. Both widths and the flag are exercised synthetically; host fonts here
  use only the narrow-`i8` case.
* **A region axis with `peak == 0` scores 1, not 0.** It means "this region
  does not constrain that axis", so it must contribute the identity to the
  product. Scoring it zero makes every multi-axis face's deltas vanish, which
  looks exactly like "the font does not vary" and is the single easiest way to
  write a reader that passes an equality test against nothing.
* **Degenerate regions are unreachable from any installed font.** `start > peak`,
  `peak > end`, or a span straddling zero with a non-zero peak all score 1.0
  following HarfBuzz; measured **0 of 199** region axes on this host, so the
  synthetic fixture is the only thing that reaches the branch. The oracle
  deliberately implements the plain spec reading instead and documents the
  divergence, so the two can never agree by transcription.
* **Cap-height and x-height are deliberately *not* taken from `MVAR`'s
  `cpht`/`xhgt`,** even though 4 host faces carry them. They are measured off
  the varied outline, which is ground truth and is what the non-variable path
  already does; reading the tag as well would make two things the source of
  truth for one number. `Face::metrics_at` therefore applies only
  `hasc`/`hdsc`/`hlgp`. Same reasoning as the advance: `gvar`'s phantom points
  also know the advance, and `HVAR` is the one that is read.

Four things learned doing steps 2-3, recorded because they bear on step 4:

* **No installed face moves a phantom point.** All 7 express their advance and
  side-bearing variation in `HVAR` instead, so in 400 glyphs per face not one
  `gvar` phantom delta is non-zero. The phantom side-bearing correction in
  `Face::outline_at` is therefore **untestable from host fonts** — flipping its
  sign left all 25 host tests passing — and is covered only by the synthetic
  variable font in `sfnt.rs`'s test module (`build_variable_test_font`). Same
  for IUP with a visible effect, and for a composite's component offset, which
  no host face varies at all. Any future variation work should expect to
  extend that fixture rather than to find a real file that exercises the path.
* **The oracle has to be split in two.** `gui/font/tools/gvar_oracle.py` is
  written from the specification rather than transcribed from `gvar.rs`, but
  its `glyf` reader and its point→path conversion cannot be independent of
  ours in any useful sense. Those are validated by a *separate* table taken at
  the **default instance**, where every tuple's scalar is provably zero, so a
  failure says which of the two kinds of bug it is.
* **A comparison needs a "did it move" assertion.** Agreement between two
  readers is satisfiable by a reader that ignores `gvar` and an oracle that
  agrees with it about nothing. `installed_variable_fonts_vary_their_outlines`
  asserts each non-default row *differs* from the unvaried glyph (51 of 63 do)
  and fails if fewer than three quarters move.
* **Wiring is a different failure from reading.** Every outline-level test
  passed with `ScaledFont` still calling `outline` instead of `outline_at`.
  What catches it is `a_weight_axis_reaches_the_rasterized_glyph`, which
  rasterizes 'H' at the heaviest weight on all 7 host faces and demands more
  ink than at the default.

Three things learned doing step 1 (`fvar`/`avar`), recorded because they change
what the remaining step has to handle:

* **This host has 7 variable faces out of 556**, and they use *two* of the five
  optional variation tables in any quantity: all 7 carry `gvar` + `HVAR` +
  `STAT`, 6 carry `avar` and a `GDEF` `ItemVariationStore`, 5 carry `MVAR`.
  **`CFF2` and `VVAR` are on no face here at all**, so those two cannot be
  checked against a real file and want a synthesized oracle (the Thai
  private-use precedent, design-decisions §432) rather than a sweep that would
  report agreement it never tested. Counted by
  `gui/font/tools/variable_survey.py`.
* **Two host faces have a degenerate axis** — ReemKufi ships
  `wght 400..400..700` and bahnschrift `wdth 75..100..100`, so one side has zero
  width and the normalization formula as written divides by it. Not a fuzzer
  case; installed on this machine.
* **Endpoint tests cannot see whether `avar` was read.** The format requires
  `avar` to fix the three identity points, so min/default/max normalize to
  -1/0/+1 with or without it and the entire correction is interior. The test
  that does see it compares all 82 named instances of the 7 faces against
  `variable_survey.py --normalize`, an independent implementation; verified by
  deleting the `avar` argument at the call site, which fails that test and
  nothing else.

**What.** `gui/font/src/device.rs` reads `GPOS` device tables — the per-pixel
correction a face hangs off a value record's field or an anchor's axis — but
declines the *other* thing that can occupy that slot. A `Device` whose
`deltaFormat` is `0x8000` is not a device table at all: it is a
`VariationIndex`, whose first four bytes are a `deltaSetOuterIndex` and
`deltaSetInnerIndex` into a variable font's `ItemVariationStore`. Reading one
needs the store (an `ItemVariationStore` in `GDEF`, plus `fvar`/`avar` to turn
the caller's axis settings into normalized coordinates), which this crate does
not parse. `Ppem::delta` recognises the format and returns zero.

**Symptom.** A variable font drawn at a non-default instance positions its
marks and its kerns at the *default* instance's values.

Until 2026-08-16 this was invisible in isolation, because the outlines were the
default instance's too — the whole face rendered at its default weight, and
positioning was consistent with that. **Step 3 removed that cover.** `gvar` is
now read, so the shapes move and the marks do not: at the heavy end of a weight
axis an accent sits where the light glyph's anchor was, which is a visible
misregistration rather than a uniformly-wrong-but-coherent page. That makes this
entry *more* urgent than it was when it was filed, not less. It is recorded as
its own entry because it is the one place where the missing feature is silently
*skipped inside code that is otherwise doing its job*, rather than absent at the
top level.

**Measured.** `gui/font/tools/device_survey.py` over this host's fonts: 9,215
`VariationIndex` records across the font directory, against 152 real device
tables. Variation indices outnumber real device tables 60 to 1 here, so the
un-read case is by far the common one — nearly all of them in
`anchor:base4/variation` (8,720), i.e. mark-to-base attachment.

**Why it was filed rather than fixed.** It is not a device-table change; it is
variable-font support, and the device-table slot is the last piece of it rather
than the first. In order: `fvar` (what axes exist and their defaults), `avar`
(the caller's axis value mapped onto the normalized -1..1 scale), `gvar`/`CFF2`
(the outlines, which is the part a user actually sees), then
`HVAR`/`MVAR`/`GDEF`'s `ItemVariationStore` (advances, metrics and *these*
deltas). Reading the store first would have applied a positioning correction for
an instance whose outlines were the default instance's — worse than applying
none, because the marks would move off the shapes they belong to.

**That precondition is now met.** Steps 1-3 landed on 2026-08-16, so the
ordering no longer argues for declining the arm; it argues for finishing it.

**Proper fix.** Steps 1-3 are done: `fvar`/`avar` parse into a normalized
coordinate vector that lives on `ScaledFont` (not on `Face` — the same parsed
face must be usable at two instances at once), and `gvar` applies it to the
outlines. What remains is the second half of step 4:

1. ~~**An `ItemVariationStore` reader**~~ — **done, commit `94748f2c0`.**
   `gui/font/src/varstore.rs` is the shared format behind `HVAR` (advances, so
   a varied glyph is spaced at its varied width), `MVAR` (the `OS/2`/`hhea`
   metrics), and the `GDEF` store *this* entry indexes into. One reader serves
   all three; they differ only in how the outer/inner index pair is arrived at
   (`HVAR` through a `DeltaSetIndexMap` keyed by glyph id, `MVAR` through a
   value-tag record, `GDEF` from the `VariationIndex`'s own four bytes). The
   first two are wired; `VarStore::delta` takes the pair directly and is
   already what the third needs.
2. ~~**Coordinates at the `Ppem` boundary**~~ — **done, commit `2ee86901d`.**
   `Ppem` got the sibling described below, `Corrections<'a>`, carrying the
   normalized coordinates and a borrow of the store; `Corrections::delta`
   dispatches on `deltaFormat` and computes the `0x8000` arm. The decline and
   the "What is not here" paragraph are gone from `device.rs`'s module doc,
   replaced by "The other thing that can occupy the slot".

   Both things this said to get right were right, and were the two that
   mattered: the font-units asymmetry is exactly as described below, and
   `Face` does parse the Offset32 itself. The one thing the plan did not
   anticipate is that reading the arm at all would expose three unrelated
   bugs — see the status block at the top.

   Two things to get right in that piece, both found while planning it:

   * **A `VariationIndex` delta is in font design units, not pixels.** A real
     device table stores a *pixel* correction, which is why `Ppem::delta` runs
     its result through `to_font_units`. HarfBuzz's `VariationDevice::get_x_delta`
     instead scales the store's value by `em_scalef_x`, i.e. treats it as
     already being in design units. So the new arm must **bypass**
     `to_font_units` — and, consequently, it applies at `Ppem::NONE` too,
     where a device table correctly contributes nothing. A reader that routes
     both arms through the same conversion is wrong twice: wrong magnitude,
     and silently zero whenever the caller did not supply a ppem.
   * **`Face` must parse the `GDEF` store itself.** The offset is an Offset32
     at `GDEF` + 14 and exists only from version 1.3; earlier `GDEF`s stop
     before it, so reading it unconditionally reads whatever follows the
     header. 6 of this host's 7 variable faces carry one.

`CFF2` stays out of scope until a face on some host actually has one; see the
missing-pieces list at the top of this entry.

**Where it landed.** `gui/font/src/device.rs` (`Corrections`, `variation_delta`,
the module doc), `gui/font/src/sfnt.rs` (`gdef_store`, `Face::corrections`,
`Face::glyph_bbox_at`), `gui/font/src/gpos.rs`, `gui/font/src/kern.rs`,
`gui/font/src/mark.rs`, `gui/font/src/scaled.rs`, `gui/font/src/var.rs`
(`round_div`), `gui/font/tests/host_fonts.rs`, and the oracle
(`gui/font/examples/shape_dump.rs`, `gui/font/tools/harfbuzz_sweep.py`, which
can now be asked at an instance with `--axes`).
