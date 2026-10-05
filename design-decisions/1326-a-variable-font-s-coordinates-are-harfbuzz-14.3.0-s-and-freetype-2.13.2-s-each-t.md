## 1326. A variable font's coordinates are HarfBuzz 14.3.0's and FreeType 2.13.2's, each to the bit, malformed tables included

**Date:** 2026-09-26
**Lane:** F
**Decided by:** Claude (autonomous) -- within §448, which settled that the
number an instance is chosen with is HarfBuzz's, and §1325, which put
FreeType's beside it for the hinter.

**In short:** asking a variable font for "weight 700" means turning 700 into
the font's own scale, where every table is indexed, and that conversion
rounds at several steps. HarfBuzz (which this crate's text layout follows)
and FreeType (which its hinting follows) round at different steps, and the
crate now reproduces each exactly, including the newer `avar` table format
and fonts whose tables break the rules. Until now it followed an older
HarfBuzz and was one step in 16384 off today's on one request in eight --
Bold among them -- which moved an occasional glyph edge, mark or kern by a
font unit.

**Decision.**

* **The HarfBuzz reading is HarfBuzz 14.3.0's pipeline** -- the version the
  crate's oracle (uharfbuzz 0.56) runs -- step by step: the value normalized
  in `f32`, rounded to 16.16, `avar`'s curve applied in `f32` by
  `SegmentMaps::map_float` and rounded again, version 2's store read at
  `roundf(c / 4)` and added at 16.16, then `(c + 2) >> 2`. Every rounding is
  HarfBuzz's own `roundf`, `floorf(x + 0.5f)`, which sends a half up
  (`hbcalc.rs`); the same rule now rounds `HVAR`, `MVAR` and `GDEF` deltas
  and the varied glyph extents the mark fallback stacks by, where the crate
  rounded halves away from zero or truncated. Store sums are unfused, a
  region scores 0 at the default before its malformations are looked at, an
  index map ignores its reserved bits, and `avar` version 2's rows go through
  HarfBuzz's 2^-30 scalar cache (`varstore.rs`).
* **The FreeType reading is `ft_var_to_normalized`'s**, in 16.16 integers,
  with FreeType's own loaders for `avar` version 2's store and axis map
  (`FtItemStore`), and the design coordinates FreeType is handed as the
  `Fixed`s they are: an axis nobody names sits at the file's exact default,
  a named instance at its record's exact values.
* **Malformed tables get each library's own answer, not a sane one.** Where
  HarfBuzz and FreeType disagree about damage -- an `avar` declaring another
  axis count (HarfBuzz pairs curves with axes by position; FreeType ignores
  the table), an unsorted or repeated curve (HarfBuzz's `CoreText`-compatible
  rules; FreeType's first-segment-past walk), an axis whose default is
  outside its range (HarfBuzz widens the range; FreeType pins it) -- each
  reading does what its library does. Before, the crate discarded a
  mismatched `avar` and treated an unsorted curve as the identity, for
  both, as the "honest reading".
* **Checked against the libraries, not against a reading of their source.**
  `tools/gen_var_fixture.py` builds faces that reach every rule (fontTools,
  with bytes written by hand where fontTools refuses) and records what
  uharfbuzz and freetype-py answer at 588 instances; `tools/var_oracle.py`
  asks both about thousands of instances of real faces (2,598 across 13, all
  agreeing); and the host tests' tables for named instances, `HVAR` advances
  and `MVAR` corrections now come from uharfbuzz itself instead of an
  independent Python transcription -- which had shared the crate's old
  rounding and so hidden it.

**Why follow each library even where it is arguably wrong.** The two
readings exist only so that this crate's answers can be compared, unit for
unit, with the libraries every other desktop's text comes from; a "better"
answer to a malformed table is a disagreement no comparison can tell from a
bug (§448's reasoning). A font whose tables break the rules draws the way it
draws under HarfBuzz and FreeType, which is what its designer saw.

**Not reproduced**, deliberately: HarfBuzz keeps the `HVAR` scalar cache for
the life of a font and the `GDEF` one for a shaping call, so which glyph
reads a region first -- and gets its scalar unrounded -- depends on what was
shaped before; there is no one answer to reproduce, and the two differ by at
most 2^-31 of a scalar. An `F2Dot14` coordinate past what an `i16` holds --
which only a curve mapping beyond ±2 reaches -- is clamped where HarfBuzz
keeps an `int`. A `VarStore` whose region list names a different axis count
from `fvar` is refused (HarfBuzz reads it with its own count).

**Alternatives.**

| | For | Against |
|---|---|---|
| Keep HarfBuzz 8's pipeline | no change | disagrees with the oracle on one instance in eight, Bold among them |
| One normalization for both | simpler | FreeType and HarfBuzz differ below 1/65536, enough to round a hinted stem the other way (§1325) |
| Sane answers for malformed tables | defensible in isolation | indistinguishable from bugs in every differential test |

**How to reverse.** `Variations::hb_coords` and `ft_coords` are the two
pipelines; `hbcalc::roundf` is the rounding. Reverting the commit restores
the old pipeline; the fixture would then report exactly which instances move.
