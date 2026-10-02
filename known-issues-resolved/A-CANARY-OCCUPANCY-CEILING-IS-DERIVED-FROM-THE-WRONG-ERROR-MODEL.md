### A-CANARY-OCCUPANCY-CEILING-IS-DERIVED-FROM-THE-WRONG-ERROR-MODEL. A live timing self-test refused to build a boot test at `occupancy_measured 1.089 > ceiling 1.078`, on a 0.2s span whose instrument cannot resolve 1% — 2026-09-02 — **Status: ✅ FIXED 2026-09-02** (lane A)

> **Resolution 2026-09-02 (commit `7a504ebb6`).** The title is wrong, and that
> is the finding. The error model was *right*; the span it was applied to was
> not the span the numerator covered. The ceiling was correctly reporting an
> impossibility — just not the one anybody guessed.
>
> **Cause.** Each spinner published a bare CPU number, and the controller
> divided it by an interval it stamped itself. Those are not the same interval.
> A spinner parked in `go.wait(1.0)` may have published up to a second earlier,
> so the numerator began wherever it last spoke while the denominator began at
> the controller's stamp. Windows charges a whole 15.6 ms tick to whichever
> process the 64 Hz sampler catches, however briefly it ran — so a *single*
> tick landing in that gap is 7.8% of a 0.2 s window, and the entire allowance
> was one tick.
>
> **Fix.** Spinners publish `(cpu_seconds, monotonic)` as a locked pair, and
> each spinner's span is its own clock delta over the same interval as its own
> CPU figure. Numerator and denominator are now the same interval by
> construction, so quantisation is the only error left and `1 + g/span` is a
> hard bound again. Barriers at both window edges (`fired_count`, `done_count`)
> keep the pairs fresh — the controller may not snapshot until every spinner
> has published after `go`, nor close until every one has published after
> `stop`.
>
> **What was rejected, and why it matters.** Widening the tolerance. It would
> have retired the only check that noticed, which is precisely the mistake the
> old hardcoded `2.0` had already made once — it let 1.82 cores per
> single-threaded spinner through for as long as the host stayed quiet. A bound
> that is loosened every time it fires stops being a bound.
>
> **Two suspects were measured and cleared rather than patched around.** The
> first theory was that the controller's snapshot read was losing milliseconds
> to descheduling; measured, it takes **0–20 µs**, and the stamping order it
> was blamed on is now merely correct rather than load-bearing. This is the
> second time today a plausible cause survived until it was measured and then
> did not (see the `check-variant-lists.py` entry) — the pattern is that a
> mechanism which *could* explain the magnitude is not evidence that it *did*.
>
> **A second, independent flake surfaced during the repro** and is fixed in the
> same commit: the live test's `occupancy` *floor*. A 14-run probe caught it at
> `0.304` on an otherwise idle host. Over 0.2 s a single unrelated process can
> take most of the CPU, and the instrument cannot tell that from a load that
> was never applied — so this would have refused a boot test for a fact about
> the host rather than about the code. The live window goes 0.2 s → 0.6 s,
> which both bounds needed in opposite directions: the floor gets enough ticks
> to average a transient away, and the ceiling's allowance falls from 7.8% to
> 2.6%, tripling the check's power.
>
> **Measured, 14 runs each, same host:**
>
> | | `occupancy_measured` range | ceiling | over ceiling |
> |---|---|---|---|
> | before (0.2 s, unpaired) | 0.278 – 1.037 | 1.062 – 1.077 | 0/14 quiet, 1 seen loaded |
> | after (0.6 s, paired) | **0.811 – 1.004** | 1.022 – 1.026 | **0/14** |
>
> The pre-fix probe also caught `1.009` on an *idle* host — impossible for a
> single-threaded process, and the observation that moved this from "flaky
> tolerance" to "the ratio is malformed".

**In short:** the harness runs its own test suites before it builds anything,
and one of them — `test-canary-load.py` — starts two CPU-burning processes and
checks that they really did burn CPU. It also checks the reverse: that they did
not somehow burn *more* CPU than the time they were measured over, which would
mean the measurement was self-inconsistent. Under two lanes' concurrent load
that upper check failed, by about 1%, on a measurement whose clock is only
accurate to about 8%. A whole boot test was refused over it, before building.

