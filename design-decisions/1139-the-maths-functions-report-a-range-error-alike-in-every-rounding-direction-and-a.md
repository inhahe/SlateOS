## 1139. The maths functions report a range error alike in every rounding direction, and answer an overflow or underflow as the direction rounds

**Date:** 2026-09-28
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** a C program can make the processor round every calculation up,
down or toward zero instead of to the nearest value (`fesetround`). The maths
functions did badly when it did: the sine of pi came out a hundred thousand
times too large, `exp` answered 0 where rounding up owes the smallest positive
number, and `acosh(1)` was "minus zero". Those are bugs, fixed without a
choice to make. What this entry records is smaller. When a result is too large
or too small for the type ("out of range"), a function reports it in `errno`,
the C error code -- but in the rounding modes other than to-nearest, C leaves
to the library whether and when, and glibc is inconsistent. This library
reports it when the answer to nearest would be out of range *or* the answer
actually returned is: `exp(1000)` reports it whether it returns infinity
(rounding up) or the largest finite number (rounding down), and no function
returns an infinity or a zero from ordinary arguments without reporting it.

**The rule.** `ERANGE` when either (a) the round-to-nearest result is out of
range by glibc's rule for the function -- an infinity from finite arguments, a
zero where the function is not zero -- or (b) the result returned is, by the
same rule. (a) makes the error independent of the direction wherever it can
be; (b) covers the answers that are out of range only as rounded:
`DBL_MAX + 1` rounded upward is an infinity, and `5e-324 * 0.9` rounded
downward zero. `posix/src/math.rs`, `ranged`.

**Alternatives:**

- **glibc's behaviour, exactly.** It follows from how each glibc function is
  written: its `exp` rounding downward sets `ERANGE` for `DBL_MAX` past
  |x| = 1024 but not between 709.79 and 1024; its `ldexp` never does for
  `DBL_MAX`; its `tgamma(-190.5)` rounding downward answers the least
  subnormal and no error, while to nearest the same call answers -0 and
  `ERANGE`. Copying that means copying each function's thresholds, and keeps
  answers like "`exp(800)` rounding downward is `DBL_MAX`, no error".
  Rejected.
- **(b) alone** -- glibc's wrapper templates, and this library before: then
  `exp(1e10)` rounding downward is `DBL_MAX` with no error, though the exact
  value is 10^4342944 times larger. Rejected.
- **(a) alone** -- the simplest rule to state, `errno` a function of the
  arguments only -- but `fdim(DBL_MAX, -1)` rounding upward returns an
  infinity with no error, being `DBL_MAX` to nearest. An infinity from finite
  arguments without an error is what a rule here most needs to rule out.
  Rejected.

**Cost.** Nothing to nearest: a result is judged only when it is at an end of
the range, by comparisons the functions made before. In a directed mode, such
a result is computed a second time, to nearest, for (a); an overflow or
underflow found that way is then answered by one multiplication that
overflows or underflows in the caller's direction, which gives exactly the
value IEEE 754 prescribes for it (`DBL_MAX * DBL_MAX`, `2^-600 * 2^-600`).

**Where glibc is not IEEE's.** The replay of glibc's directed modes found
three kinds of answer IEEE 754 makes otherwise, and the test
(`directed_answer`) expects IEEE's: `atan2(5e-324, 2)` rounding upward is 0,
where the least subnormal is owed; `pow(5e-324, 1)` rounding downward is 0
with `ERANGE` and `powf(FLT_MAX, 1)` rounding upward an infinity, where
`x^1` is `x`; and `remainderf(3, 1)` rounding downward is -0, where a zero
remainder has `x`'s sign.

**Not chosen here.** The sine, cosine, tangent and Bessel functions now
answer in every direction what they answer to nearest -- musl's accuracy,
within an ulp, but not rounded in the direction asked for. glibc's do the
same. CORE-MATH's correctly rounded `sin`, `cos` and `tan` would honour the
direction exactly; that is a step of its own (it would replace §1132's
musl for those functions), not part of this fix.

**The `long double` functions** (`mathl.rs`) follow the same rule, in the
x87 unit's rounding direction: `ranged` is generic over the type, and its
second evaluation switches both units to nearest
(`fenv::in_nearest_x87`).
