### B-BENCH-COMPARES-TO-ONE-PRIOR-RUN-NOT-THE-DISTRIBUTION

**Status:** FIXED 2026-08-15 (see the subsection at the end of this entry).
Found the same day while grading P21.

`scripts/bench-history.py` computes its REGRESSED/IMPROVED verdicts against the
**immediately preceding run** on the host. For a benchmark with a tight
distribution that is correct and sensitive. For a volatile one it is neither: it
reports the *difference between two samples of the same noise* as a change in
the code.

Demonstrated, not assumed: in the P21 run, `ipc_channel` was reported as
`+31 % vs suite` on a move from 542 → 688 ns, while its own 16-run release
history spans **420–1475 ns**. 688 is close to its median. Nothing regressed.

**Proper fix.** Compare against a robust summary of the recent distribution
(e.g. median and MAD, or an inter-quartile band, over the last N runs on the
same host and profile) rather than against `runs[-1]`, and report a benchmark
only when it moves outside that band. The machinery is already present — the
tool computes a `x0.976 whole-suite vs the median of the last 8 run(s)` line, so
it has the window and the median; the per-benchmark verdicts simply do not use
them. Keep the raw one-run delta in the output as context, but stop making the
verdict from it.

**Why it matters beyond tidiness.** The regression list is the only automated
guard on 70 benchmarks. Every false positive spends reader attention and makes
the next real regression likelier to be waved through as "probably noise again"
— and this file's standing lesson is that a check nobody trusts is worth about
as much as a check nobody runs.

#### FIXED 2026-08-15, and the choice of estimator was decided by the data

`scripts/bench-history.py` now consults each benchmark's **own** recent range
before calling a movement a regression. The run-over-run threshold still
selects what is worth looking at; the band decides what may be *called*
something. Three outcomes, and the middle one is what keeps the check honest:

| outcome | meaning | heading | fails `--fail-on-regression` |
|---|---|---|---|
| outside its range | a real finding | `REGRESSED` / `IMPROVED` | yes |
| too few prior runs | cannot judge | `REGRESSED, UNCONFIRMED` | **yes** |
| inside its range | not a finding either way | `WITHIN ITS OWN RANGE` | no |

Withholding the *word* "regressed" for want of history is not the same as
clearing the change, so an unjudgeable movement is still reported and still
fails the build. A new benchmark's first real regression must not be silenced
by the fact that it is new.

The band is consulted **only after** the threshold has been crossed, so it can
demote a report and can never invent one. That asymmetry is what makes it safe:
a bug in the band costs sensitivity, not correctness.

**Deviation from the prescription above — Tukey's fence, not median/MAD.** The
entry offered either; consistency with the rest of the file argued for MAD, and
the data overruled it. These per-benchmark distributions are **clustered, not
unimodal**: `ipc_channel` alternates between a ~545 ns and a ~650 ns cluster
across builds. Over the eight runs preceding the motivating one its median is
648.5 with a MAD of 7.5, so a 3-MAD band is **626–671 ns** — narrower than the
gap between that benchmark's own two clusters, on a benchmark whose observed
span is 420–1475. **It flags 688: it would have reproduced the exact false
positive this fix exists to remove.** Quartiles do not have that failure mode —
a bimodal core widens the box instead of shrinking it. The same window gives a
fence of **505–746 ns**, which declines 688 and still catches the next run's
953 ns (+40 %).

Neither constant was tuned; `k = 1.5` is the textbook boxplot rule, chosen
before it was evaluated. Evaluated afterwards by replaying the ten most recent
release comparisons in `bench/history.jsonl`: **47** movements cross the 25 %
threshold, of which MAD confirms 35 and this rule confirms 29. The six they
disagree on include the written-up `ipc_channel` non-event.

`scripts/test-bench-history.py` pins this with a positive control that replays
the real 542 → 688 → 953 sequence out of `bench/history.jsonl` and asserts the
first is declined and the second caught. A silent revert to MAD fails it.

**What this does not fix.** The window is the same eight comparable records
everything else here uses, so a genuine, permanent speed-up reads as IMPROVED
for a run or two and is then absorbed into the band — correct, but it means the
band must not be read as "the code's true value". And a wholly contaminated run
still produces confirmed movements (the last recorded run has 9 threshold
crossings, 8 of them outside their own ranges); separating those is the **run
verdict's** job, not this one's.
