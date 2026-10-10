## D-POSIX-CONVERSIONS-IGNORE-THE-ROUNDING-MODE — `printf` and `strtod` always round to nearest; glibc's follow `fesetround` (lane D, 2026-09-28) — **Status: FIXED 2026-09-28 -- `printf`'s `%f %e %g %a` and their `%L` forms, `strtod`, `strtof`, `strtold`, `wcstod`, `wcstof`, `wcstold`, `scanf`, the `ecvt` family -- every line of glibc 2.39's in all four modes replayed (`posix/tools/oracle/conv_harness.py`); `long double` the same day as `double`, with its full precision (TD-POSIX-LONG-DOUBLE-PRECISION)**

**In short:** a program that changes the rounding direction with
`fesetround` -- to round up, say, for interval arithmetic -- gets glibc's
directed rounding from `printf` and `strtod` there, and not here: ours round
to nearest whatever the mode. `printf("%.1f", 0.25)` under `FE_UPWARD` prints
`0.3` on glibc and `0.2` here; `strtod("0.3")` under `FE_UPWARD` is
`0x3fd3333333333334` on glibc and `...333` here. Under the default mode, which
nearly every program keeps, the two agree. It became reachable on 2026-09-27,
when `fesetround` started working (`posix/src/fenv.rs`).

| Call, under the mode | glibc 2.39 | ours |
|---|---|---|
| `printf("%.1f", 0.25)`, `FE_UPWARD` | `0.3` | `0.2` |
| `printf("%.1f", -0.25)`, `FE_DOWNWARD` | `-0.3` | `-0.2` |
| `printf("%.0e", 25.0)`, `FE_UPWARD` | `3e+01` | `2e+01` |
| `printf("%.2a", 1 + 0x1p-12)`, `FE_UPWARD` | `0x1.01p+0` | `0x1.00p+0` |
| `strtod("0.3")`, `FE_UPWARD` | `0x3fd3333333333334` | `0x3fd3333333333333` |
| `strtof("0.3")`, `FE_DOWNWARD` | `0x3e999999` | `0x3e99999a` |

**Where:** `posix/src/decfloat.rs` -- `Decimal::round_to_significant`
(printf's `%f`/`%e`/`%g`), `round_to_binary` (`strtod`, `strtof`, `wcstod`,
`scanf`) -- and `printf.rs`'s `%a` rounding: each rounds ties-to-even and
never reads the mode.

**Proper fix:** read `fegetround()` once per conversion and round the exact
expansion (which decfloat already has, so every case is decidable) in that
direction: toward +inf rounds a positive value's magnitude up when anything
nonzero is dropped, toward -inf a negative one's, toward zero never. Due with
the 80-bit conversions (TD-POSIX-LONG-DOUBLE-PRECISION), which go through the
same code.
