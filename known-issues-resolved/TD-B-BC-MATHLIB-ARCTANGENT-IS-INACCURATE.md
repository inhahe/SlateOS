## TD-B-BC-MATHLIB-ARCTANGENT-IS-INACCURATE (lane B, 2026-08-24) -- **Status: FIXED** 2026-09-16

**In short:** `bc -l` provides a small library of maths functions. Ours gets
the arctangent wrong — not by a rounding error in the last digit, but in the
**third** one. `a(1)` is π/4, a number every reference agrees on; GNU prints
`.7853981633` and we print `.7828982258`. Anything computing an angle with our
`bc` gets an answer wrong by about 0.03%.

**Where:** `userspace/coreutils/src/bin/bc.rs`, the `a(x)` builtin.

**Measured**, `scale=10`, `bc -l`:

| Call | GNU | Ours | True value |
|---|---|---|---|
| `a(1)` | `.7853981633` | `.7828982258` | 0.78539816339… |
| `j(0,1)` | `.7651976865` | `.7651976865` | agrees |
| `s(0)`, `c(0)`, `e(1)`, `l(1)` | — | — | all agree |

So it is `a` alone; the Bessel function beside it in the same harness row is
correct, as are sine, cosine, exp and log.

**Diagnosis, not yet confirmed against the code:** the error is far too large
for accumulated rounding and far too small for a wrong formula, which is the
signature of a **truncated series**. The Maclaurin series for arctangent
converges famously slowly at `x = 1` — it is the alternating harmonic series
there — so an implementation that sums a fixed number of terms rather than
iterating until the term falls below the current `scale` lands close to the
answer and stops. `.7828982258` being *below* the true value is consistent with
stopping just after a negative term.

**The proper fix:** the standard one, which is also GNU's. Range-reduce with
`atan(x) = 2·atan(x / (1 + sqrt(1 + x²)))` until `|x|` is small enough that the
series converges quickly, then sum **until the term is smaller than the working
precision**, with the working scale set a few digits above the requested
`scale` so the last requested digit is not itself the rounding error. Then
extend the harness row to `a(0)`, `a(0.5)`, `a(1)`, `a(2)`, `a(-1)` and a large
`scale` — a fixed-term series can be right at one argument and wrong at the
next, which is exactly why one call was enough to miss this.

---

### Fixed 2026-09-16

The diagnosis in this entry was right and is now confirmed against the code:
`atan_series` summed a **fixed 100 terms**, and its `is_negligible` guard could
never fire at `x = 1` because `x^2 = 1` leaves the numerator at ±1 for ever.
The arithmetic identifies it rather than merely fitting it: an alternating
series truncated after N terms sits within half the first omitted term, here
`1/(2*100+1)/2 = 0.00248…`, and the measured error was
`.7853981633 - .7828982258 = .0024999`.

Fixed as prescribed — `atan(x) = 2*atan(x / (1 + sqrt(1 + x^2)))` applied until
`|x| < 1/16`, then the series summed until the term falls below the working
precision, with the term cap now derived from `scale` and serving only as a
non-termination guard. `a(1)` needs five reductions and then gains ~2.4 digits
per term. The entry's warning to extend the harness was taken: `a(0)`, `a(0.5)`,
`a(1)`, `a(2)`, `a(-1)`, `a(1.0001)`, `a(100)`, `a(0.07)` and `4*a(1)` all now
match GNU exactly at scale 10, and `a(0.6)` matches to all 30 places at
scale 30.

**Two things the entry did not anticipate.**

The `|x| > 1` inversion was never the fix people assume it is: it maps
`x = 1.0001` to `0.9999`, which is just as slow to sum as what it came from. So
the reduction has to run on *both* branches, not just the small one.

And the first version of the fix made things **worse** — `a(1)` came back as
`53.18` — because the reduction calls `sqrt`, and `sqrt` turned out to have a
bug of its own that nothing had hit before:
`TD-B-BIGNUM-SQRT-IGNORED-THE-PARITY-OF-ITS-INPUT-SCALE`. That is why this
entry could not be closed on its own.

**What still limits it.** `a(1)` at `scale=30` is exact to 24 places rather
than 30, capped by `TD-B-BIGNUM-SQRT-LOSES-DIGITS-PAST-ABOUT-THIRTY-PLACES`,
which is older than this change and bounds every square root the reduction
takes. The test asserts the 24 as a prefix rather than asserting the wrong
digits, and says to raise it when that entry closes.
