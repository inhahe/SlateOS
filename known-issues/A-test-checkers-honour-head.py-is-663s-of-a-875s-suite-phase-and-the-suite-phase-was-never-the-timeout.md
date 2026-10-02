### [A] `test-checkers-honour-head.py` is 663s of a 875s suite phase, and the suite phase was never the timeout -- 2026-09-18
**Status:** OPEN (measured; the 663s is a real per-boot cost, the timeout cause is elsewhere and still unattributed)

**In short:** one of the tooling's own test suites takes eleven minutes and
the other twenty-one take under forty seconds each. That is worth knowing on
its own -- it runs on every boot -- but it does not explain the two boots
that died at their time limit, because all the suites together are about
fifteen minutes out of a three-hour phase.

**Measured in situ** after adding per-suite timing to `boot-test.sh`'s
`scripts/test-*.py` loop, which previously reported names and verdicts but
no durations:

| suite | seconds |
|---|---|
| `test-checkers-honour-head.py` | **663** |
| `test-canary-load.py` | 36 |
| `test-reclaim-space.py` | 30 |
| `test-check-boot-skips.py` | 23 |
| `test-build-usb-image.py` | 22 |
| `test-boot-test.py` | 20 |
| the other sixteen | 0-18 each |
| **suite phase total** | **~875** |

So one suite is 18x the next largest and about 76% of the phase. It is also
honest work: 124 end-to-end cases, each building a synthetic repository and
driving real push gates through it, and it passes.

**What the number refutes is the reason I went looking.** Two boots died at
their bound -- 7200s, then 10800s -- and my hypothesis was that this suite
had started hanging. It had not (776s standalone, 663s here, RC=0 both
times), and at ~875s the entire suite phase is 6% of the 10800s budget. The
time went into the 79 `=== Checking ...` gates that run alongside them, and
**those still report no durations**, so the cause remains unattributed.

**And the run that produced these numbers is itself evidence against a
permanent regression.** It reached 79 gates plus all 22 suites in 74
minutes, where the 10800s run managed fewer in 180. Same tree, same gates.
That points at contention during the failed run -- lane C had two full
workspace runs inside it and I ran a cmake cross-build, a rootfs rebuild and
two cargo-using pushes during an earlier one -- rather than at anything
having become slow.

#### Corrected the same day: the figures above came out of a truncated list

The table above says 22 suites totalling ~875s with one at 663s. Measured
again on the next run, reading **all** of them:

| suite | seconds |
|---|---|
| `test-checkers-honour-head.py` | 573 |
| `test-pre-push-fmt-gate.py` | **189** |
| `test-pre-push-doclinks-gate.py` | **97** |
| `test-pre-push-identity-gate.py` | **56** |
| `test-canary-load.py` | 33 |
| the other nineteen | 0-19 each |
| **24 suites, total** | **1078s** |

So it is 24 suites and 1078s, not 22 and ~875s, and the top four are 915s --
**85% of the phase**. Three suites in the 56-189s band were missing from the
first table entirely.

**Why they were missing is the part worth keeping, because it is the error I
recorded in `design-decisions.md` the same morning.** I read the timings with
`head -20` over an output that is **alphabetically ordered**, and the three I
missed are alphabetically late -- `test-pre-push-*`. Lane C's phrasing, from
their `tail -40` that could not have shown a failure: *the filter kept the
wrong end.* Mine kept the wrong end of my own instrument's output, hours
after writing that sentence down.

A truncation over a sorted list is not a sample. It is the first N of an
ordering that has nothing to do with the quantity being measured, and it
fails silently because twenty numbers look like a census.

**The conclusion is unchanged, which is why the correction is worth making
rather than burying:** 1078s is still 10% of the 10800s budget, so the suite
phase was not the timeout and the 79 `=== Checking ...` gates remain the
unattributed 80%. Being wrong by 200s does not move that, but publishing a
figure taken off a truncated list would have made the next reader's
arithmetic wrong for no reason.

One further datum the full list gives that the truncated one could not:
`test-checkers-honour-head.py` has now been measured at 776s standalone,
663s in one boot and 573s in the next. It is variable by a third, which
makes any single reading of it a poor basis for a threshold -- worth knowing
before anyone sets one.

**What is worth doing, and what is not.** Timing the 79 gates the way the
suites are now timed is the obvious next step and is a larger edit: they are
not a loop, they are individual functions. Not done yet, and possibly not
worth doing -- if contention is the whole story, the instrumentation that
matters is the build-contention notice already added, not per-gate
stopwatches. One more uncontended run that completes would settle it, and
that is cheaper than the edit.

`scripts/test-checkers-honour-head.py` is not in lane A's write list, so the
663s is recorded rather than optimised. If it is ever worth reducing, the
shape to look at is that each of the 124 cases builds a repository from
scratch; a shared fixture would trade isolation for time, which is a real
tradeoff and not an obvious win.
