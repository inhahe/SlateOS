### [A] PREDICTION P22 — the positional model has never been shown to predict anything

**Registered 2026-08-18, before the discriminating run exists.**

> **Renumbered from P21 on 2026-08-18, the day after it was registered.** P21
> was already taken by "the no-op path translation should stop allocating"
> (above, since graded), so for one day two unrelated predictions shared an
> identifier — including in `design-decisions.md` §229 and in the scripts that
> implement this experiment. An ambiguous identifier on a *prediction* is worse
> than on most things: the whole point of registering one in advance is that a
> later reader can check what was claimed before the evidence arrived, and they
> cannot do that if the name resolves to two different claims. The graded P21
> keeps its number because it is settled and cross-referenced from a dozen
> places; this one, being a day old, moved.

`scripts/bench-history.py` now converts the canary's positional trace into a
per-benchmark factor and prints it beside the benchmarks that ran in a disturbed
stretch (design-decisions.md §229). Every property asserted for that model so far
is a property of its *arithmetic*, proved by unit tests against synthetic traces.
Not one is a property of the machine. By this file's own maxim — a detector that
has never been shown to fire on a known stimulus is not yet a detector — the
model is at the same stage the canary itself was at before P20 graded.

The stimulus exists and is already scripted. `scripts/canary-load-test.sh N`
starts N CPU spinners once `Booting QEMU` appears, so load lands on the
measurement window. What it has never done is start them *late* — the load has
always covered the whole suite, which is precisely the uniform case this model is
designed to contribute nothing to.

- **P22(a)** — load applied to an *interior window* of the suite will produce
  canary factors above 1.0 confined to that window and to the one sampling
  interval either side of it. *Falsified if the flagged positions are spread
  across the whole suite*, which would mean the factor tracks something other
  than where the load was.
- **P22(b)** — the benchmarks the model flags will be inflated relative to their
  own historical medians, and the ones it does not flag will not. This is the
  claim a *correction* would rest on, and it is the one most likely to fail:
  the canary times a memory access, so a branch-bound benchmark may sit inside a
  flagged stretch and be barely affected. *Falsified if flagged and unflagged
  benchmarks are inflated by indistinguishable amounts.*
- **P22(c)** — the ratio between a benchmark's inflation and its canary factor
  will vary systematically with what the benchmark does, memory-bound ones
  tracking closest. *Falsified if the ratio is flat across benchmark kinds*,
  which would remove the second of the two objections in §229 to correcting
  automatically.

**What each outcome licenses.** P22(a) alone establishes attribution — the model
can say *where*, which is all the printed line currently claims. P22(a)+(b)
together are what would justify applying the factor to the recorded value;
without (b) a correction is arithmetic on an unvalidated coupling. (c) decides
whether one factor per position is enough or whether the correction would need a
per-benchmark sensitivity, which nothing currently measures.

**To run it (2026-08-18: the missing piece now exists).** The entry above
recorded that `canary-load-test.sh` could only start its spinners at `Booting
QEMU`, so every disturbance covered the whole suite and (a) could not be tested.
Both halves of that gap are now closed:

```
scripts/canary-load-test.sh 6 --load-at=crypto_x25519 --load-until=http_mime_type
```

- **Placing the load.** `--load-at`/`--load-until` follow the growing serial log
  for the result line the kernel prints when a named benchmark finishes, and
  start/stop the spinners there. The trigger is a benchmark *name* rather than a
  wall-clock delay on purpose: a delay would have to be tuned against a suite
  whose length moves with the host, and it would leave the experiment's own
  ground truth — which positions were loaded — as an estimate. Estimating the
  ground truth of an experiment about positions defeats the experiment.
- **Grading it.** `scripts/grade-positional.py` reads the completed run's SCORE
  lines to establish the loaded positions exactly, then scores the model three
  ways: sensitivity over the loaded window, localisation over the window widened
  by the sampling interval, and — the number that actually discriminates — the
  false-positive rate over the remainder, which the disturbance provably could
  not reach. Sensitivity alone cannot separate a working model from one that
  flags everything; only the clean region can, which is why the stimulus has to
  be an interior window rather than a suffix.
- **The grader can return a negative.** All five of its verdicts (SUPPORTED,
  blind, not-localised, misplaced, ungraded) have tests that produce them, and
  each was confirmed reachable by mutating the grader and watching exactly the
  intended test fail. A grader that could only say SUPPORTED would make this
  prediction unfalsifiable, which is the failure this entry exists to avoid.

Two caveats that must survive to whoever runs it. `scripts/test-grade-positional.py`
passing is **not** evidence for P22 — its end-to-end case is a synthetic log whose
trace was written by hand to be the right answer. And a window near either end of
the suite grades as UNGRADED rather than SUPPORTED, because once widened it leaves
no clean region and would pass whatever the model did.

**Until P22 grades**, treat the attribution line as a *hypothesis about which
benchmarks to re-run*, not as evidence about any of them, and do not build a
correction on it. The line's own printed caveat says the same thing, which is
deliberate: the caveat has to survive being read by someone who never opens this
file.
