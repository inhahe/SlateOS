### BUG-POSIX-STRTOD-INEXACT. `strtod` could not reach `DBL_MAX` or any subnormal, and drifted by several ulps everywhere else — 2026-07-30 — ✅ **RESOLVED 2026-07-30**

**Where:** `posix/src/stdlib.rs::strtod` (and therefore `atof`, `strtof`,
`wcstod`, and `scanf`'s `%f`/`%e`/`%g`, which all delegate to it); the fix
lives in `posix/src/decfloat.rs::decimal_to_f64`.

**What it was:** the parser accumulated the value in `f64` as it read —
`int_part = int_part * 10.0 + digit`, `frac_part += digit / divisor`, then
`result *= pow10(exp)`. Every one of those steps rounds, and the errors
compound, so the result was only ever *approximately* the nearest double.
Measured against Rust's (correctly rounded) `str::parse::<f64>()`, 7 of 12
probe inputs were wrong:

| input | ours (before) | correct |
|---|---|---|
| `1.7976931348623157e308` | `inf` | `DBL_MAX` |
| `5e-324` | `0` | `4.94065645841246544e-324` |
| `1e-310` | `0` | `9.99999999999996945e-311` |
| `2.2250738585072014e-308` | 3 ulps off | — |
| `8.98846567431158e307` | 3 ulps off | — |

The first two rows are the severe ones and they are structural, not
accuracy debt:

- **`DBL_MAX` parsed as infinity.** `int_part * pow10(292)` overflows on the
  way to a value that is itself finite. So the largest representable double
  was unreachable through the C API, and `strtod("1.79769313486231570815e308")`
  set `ERANGE` on a perfectly in-range input.
- **The whole subnormal range parsed as zero.** `pow10(-324)` is itself
  subnormal and has lost most of its precision, and dividing by it flushes
  everything below `~2.2e-308` to zero. Round-tripping any subnormal through
  `printf`/`strtod` therefore destroyed it.

**Fix (DONE).** `strtod` no longer does arithmetic while parsing. It collects
the significant digits into a fixed `[u8; 768]` with a decimal exponent, so
the parse produces the *exact* value `digits * 10^exp10`, then hands that to
`decfloat::decimal_to_f64`, which rounds once, correctly, ties to even:

    exp10 >= 0:  value = (D * 10^exp10) * 2^0
    exp10 <  0:  value = floor(D * 2^L / 5^Q) * 2^-(L+Q),  Q = -exp10

using the same fixed-capacity big integer as the printing direction (`L` is
chosen so the quotient keeps at least 64 bits, and a nonzero division
remainder feeds the sticky bit). 768 digits is the point past which no
further digit can change *which* double the input rounds to; beyond it the
digits can only decide a tie, so they are not stored — they just set the
sticky bit. Subnormals fall out of the same code path by clamping the binary
exponent at `-1074` and dropping more bits, so gradual underflow is rounded
rather than flushed.

`ERANGE` now comes from the converter, which reports overflow to infinity,
underflow to zero, *and* a subnormal result (gradual underflow) — matching
glibc. The old `pow10` helper in `stdlib.rs` had no other caller and is gone.

**Verification:** new tests pin `DBL_MAX`, five subnormals, both sides of the
half-way point to the least subnormal, the `2^53+1`/`2^53+3` ties, the
overflow boundary (`1.7976931348623158e308` is finite, `…9e308` is not), and
digit tails past the buffer. A sweep of 4000 pseudo-random bit patterns × 3
textual forms each is compared against Rust's parser, plus a second 4000-value
sweep directly against `decimal_to_f64`. All 20 069 posix tests pass; clippy
clean.
