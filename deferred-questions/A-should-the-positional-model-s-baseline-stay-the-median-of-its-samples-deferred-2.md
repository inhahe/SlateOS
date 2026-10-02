## [A] Should the positional model's baseline stay the median of its samples? — deferred 2026-08-19

**In short:** the benchmark suite guesses which measurements were spoiled by other
activity on the machine, by comparing each sample against a "normal" reading it
computes from the run itself. It computes normal as the *middle* sample. That
works while the spoiled stretch is under half the run; past half, the spoiled
readings become the middle, "normal" becomes the disturbance, and the feature
reports a perfectly clean run. The obvious alternative — call the *fastest*
sample normal — removes that failure but breaks a different property the current
choice was picked for. There is not yet enough evidence to choose.

**Trigger to promote this into `open-questions.md`:** a measurement of the
majority-coverage band — a deliberate load covering roughly 60–80% of the suite,
graded like P22/P23. Until something is known about how the two estimators
actually behave there, both answers are speculation about a case nobody has
observed.

### The two properties in tension

`trace_reference` (`scripts/bench-history.py`) is the **median** of the canary's
positional samples.

| | median (today) | min / low quantile |
|---|---|---|
| host uniformly busy for the whole run | every factor 1.0; correction left to `global_drift`, the estimator built for it | every factor 1.0 *too*, because the whole trace is elevated together — the min moves with it. **No difference.** |
| local burst over <50% of the suite | detected; measured 7/12 and (predicted) 28/32 | detected, slightly more of it |
| burst over >50% of the suite | **silent — baseline becomes the burst, sensitivity 0** (`scripts/positional-model-limits.py`) | detected; the cliff disappears entirely (confirmed by mutation) |
| noise floor | one sample low by chance shifts the baseline barely | one sample low by chance shifts the baseline *fully*, inflating every factor and manufacturing false positives across the whole run |

The last row is why this is not simply a fix. The median's virtue is that it is
robust to a single unlucky sample and the min is maximally *not*: with 11
samples, one that reads 8% low turns the entire run's factors up by 8%, and the
flag threshold is 10%. The false-positive rate is the half of P22's result that
was not resolution-limited, and it is the half a min-baseline puts at risk.

A low quantile (say the 25th percentile) sits between the two: it survives one
unlucky sample and moves the cliff from >50% coverage to >75%. It does not
remove the cliff, it relocates it — which may be the honest answer, since *some*
coverage level must defeat a within-run baseline. A run that is disturbed
end-to-end has no undisturbed reading to compare against, by construction.

### Why it cannot be settled now

1. **The band nobody has measured.** The two graded runs sit on either side of
   it: P20 loaded the whole suite, P22 loaded 12 of 86. Nothing has been
   measured between ~14% and 100% coverage except by derivation from synthetic
   traces, which assume a uniform elevation that no real load produces.
2. **`global_drift`'s behaviour there is also unmeasured**, and it is the other
   half of the answer. If it turns out to handle 60–80% coverage acceptably, the
   gap is narrower than it looks and the median stays. If it under-corrects the
   disturbed part while over-correcting the clean part — which is what its
   whole-suite construction implies — then something has to own the band.
3. **Changing the estimator while running the experiment that measures the
   model would be tuning the instrument to its own test.** P23 is registered
   against the current code.

### If it is never answered

Safe, and it does not get worse with time. The current behaviour is a *silent*
failure rather than a wrong number: the model declines to flag, and nothing is
corrected on its say-so (§229 — no correction is applied to any recorded value).
The cost is a missed detection in a band no run has yet landed in, and the
failure is now documented in `known-issues.md` with a script that reproduces it,
so it cannot be rediscovered as a surprise.
