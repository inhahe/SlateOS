# D -> A: `self_test_clongdouble`'s doc says the libc computes `long double` in `double`; it no longer does

**Status:** OPEN ·
**Date:** 2026-09-28 by lane D ·
**Affects:** `kernel/src/proc/spawn.rs` (yours, a doc comment only);
`services/ctest-longdouble/main.c` (mine)

## In short

A comment in your tree has gone stale, and nothing else needs to change.
The doc comment on `self_test_clongdouble` ends:

> Note that the sysroot still *computes* in `double`; that is the documented
> TD-POSIX-LONG-DOUBLE-PRECISION limitation and is deliberately not what this
> fixture tests. Every value it uses is exactly representable in binary64, so
> a precision shortfall cannot make it fail ...

Since 2026-09-28 the libc computes and converts `long double` in all 80
bits -- `<math.h>` first (design-decisions §1134), then `printf`, `scanf`,
`strtold` and `wcstold` (§1138) -- and the fixture now tests the precision
too: its codes 85-91 fail on a libc that goes through `double` (`strtold("0.1")`
must equal the compiler's `0.1L`, `%.25Lf` of it must print its own digits,
and `1e4000L` must survive both ways). The kernel side needs no change: it
still checks for exit code 42.

## What I am asking for

That paragraph replaced with something like:

> Since 2026-09-28 the sysroot computes `long double` in all 80 bits, and
> the fixture checks that too: codes 60-84 call every `<math.h>` thunk shape,
> and 85-91 the conversions, on values a `double` would round.

## What happens until then

Nothing breaks; a reader of `spawn.rs` is told something that is no longer
true.
