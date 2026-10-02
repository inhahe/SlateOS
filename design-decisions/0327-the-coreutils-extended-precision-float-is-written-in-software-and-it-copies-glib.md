## §327 — The coreutils' extended-precision float is written in software, and it copies glibc's rounding bug on purpose

**Date:** 2026-08-17
**Decided by:** Claude (autonomous)

**In short:** `seq` has to print numbers exactly the way GNU's `seq` prints
them, and GNU's does its arithmetic in a number format that is 80 bits wide —
wider than the 64-bit `double` that Rust calls `f64` and that this project had
been using. Two questions came out of that. First, where do the 80-bit numbers
come from: the CPU has hardware for them (the old x87 unit), or we write the
arithmetic ourselves. Second, in one corner — numbers so small they are almost
zero — the GNU C library rounds them slightly *wrong*, and we can either match
its wrong answer or print the right one. We wrote the arithmetic ourselves, and
we reproduce the wrong answer.

**Terms.** *`long double`* on x86-64 Linux is the 80-bit "extended precision"
format: 64 bits of significand where `f64` has 53, so it holds about 19 decimal
digits instead of 16. *x87* is the 1980s floating-point unit still present in
every x86 CPU, and the only hardware that speaks that format. *Subnormal*
numbers are the ones too small for the format's normal representation, which
trade away significand bits to stretch a little closer to zero. *Double
rounding* is rounding twice — once to an intermediate width, once to the final
one — which can land a bit away from where rounding once would have.

**Where the 80-bit arithmetic comes from**

- **A — inline x87 assembly.** The hardware does it; a handful of `fld`/`fadd`
  instructions replace a few hundred lines.
  *What changes:* nothing visible, when it works. Rejected for three reasons.
  The x87 rounding mode lives in a control word that is process-global state
  the rest of the program also owns; the unit is deprecated on the target and
  is not required to be usable in kernel-adjacent contexts; and the code could
  not be unit-tested on any host that is not x86.
- **B — keep `f64` and accept the difference.**
  *What changes:* `seq 0 0.1 1` prints a line that reads `0.30000000000000004`
  where GNU prints `0.3`, and long ranges drift. Rejected: this is not a corner
  case, it is the first example in `seq`'s own manual.
- **C — a software 80-bit float (`coreutils::extfloat`).** ~1200 lines: a
  128-bit-integer significand, `strtold`, and `printf`'s eight float
  conversions.
  *What changes:* nothing observable versus A, but it is portable, testable off
  x86, and touches no global CPU state. **Chosen.**

The cost of C is that it must be *proved*, not believed, which is what
`scripts/extfloat-diff.sh` is: 12278 generated cases run through both this
module and a C program compiled against glibc, compared byte for byte in two
directions (what `strtold` read, and what `printf` wrote). It reports zero
differences.

**Whether to copy glibc's subnormal rounding**

When `strtold` reads a decimal literal that lands in the subnormal range, glibc
rounds it twice — to the full 64-bit significand first, then down to the fewer
bits a subnormal actually has — and so returns a value one unit away from the
correctly-rounded one for some inputs. Our first version rounded once, which is
the mathematically right answer.

- **A — round correctly.** *What changes:* for a handful of inputs near
  10⁻⁴⁹³², our `seq` prints a number one bit different from GNU's — and the
  differential harness has to carry a skip list saying so.
- **B — reproduce the double rounding.** *What changes:* nothing a user sees;
  the harness's skip list disappears. **Chosen.**

The reason is what the skip list does to the test, not what the rounding does
to the number. A harness with an exception in it stops measuring the cases the
exception covers — and those were the hardest cases in the file, the ones the
module exists to get right. Waving them through to protect a rounding
improvement that no `seq` output can distinguish trades the whole subnormal
range's coverage for nothing. Being bug-compatible with the reference is also
what "certified against glibc" has to mean if it is to mean anything: a module
that is deliberately better than its reference in one place cannot be checked
against it anywhere.

**If this is revisited:** the correctly-rounded path is a two-line change in
`extfloat.rs`'s `strtold` (round once at the subnormal width instead of twice),
and the trigger would be a caller that is *not* trying to match glibc — a
`printf` for our own shell, say, or a numeric type for the calculator. At that
point the honest shape is two entry points with different names, not a flag,
so that each caller's harness measures the thing that caller claims.
