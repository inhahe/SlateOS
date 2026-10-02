## 1140. The `long double` Bessel functions are computed in pairs of long doubles and rounded once: correctly rounded, at many times glibc's cost

**Date:** 2026-09-29
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** `j0l`, `j1l`, `jnl`, `y0l`, `y1l` and `ynl` -- the Bessel
functions for C's `long double`, which physics and engineering programs
call -- had to be written here: no library whose licence the C library can
take has them for the 80-bit type (musl and FreeBSD have none; glibc's are
LGPL, open-questions D-Q6). They are written to give the correctly rounded
answer -- the long double nearest the exact value, or the next one up or
down when the program has asked for rounding up or down -- and give it for
13,338 of the 13,339 values they are tested at, including long doubles
right next to the functions' zeros, where glibc's answers have no correct
bits at all (at 3,034 such points every one of glibc's is wrong, by a median
of 3 x 10^13 units in the last place; away from the zeros two in three of
glibc's are off, by up to 6). The one miss is a value 3.7 x 10^-7 of a unit
from a rounding boundary, and is one unit off. The price is time:
every step is done in double-long-double arithmetic (a pair of long doubles,
about 128 bits), and a call takes 3 to 31 microseconds in a release build
where glibc's takes a fraction of one. (Written first with `ld80.rs`'s
operators, each its own assembly block through memory, it took 20 to 240:
the pair's sum and products are now each one block on the x87 register
stack, bit for bit the same operations.)

**How** (`posix/src/besl.rs` has the detail): Miller's backward recurrence
and the Neumann series to `x = 48`; within 2^-32 of each zero below 48,
the Taylor series about it, the zero held to 192 bits
(`posix/tools/oracle/besl_tables.py`); past 48, Hankel's expansion written
as phase and amplitude, `J = M cos(theta)`, the phase reduced by pi/4 in an
exact sum so that the cancellation next to a zero happens without error;
below 2^-33 or 2^-70, the leading terms. The pieces check each other in the
tests where their ranges overlap, besides the oracle.

**Alternatives:**

- **glibc's kind: rational approximations in plain long double.** Fast --
  a few dozen operations -- and a few units in the last place out in
  general, but with no bound on the *relative* error next to a zero, which
  is where a program looking for one evaluates the function. Rejected: a
  program asks for `long double` to have the digits.
- **Cephes' `j0l` family**, the permissive source known-issues first
  named: the same kind of approximation as glibc's, with the same accuracy.
  Rejected for the same reason.
- **Correct rounding everywhere**, by redoing the one-in-thousands hard
  case in more precision (Ziv's strategy). It needs a third long double
  throughout, or a multiprecision fallback, for cases that are one unit off
  when they occur. Not done; it can be added over this without changing
  anything else if it is ever wanted.

**Where the answers part from glibc's**, each on purpose and each in the
tests: the values, which are the correctly rounded ones; the directed
rounding modes, which these honour and glibc's answer as to nearest; at an
encoding the x87 refuses (an unnormal, a pseudo-infinity), the unit's NaN
with invalid, as `mathl.rs` answers everywhere, where glibc's `jnl` and
`ynl` return 0 or set `ERANGE`; `errno` in the directed modes, §1139's one
rule; and `jnl`'s underflow at a huge order, +0 where glibc answers -0.

**Huge orders.** From order 512 `jnl` and `ynl` use Debye's
expansions, whose terms fall as powers of `1/n`: the sech(alpha) form
before the turning point `x = n`, the sec(beta) form -- in phase and
amplitude, reduced as Hankel's is -- past it; and from order 2048, within
`32 n^(1/3)` of the turning point, where neither holds, a recurrence from
where they do (upward for Y, Miller's downward for J, scaled to Debye's
values by least squares over two orders). A recurrence all the way from
order 0, as glibc's does and this did at first, costs time in proportion
to the order: minutes at 2^31, where this takes at most 6 ms. The phase at such an order is of the order of `n`, held to 2^-128
of itself, so the answer there is good to about 2^-97 of the amplitude --
correctly rounded but next to a zero or in the rare hard case.
