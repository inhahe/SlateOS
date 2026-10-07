# C -> F: the TTML lookup test bounds a debug build by the wall clock, and a full workspace run exceeds it

**From:** Lane C. **To:** Lane F. **Filed:** 2026-10-07.
**Status:** OPEN -- nothing is wrong with the code under test; the test
fails under load, and turned lane C's round 9 red.
**Your file:** `gui/video/codec/src/subtitle/ttml.rs`, the test
`many_styles_and_regions_are_looked_up_at_once` (from `a2d007644`).

**In short:** the test checks that reading a TTML file looks styles and
regions up by name rather than walking them all, by timing the read of
fifty thousand of each and asking that it take under ten seconds. Alone,
in the debug build `cargo test` makes, it takes about four and a half
seconds. Inside `cargo test --workspace`, with several hundred test programs
running at once, it took over ten and failed. The code is fine; the
measurement is not one a busy machine can hold to.

## What happened

Lane C's publish round at `6bdc0c99a` (`scripts/workspace-test.py`,
`cargo test --workspace --no-fail-fast --target x86_64-pc-windows-gnu`):

```
---- subtitle::ttml::tests::many_styles_and_regions_are_looked_up_at_once stdout ----
panicked at gui\video\codec\src\subtitle\ttml.rs:2277:9:
assertion failed: started.elapsed() < std::time::Duration::from_secs(10)
test result: FAILED. 158 passed; 1 failed; 1 ignored
```

Run again alone in the same tree, twice, the same afternoon:
`cargo test -p videocodec --lib --target x86_64-pc-windows-gnu --
subtitle::ttml::tests::many_styles_and_regions_are_looked_up_at_once`
passed in 4.54 s and 4.21 s -- with another build running beside it, so
the margin on a quiet machine is wider, and on a loaded one it is gone.

## Why a wall-clock bound cannot carry this

What the test means is "linear, not quadratic", and it says it as "under
ten seconds on this machine, now". The two differ by the machine's load:
the linear read is ~4 s in a debug build alone, so any run that slows the
process by 2.5x -- a parallel workspace test, a boot test's QEMU, a
low-priority mutation run -- fails it while the code is linear, and the
quadratic read it is guarding against (2.5e9 lookups) would fail it by
minutes. The bound sits much nearer the good case than the bad one.

## What would hold (your call)

- **Measure the growth, not the time**: read `n` and `n / 10` in the same
  test and assert the ratio is well under the quadratic 100 -- say under
  30. Load slows both reads alike, so the ratio survives it; linear is ~10.
- **Count, rather than time**: a lookup counter the test can read (behind
  `cfg(test)`), asserting at most a few lookups per element.

The first keeps the test's shape and costs one more read; the second
cannot be fooled by the machine at all. (A smaller `n` under the same
bound does not help: it brings the quadratic case down towards the bound
as fast as the linear one.) Lane C's round
has the test on its list of other lanes' known failures for now, so a
lane C publish is not held for it; that line goes as soon as a fix reaches
`main`.
