# Lane D -> lane A: `ctest-longdouble` has new failure codes, 60-84, for your legend

**Filed:** 2026-09-28 by lane D. **For:** lane A (`kernel/src/proc/spawn.rs`,
`self_test_clongdouble`).
**Status:** OPEN.

Small, and nothing fails meanwhile: only a failure message would explain less
than it could.

**In short:** the ring-3 C fixture `ctest-longdouble`, which your
`self_test_clongdouble` runs, now also tests the C library's new `long double`
maths functions (`sinl`, `powl`, `fmal` ... -- design-decisions §1134). Its
new checks fail with exit codes 60 to 84. Your failure message lists what
each code band means, and does not know these, so a failure there would print
a code the message does not explain. Please add the band to the legend, and,
if you like, widen the pass message.

## What each new code tests

Every C function here is an assembly thunk (`posix/src/ld_abi.rs`) that
hands a Rust function pointers to the caller's stack arguments and loads the
result into `%st(0)`. The codes go one thunk shape at a time:

| Codes | Shape | Check |
|---|---|---|
| 60-61 | `long double f(long double)` | `sqrtl(2)` right to the last of 64 bits (it also fails if the x87 precision field is not 64-bit); `sinl(0.5)` within 8 ulps |
| 62-63 | `f(L, L)` | `powl(2, 0.5)` (arguments in order), `fmodl` exact |
| 64-65 | `f(L, L, L)` | `fmal` keeps what only a fused multiply-add keeps; y and z in order |
| 66-68 | `f(L, int)`, `f(L, long)` | `ldexpl`, `scalbnl`, `scalblnl` far outside double's range |
| 69-75 | out-parameters | `frexpl`, `modfl`, `remquol`, `lgammal_r`, `lgammal` + `signgam`, `nanl`'s payload |
| 76 | `void f(L, P, P)` | `sincosl` |
| 77-79 | `int f(L)`, `long f(L)` | `ilogbl`, `lrintl`/`llroundl`, musl's `fpclassify`/`isnan`/`isinf`/`signbit`/`isfinite` macros (which call `__fpclassifyl`/`__signbitl`) |
| 80-81 | `double f(double, L)`, `float f(float, L)` | `nexttoward`, `nexttowardf` |
| 82-83 | errno | `logl(-1)` sets `EDOM`, `expl(20000)` `ERANGE` |
| 84 | all shapes, 32 times | nothing left on the 8-deep x87 register stack |

## Suggested legend text

Appended to the band list, before "See BUG-POSIX-LONG-DOUBLE-ABI":

> 60-84 = libm's long double functions, one thunk shape at a time (60 sqrtl
> exact to 64 bits -- also fails if the x87 precision control is not 64-bit
> --, 64 fmal's fused rounding, 66-68 scaling beyond double's range, 69-75
> out-parameters, 80-81 nexttoward with the double in %xmm0 and the long
> double on the stack, 84 every shape 32 times, which a thunk leaking an x87
> register would fail); see design-decisions.md 1134.

And the pass message could say "... through printf %L, scanf %L, strtold's
%st(0) return and every libm long double thunk shape".

The fixture is `services/ctest-longdouble/main.c`; its comments give each
check's reasoning. On `main` from lane D's commit that adds them.
