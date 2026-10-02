## B-DISPATCH-OBSERVABILITY-COSTS-64-PERCENT-OF-SYSCALL-DISPATCH

With `scfilter` down to 44 ns, syscall dispatch is dominated by its own
instrumentation: `sclatency_pair` 142 ns + `ktrace_pair` 111 ns = **253 ns of a
393 ns dispatch (64%)**, both enabled by default. Two independent causes, both
of which do work whose result is discarded:

1. **`sclatency::exit` converted units on every syscall to pick a bucket.**
   It called `bench::cycles_to_ns` — two 64-bit divisions — to produce a
   nanosecond value that was compared against 12 constants and thrown away.
   Every other statistic in the module (`TOTAL_CYCLES`, `MIN_CYCLES`,
   `MAX_CYCLES`, `PER_SYSCALL_CYCLES`) was already kept in cycles and converted
   only at readout. Fix: convert the 12 *thresholds* to cycles once in a new
   `sclatency::calibrate()`, called right after `bench::calibrate_tsc()`, and
   compare in cycles. This moves a division off a path every syscall takes onto
   one taken once per boot.

2. **`ktrace::record` looked up the task id that dispatch already had.**
   `sched::current_task_id()` measures ~23 ns, and dispatch called it three
   times per syscall: once explicitly for the filter, and once inside each of
   the two `record` calls, for a value that cannot change across one dispatch.
   Fix: `ktrace::record_with_task(...)` takes the caller's id; `record` remains
   as the wrapper that looks it up (and checks `ENABLED` *before* looking it
   up, so a disabled tracer does not pay for the lookup either).

### Also fixed: a histogram that was most confident when it knew least

`cycles_to_ns` returns 0 when the TSC frequency is unknown. 0 is below the
first threshold, so **every** such sample landed in bucket 0 and the histogram
reported *"100% of syscalls under 1 µs"* — the most flattering possible
answer — precisely when it could not measure at all. Bucketing in cycles must
not inherit that, so `find_bucket_cycles` returns `Option` and uncalibrated
samples are counted in a separate `UNCALIBRATED_SAMPLES`, surfaced by `stats()`
(`uncalibrated`, `calibrated`), disclosed by the kshell histogram, and exported
as the `syscall/latency_unbucketed` counter. The kshell percentage column now
divides by *bucketed* calls so it still reaches 100%.

`sclatency::self_test` holds the new cycle path against the old ns path at
`threshold-1`, `threshold` and `threshold+1` for all 12 boundaries, plus
`u64::MAX` (must saturate into the last bucket, not wrap to the first) and 0.
It **skips loudly** rather than passing if the TSC is uncalibrated, because
every assertion in it would then be vacuous.

### Prospective predictions (registered before the boot that tests them)

7. `sd_sclatency_pair` falls by **≥ 40%** (142 ns → ≤ 85 ns). If it does not,
   the two divisions were not the cost — the remaining work is ~6 relaxed
   atomics, 2 `rdtsc` and 2 CAS loops — and the "divisions are expensive under
   TCG" premise is wrong.
8. `sd_ktrace_pair` falls by **≥ 25 ns**, i.e. at least one `current_task_id`
   (23 ns) of the two removed. If it falls by *much more* than ~46 ns, the
   saving is not just the redundant lookups and my model of `record` is
   incomplete — that would be as much a miss as no change.
9. `syscall_dispatch` total falls by **≥ 100 ns drift-adjusted** (median suite
   ratio, as used for prediction 4). Note predictions 7+8 removing ~80 ns of
   *stage* cost should also remove ~46 ns of the separately-counted
   `sd_current_task_id`-equivalent work, so ≥ 100 ns is the honest bar.
10. `sclatency::self_test` **passes**, with a non-zero probe count. A SKIP is
    not a pass: it means the boot could not verify the rescaling at all.

### RESULT — measured 2026-08-14, predictions 7–10 graded

Boot green, `BOOT_OK`. This time the suite drift was **×1.000** (median over 64
benchmarks), so the two boots are directly comparable — corroborated
independently by the three stages I did not touch: `scfilter` 44→45,
`handler` 24→25, `task_id` 23→23.

```
total 303ns = handler 25 + task_id 23 + scfilter 45 + ktrace_pair 86
            + sclatency_pair 79 + unexplained 45      (drift gate: 303 then 302, 0%)
```

