### BUG-POSIX-STRTOF-DOUBLE-ROUNDING. `strtof` and `scanf`'s `%f` rounded twice, so some inputs landed on the wrong `float` — 2026-07-30 — ✅ **RESOLVED 2026-07-30**

**Where:** `posix/src/stdlib.rs::strtof` (`let f = d as f32;` over a `strtod`
result) and `posix/src/scanf.rs::scan_float` (`*p = val as f32`).

**What it was:** both produced an `f32` by rounding the decimal to `f64` and
then narrowing. Two roundings are not one. A value slightly above the midpoint
between two `float`s can be close enough to that midpoint that rounding to
`f64` lands *exactly* on it — and once it is exactly on the midpoint,
ties-to-even sends it to the even neighbour, which is the wrong side.

```
strtof("1.000000059604644830901776231257827021181583404541015625")
  ours (before): 1.0f        bits 0x3f800000
  correct:       1.00000012f bits 0x3f800001
```

The input is above the midpoint between `1.0f` and its successor, so it must
round up; via `f64` it rounds down. A second, cruder consequence: `strtof`
inherited `strtod`'s `ERANGE` decisions, which are about the `f64` range, and
then patched up the `f32` cases afterwards by inspecting the narrowed result.

**Fix (DONE).** `decfloat`'s rounder is now parameterised by a `Format`
(significand width, subnormal floor, exponent bias, infinity encoding), with
`F64_FORMAT` and `F32_FORMAT` instances, so `decimal_to_f32` rounds straight
from the exact decimal expansion — one rounding, in the destination's own
precision. `DigitCollector` gained `to_f32` alongside `to_f64`.

`strtod` and `strtof` were split so they share one scanner
(`stdlib.rs::scan_float_token`, which handles whitespace, sign, `inf`/`nan`
and the digits) and differ only in which conversion they finish with; `ERANGE`
therefore comes from the right range for each. `scan_float` likewise picks
`to_f32` or `to_f64` by length modifier rather than converting once and
casting. `inf`/`nan` still narrow by cast, which is exact.

**Verification:** the counterexample above is pinned as a test in all three
places (`decfloat`, `strtof`, `scanf %f`), each asserting *both* that the
`f64`-then-narrow route gives the wrong answer and that the direct route gives
the right one. Plus `f32` range tests (`FLT_MAX`, the least subnormal, half of
it, and values in `f64` range but out of `f32` range) and two 4000-value
sweeps against Rust's `str::parse::<f32>()`. 20 081 posix tests pass; clippy
clean.
