## D-POSIX-MATH-WAS-HAND-WRITTEN — the C library's maths was written here, and wrong in places no test looked (lane D, 2026-09-27) — **Status: FIXED 2026-09-27**

**In short:** `sin`, `exp`, `pow`, `sqrt`, `round` and the rest of `<math.h>`
were hand-written approximations -- "accurate to roughly 10-15 digits", their
documentation said -- and some were much worse than that. Every Rust program
on SlateOS used them too, since `f64::sin` and its kin compile to these
symbols there. They are now musl's libm through rust-lang's `libm` crate, with
glibc's `errno` (design-decisions §1132).

| Was | Example |
|---|---|
| `round` as `floor(x + 0.5)` | `round(0.49999999999999994)` = 1; `round(2^52 + 1)` = 2^52 + 2 (lane E's `requests/e-d-libc-round-is-wrong-just-below-a-half-and-past-2-52.md`) |
| `ceil`, `trunc`, `rint` lost a zero's sign | `ceil(-0.3)` = +0.0 |
| `fma` as `x*y + z`, two roundings | `f64::mul_add` was not fused |
| `sqrt` by Newton's method | not correctly rounded, which IEEE 754 requires |
| `sin`/`cos`/`tan` reduced by `fmod(x, 2*pi)` | `sin(1e22)` was noise |
| `exp` saturated at |x| = 709 | `exp(709.5)` was infinity; `exp(-710)` was 0, not a subnormal |
| `ldexp` truncated subnormal results, and misread subnormal arguments | `ldexp(5e-324, 1)` wrong |
| `lround`/`lrint` saturated | glibc and musl answer `LONG_MIN` out of range |
| no `errno` anywhere | `log(-1)` left `errno` alone; glibc sets `EDOM` |
| `nan(tag)` ignored the tag | glibc puts it in the payload |
| missing: `llrint`, `llrintf`, `__fpclassify`, `__fpclassifyf`, `__signbit`, `signgam`, `j0f`..`ynf`, `roundeven` | C programs using them did not link |

**Where:** `posix/src/math.rs` (the C ABI and glibc's `errno` rules),
`posix/vendor/libm` (the implementations, vendored as published).
**Tests:** `math::tests::every_answer_is_glibcs_or_within_its_error` replays
23,113 calls answered by glibc 2.39 under WSL (`posix/tools/oracle/math_harness.py`).
