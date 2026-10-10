## D-POSIX-LGAMMA-LOSES-DIGITS-NEAR-NEGATIVE-ROOTS — `lgamma`, `lgammaf` and `lgammal` are only absolutely accurate where the gamma function is +-1 below -2 (lane D, 2026-09-28) — **Status: FIXED 2026-09-28 -- `lgamma` and `lgammaf` CORE-MATH's, correctly rounded (`posix/src/lgamma.rs`); `lgammal` by an expansion about each zero in double-long-double (`posix/src/mathl.rs`), within 3 ulps where glibc's reaches 7**

**Status (2026-09-28):** `lgamma`, `lgammaf`, `lgamma_r`, `lgammaf_r`, `gamma` and `gammaf` are CORE-MATH's correctly rounded functions now (MIT, ported in `posix/src/lgamma.rs`), checked bit for bit against its C on every float and on 1.6 million hard and 2 billion random doubles in all four rounding directions; `math.rs`'s glibc replay no longer excuses them (`near_root`). The first row of the table below is 5.619192358950097e-17 now, 0.17 ulp from mpmath's value. `lgammal` followed the same day. CORE-MATH has no 80-bit `lgamma`, so `mathl.rs` does what the proper fix below describes, within 1/4 of each of the 58 zeros for n = 2..=30 (`lgammal_near_zero`): `lgamma(x) = A + B` about the zero, `A` the logarithm of a ratio of sines formed without subtracting them, `B` a Taylor series about `1 - x0`, both in double-long-double -- at the zero near -2.748 they still cancel by a factor of 2.2 -- with the zeros and coefficients from mpmath (`posix/tools/oracle/lgammal_zeros.py`). Against mpmath at 80 digits, on 3,776 points beside every zero and pole plus 28,000 across (-4, -2), the worst error is 3 ulps (on musl's side of the window, near -2.17) and 89% are exact; glibc's `lgammal` reaches 7 ulps on the same points. `mathl.rs`'s glibc replay compares `lgammal` relatively everywhere now.

**In short:** `lgamma(x)` is the logarithm of |gamma(x)|. Below -2 the
gamma function passes through 1 or -1 twice in every unit interval, so
`lgamma` is 0 there and tiny near it -- and there our library's answers are
right only to about 1e-16 *absolutely*, not relatively. `lgamma(-2.4570247382208006)`
is 5.6191923589500965e-17 (glibc returns that); ours returns 1.1e-16, twice
it. Nothing crashes and no other argument is affected; a program sees it only
if it takes `lgamma` of a negative number near one of those points and relies
on the digits of the near-zero result.

**Where:** `posix/src/math.rs` (`lgamma`, `lgammaf`, `lgamma_r`, `lgammaf_r`,
`gamma`, `gammaf`, from musl's `e_lgamma_r.c` via the vendored `libm`) and
`posix/src/mathl.rs` (`lgammal_core`, musl's ld80 `lgammal.c`). All three use
the reflection formula, `lgamma(x) = log(pi / |x sin(pi x)|) - lgamma(-x)`,
which subtracts two numbers near 1 to get one near 0.

| x | true value (mpmath) | glibc 2.39 | ours (musl) |
|---|---|---|---|
| -2.4570247382208006 | 5.6191923589500965e-17 | 5.6191923589500967e-17 | 1.1102230246251565e-16 |
| -2.457024738220801 | -6.1687121408846648e-16 | -6.1687121408846647e-16 | -7.2164496600635175e-16 |
| -3.1435808883499798 | 1.6978655906121084e-15 | 1.6978655906121083e-15 | 1.7763568394002505e-15 |

`lgammal` loses less, relatively (5.6568e-17 for glibc's 5.6521e-17 at the
long double nearest the first root), for the same reason.

**The tests allow it:** `math.rs`'s oracle replay compares `lgamma` by
absolute error when glibc's result is under 1 (`near_root`), and `mathl.rs`'s
does the same for `lgammal`. Both allowances go when this is fixed.

**Proper fix:** evaluate near each root from an expansion about the root, as
glibc's `lgamma_neg.c` does -- but not by translating it: glibc is LGPL, and
this library is linked statically into every program (design-decisions §1133's
licence note). The mathematics is public: tabulate each root `x_k` below -2
where |gamma| = 1, to twice the working precision (`x_k = hi + lo`), and the
Taylor coefficients of `log|gamma|` about it (`psi(x_k)`, `psi'(x_k)/2`, ...),
both computed offline with mpmath; near `x_k` answer the polynomial in
`(x - hi) - lo`. Only finitely many roots matter -- the k-th lies about
`1/k!` from a negative integer, and once that is under the spacing of the
numbers there, no argument can land near it: about 16 integers deep for
double, 20 for long double. For float, CORE-MATH's correctly rounded
`lgammaf` (MIT) is a ready alternative.
