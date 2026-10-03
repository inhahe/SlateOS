## 1138. `strtold` keeps every digit that can decide its rounding: on the stack while they fit, on the heap past that, and `ENOMEM` when the heap has none

**Date:** 2026-09-28
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** `strtold` turns text into a `long double`. To round correctly
it must sometimes look at thousands of digits: a number written out to 11,500
digits can sit so close to the half-way point between two `long double`s that
only its last digit decides which way it goes. glibc's answer is always right,
and this library's now is too. Holding those digits, and the arithmetic done
on them, takes up to about 21 KB. So the first 768 digits and a small
workspace live on the stack, exactly as for a `double`, and the rest is
borrowed from the heap -- only for text that needs it: more than 768
significant digits, or a value outside about `1e-2550` to `1e1800`. If the
heap has nothing to lend, `strtold` converts nothing and sets `errno` to
`ENOMEM` (out of memory), rather than return a value the text does not name.
glibc never borrows memory here, so it has no such case.

**The sizes.** A rounding boundary of the 80-bit format is, at its finest,
an odd multiple of `2^-16446` below `2^65` times it, so its decimal expansion
runs to 11,515 significant digits; an input that agrees with one that far is
decided by the digits after (`LD_PARSE_DIGITS`, 11,520). The exact integer the
rounding is done on is at most about 76,600 bits -- 1,200 limbs, 9.6 KB. For a
`double` the same bounds are 768 digits and 96 limbs, which is what the stack
holds, and what a `long double` of a few hundred digits and a moderate
exponent needs too.

**Alternatives:**

- **Everything on the stack.** glibc's `strtold` does this: a few kilobytes of
  fixed arrays, reading the digits back out of the string rather than storing
  them. This scanner stores them, because it also reads from streams
  (`scanf`) where there is no going back, so it would need about 21 KB in one
  frame -- more than a thread started with `PTHREAD_STACK_MIN` (16 KB) has,
  which would crash on the guard page. Rejected.
- **Keep 768 digits, fold the rest into a "something nonzero followed" bit**,
  the `double` path's rule, and never allocate. Right for nearly every input,
  but one within `10^-768` of a boundary can round the wrong way, silently --
  and those are the inputs conversion test suites are made of. Rejected:
  silent wrong answers.
- **Fall back to that rule when the allocation fails**, instead of `ENOMEM`.
  The answer is then right except in the same rare case, and no error is
  reported -- so a caller cannot tell a result that might be wrong from one
  that is right. Rejected for the same reason. Nothing converted
  (`*endptr == nptr`) with `ENOMEM` is an answer a careful caller can act on
  and a careless one reads as "not a number", which is safe.

**Printing has the same shape.** `printf("%Lf")` of a value within about a
`double`'s range formats on the stack; one far outside it (`%Lf` of
`LDBL_MAX` is 4,933 digits, `%.11600Le` of the least subnormal 11,600) takes
a block, and without one the call returns -1 with `errno` `ENOMEM` -- as
glibc's `printf` does when it cannot allocate its buffers.
