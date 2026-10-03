## §448 — A variable font's chosen instance belongs to the scaled font, and the number it is chosen with is HarfBuzz's

**Date:** 2026-08-16
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Some fonts are adjustable — one file can draw at any weight from
thin to bold, and at any width, rather than shipping one file per weight. The
file describes the *dials* (weight from 200 to 700, say) and the app turns
them. Two questions had to be settled before writing any of it: **where the
turned-dial setting is stored**, and **exactly what number a given dial
position turns into** — because that number gets used by four other tables, and
being off by one in the last digit shows up as a glyph that is subtly the wrong
shape.

Both answers were forced by things already true of this crate, so neither is a
close call — but both are easy to get wrong later by someone who does not know
why, which is why they are written down.

### The setting lives on the scaled font, not the parsed face

`Face` is the parsed file. `ScaledFont` is that file at a size, for drawing.
The instance ("weight 600") goes on `ScaledFont`.

- *What changes:* one loaded file can be drawn at two weights at the same
  time — a document with bold and regular text loads the font once.
- **The alternative** — putting it on `Face` — means a bold run and a regular
  run of the same family are two parsed copies of the same megabyte of data,
  or one copy mutated between draws, which is a data race waiting to happen in
  the compositor. Neither is acceptable, and the second is worse than it looks
  because the mutation is invisible at the call site.
- **Cost of the choice:** every function that reads a variation-dependent
  table needs the coordinates passed to it, so signatures downstream get one
  more argument. That is real but it is the honest shape: those functions
  genuinely do depend on the instance, and a version that reads it off the
  face is hiding a dependency rather than removing one.

`Face::variation_axes()` therefore returns the *axis description* — what dials
exist and their ranges — and never a position along them.

### The dial position is converted with HarfBuzz's arithmetic, bit for bit

Turning "weight 600" into the internal number is two steps: a proportion
(600 is two-thirds of the way from 400 to 700), then an optional per-font
correction curve the designer supplies (`avar`, which lets them say "the
two-thirds point should really be three-fifths"). The curve is stored as fixed
point — integers over 16384 — and interpolating along it involves a division.

**Decision: land on the number HarfBuzz lands on, whatever rounding that
takes, rather than on the number the arithmetic looks like it should give.**

- *What changes:* nothing a user can see directly. What is at stake is whether
  this crate's number can differ from the oracle's by one 16384th.
- **Why the oracle and not accuracy:** HarfBuzz is this crate's oracle
  everywhere else — shaping, kerning, mark placement are all checked against
  it by `gui/font/tools/harfbuzz_sweep.py`. A one-unit divergence here does
  not stay here: it feeds `gvar`'s outline deltas and `GPOS`'s positioning
  deltas, where it becomes a sub-pixel disagreement in a *glyph shape*, at
  which point no sweep can tell "we round better" from "we have a bug".
- **Which rounding that turns out to be, per operation** — the two are not the
  same and reading one off the other is exactly the mistake made here:
  - `avar`'s segment map **rounds to nearest, halves away from zero**.
    HarfBuzz's `SegmentMaps::map` interpolates in `float` and takes `roundf`.
    Done here in integers, which reaches the same answer without a float's
    own rounding.
  - Reaching fixed point from a user-space float is also half-away-from-zero,
    for the same `roundf`.
  - Device-table pixel-to-font-unit conversion, by contrast, **truncates**
    (§440): `Device::get_delta` does `pixels * (int64_t)scale / ppem` in C
    integers.

**Correction, 2026-08-17 (commit `777a040ff`).** This entry originally read
"divide the way HarfBuzz divides, **including truncating toward zero**", and
`SegmentMap::map` truncated to match. That was wrong: the truncation belongs
to `Device::get_delta`, a different operation, and was generalized to `avar`
without checking. The cost was not a rounding curiosity — the normalized
coordinate is the input to *every* delta the face computes, so Segoe UI
Variable at weight 550 (`8192 * 8192 / 10923` = 6143.99, truncated to 6143
where HarfBuzz has 6144) had every `gvar` outline delta and every
`HVAR`/`MVAR` metric scaled by 0.99995 of what it should be, at that instance
and no other. The *decision* above did not change; only the belief about what
it required. The lesson kept here: pinning to an oracle means reading the
oracle for each operation, not extrapolating a rule from a neighbouring one.

**Consequence for testing.** Because the arithmetic is pinned to another
implementation, "it matches the spec" is not the property under test —
"it matches, unit for unit" is. So the host-font test compares all 82 named
instances of this machine's 7 variable faces against
`variable_survey.py --normalize`, a second implementation written from the
specification in a different language, *including* its rounding. A
disagreement is then a bug rather than a rounding preference. Verified by
injection: changing the rounding to truncation is caught at one unit
(9829 against 9830).

**But that host test did not catch the truncation bug** — a second oracle only
helps if it was written independently, and this one inherited the same wrong
belief from this entry (it truncated too, until the same day). Even had it
not, a named instance almost always sits *on* a segment endpoint, where the
interpolation is exact and every rounding agrees: re-running the survey after
correcting it produced byte-identical output for all 82 instances. What
catches it is `avar_rounds_to_the_nearest_coordinate_rather_than_truncating`
in `var.rs`, an arithmetic unit test at a deliberately fractional point. Two
implementations of the same misunderstanding are one implementation.

### A malformed variation table costs variability, not drawability

`Variations::parse` returns `Option`, not `Result`, and `Face::parse` drops the
`None` on the floor.

- *What changes:* a font whose `fvar` is corrupt still opens and still draws,
  at its default weight, instead of failing to open.
- The `avar` case is the sharper one: an `avar` that disagrees with `fvar`
  about how many axes there are is discarded **whole**, not per axis. Pairing
  correction curve *k* with axis *k* across a count mismatch silently applies
  the weight correction to the width — a wrong answer that still looks like a
  font, which is the failure mode this crate spends most of its effort
  avoiding.
- An `avar` curve whose points are out of order becomes the identity **for
  that axis only**, since the identity is exactly what the table's absence
  would have meant and the other axes' curves are still readable.
