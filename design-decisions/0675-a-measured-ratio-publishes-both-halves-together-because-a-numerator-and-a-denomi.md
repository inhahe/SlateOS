## §675 — a measured ratio publishes both halves together, because a numerator and a denominator timed by different parties are not the same interval

**Date:** 2026-09-02
**Lane:** A
**Decided by:** Claude (autonomous)

**In short:** the build harness runs its own self-tests before it compiles
anything, and one of them starts two CPU-burning helper processes and checks
they really burned CPU. It also checks the reverse — that they did not burn
*more* CPU than the stretch of time they were watched over, which would be
physically impossible and would mean the measurement was broken. That
impossible reading happened, and a whole boot test was refused over it. The
decision was where to put the fix: loosen the impossibility check so it stops
firing, or change how the measurement is taken so the check becomes true
again. We changed the measurement.

### What was actually wrong

The helper processes published a single number — how much CPU they had used so
far. The controller watching them divided that by a stretch of time *it*
measured with its own stopwatch. Those two things sound like they describe the
same interval. They do not.

A helper that is waiting for the start signal only speaks occasionally, so the
number the controller reads may be up to a second old. The division therefore
ran from "whenever that helper last spoke" on top to "when the controller
started its stopwatch" on the bottom. Any CPU charged in between counted in the
numerator with no matching time underneath it.

That gap is normally tiny, and it stayed invisible for exactly that reason. But
Windows measures per-process CPU by sampling 64 times a second and charging a
whole 15.6 ms tick to whichever process it catches — even one that ran for a
microsecond. One stray tick is 7.8% of a 0.2 s window, and 7.8% was the entire
error allowance. So a single unlucky sample was enough to produce an impossible
number, and it did: `1.089` against a ceiling of `1.078` on a loaded machine,
and later `1.009` on a completely idle one.

### Options

| | *What changes:* |
|---|---|
| **A. Widen the tolerance** | the check stops firing, and also stops detecting anything |
| **B. Delete the check** | nothing detects a malformed measurement at all |
| **C. Publish CPU and clock together (chosen)** | the two halves of the ratio describe the same interval, so the bound is arithmetic rather than empirical |

**Chosen: C.** Each helper now publishes a *pair* — its CPU total and the clock
reading taken with it, written together under one lock — and its span is its
own clock delta. Numerator and denominator cover the same interval by
construction. Rounding is then the only error left, and the existing
`1 + 15.6 ms / span` bound is a real bound again rather than an estimate.

### Why not A

A is the option that had already been taken once here, and its cost is on
record. The bound used to be a hardcoded `2.0`, and under it a defect that made
each single-threaded helper appear to use **1.82 cores** passed unnoticed for
as long as the machine stayed quiet. 1.82 is not a tolerance question; one
thread cannot use two cores. The tolerance was wide enough to swallow an
impossibility, so nothing was watching.

The general form: *a bound loosened every time it fires is not a bound, it is a
record of how often it has been ignored.* When a check reports something
impossible, the useful assumption is that the check is right and the
measurement is wrong — which is what it turned out to be.

### What this cost, and the discipline it enforces

Two plausible causes were proposed and both were **measured and cleared** before
anything was changed:

1. *The controller loses milliseconds to descheduling between reading the
   helpers' clocks and stamping its own.* Measured: **0–20 µs**. The stamping
   order was fixed anyway because it is correct, but it explains nothing.
2. *The clock's rounding is worse than one tick.* It is not — the user and
   kernel counters share one sampler, so their sum advances at most once per
   tick event.

Both could have explained the magnitude, and neither did. The rule this
reinforces: **a mechanism that could account for the number is not evidence
that it did.** The same trap was walked into and out of once more the same day,
on `check-variant-lists.py`, where a confident theory about a directory walk
would have produced a real change to a correct file.

### A second finding, kept rather than filed

Reproducing this surfaced an unrelated flake in the same test: the *floor*
check, which asks whether the helpers ran at all, read `0.304` on an idle
machine in a 14-run probe. Over 0.2 s one unrelated process can take most of
the CPU, and the instrument cannot distinguish that from a load that was never
applied — so it would have refused a boot test over a fact about the host.

Both bounds wanted the same remedy from opposite directions, so the live test's
window went from 0.2 s to 0.6 s: the floor gets enough samples to average a
transient away, and the ceiling's allowance falls from 7.8% to 2.6%, tripling
the check's power. Measured across 14 runs each, the spread narrowed from
`0.278 – 1.037` to `0.811 – 1.004`, with nothing over the ceiling.

**If this is ever revisited:** the thing to preserve is not the window length
but the invariant — whoever produces the numerator must also produce the
denominator, at the same instant, under the same lock. Any future change that
reintroduces a controller-timed span reintroduces this bug silently, because a
numerator outrunning its denominator is indistinguishable from a clock that
rounded up.
