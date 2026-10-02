## §231 — The load controller measures the load instead of inferring it, because on a host with spare cores the inference is not merely weak but invertible

**Date:** 2026-08-19
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** to test a feature that guesses which benchmarks were disturbed, we
deliberately slow the machine down during part of the benchmark run. We then need
to confirm the slowdown actually happened. We were confirming it by timing a small
fixed job on the host and checking it got slower — but on a machine with more
cores than we use, that job just runs on a spare core and barely notices. Worse,
the number we computed from it was unstable enough to say the host got *faster*
while under load, which it did, three times in twelve trials. The fix is to stop
guessing: each load process now reports how much CPU it actually consumed.

**The problem.** The controller applies load by starting 6 spinner processes, and
verified the load "landed" by timing a fixed unit of work on the host and
comparing the median before and during. On this 12-core machine that comparison
does not work, for two independent reasons:

- **Physical.** 6 spinners on 12 cores leaves the probe a free core. The best-case
  cost of the probe moves by ~0.4% (×1.004). The probe is *correctly* reporting
  almost no effect; there is almost no effect to report, on the probe.
- **Statistical.** The median of a probe sample is wildly unstable. Over 12
  back-to-back trials with the spinners verifiably alive throughout, the median
  ratio ranged ×0.784–×1.526 (a 1.95× spread) and came out **below 1.0 — "the
  host sped up" — in 3 of 12 trials.**

The second is what made it dangerous rather than merely useless. Run 3 of the P22
experiment reported ×0.78 and cost a day of investigation into a physical effect
that does not exist. (A zero-spinner control reproduced ×0.77 with no load at all.)

**Options considered.**

1. **Report a stable statistic (min or trimmed best-case) and keep inferring.**
   *What changes:* the alarming number goes away — run 3 recomputes to ×1.018.
   Cheap, and strictly better than the median. But it converts a misleading
   reading into an uninformative one: ×1.004 is what you get whether the load ran
   or not, so the question "was the load applied" remains unanswered.
2. **Make the probe compete — pin probe and spinners to one core set.**
   *What changes:* the probe would feel the load. But it would then be measuring
   contention on an artificial core subset rather than the host's spare capacity,
   which is not the quantity QEMU's vCPU thread competes for. It would also make
   the probe part of the load it is measuring.
3. **Raise the spinner count above the core count.** *What changes:* the probe
   would feel the load, at the cost of changing the stimulus the experiment
   applies to the guest — i.e. changing the experiment to suit the instrument.
4. **Chosen: measure the spinners' own CPU consumption directly.** Each spinner
   publishes its `process_time` into a shared cell on every chunk boundary; the
   controller snapshots at fire and at release. Occupancy = CPU burned ÷ (spinners
   × window). *What changes:* the output states "102% occupancy, 12.25s of CPU
   burned of 12.06s available" instead of a ratio that needs interpreting.

**Why 4.** It is a direct measurement of the thing in question rather than a proxy
for it. It needs no baseline, so there is no unequal-windows problem; it cannot be
confounded by QEMU's own phase-dependent host demand, which is roughly 4× larger
than the spinner signal and was the leading rival explanation for the ×0.78; and
it is independent of the host's core count, so it does not silently degrade on a
different machine the way the probe did. It also has an *absolute* expectation
(1.0), which means it can be asserted in a test rather than merely observed — the
regression suite now checks a live 2-spinner run clears the floor and does not
exceed what wall time allows.

**Costs, accepted.** It adds a `process_time()` call per spin chunk (negligible
against 200 000 empty loop iterations) and a shared-memory cell per spinner. It
measures only the spinners, so it cannot detect load from *other* sources — but
that was never what it was for, and the benchmarks' own measured disturbance
(reported separately, and independently of both the model and the controller)
remains the evidence that the guest actually felt something.

**The probe is kept, demoted.** `inflation` now carries the best-case ratio
(option 1, adopted as well as rather than instead of option 4), the median is
retained as `median_inflation` and labelled unreliable in both the JSON and the
printed output, and old records are detected and marked `(median, unreliable)` on
display rather than silently compared against new ones. The probe still answers a
real question — *when*, across the run, was the host contended — which is what its
timestamped samples were added for and which occupancy does not address.

**Threshold.** Occupancy below 0.5 marks the run `load-not-applied`. Set low
deliberately: its job is to catch spinners that never ran at all (a failure mode
that silently voided an earlier standalone probe, which reported a confident ratio
with zero live spinners), not to police scheduling jitter on a busy desktop. Run 3
sat far above it.
