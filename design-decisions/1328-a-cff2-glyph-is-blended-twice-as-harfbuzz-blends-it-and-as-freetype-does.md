## 1328. A `CFF2` glyph is blended twice, as HarfBuzz blends it and as FreeType does

**Date:** 2026-09-26
**Lane:** F
**Decided by:** Claude (autonomous) -- within §448 and §1325, which settled
that drawing and measuring follow HarfBuzz and hinting follows FreeType.

**In short:** a variable font with PostScript-style outlines (a `CFF2` table)
stores each point as a default plus adjustments for each design direction,
and the program drawing the glyph mixes them for the weight asked for. The
two libraries this crate follows mix them with slightly different
arithmetic, so the crate now does it both ways: HarfBuzz's for the shape it
draws and the boxes it measures, FreeType's for the points its hinter
adjusts. Fonts of this kind would not open at all before.

**Decision.**

* **One interpreter, three instances.** `cff::Instance` says how a glyph's
  `blend`s are weighed: `Default` (every blend its default), `HarfBuzz`
  (`f32` region scalars at the `F2Dot14` coordinates, deltas summed in `f64`
  and then added to the default, as `cff2_cs_interp_env_t::blend_deltas`
  does) or `FreeType` (the blend vector `cff_blend_build_vector` builds from
  `FT_DivFix`ed factors multiplied by `FT_MulFix`, at the 16.16 coordinates,
  each delta `FT_MulFix`ed into a wrapping 32-bit sum, as `cf2_doBlend` does).
  `Face::outline_at` and the box use the first two; the hinter's points the
  first and the third.
* **Two readings of the store.** HarfBuzz's region scalars come from
  `varstore::VarStore`, as for `HVAR`; FreeType's from a reading of its own
  CFF loader (`cff_vstore_load`), since FreeType's CFF driver weighs regions
  with different arithmetic and refuses different things from its TrueType
  one.
* **Malformed blends as each library has them.** A `vsindex` after a
  `blend` fails the glyph in both; a `vsindex` or region the store lacks, or
  coordinates for another number of axes, fails it in FreeType and weighs
  nothing in HarfBuzz; an explicit `return` or `endchar`, which `CFF2`
  removed, is ignored, as FreeType ignores it.
* **Boxes in charstring units.** HarfBuzz reads no `FontMatrix`, and
  measures a CFF glyph in `double`; the crate's CFF boxes -- CFF1's too --
  now do the same (`Cff::bounds`), while the drawn outline keeps applying the
  matrix, as FreeType does.

**Alternatives.**

| | For | Against |
|---|---|---|
| Both readings (chosen) | each consumer matches its library to the bit | two arithmetic paths in one interpreter |
| HarfBuzz's for everything | one path | the hinter's points a 65536th off FreeType's, enough to move a hinted stem (§1325) |
| FreeType's for everything | one path | shaping and mark placement a unit off HarfBuzz's on some glyphs |

**How to reverse.** `Face::outline_at`, `cff_extents` and `tagged_outline_at`
choose the instance; passing another there changes whose arithmetic applies.
