## §230 — The experiment that grades the drift model must first prove its own disturbance happened, and the proof's thresholds come from the measured noise floor

**Date:** 2026-08-18
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** §229 built a tool that guesses *which* benchmarks were spoiled by
something else hogging the machine, and left it ungraded. To grade it we
deliberately hog the machine ourselves during a known stretch of the benchmark
run and see whether the tool points at that stretch. The first attempt failed in
a way worth a permanent rule: the hogging arrived late and outstayed its welcome,
so the stretch we *told* the grader was slowed was actually untouched — and the
grader, trusting that label, reported the tool had guessed wrong when in fact it
had guessed right. The decision is that **the grader now measures whether the
disturbance really landed where the label says, and refuses to grade at all if it
did not** — and that the numbers it uses to decide "really landed" are taken from
how much this suite wobbles on its own, not from any round figure.

### The decision, in two parts

**1. A verdict of `UNGRADED` outranks every verdict about the model.** Before
`scripts/grade-positional.py` looks at a single one of the model's flags, it
compares three regions of the run against the previous clean run: the *prefix*
(everything before the labelled window — clean by construction, since the trigger
is a line the kernel has already printed), the *window* itself, and the region
*beyond the disturbance's reach*. It returns `UNGRADED` unless the window ran
measurably slower than the prefix (*the load arrived*) **and** the far region did
not (*the load stayed put*). Whatever the model did is not reported, because on
such a run nothing the model did means anything.

**2. The thresholds are the measured null distribution, not a principle.** The
first draft reused the model's own 10% reporting threshold. That is *below the
instrument's noise floor*: across 4567 benchmark-pairs from 59 consecutive clean
release runs on this host, **38% of benchmarks move ≥10% on the mean** with
nothing disturbing them at all (19% on the best case). The gate does not compare
single benchmarks, though — it compares region medians, which averages most of
that away. Partitioning each clean run into five contiguous blocks by sorted
benchmark name and testing blocks 1–4 against block 0 gives 236 region-pairs of
pure noise:

| statistic | median | p95 | max |
|---|---|---|---|
| best-case ratio | 0.998 | 1.124 | 1.609 |
| mean ratio | 1.001 | 1.678 | 2.585 |

The two statistics are **not** equally good, and the table says which is which.
The best-case p95 (1.124) sits just below its 1.15 threshold, so best-case alone
already fires on under 5% of clean pairs. The mean's p95 (1.678) is far *above*
its 1.30 threshold — on the tightest single-host subset the mean alone fires on
**13% of clean region-pairs**, three times the ceiling. So the mean is not a
second opinion of equal weight: **the best-case ratio is the discriminating
statistic, and the mean is a weaker corroborating condition** that mostly rules
out a best-case fluke. Requiring both — best-case median ≥ 1.15 *and* mean median
≥ 1.30 — fires on **1 of those 236 clean region-pairs (0.4%)**.

That asymmetry is not a defect to be tuned away; it is a property of what the two
numbers measure. The best case is the fastest iteration a benchmark achieved, so
it moves only when *every* iteration was slowed — a frequency or cache effect
that lasts. The mean is dragged by single outlier iterations, and one preemption
inside one iteration of a 2000-iteration benchmark can move it by an order of
magnitude. Measured on an *undisturbed* prefix, per-benchmark mean ratios ranged
from ×0.06 to ×34 while best-case ratios stayed within ×0.9–×1.1. A region median
tames that, but it does not make the mean into a precise instrument, and no
threshold on it could.

`scripts/test-grade-positional.py` re-derives that rate from the real
`bench/history.jsonl` on every run and fails if it climbs above 5%. A suite that
grows noisier therefore breaks the test rather than quietly turning the gate back
into a rubber stamp.

### The alternatives, and why each loses

**Trust the label.** This is what the first attempt did, and it is the reason
this entry exists: the run was reported `FAILED (misplaced)` with 22 of 44
"provably-clean" benchmarks flagged, and all 22 were the model **correctly
reporting a real disturbance that the run's own label denied**. A grader that can
convict a model for being right is worse than no grader, because its output is
not merely absent but actively misleading, and it is misleading in the direction
that looks like a finding. Cheapest to build, and it cost a whole run.

**One statistic instead of two.** Simpler to explain and strictly more
sensitive. But the table above shows why it fails: the mean ratio alone at 1.30
would fire on roughly a twentieth of clean region-pairs, and every such firing
licenses a verdict about the model that rests on nothing. The cost of requiring
both is that a *faint* real disturbance grades `UNGRADED` and the run must be
repeated — which is the correct way to spend a re-run.

**The model's own 10% threshold.** Tempting for consistency: grade the stimulus
by the same bar the model reports at. It is the worst option available, because
it is the one whose failures are invisible — 38% of undisturbed benchmarks clear
it, so a gate built on it would pass almost any run and be indistinguishable from
no gate at all while appearing rigorous.

**A median of the last 8 runs as the baseline, instead of the single previous
clean run.** More samples, less noise — the usual reason to widen a window. It
is wrong here for a reason specific to what is being measured: eight runs span
eight commits of kernel changes, and those changes are not spread evenly over the
suite, so different *regions* acquire different apparent drifts. Measured on the
void run, the 8-run baseline put the untouched prefix at ×0.70 where the previous
run put it at ×0.88 — **an error larger than the disturbance being measured.**
The tightest baseline wins whenever the thing being measured is smaller than the
drift between the runs being averaged.

### The general rule this bought, which outlives the experiment

> You cannot grade "did the model find the disturbance" until you have
> established "there was a disturbance to find."

It is now the first paragraph of `scripts/grade-positional.py`. The same shape
applies to any experiment with an actuator: **a controller whose latency is
comparable to the window it controls does not control that window.** The
benchmark suite is a ~6 s tail of a ~131 s boot, so a 26-benchmark window is
about two seconds — against a `sleep 1` poll and six MSYS `python -c` process
spawns. Nothing in the old script did anything other than what it said; the costs
it paid were simply larger than the thing it was measuring. The replacement,
`scripts/canary-load.py`, pays every cost up front: workers are spawned before
QEMU boots and park on a semaphore, so firing the load is a kernel wakeup, and
the log is followed in-process at 0.1 s.

### What this does not decide

It does not decide P22. The re-run has not happened; §229's correction ban stands
untouched. The one thing the void run did establish — because it needs no label —
is that benchmarks the model flagged were inflated (×1.31 best-case / ×1.84 mean,
n=34) where unflagged ones were not (×0.97 / ×0.92, n=52). That is a single
observation rather than 34, since the flagged set is one contiguous stretch; and
the *magnitude* does not track at all (Pearson r = +0.30 best-case, +0.03 mean).
A correction multiplies by the factor, and the factor is the part carrying no
information — which strengthens §229 rather than weakening it.
