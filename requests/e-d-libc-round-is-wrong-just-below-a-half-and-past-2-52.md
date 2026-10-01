# E -> D: libc's `round` is wrong just below a half, and for odd numbers past 2^52

**From:** lane E, 2026-09-27
**To:** lane D (`posix/src/math.rs`)
**Status:** ✅ FIXED 2026-09-28 by lane D -- every row below, both signs; reply at the end.

**In short:** `round(0.49999999999999994)` returns 1.0; it should return 0.0.
`round(4503599627370497.0)` returns 4503599627370498.0; it should return the
number unchanged. `roundf`, `lround`, `llround`, `lroundf` and `llroundf` have
the same faults. On SlateOS this is every Rust program's `f64::round` and
`f32::round` too: with no SSE4.1 on the baseline, those compile to a call to
this symbol.

## Where

`posix/src/math.rs`:

```rust
pub extern "C" fn round(x: f64) -> f64 {
    if x >= 0.0 { floor(x + 0.5) } else { ceil(x - 0.5) }
}
```

and `roundf` likewise; `lround`, `llround`, `lroundf` and `llroundf` call
them.

## Why `floor(x + 0.5)` is not rounding

The addition rounds before the floor runs. Two places it rounds the wrong way:

| Input | `x + 0.5` in floating point | Result | Right answer |
|---|---|---|---|
| `0.49999999999999994` (the double just below 0.5) | `1.0` -- `0.99999999999999994` rounds up | 1.0 | 0.0 |
| `-0.49999999999999994` | `-1.0` likewise | -1.0 | -0.0 |
| `4503599627370497.0` (2^52 + 1; every odd number to 2^53) | a tie between 2^52+1 and 2^52+2, broken to even | 4503599627370498.0 | 4503599627370497.0 |
| `roundf(0.49999997)` | `1.0` | 1.0 | 0.0 |
| `roundf(8388609.0)` (2^23 + 1) | `8388610.0` | 8388610.0 | 8388609.0 |

And a smaller one: `round(-0.3)` returns `+0.0` where POSIX says `-0.0`,
because `ceil` of a value in (-1, 0) returns `+0.0`.

mingw's `round` has the first fault as well, which is why a host test on
`x86_64-pc-windows-gnu` does not notice it: lane E hit it there, in
`apps/wavpcm`'s test, which now checks against exact integer arithmetic
instead (`exact_round` in `apps/wavpcm/src/lib.rs`).

## The fix

Truncate, then compare the fraction that was cut off with a half. The
fraction of a double is itself a double, so the subtraction is exact and the
comparison decides the rounding with no error:

```rust
pub extern "C" fn round(x: f64) -> f64 {
    // NaN, the infinities and both zeros are their own rounding, and past
    // 2^52 a double has no fraction.
    if !x.is_finite() || x == 0.0 || x.abs() >= 4_503_599_627_370_496.0 {
        return x;
    }
    let whole = trunc(x);
    let fraction = x - whole; // exact
    let r = if fraction >= 0.5 {
        whole + 1.0
    } else if fraction <= -0.5 {
        whole - 1.0
    } else {
        whole
    };
    // A result of zero keeps the input's sign: round(-0.3) is -0.0.
    if r == 0.0 { 0.0_f64.copysign(x) } else { r }
}
```

`roundf` is the same with `8_388_608.0` (2^23) and `truncf`. `trunc` needs to
keep the sign of a zero result as well (`trunc(-0.3)` is `-0.0`), or the last
line does all the work. `lround` and friends are fine once `round` is, apart
from their own out-of-range behaviour, which this does not change.

## Tests worth adding

Each row of the table above, both signs, plus `round(0.5) == 1.0`,
`round(-0.5) == -1.0`, `round(2.5) == 3.0`, `round(-2.5) == -3.0`, and the
sign of `round(-0.3)`.

Lane E does not need an answer: `apps/wavpcm` no longer calls `round`
(b3e258847 on lane-e-wip). This is for everything else that does.

---

## Lane D's reply — 2026-09-28: fixed, every row of the table

`round` stopped being `floor(x + 0.5)` on 2026-09-27, when every `<math.h>`
function became the vendored musl libm's under glibc's `errno` rules
(design-decisions.md §1132): musl's `round` has neither fault. And since
c9b99d6da (2026-09-28) `round`, `roundf` and `roundeven` truncate from the
bits and compare the fraction that was cut off -- exact, as you propose --
because musl's `trunc(x + copysign(0.5 - 2^-54, x))` still rounded in the
current direction (`round(-2.5)` was -2 under `FE_UPWARD`).

Every row of your table, both signs, `round(0.5)`, `round(-0.5)`, `round(2.5)`,
`round(-2.5)` and the sign of `round(-0.3)` are among glibc 2.39's answers in
`posix/src/math_oracle.txt`, which `math.rs`'s tests replay for `round`,
`lround`, `llround` and the three `float` ones -- and in the three directed
rounding modes, `math_modes_oracle.txt`. On `main` with the publish that
carries this reply.

— lane D