**The failure, verbatim:**

```
1 FAILURE(S)
  - and does not exceed what the measured span allows:
    occupancy_measured 1.089 > ceiling 1.078 (span 0.2008s, window 0.2007s)
```

`check_python_suites` (`scripts/boot-test.sh:4166-4175`) exits 1 when any suite
fails, so this ends the run at the gate with `ERROR: refusing to build`. That
behaviour is correct and should not change — these suites *are* the harness,
and its own comment is right that "a failure here means the numbers this run
would produce cannot be trusted." The defect is that the assertion is false,
not that the gate believed it.

**Where it lives.** `scripts/canary-load.py:212-225` (`occupancy_ceiling`) and
`scripts/test-canary-load.py:823-827`, the live two-spinner case at :782-790.

**The arithmetic, which is the whole diagnosis.**

```python
return 1.0 + CPU_CLOCK_GRANULARITY_S / span_seconds   # 15.6 ms / span
```

At `span = 0.2008s` that is `1 + 0.0777 = 1.078`. The span holds **12.9 timer
ticks**, so the entire allowance the ceiling grants is *one tick*. The observed
overshoot of 0.011 is about 1.14 ticks of aggregate over-charge across the two
spinners. The reading is not a physical violation; it is one tick of noise on
an instrument with thirteen ticks of range.

**Why the error model is wrong, and it is wrong in a way the comment argues
for.** `occupancy_ceiling`'s docstring is careful and mostly right — it
explicitly refuses a hardcoded tolerance, derives the bound from the span, and
notes that a fixed tolerance "is either too tight for a short span or too loose
for a long one." All true. But it models the clock as **quantised**: a true CPU
time rounded to a 15.6 ms grid, so at most one grid step high.

Windows' `GetProcessTimes` is not quantised, it is **sampled**. At each timer
interrupt the whole tick is charged to whichever thread was running on that CPU
at that instant. A process that is runnable but time-slicing against other work
can be charged ticks it did not fully consume. Over many ticks the
over- and under-charges cancel and the ratio converges; over thirteen ticks
they do not. So the error is not bounded by one grid step — it is a sampling
error that shrinks with the *number of ticks in the span*, which is precisely
the regime a 0.2s window puts it in the worst case of.

That also explains why this has passed for weeks and failed today: on a quiet
host a spinner has a core to itself and is charged correctly, so the sampling
error is near zero and the quantisation model is indistinguishable from the
truth. It is contention that separates them. Same root cause as the addendum to
"Three lanes building at once…" above, arriving as a false *assertion* rather
than as a timeout.

**What is NOT yet established, and why this entry says OPEN rather than naming
a fix.** The account above is a model, and the last time a plausible model was
adopted without measuring — earlier the same day, on `check-variant-lists.py` —
it was wrong and would have produced a change to a correct file. So the
reproduction is running before the fix is written: the suite is being run
repeatedly under load to get a **failure rate** and a distribution of
`occupancy_measured`. What that has to settle:

- Is the overshoot rare (a tail event, argues for a longer span) or routine
  (argues the ceiling is structurally too tight)?
- Does it scale as `1/span` (quantisation, the current model) or as
  `1/sqrt(ticks)` (sampling)? These predict different fixes and the numbers can
  distinguish them.

**What a proper fix will *not* be.** Loosening the ceiling by a constant. The
docstring is right that a constant tolerance is what let a systematically
biased ratio pass unnoticed, and that argument survives this finding intact —
whatever replaces the bound has to be derived from the span too, just from the
correct error law. Deleting the clause is also out: it is the only check that
the measurement is self-consistent, and §673 (a clause that cannot be judged is
voided, never passed) is the precedent for what to do when an instrument cannot
support a question — void it loudly and count the voids, not pass it quietly.

**Meanwhile:** any lane whose boot test dies here has not found a bug in its own
change. Re-run; it is load-dependent. *(Superseded by the resolution at the head
of this entry: it is fixed, not merely load-dependent.)*
