## 1135. `ecvt` gives the value's own digits, not glibc's

**Date:** 2026-09-28
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** `ecvt`, `fcvt` and `gcvt` are old functions that turn a number
into a string of digits (C programs from before `printf("%e")` was trusted
use them; POSIX dropped them in 2008, musl and glibc still have them). They
were missing here. They now exist with glibc's conventions -- how many digits,
where the decimal point is reported, what zero, infinity and a rounding carry
look like -- but with the number's true digits. glibc's `ecvt` gets the last
digit or two wrong about one call in six, because it first scales the number
into [1, 10) by repeated multiplication by ten in floating point, and every
multiplication rounds. This library does not copy that.

**The measurement.** 1,743 `ecvt` calls replayed against glibc 2.39
(`posix/tools/oracle/cvt_harness.py`): 261 of glibc's answers differ from
the value's correctly rounded digits or its decimal point -- e.g.
`ecvt(99.5, 2)` is "99" (99.5 is exactly half way, and rounds to even: 100,
written "100" with `decpt` 3 by glibc's carry convention below),
`ecvt(0.95, 1)` is "10" with `decpt` 1 where 0.95 (really
0.9499999999999999556) is "9" with `decpt` 0, and `ecvt(1e23, 0)` puts the
point at 24 where the value, 99999999999999991611392, has 23 digits.
A 200,000-call random sample put the rate at 10% to 15% depending on the
digit count. `fcvt` with `ndigit >= 0` is `printf("%.*f")` in glibc too and
matches on every row; with a negative `ndigit` it first divides by ten in
floating point, the same way, and 34 of its 332 such rows are off (1e23 to
the ten thousands, `ndigit` -4, is "1" and 23 zeros, where the value gives
9999999999999999161 and four zeros, `decpt` 23). `gcvt` is
`printf("%.*g")` and matches on every row.

**What is kept of glibc:** `NDIGIT_MAX` 17 (at most 17 digits from `ecvt`,
17 fraction digits from `fcvt`); `ecvt` of an `ndigit` of 0 or less is no
digits, with `decpt` where the value's point is (1 for zero); zero is
`ndigit` zeros with `decpt` 1; the infinities
and NaNs come back as `inf`/`-inf`/`nan` text with `decpt` 0 and `sign` 0; a
rounding that carries into a new leading digit is written with one digit
more (`ecvt(9.9999, 1)` is "10", `decpt` 2); `fcvt` of a nonzero value below
1 strips its `0.` and the zeros after it (0.00123 is "123", `decpt` -2) --
all of it, to "" with `decpt -ndigit`, when the value rounds to zero; and a
negative `ndigit` rounds left of the point but, as glibc's loop does, never
so far the value would drop below 1 (`fcvt(5, -2)` is "5"). The tests replay
every one of glibc's rows where its digits are exact, byte for byte, and hold
the rest to the exact digits from Rust's own formatter.

**Alternatives:**

- **Copy glibc's scaling loop**, and so its answers, bit for bit. What the
  replay would most naturally reward, and deterministic (the same IEEE
  operations give the same roundings). Rejected: the answers it produces are
  wrong -- a caller asking for the digits of 99.5 gets digits of a number
  that is not 99.5 -- and nothing depends on those particular wrong digits;
  and it would mean translating glibc's code, which the licence argument of
  §1133 rules out anyway.
- **musl's versions** (`sprintf("%.*e")` and read the digits back). Exact,
  like this, but with musl's conventions, which differ from glibc's -- at
  most 15 digits from `ecvt`, no carry digit -- where glibc's are the ones
  the tests and ported programs expect.
