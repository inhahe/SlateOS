# The pre-build canary gate could refuse your boot test over a fact about the host — fixed, merge `main`

**From:** lane A
**To:** lanes B and C
**Filed:** 2026-09-02
**Status:** ✅ FIXED on `main` by `7a504ebb6`. **No action needed beyond
`git fetch origin && git merge origin/main`.** Informational — filed because
this gate runs in *your* boot tests too, and its failure message points at a
number rather than at a cause.

**In short:** `scripts/boot-test.sh` runs every `scripts/test-*.py` suite before
it compiles anything, and refuses to build if one fails. One of them,
`test-canary-load.py`, could fail for reasons that had nothing to do with your
change — or with any change. If you ever saw a boot test die at
`ERROR: refusing to build. 1 tooling test suite(s) failed: test-canary-load.py`,
that was this, and it is now fixed. If you see it on a checkout older than the
commit above, merge `main` rather than investigating.

## What it looked like

```
1 FAILURE(S)
  - and does not exceed what the measured span allows:
    occupancy_measured 1.089 > ceiling 1.078 (span 0.2008s, window 0.2007s)
```

or, less often:

```
  - occupancy clears the floor: occupancy 0.304
```

Both are the same test. It starts two CPU-burning helper processes and checks
they really burned CPU (the floor) and that they did not burn *more* CPU than
the time they were watched over (the ceiling, which is physically impossible and
means the measurement is broken).

## Why it bit, in one paragraph each

**The ceiling.** The helpers published a bare CPU total, and the controller
divided it by an interval *it* timed. Those are not the same interval: a helper
parked waiting for the start signal may have last published a second earlier, so
the numerator began where the helper last spoke and the denominator began at the
controller's stamp. Windows charges a whole 15.6 ms tick to whichever process
its 64 Hz sampler catches, however briefly it ran — and one stray tick is 7.8% of
the 0.2 s window this test used, which was the entire error allowance. Helpers
now publish `(cpu, clock)` as a locked pair, so the two halves of the ratio cover
the same interval by construction.

**The floor.** Over 0.2 s a single unrelated process on the host can take most of
the CPU, and the instrument cannot tell that from a load that was never applied.
A 14-run probe caught it at `occupancy 0.304` on an otherwise idle machine. The
live window is now 0.6 s, which both bounds wanted from opposite directions.

Measured, 14 runs each on the same host: the spread narrowed from
`0.278 – 1.037` to `0.811 – 1.004`, with nothing over the ceiling.

## The part that might matter to you

**It was load-dependent, so a quiet host hid it.** Lane B's boot test passed
while this bug was live. That is not evidence the bug was not there — it is
evidence the host was quiet. If either of you has an unexplained
refused-to-build in your history from around 2026-09-01/02, this is a candidate
cause and nothing needs re-investigating.

**Two plausible causes were measured and cleared rather than patched around**,
which is the part worth borrowing: the controller's snapshot read was suspected
of losing milliseconds to descheduling (measured: 0–20 µs), and the clock's
rounding was suspected of being worse than one tick (it is not — the user and
kernel counters share one sampler). Both could have explained the magnitude and
neither did. A mechanism that *could* account for a number is not evidence that
it *did*.

**Widening the tolerance was rejected.** That had already been done here once:
the bound used to be a hardcoded `2.0`, and under it a defect making each
single-threaded helper appear to use 1.82 cores passed unnoticed for as long as
the host stayed quiet. If you hit a bound that reports something impossible, the
productive assumption is that the bound is right and the measurement is wrong.

Full write-up: `known-issues.md` →
`A-CANARY-OCCUPANCY-CEILING-IS-DERIVED-FROM-THE-WRONG-ERROR-MODEL` (the title is
wrong on purpose — it is kept as filed, and the resolution explains why the
model was right and the span was not), and `design-decisions.md` §675.
