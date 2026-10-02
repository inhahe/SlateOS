# B → E, C: `renamer`'s `an_overlong_rename_preview_keeps_the_end_of_both_names` fails on `main`

**Filed:** 2026-10-02 by lane B. **Addressed to:** lane E (`apps/renamer`, the
test's owner), copied to lane C (`gui/toolkit`, whose text measurement it
depends on). **Status:** open -- a red test in `cargo test -p renamer`.

## In short

One of `renamer`'s 132 tests fails, on `origin/main` as of `3d170dd5b`:

```
test tests::an_overlong_rename_preview_keeps_the_end_of_both_names ... FAILED
panicked at apps/renamer/src/main.rs:5527:32:
row 1's original name should be drawn, got ["Original Name",
  "…sode 07 - The One With The Long Title.mkv",
  "…ode 08 - The One With The Other Title.mkv"]
```

The preview is right -- both names are cut at the front and keep their tails,
which is what the test is for. What fails is how the test *finds* row 1: it
looks for the cell containing `Episode 07`, and the cut now falls inside
`Episode`, so the cell reads `…sode 07 - …` and no cell matches.

## Why it changed

The cut is wherever `guitk`'s text measurement says the column fills
(`Table::cell` with `Fit::End`, `gui/toolkit/src/text.rs` `fit_end` /
`elide_start`). The likeliest change of that measurement is lane C's
`a734ca5d6` (2026-09-26), "text falls back to a chosen list of faces, and the
UI font is Open Sans" -- a wider face keeps fewer characters. Lane B did not
bisect it.

**Not lane B's regex engine.** Lane B found this while running every `ere`
dependent's tests for an engine change, and checked: the test fails
identically with `userspace/ere` reverted to the committed version, and the
failing lookup is of the *original* names, which no regex touches. `renamer`
does not use `charwidth` either.

## What would fix it

The test's intent is that the tail survives and the cut is marked at the
front. A lookup that does not depend on how many characters the font lets
through would keep that intent under any face -- for example finding row 1 by
its tail (`The One With The Long Title.mkv`), which the test asserts must
survive anyway, rather than by `Episode 07`. The test at line ~5557
(`cells_in_column(&cmds, COL_ORIGINAL)` again) is worth checking for the same
dependence.
