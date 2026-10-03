### [A] OPEN — the canary's excursions are discrete, position-independent, and invisible to the benchmarks beside them; the arms that would explain them were discarded — 2026-08-19

**In short:** The contamination canary — the little A/B measurement the suite takes
between benchmarks to notice when the host got busy — sometimes reads 12.6% high. It
is supposed to mean "the machine was loaded here, distrust the numbers around it". But
the benchmarks measured at that exact point in the suite are not slow at all. Either
the canary is crying wolf, or it is detecting something the benchmarks are immune to.
The measurement that would tell the two apart was being computed and thrown away on
every run; it is now recorded, and this entry says what to do with it.

**The observation.** 104 trace samples across the 8 runs that carry a CANARY-TRACE
line fall into three tight clusters and nothing else:

| cluster | n | mean | vs. mode |
|---|---|---|---|
| ~5.04 centicycles | 15 | 5.044 | −2.3% |
| ~5.16 centicycles (mode) | 85 | 5.161 | — |
| ~5.80 centicycles | 4 | 5.798 | **+12.3%** |

There is nothing between 5.06 and 5.14, and nothing at all between 5.18 and 5.78. For
a quantity that is supposed to track a continuously-varying host load, that is the
wrong shape: it is the profile of a mechanism with discrete states, not of drift.

**It is not positional.** The four high samples land at `end`, at position 32, at
`end` again, and at position 8, in three different runs. The low samples scatter
similarly — position 24/32/64/72 in one run, 0/8/80 in another. So this is neither the
suite's own cache/TLB residue (which would recur at the same positions) nor a warm-up
or tail effect (which would concentrate at the edges).

**The benchmarks beside it do not see it.** The clearest case is the run at
2026-08-19T02:28:02, whose only excursion is a single +12.6% sample at position 8,
against the immediately preceding run as baseline:

| pos | benchmark | baseline → spike run | move |
|---|---|---|---|
| 6 | `compress_zero_page` | 6497 → 6488 | −0.1% |
| 7 | `compress_repeating` | 116478 → 117137 | +0.6% |
| **8** | **`rdtsc_overhead`** | **37 → 38** | **+2.7%** |
| 9 | `hpet_read` | 464 → 449 | −3.2% |
| 10 | `context_switch` | 1375 → 1403 | +2.0% |

Median move for positions 4–12 is **+0.6%**; median move across all 86 benchmarks in
the run is **+0.8%**. The benchmarks measured where the canary claims a 12.6% host
slowdown are indistinguishable from the ones measured anywhere else.

**Why "it was just a brief spike" does not survive the timescales.** The canary value
is a **minimum** over ~525 rounds, a window of roughly **4.7 ms**. A minimum cannot
register a transient: to move the min, an effect must cover essentially the *whole*
window. But in that same run the median benchmark takes **1.64 ms** and 60 of 86 are
shorter than the canary's own window. Anything sustained enough to lift a 4.7 ms
minimum by 12.6% would comfortably cover the adjacent benchmarks — and it did not
touch them. So a real, uniform host slowdown is not supported by the data.

**Why the recorded value cannot settle it.** The traced figure is
`(min_store − min_nop) * 100 / n` — a **difference of two minima**. A move in it is
ambiguous between the arms, and the two arms mean opposite things:

- a **dearer store arm** is the host genuinely getting slower at the thing being
  measured — a real contamination signal, and the canary doing its job;
- a **cheaper nop arm** is the measurement's own baseline shifting, which is an
  instrument artefact and says nothing whatever about the host.

The derived difference looks identical either way. Every trace recorded up to this
point computed both arms and discarded them, keeping only the difference, which is
exactly why the +12.3% cluster cannot be attributed today.

Note also that under TCG these "cycles" are host wall time, not guest work: the TSC is
calibrated to the host i7-8700K's real 3708.8 MHz, so the canary is measuring host
nanoseconds per emulated store — i.e. TCG throughput. A discrete step in TCG
translation behaviour is a live candidate for a mechanism with exactly three states.

**What was done.** `kernel/src/bench.rs` now retains both raw arm totals per traced
sample and emits `[bench] CANARY-ARMS <pos>:<nop>:<store> ...` beside the existing
CANARY-TRACE line; `scripts/bench-history.py` parses it and merges the arms onto the
matching trace samples, so `canary.trace[i]` gains `nop` and `store` alongside `centi`.
The merge is all-or-nothing and label-checked: a length or label disagreement drops
every arm rather than attaching any, because a *misplaced* arm is evidence pointing
confidently at the wrong slot and nothing downstream could detect it, whereas a missing
arm is a legible gap.

**What this does not do.** It does not explain the clusters — it collects the evidence
that can. The 79 existing records have no arms and never will; this needs new runs.

**Next step.** Accumulate runs until the ~5.80 cluster recurs (it appeared in 3 of 8
traced runs, so a handful of `--bench` boots should catch it), then read the arms at
that sample. If `store` rose while `nop` held, the canary is right and the benchmarks'
immunity is the thing to explain. If `nop` fell, the excursion is an instrument
artefact and the canary's contamination verdict is over-sensitive by ~12% in a way
that has been silently inflating `spread` all along. The ~5.04 cluster is the same
question mirrored and should be read the same way.