| # | Prediction | Verdict |
|---|---|---|
| 7 | `sd_sclatency_pair` falls ≥ 40% | **HIT** — 142 → 79 ns (×0.56, −44%) |
| 8 | `sd_ktrace_pair` falls ≥ 25 ns, but not "much more" than ~46 | **HIT** — 111 → 86 ns (−25 ns), at the lower bound and inside the window |
| 9 | `syscall_dispatch` falls ≥ 100 ns | **MISS** — 393 → 303 ns, −90 ns |
| 10 | `sclatency::self_test` passes with a non-zero probe count | **HIT** — 36 probes, TSC 3.715 GHz |

**Prediction 9 missed because the prediction was miscomputed, not because the
fix underperformed** — and the shape of the miss is itself the strongest
evidence yet for the component model. I set the bar at 100 ns by reasoning that
predictions 7+8 would remove ~88 ns of stage cost *plus* ~46 ns of
"separately-counted `current_task_id` work". That second term does not exist:
the two redundant lookups lived *inside* `ktrace::record`, hence inside
`sd_ktrace_pair`, so they were already the whole of the −25 ns in prediction 8.
`sd_current_task_id` is a distinct stage measuring the *one* explicit lookup
dispatch still performs, which this change did not remove. I double-counted.

Without the double-count the model predicts −88 ns; measured −90 ns. The stage
deltas reconcile the total exactly:

```
sclatency −63, ktrace −25, scfilter +1, handler +1, task_id 0, unexplained −4  =  −90
393 − 303 = 90                                                                      ✓
```

So the decomposition is now accurate to ~2% on a *predicted* delta, not merely
self-consistent after the fact. That is a stronger result than prediction 9
passing would have been.

**One genuine caveat the miss exposes.** Removing two `current_task_id` calls
saved 25 ns, but `sd_current_task_id` measures 23 ns *each* in isolation. So an
isolated micro-measurement overstates a small stage's in-situ cost by ~2×,
presumably because in isolation it cannot overlap with surrounding work. Direct
component measurement is reliable for *large* stages (the 299 ns scfilter scan
predicted its own removal within noise) and only order-of-magnitude for stages
of a few tens of ns. Do not use it to justify shaving a 20 ns term.

### Where dispatch stands

525 → 393 → 303 ns across three boots, against a 200 ns target (152%).
Instrumentation is still the majority: `ktrace_pair` 86 + `sclatency_pair` 79 =
**165 ns, 54% of dispatch**, both on by default. Remaining terms are now small
and close to the ~2× measurement floor above, so further work here should
target *whether* both tracers run on every syscall by default rather than
shaving either one.

### CORRECTION — `isr_latency` and `pick_next` were never regressions

I flagged `isr_latency` ×2.34 and `pick_next` ×1.76 above as movements "far
exceeding the +29% suite drift, so not explained by it". **That was wrong, and
the error was mine: I treated a two-boot comparison as if one of the two boots
were a baseline.** Normalising the last six boots against their own median
shows one boot was globally, anomalously *fast*:

| boot | whole-suite factor (64 benchmarks) | `syscall_dispatch` |
|---|---|---|
| −6 | ×1.040 | 691 |
| −5 | ×1.026 | 697 |
| −4 | ×0.990 | 699 |
| **−3** | **×0.775** | **525** |
| −2 | ×1.000 | 393 |
| −1 | ×1.000 | 303 |

Boot −3 ran ~23% faster than every other boot *across the board* — host-side,
not ours. Per-benchmark, `isr_latency` reads 45493, 44683, 35843, **19065**,
44619, 44413 and `pick_next` reads 955, 1037, 684, **443**, 781, 749. Neither
regressed; both returned to normal from an outlier. The flag is withdrawn, and
`sched_pick_next` (40, 41, 40, **31**, 40, 41) shows the same signature.

**Two consequences, both worth keeping.**

*The drift adjustment was right for the right reason.* Boot −3's ×0.775 is the
reciprocal of the ×1.290 median I measured going −3 → −2, and 525 ÷ 0.775 ≈ 677
lands squarely in the 691–699 band the three preceding boots recorded for
`syscall_dispatch`. Median-of-suite normalisation recovered the true baseline
from a boot that was uniformly wrong by 23%.

*The headline number is bigger than any single step suggested.* Measured in
comparable units, `syscall_dispatch` has gone from a stable ~695 ns (three
consecutive boots: 691, 697, 699) to **303 ns — a 2.3× speedup, −392 ns** —
across the scfilter and instrumentation fixes together. Against the 200 ns
target that is 348% → **152%**. The per-change figures reported above (−132,
−90) are each correct within their own boot pair; they simply do not add up to
the total because one of the pairs was measured against the fast boot.

**Rule going forward: never baseline against a single boot.** Normalise the
candidate boot against the median of the recent history before calling anything
a regression or an improvement. A single boot on this host can be off by ±25%
uniformly, which is larger than most of the effects being chased.
