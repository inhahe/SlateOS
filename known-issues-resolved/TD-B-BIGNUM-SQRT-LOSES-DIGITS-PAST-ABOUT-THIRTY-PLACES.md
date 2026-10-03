## TD-B-BIGNUM-SQRT-LOSES-DIGITS-PAST-ABOUT-THIRTY-PLACES (lane B, 2026-09-16) -- **Status: FIXED** 2026-09-16

**In short:** ask for a square root to more than about thirty decimal places
and the last ones are wrong. `sqrt(2)` at `scale=40` gives
`1.4142135623730950488016887242097601756851` where the true value is
`…2096980785696`: correct for 30 places, then drifting. It is not a rounding
difference in the final digit — the tail is simply wrong.

**Where:** `userspace/bignum/src/lib.rs`, `BigNat::isqrt`, reached through
`Decimal::sqrt`. Affects `bc` and `dc` alike, and through the arctangent
reduction it caps `a(x)` as well: `a(1)` at `scale=30` is exact to 24 places.

**Measured**, against GNU bc 1.07.1:

| Call | GNU | Ours |
|---|---|---|
| `scale=30; sqrt(2)` | `1.414213562373095048801688724209` | agrees |
| `scale=40; sqrt(2)` | `…242096980785696` | `…242097601756851` |
| `scale=70; sqrt(2)` | `…2096980785696718…` | diverges at ~33 places |
| `scale=0; sqrt(2*10^62)` | `…2096` | `…2097` (one too large) |

**Not caused by the parity fix above** — measured on the commit before it and
the same wrong digits come out. The two are independent.

**Diagnosis, not yet confirmed.** `isqrt` is Newton's method from an initial
guess of `10^ceil(digits/2)`, terminating when the iterate stops decreasing.
That shape is correct when the guess is at or above the true root, and it
returns exact answers for large perfect squares — `sqrt(x*x) == x` was checked
at 32, 40 and 41 digits. So the integer arithmetic is sound and the fault is
more likely in the *termination*: the last iteration is skipped, leaving a
value one too large, which then propagates into every digit the rescale keeps.
The `sqrt(2*10^62)` row is the cleanest handle on it — a single `isqrt` call,
no scaling, answer off by exactly one.

**The proper fix:** after the loop, correct the result downward while
`guess*guess > n` and upward while `(guess+1)^2 <= n`. Two multiplications
settle it and make the answer exact by construction rather than by trusting the
iteration to have converged. Then assert the known digits of `sqrt(2)` at
scale 40 and 70 (the test in `decimal.rs` deliberately stops at 30 and says
so), and raise the `a(1)` prefix assertion in `bc.rs` from 24 places to 30.

### Fixed 2026-09-16 — and the diagnosis above was WRONG

It was not `isqrt`. `isqrt` was doing the best it could with a division that
was lying to it: the real fault was
`TD-B-BIGNUM-LONG-DIVISION-DROPS-A-BORROW-BIGGER-THAN-ONE-LIMB`, filed below,
in which `BigInt::divmod` returned wrong quotients for multi-limb divisors.
Newton's method is nothing but repeated division, so every iterate was
computed from a wrong value and the sequence settled wherever that left it.

The write-up above said the integer arithmetic was "sound" because
`sqrt(x*x) == x` held at 32, 40 and 41 digits. That evidence was real and the
inference from it was wrong: those are *perfect squares*, where the iteration
lands on an exact value early and the division never has to produce the
awkward quotient that triggers the bug. **A check that passes tells you what it
covers, not what it implies.** The right next step was the one that found it —
divide two large numbers and compare against GNU, rather than reason about
which component "must" be at fault.

Both halves were done anyway, and the exactness correction is kept: after the
Newton loop, `isqrt` now walks the result down while `guess^2 > n` and up while
`(guess+1)^2 <= n`, so the answer satisfies the definition of an integer square
root by construction instead of by trusting convergence. With the division
fixed those loops run once or not at all. It is also what made the division bug
*visible*: with the old `divmod` the correction had to walk roughly `6*10^29`
steps, so `sqrt(2)` at `scale=40` stopped returning a wrong answer quickly and
started hanging instead — which is how a silent wrong number became something
that demanded an explanation.

Now exact: `sqrt(2)` at scale 40 and 70, `sqrt(2*10^62)`, `sqrt(4)` at scale
50, and `a(1)` at scale 30 and 50 all match GNU bc 1.07.1 digit for digit. The
tests that said "stops at 30 because of a separate defect" now go to 70, and
the `a(1)` prefix assertion in `bc.rs` is a full equality.
