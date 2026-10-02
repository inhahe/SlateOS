### [A] The contamination verdict said `Canary OK` on run 3 — the positional model is the more sensitive instrument, not a refinement of it — 2026-08-19

**Status:** OPEN

**In short:** the suite has two ways of noticing that other activity on the
machine spoiled its measurements. On the run where we deliberately spoiled them,
the *older and more authoritative* one — the one whose word decides whether a
run's numbers are trusted — said everything was fine. The newer one caught it.
That is the opposite of the relationship they were assumed to have, and it means
a run can be waved through by the gate while carrying a disturbance the other
half of the same instrument can see.

Found while deriving P23's threshold, not by looking for it. It was in run 3's
serial log the whole time and the run 3 write-up above missed it.

#### What run 3 actually printed

```
[bench] CANARY 5 5 100 5 6 22 13 0 504 617
[bench] Canary OK: reference access cost stable across 13 samples (5.0-6.1 cycles, spread 22%).
```

Six CPU spinners, verifiably burning the host (occupancy measured directly), for
**37.6% of the suite**. Inside the loaded window the benchmarks ran ×2.58 mean /
×1.34 best-case slower than the clean prefix. And the verdict was **`Canary OK`**.

22% against a `CANARY_TOLERANCE_PCT` of 25 — a **3-point** margin. This is not a
rounding artefact: the kernel computes the spread in hundredths of a cycle
(504/617 are the last two fields) precisely so the verdict is never rounded into
existence, so 22% is the real figure.

#### Why the spread verdict cannot see this

Spread is `(max − min) / min` **over the samples**, and that is a function of the
extremes only — it is completely blind to *how much* of the suite sits at the
high end. One elevated sample out of 13 and six elevated out of 13 produce the
**same** spread, so a disturbance can grow without limit in extent and never move
this number. What moves it is the *depth* of the excursion, and a load that
elevates the reference access by ×1.196 is simply not deep enough to clear 25%,
however long it runs.

The positional model asks the other question — where the samples sit relative to
each other — and that is why it fired on the same data.

**Consequence: these two are not a coarse check and a refinement of it. They are
sensitive to orthogonal properties** (depth vs. extent), and the one that gates
trust is the one blind to extent.

#### The reverse failure: the positional model goes silent above half-suite coverage

Measured against the real code (`scripts/positional-model-limits.py`), sweeping window
width with the elevation held at run 3's ×1.196:

| window width | samples inside | baseline | sensitivity | false positives |
|---|---|---|---|---|
| 8 | 1 | 5.16 | 4/8 | 0/62 |
| 16 | 2 | 5.16 | 12/16 | 0/54 |
| 24 | 3 | 5.16 | 20/24 | 0/46 |
| 32 | 4 | 5.16 | 28/32 | 0/38 |
| 40 | 5 | 5.16 | 36/40 | 0/30 |
| **48** | **6** | **6.17** | **0/48** | 0/22 |
| 56 | 7 | 6.17 | 0/56 | 0/14 |
| 64 | 8 | 6.17 | 0/64 | 0/8 |

`trace_reference` is the **median** of the samples, so once more than half of
them are elevated *the disturbance becomes the baseline* and every factor lands
at or below 1.0. Sensitivity does not degrade — it collapses from 90% to zero
between two adjacent window widths.

**This is a consequence of a deliberate choice, not an oversight.** The median is
there so that a uniformly busy host produces factors of exactly 1.0 and is left
to `global_drift`, which is the estimator built for it; a mean would let the
burst drag the baseline toward itself. That reasoning is correct and the
docstring states it.

**But it leaves a band that neither estimator owns: coverage above ~50% and below
100%.** There, the positional model is silent by construction, while
`global_drift` derives one whole-suite factor from a run that is genuinely part
clean and part disturbed — so it under-corrects the disturbed part and
*over-corrects* the clean part, which is worse than not correcting at all. Nobody
has measured that band; the two graded runs (P20 whole-suite, P22 12 of 86) sit
on either side of it.

Not filed as a bug against either estimator, because neither is behaving other
than as designed. Filed because **both failure modes look exactly like a clean
run** — `Canary OK` and "no stretch ran more than 10% above baseline" are the
same two lines you get from an idle machine — and this file's standing maxim is
that a check which cannot fire is indistinguishable from one that passes.

#### What follows

- **P23 gains a fourth claim** (registered below in the P23 entry): the whole-run
  verdict will *again* read `Canary OK`, because widening the window changes the
  extent and not the depth. If it does, the asymmetry is demonstrated twice
  rather than once, on windows differing by a factor of nearly three.
- **P23's window was sized under the cliff on purpose** — 4 of 11 positioned
  samples — and the margin is two samples, which is thinner than it looks. See
  the caveat added to that entry.
- **Not repaired here.** Changing `trace_reference` to a low quantile would move
  the cliff, and doing that in the same breath as running the experiment that
  measures the model would be tuning the instrument to its own test. The
  measurement comes first.
