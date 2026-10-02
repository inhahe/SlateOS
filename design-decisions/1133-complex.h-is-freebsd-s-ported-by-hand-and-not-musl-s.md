## 1133. `<complex.h>` is FreeBSD's, ported by hand, and not musl's

**Date:** 2026-09-28
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** C's complex-number functions (`cabs`, `cexp`, `csqrt`, the
inverse sines and cosines ...) did not exist in the C library, so a program
using them did not link. They now do, translated into Rust from FreeBSD's
maths library, not from musl's -- although musl is where the rest of the maths
came from (§1132) -- because several of musl's are the schoolbook formulas and
give visibly wrong answers in places a program can easily land: `clog(0.6 +
0.8i)` has a real part of about 2.2e-17, and musl's formula answers 0;
`casin(1e300)` is pi/2 + 691.5i, and musl's -- which squares 1e300 on the
way, and overflows -- answers -pi/2 - infinity i. musl's own files mark both
`// FIXME`.

**Where they come from, function by function:**

| Functions | Source | Why |
|---|---|---|
| `casin`, `cacos`, `casinh`, `cacosh`, `catan`, `catanh` | FreeBSD `catrig.c`, `catrigf.c` (Montgomery-Smith, after Hull, Fairgrieve and Tang, ACM TOMS 1997) | accurate to 4 ulp everywhere, branch points included; musl's `casin` is `-i log(iz + sqrt(1 - z*z))`, which cancels near `z = +-1` and overflows once `z*z` does |
| `clog` | FreeBSD `s_clog.c` (Evans) | keeps `log|z|` accurate when `|z|` is near 1, by squaring exactly (Dekker) and using `log1p`; musl's is `log(cabs(z))` |
| `csqrt`, `cexp`, `ccosh`, `csinh`, `ctanh` and `ccos`, `csin`, `ctan` | FreeBSD `s_csqrt.c` ... `s_ctanh.c` | the same code musl has -- musl took these from FreeBSD |
| `cpow` | glibc's definition, `cexp(y * clog(x))`, the product by `__muldc3` | FreeBSD's `cpow` is a different formula (Moshier's); `cpow` magnifies any difference in `clog`, and glibc's answers are the ones the tests replay |
| `cabs`, `carg` | `hypot`, `atan2` -- the errno-setting ones, as glibc's are | |

**Where Annex G leaves a choice open, glibc's choice.** The standard fixes most
special values but leaves some signs unspecified, and there FreeBSD and glibc
differ. The replay found six places, and each now answers as glibc 2.39 does,
commented at the line: `cexp(-inf +- i inf|NaN)` gives the imaginary zero the
sign of the imaginary part; `ccosh(+-0 + i inf|NaN)`'s imaginary zero is +0, and
`ccosh(NaN +- i0)` keeps the argument's zero; `csinh(+-inf + i inf|NaN)`'s real
part is +inf; `casinh(NaN +- i inf)`'s infinity takes the NaN's sign; and
`cacosh(+-0 + i NaN)` is NaN + i pi/2 rather than NaN + i NaN. (`csin`, `ccos`,
`casin` and `cpow` inherit these.)

**`errno`, as glibc sets it:** FreeBSD sets none. glibc's complex functions
reach its errno-setting real ones in three places, reproduced exactly: `cabs`
and `carg`; `cexp` (and so `cpow`), `ERANGE` when `e^re` underflows to zero --
never for an overflow, which glibc's scaling turns into a plain infinite
product; and `catanh`/`catan` at +-1, `ERANGE` from the `log(0)` glibc's formula
takes on the way.

**The one difference kept:** where glibc's `csqrt` rounds twice and flushes a
result to zero that FreeBSD's rounds once to the smallest subnormal (`csqrt(0.5 -
5e-324 i)`'s imaginary part), FreeBSD's is the correctly rounded answer and
stays; the tests treat a zero against a same-signed nonzero as an accuracy
difference, within the tolerance, not as a special value.

**Alternatives:**

- **musl's `src/complex`, ported.** Consistent with §1132 and simpler (about
  half the code), and wrong in the places above by construction. Rejected
  because the errors are not rounding noise but lost digits and NaNs, and
  glibc -- the behaviour ported programs were written against -- has none of
  them.
- **glibc's own complex code.** As accurate, and the reference the tests use.
  Rejected on licence: glibc is LGPL, and this library is linked statically
  into every program on the system, which LGPL encumbers (the user must be
  able to relink); FreeBSD's and musl's are BSD/MIT.
- **Compile the C, rather than translate it.** Would keep the upstream text
  byte for byte, as §1132 vendored rust-lang's `libm` byte for byte. But no
  maintained Rust translation of these files exists to vendor, and compiling C
  into the `posix` crate would bring a C compiler into the libc build, which
  has none today.

**What makes a hand translation safe here:** the tests replay glibc 2.39 for
every function in both precisions over tens of thousands of calls -- every
pair of 26 special values per part, random points at every magnitude, and
points crowding the branch points and `|z| = 1` -- requiring Annex G's
special values (infinities, NaNs, signed zeros) to match glibc's exactly and
the rest to lie within a few ulp (`posix/src/complex.rs` tests; the harness is
`posix/tools/oracle/complex_harness.py`). A line mistranslated in a
branch shows up as a run of mismatches in the region the branch covers.

**Not done:** the `long double` functions (`cabsl` ...), which need 80-bit
arithmetic the library does not have yet
(`D-POSIX-MATH-HAS-NO-FENV-LONG-DOUBLE-OR-COMPLEX`).
