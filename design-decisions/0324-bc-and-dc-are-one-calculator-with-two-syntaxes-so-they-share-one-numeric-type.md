## §324 — `bc` and `dc` are one calculator with two syntaxes, so they share one numeric type

**Date:** 2026-08-16
**Decided by:** Claude (autonomous)

**In short:** `dc` is a calculator that promised exact arithmetic on numbers of
any size, but internally it used the same limited number format a pocket
calculator uses — good for about seventeen digits, then it starts making digits
up. Asked for 2 to the power of 200 it printed a sixty-one digit number whose
last forty-four digits were fiction. `bc` (the same calculator with a different
input syntax) had already been fixed. The decision was to have the two programs
compute on *one shared implementation* rather than each carrying its own, so
they cannot give different answers to the same question.

### What was wrong

`dc` stored every value as an `f64` (a "double" — the binary floating-point
format hardware provides, holding about 17 significant decimal digits). Its own
manual page advertised arbitrary precision. Concretely:

| Expression | `dc` answered | The true value |
|---|---|---|
| `2 200 ^ p` | `1606938044258990` + 45 zeros | `…2202993782792835301376` |
| `[d1-d1<F*]sF 30 lFx p` (30!) | a rounded approximation | `265252859812191058636308480000000` |
| `20 k 1 3 / p` | 17 good digits, then noise | 20 exact digits |

This is worse than a program that simply refuses: every wrong digit was
presented with the same confidence as a right one.

### The decision

`dc`'s numeric core is now `bignum::Decimal` — the fixed-point decimal type
`userspace/bc` was moved onto first. Not a copy of it; the same crate.

*What changes:* every arithmetic answer is exact to the digit the user asked
for, and any expression written in both syntaxes gives the same string.

### The alternatives

| Option | *What changes* | Against |
|---|---|---|
| Give `dc` its own bignum implementation | Same answers today | Two implementations of POSIX's scale rules drift. `bc` and `dc` are specified to agree; nothing would keep them agreeing except vigilance. |
| Leave `dc` on `f64`, correct the documentation | `dc`'s manual stops lying | The program remains useless for the thing `dc` is *for*. `dc` exists because `expr` cannot do arbitrary precision. |
| **Share `bignum::Decimal` (chosen)** | Exact answers; the two syntaxes provably agree | The shared type's bugs are now two programs' bugs — mitigated by the cross-check below. |

The sharing paid for itself immediately. Moving `dc` onto the type surfaced
three defects in it that `bc`'s own tests had not: the formatter trimmed
trailing zeros (which made `scale` unobservable), the base-ten parser scored
`F` as 22 rather than 15, and a non-decimal fraction kept an inflated scale.
All three were `bc` bugs too, and all three are fixed in §324's companion
commit. One implementation exercised by two front-ends finds more than two
implementations exercised by one each.

### Two decisions taken along the way

**Comparisons put the top of the stack on the left.** `5 3 >a` asks whether
3 — the value on top — exceeds 5. This reads backwards, and the previous
implementation had it reversed. It is not a matter of taste: the two idioms
every `dc` program is built from, `[d 1 - d 1 <F *] sF` (factorial) and
`[1 + d 20 >L] sL` (count to twenty), terminate only under this reading. Under
the reversed one they run exactly one pass, and 30! came out as 870.

**A macro call in tail position replaces the current chunk rather than nesting.**
`dc` has no loop construct; a loop is a macro that ends by invoking itself. If
that invocation consumed a Rust stack frame, the number of iterations a `dc`
program could run would be capped by the process stack — a 20000-iteration loop
would abort the process rather than answer. Calls that are *not* in tail
position still nest, and are bounded by an explicit depth limit that reports
"too deep" instead of crashing.

### Verification, and its limits

There is no `bc` and no `dc` on the development machine, so the differential
harness used for `sed`, `awk` and `expr` — run ours and the system's on the same
input, compare — was not available. Two weaker checks stand in:

- **`bc` against `dc`.** 22 expressions written in both syntaxes, covering
  `2^200`, 50-place quotients, the scale rules for `*`, `%` and `^`, and base
  conversion in both directions. All 22 agree exactly. This cannot catch a
  misreading of POSIX that both inherit from the shared type, which is precisely
  what it is weak against.
- **Hand-computed expected values**, checked against an independent arbitrary-
  precision implementation (Python) for the cases where a wrong answer would be
  hard to spot: 2^4096 mod (10^30+1), 30!, sqrt(2) to thirty places.

`known-issues.md` records the output details still unverified against a
reference implementation — chiefly whether a value below one prints as `0.5` or
as `.5`, where every traditional `bc` omits the leading zero and ours does not.
